//! Production Emergency Recall channel and terminal-candidate preparation.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-010`,
//! `LOOT-002`, `LOOT-033`, and `TECH-015`/`021`-`023`;
//! `Gravebound_Content_Production_Spec_v1.md` Core dangerous-route and Hall
//! contracts; `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08`; and
//! accepted `SPEC-CONFLICT-029`.
//!
//! Client ticks are diagnostic only. Server ticks own start, cancellation,
//! explicit completion at tick 12, and `LinkLost` completion at tick 90. This
//! controller never commits inventory itself; it prepares the exact persistence
//! plan and submits only an opaque candidate to the five-producer coordinator.

use std::future::Future;

use persistence::{
    PersistenceError, PostgresPersistence, PreparedProductionRecallV1,
    ProductionRecallCommitRequestV1, ProductionRecallExpectedVersionsV1, ProductionRecallTriggerV1,
    StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    RecallFrameV1, RecallIntentV1, RecallResultV1, TERMINAL_INVENTORY_SCHEMA_VERSION,
    TERMINAL_MATERIAL_CAPACITY, TERMINAL_PENDING_ITEM_CAPACITY, TerminalInventoryRejectionCodeV1,
};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreRecallIntentAuthority,
    CoreTerminalEvaluation, CoreTerminalProducer, TerminalBinding,
    production_recall_terminal_candidate,
};

pub const PRODUCTION_RECALL_MOVEMENT_BASIS_POINTS: u16 =
    sim_core::EMERGENCY_RECALL_MOVEMENT_BASIS_POINTS;

const MUTATION_ID_CONTEXT: &str = "gravebound.production-recall-channel-mutation.v1";
const TERMINAL_ID_CONTEXT: &str = "gravebound.production-recall-channel-terminal.v1";
const MAX_DURABLE_CLIENT_TICK: u64 = 9_223_372_036_854_775_807;

pub trait ProductionRecallClock: Send + Sync {
    fn unix_millis(&self) -> u64;
}

pub trait ProductionRecallPlanner: Send + Sync {
    fn prepare(
        &self,
        request: &ProductionRecallCommitRequestV1,
    ) -> impl Future<Output = Result<PreparedProductionRecallV1, PersistenceError>> + Send;
}

impl ProductionRecallPlanner for PostgresPersistence {
    async fn prepare(
        &self,
        request: &ProductionRecallCommitRequestV1,
    ) -> Result<PreparedProductionRecallV1, PersistenceError> {
        self.prepare_production_recall_v1(request).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionRecallStartAuthorityV1 {
    pub account_id: [u8; 16],
    pub selected_character_id: [u8; 16],
    pub server_tick: u64,
    pub pending_item_count: u16,
    pub pending_material_stack_count: u8,
}

impl ProductionRecallStartAuthorityV1 {
    fn validate(self) -> Result<(), ProductionRecallChannelError> {
        if self.account_id == [0; 16]
            || self.selected_character_id == [0; 16]
            || self.server_tick == 0
            || self.pending_item_count > TERMINAL_PENDING_ITEM_CAPACITY
            || usize::from(self.pending_material_stack_count) > TERMINAL_MATERIAL_CAPACITY
        {
            return Err(ProductionRecallChannelError::InvalidServerAuthority);
        }
        Ok(())
    }
}

/// Mutable actor projection used only to populate the player-visible pending result. Exact
/// completion custody is always replanned from locked persistence authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionRecallPendingAuthorityV1 {
    pub pending_item_count: u16,
    pub pending_material_stack_count: u8,
}

impl ProductionRecallPendingAuthorityV1 {
    fn validate(self) -> Result<(), ProductionRecallChannelError> {
        if self.pending_item_count > TERMINAL_PENDING_ITEM_CAPACITY
            || usize::from(self.pending_material_stack_count) > TERMINAL_MATERIAL_CAPACITY
        {
            return Err(ProductionRecallChannelError::InvalidServerAuthority);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionRecallCompletionAuthorityV1 {
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub instance_lineage_id: [u8; 16],
    pub entry_restore_point_id: [u8; 16],
    pub expected_versions: ProductionRecallExpectedVersionsV1,
    pub content_revision: StoredWorldFlowRevisionV1,
    pub server_tick: u64,
    pub final_lifetime_ticks: u64,
    pub final_permadeath_combat_ticks: u64,
}

impl ProductionRecallCompletionAuthorityV1 {
    fn binding(&self) -> Result<TerminalBinding, ProductionRecallChannelError> {
        TerminalBinding::new(
            self.account_id,
            self.character_id,
            self.instance_lineage_id,
            self.entry_restore_point_id,
        )
        .map_err(|_| ProductionRecallChannelError::InvalidServerAuthority)
    }

    fn absent(
        &self,
        producer: CoreTerminalProducer,
    ) -> Result<CoreTerminalEvaluation, ProductionRecallChannelError> {
        if self.server_tick == 0
            || self.expected_versions.character == 0
            || self.expected_versions.character == u64::MAX
        {
            return Err(ProductionRecallChannelError::InvalidServerAuthority);
        }
        Ok(CoreTerminalEvaluation::absent(
            producer,
            self.binding()?,
            self.server_tick,
            self.expected_versions.character,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionLinkLostRecallAuthorityV1 {
    pub completion: ProductionRecallCompletionAuthorityV1,
    pub lost_tick: u64,
    pub issued_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionRecallTickPreparation {
    Absent(CoreTerminalEvaluation),
    Fresh {
        prepared: Box<PreparedProductionRecallV1>,
        evaluation: CoreTerminalEvaluation,
    },
    CommittedReplay {
        prepared: Box<PreparedProductionRecallV1>,
    },
}

impl ProductionRecallTickPreparation {
    #[must_use]
    pub fn prepared(&self) -> Option<&PreparedProductionRecallV1> {
        match self {
            Self::Fresh { prepared, .. } | Self::CommittedReplay { prepared } => {
                Some(prepared.as_ref())
            }
            Self::Absent(_) => None,
        }
    }

    #[must_use]
    pub const fn evaluation(&self) -> Option<&CoreTerminalEvaluation> {
        match self {
            Self::Absent(evaluation) | Self::Fresh { evaluation, .. } => Some(evaluation),
            Self::CommittedReplay { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelingRecall {
    account_id: [u8; 16],
    character_id: [u8; 16],
    request_sequence: u32,
    client_tick: u64,
    issued_at_unix_ms: u64,
    started_tick: u64,
    completion_tick: u64,
    pending_item_count: u16,
    pending_material_stack_count: u8,
}

impl ChannelingRecall {
    fn pending_result(&self) -> RecallResultV1 {
        RecallResultV1::Pending {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: self.request_sequence,
            character_id: self.character_id,
            started_tick: self.started_tick,
            completion_tick: self.completion_tick,
            pending_item_count: self.pending_item_count,
            pending_material_stack_count: self.pending_material_stack_count,
        }
    }

    fn matches_start(&self, frame: &RecallFrameV1) -> bool {
        self.request_sequence == frame.sequence
            && self.character_id == frame.character_id
            && self.client_tick == frame.client_tick
            && frame.intent == RecallIntentV1::Start
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProductionRecallChannelState {
    Inactive,
    Channeling(ChannelingRecall),
    Prepared {
        channel: ChannelingRecall,
        prepared: Box<PreparedProductionRecallV1>,
    },
}

#[derive(Debug, Clone)]
pub struct ProductionRecallChannel<Clock> {
    clock: Clock,
    state: ProductionRecallChannelState,
}

#[derive(Debug)]
struct ProductionRecallIntentActorState<Clock> {
    channel: ProductionRecallChannel<Clock>,
    pending: ProductionRecallPendingAuthorityV1,
}

/// One live character actor's Recall intent and channel authority.
///
/// This type is intentionally actor-scoped: it owns no global account map and performs no
/// repository lookup from the transport path. The gameplay actor refreshes bounded pending counts,
/// supplies authoritative completion versions to [`Self::evaluate_explicit_tick`], and remains the
/// owner of the shared terminal coordinator and execution service.
#[derive(Debug)]
pub struct ProductionRecallIntentActor<Clock> {
    account_id: [u8; 16],
    character_id: [u8; 16],
    state: Mutex<ProductionRecallIntentActorState<Clock>>,
}

impl<Clock> ProductionRecallIntentActor<Clock> {
    pub fn new(
        clock: Clock,
        account_id: [u8; 16],
        character_id: [u8; 16],
        pending: ProductionRecallPendingAuthorityV1,
    ) -> Result<Self, ProductionRecallChannelError> {
        if account_id == [0; 16] || character_id == [0; 16] {
            return Err(ProductionRecallChannelError::InvalidServerAuthority);
        }
        pending.validate()?;
        Ok(Self {
            account_id,
            character_id,
            state: Mutex::new(ProductionRecallIntentActorState {
                channel: ProductionRecallChannel::new(clock),
                pending,
            }),
        })
    }

    #[must_use]
    pub const fn account_id(&self) -> [u8; 16] {
        self.account_id
    }

    #[must_use]
    pub const fn character_id(&self) -> [u8; 16] {
        self.character_id
    }

    /// Refreshes only server-observed pending counts for a future Start result. An active channel
    /// retains the snapshot captured at its original start tick.
    pub async fn refresh_pending_authority(
        &self,
        pending: ProductionRecallPendingAuthorityV1,
    ) -> Result<(), ProductionRecallChannelError> {
        pending.validate()?;
        self.state.lock().await.pending = pending;
        Ok(())
    }

    /// Handles one reliable Start/Cancel intent against the actor's immutable account/character
    /// binding and the caller-supplied authoritative server tick.
    pub async fn handle(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &RecallFrameV1,
        server_tick: u64,
    ) -> RecallResultV1
    where
        Clock: ProductionRecallClock,
    {
        let mut state = self.state.lock().await;
        let authority = ProductionRecallStartAuthorityV1 {
            account_id: self.account_id,
            selected_character_id: self.character_id,
            server_tick,
            pending_item_count: state.pending.pending_item_count,
            pending_material_stack_count: state.pending.pending_material_stack_count,
        };
        state.channel.handle(authenticated, authority, frame)
    }

    /// Evaluates the explicit producer through the same serialized actor channel used by reliable
    /// intent dispatch. The caller retains ownership of all other producer evaluations and commit.
    pub async fn evaluate_explicit_tick<Planner>(
        &self,
        planner: &Planner,
        authority: &ProductionRecallCompletionAuthorityV1,
    ) -> Result<ProductionRecallTickPreparation, ProductionRecallChannelError>
    where
        Clock: ProductionRecallClock,
        Planner: ProductionRecallPlanner,
    {
        self.state
            .lock()
            .await
            .channel
            .evaluate_explicit_tick(planner, authority)
            .await
    }

    #[must_use]
    pub async fn is_channeling(&self) -> bool {
        self.state.lock().await.channel.is_channeling()
    }

    #[must_use]
    pub async fn movement_basis_points(&self) -> u16 {
        self.state.lock().await.channel.movement_basis_points()
    }

    #[must_use]
    pub async fn blocks_combat_interaction_and_consumables(&self) -> bool {
        self.state
            .lock()
            .await
            .channel
            .blocks_combat_interaction_and_consumables()
    }
}

impl<Clock> CoreRecallIntentAuthority for ProductionRecallIntentActor<Clock>
where
    Clock: ProductionRecallClock,
{
    #[allow(
        clippy::manual_async_fn,
        reason = "the desugared public trait contract guarantees a Send future for spawned QUIC workers"
    )]
    fn handle_recall<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a RecallFrameV1,
        server_tick: u64,
    ) -> impl Future<Output = RecallResultV1> + Send + 'a {
        async move { self.handle(authenticated, frame, server_tick).await }
    }
}

impl<Clock> ProductionRecallChannel<Clock> {
    #[must_use]
    pub const fn new(clock: Clock) -> Self {
        Self {
            clock,
            state: ProductionRecallChannelState::Inactive,
        }
    }

    #[must_use]
    pub const fn is_channeling(&self) -> bool {
        matches!(self.state, ProductionRecallChannelState::Channeling(_))
    }

    #[must_use]
    pub const fn movement_basis_points(&self) -> u16 {
        if self.is_channeling() {
            PRODUCTION_RECALL_MOVEMENT_BASIS_POINTS
        } else {
            10_000
        }
    }

    #[must_use]
    pub const fn blocks_combat_interaction_and_consumables(&self) -> bool {
        self.is_channeling()
    }

    #[must_use]
    pub const fn damage_cancels_channel(&self) -> bool {
        false
    }
}

impl<Clock> ProductionRecallChannel<Clock>
where
    Clock: ProductionRecallClock,
{
    /// Handles an authenticated Start/Cancel intent. The client tick is retained
    /// only to detect altered replay; all result ticks come from server authority.
    pub fn handle(
        &mut self,
        authenticated: AuthenticatedAccount,
        authority: ProductionRecallStartAuthorityV1,
        frame: &RecallFrameV1,
    ) -> RecallResultV1 {
        let rejection = |code| RecallResultV1::Rejected {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: frame.sequence,
            character_id: frame.character_id,
            code,
        };
        if frame.validate().is_err()
            || frame.client_tick > MAX_DURABLE_CLIENT_TICK
            || authority.validate().is_err()
        {
            return rejection(TerminalInventoryRejectionCodeV1::InvalidRequest);
        }
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || authenticated.account_id.as_bytes() != authority.account_id
            || frame.character_id != authority.selected_character_id
        {
            return rejection(TerminalInventoryRejectionCodeV1::ForeignAuthority);
        }

        match frame.intent {
            RecallIntentV1::Start => match &self.state {
                ProductionRecallChannelState::Inactive => {
                    let issued_at_unix_ms = self.clock.unix_millis();
                    let Some(completion_tick) = authority
                        .server_tick
                        .checked_add(persistence::PRODUCTION_RECALL_EXPLICIT_CHANNEL_TICKS)
                    else {
                        return rejection(TerminalInventoryRejectionCodeV1::StaleAuthority);
                    };
                    if issued_at_unix_ms == 0 {
                        return rejection(TerminalInventoryRejectionCodeV1::DatabaseUnavailable);
                    }
                    let channel = ChannelingRecall {
                        account_id: authority.account_id,
                        character_id: authority.selected_character_id,
                        request_sequence: frame.sequence,
                        client_tick: frame.client_tick,
                        issued_at_unix_ms,
                        started_tick: authority.server_tick,
                        completion_tick,
                        pending_item_count: authority.pending_item_count,
                        pending_material_stack_count: authority.pending_material_stack_count,
                    };
                    let result = channel.pending_result();
                    self.state = ProductionRecallChannelState::Channeling(channel);
                    result
                }
                ProductionRecallChannelState::Channeling(channel)
                | ProductionRecallChannelState::Prepared { channel, .. }
                    if channel.matches_start(frame) =>
                {
                    channel.pending_result()
                }
                ProductionRecallChannelState::Channeling(_)
                | ProductionRecallChannelState::Prepared { .. } => {
                    rejection(TerminalInventoryRejectionCodeV1::IdempotencyConflict)
                }
            },
            RecallIntentV1::Cancel => {
                let ProductionRecallChannelState::Channeling(channel) = &self.state else {
                    return rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable);
                };
                if authority.account_id != channel.account_id
                    || authority.selected_character_id != channel.character_id
                {
                    return rejection(TerminalInventoryRejectionCodeV1::ForeignAuthority);
                }
                if authority.server_tick >= channel.completion_tick {
                    return rejection(TerminalInventoryRejectionCodeV1::TerminalLost);
                }
                let result = RecallResultV1::Cancelled {
                    schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
                    request_sequence: frame.sequence,
                    character_id: channel.character_id,
                    started_tick: channel.started_tick,
                    cancelled_tick: authority.server_tick,
                };
                self.state = ProductionRecallChannelState::Inactive;
                result
            }
        }
    }

    /// Evaluates the explicit Recall producer for one authoritative tick.
    pub async fn evaluate_explicit_tick<Planner>(
        &mut self,
        planner: &Planner,
        authority: &ProductionRecallCompletionAuthorityV1,
    ) -> Result<ProductionRecallTickPreparation, ProductionRecallChannelError>
    where
        Planner: ProductionRecallPlanner,
    {
        let producer = CoreTerminalProducer::EmergencyRecall;
        match &self.state {
            ProductionRecallChannelState::Inactive => Ok(ProductionRecallTickPreparation::Absent(
                authority.absent(producer)?,
            )),
            ProductionRecallChannelState::Prepared { prepared, .. } => {
                prepared_preparation(prepared.as_ref().clone(), authority, producer)
            }
            ProductionRecallChannelState::Channeling(channel) => {
                validate_completion_binding(channel, authority)?;
                if authority.server_tick < channel.completion_tick {
                    return Ok(ProductionRecallTickPreparation::Absent(
                        authority.absent(producer)?,
                    ));
                }
                if authority.server_tick > channel.completion_tick {
                    return Err(ProductionRecallChannelError::MissedCompletionTick {
                        expected: channel.completion_tick,
                        actual: authority.server_tick,
                    });
                }
                let request = recall_request(
                    channel.account_id,
                    channel.character_id,
                    authority,
                    ProductionRecallTriggerV1::Explicit,
                    Some(channel.request_sequence),
                    Some(channel.client_tick),
                    channel.issued_at_unix_ms,
                    channel.started_tick,
                    channel.completion_tick,
                )?;
                let prepared = planner
                    .prepare(&request)
                    .await
                    .map_err(ProductionRecallChannelError::Persistence)?;
                let result = prepared_preparation(prepared.clone(), authority, producer)?;
                self.state = ProductionRecallChannelState::Prepared {
                    channel: channel.clone(),
                    prepared: Box::new(prepared),
                };
                Ok(result)
            }
        }
    }
}

/// Evaluates the automatic disconnect-recovery producer. Reconnect before the
/// exact deadline simply stops callers from invoking this due path.
pub async fn evaluate_link_lost_tick<Planner>(
    planner: &Planner,
    authority: &ProductionLinkLostRecallAuthorityV1,
) -> Result<ProductionRecallTickPreparation, ProductionRecallChannelError>
where
    Planner: ProductionRecallPlanner,
{
    let producer = CoreTerminalProducer::DisconnectRecovery;
    if authority.lost_tick == 0 || authority.issued_at_unix_ms == 0 {
        return Err(ProductionRecallChannelError::InvalidServerAuthority);
    }
    let completion_tick = authority
        .lost_tick
        .checked_add(persistence::PRODUCTION_RECALL_LINK_LOST_TICKS)
        .ok_or(ProductionRecallChannelError::InvalidServerAuthority)?;
    if authority.completion.server_tick < completion_tick {
        return Ok(ProductionRecallTickPreparation::Absent(
            authority.completion.absent(producer)?,
        ));
    }
    if authority.completion.server_tick > completion_tick {
        return Err(ProductionRecallChannelError::MissedCompletionTick {
            expected: completion_tick,
            actual: authority.completion.server_tick,
        });
    }
    let request = recall_request(
        authority.completion.account_id,
        authority.completion.character_id,
        &authority.completion,
        ProductionRecallTriggerV1::LinkLost,
        None,
        None,
        authority.issued_at_unix_ms,
        authority.lost_tick,
        completion_tick,
    )?;
    let prepared = planner
        .prepare(&request)
        .await
        .map_err(ProductionRecallChannelError::Persistence)?;
    prepared_preparation(prepared, &authority.completion, producer)
}

fn prepared_preparation(
    prepared: PreparedProductionRecallV1,
    authority: &ProductionRecallCompletionAuthorityV1,
    producer: CoreTerminalProducer,
) -> Result<ProductionRecallTickPreparation, ProductionRecallChannelError> {
    if prepared.replayed() {
        return Ok(ProductionRecallTickPreparation::CommittedReplay {
            prepared: Box::new(prepared),
        });
    }
    let candidate = production_recall_terminal_candidate(&prepared)
        .map_err(|_| ProductionRecallChannelError::InvalidPreparedAuthority)?;
    Ok(ProductionRecallTickPreparation::Fresh {
        evaluation: CoreTerminalEvaluation::candidate(
            producer,
            authority.binding()?,
            authority.server_tick,
            authority.expected_versions.character,
            candidate,
        ),
        prepared: Box::new(prepared),
    })
}

fn validate_completion_binding(
    channel: &ChannelingRecall,
    authority: &ProductionRecallCompletionAuthorityV1,
) -> Result<(), ProductionRecallChannelError> {
    if channel.account_id != authority.account_id || channel.character_id != authority.character_id
    {
        return Err(ProductionRecallChannelError::BindingMismatch);
    }
    authority.binding()?;
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the helper binds the complete server-owned terminal request"
)]
fn recall_request(
    account_id: [u8; 16],
    character_id: [u8; 16],
    authority: &ProductionRecallCompletionAuthorityV1,
    trigger: ProductionRecallTriggerV1,
    request_sequence: Option<u32>,
    explicit_client_tick: Option<u64>,
    issued_at_unix_ms: u64,
    trigger_started_tick: u64,
    completion_tick: u64,
) -> Result<ProductionRecallCommitRequestV1, ProductionRecallChannelError> {
    if authority.server_tick != completion_tick {
        return Err(ProductionRecallChannelError::InvalidServerAuthority);
    }
    let mutation_id = recall_identity(
        MUTATION_ID_CONTEXT,
        account_id,
        character_id,
        authority.instance_lineage_id,
        authority.entry_restore_point_id,
        trigger,
        request_sequence,
        trigger_started_tick,
        completion_tick,
    );
    let mut terminal_id = recall_identity(
        TERMINAL_ID_CONTEXT,
        account_id,
        character_id,
        authority.instance_lineage_id,
        authority.entry_restore_point_id,
        trigger,
        request_sequence,
        trigger_started_tick,
        completion_tick,
    );
    if terminal_id == mutation_id {
        terminal_id[15] ^= 1;
        if terminal_id == [0; 16] {
            terminal_id[15] = 1;
        }
    }
    let request = ProductionRecallCommitRequestV1 {
        contract_version: persistence::PRODUCTION_RECALL_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        account_id,
        character_id,
        mutation_id,
        terminal_id,
        trigger,
        request_sequence,
        explicit_client_tick,
        instance_lineage_id: authority.instance_lineage_id,
        entry_restore_point_id: authority.entry_restore_point_id,
        expected_versions: authority.expected_versions,
        content_revision: authority.content_revision.clone(),
        issued_at_unix_ms,
        trigger_started_tick,
        completion_tick,
        final_lifetime_ticks: authority.final_lifetime_ticks,
        final_permadeath_combat_ticks: authority.final_permadeath_combat_ticks,
    };
    request
        .validate()
        .map_err(|_| ProductionRecallChannelError::InvalidServerAuthority)?;
    Ok(request)
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal identity binds every server-owned operation axis"
)]
fn recall_identity(
    context: &str,
    account_id: [u8; 16],
    character_id: [u8; 16],
    lineage_id: [u8; 16],
    restore_point_id: [u8; 16],
    trigger: ProductionRecallTriggerV1,
    request_sequence: Option<u32>,
    started_tick: u64,
    completion_tick: u64,
) -> [u8; 16] {
    let trigger_code = match trigger {
        ProductionRecallTriggerV1::Explicit => 0_u8,
        ProductionRecallTriggerV1::LinkLost => 1,
    };
    let sequence = request_sequence.unwrap_or(0).to_be_bytes();
    let mut hasher = blake3::Hasher::new_derive_key(context);
    for part in [
        account_id.as_slice(),
        character_id.as_slice(),
        lineage_id.as_slice(),
        restore_point_id.as_slice(),
        &[trigger_code],
        sequence.as_slice(),
        started_tick.to_be_bytes().as_slice(),
        completion_tick.to_be_bytes().as_slice(),
    ] {
        hasher.update(&(part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    if identity == [0; 16] {
        identity[15] = 1;
    }
    identity
}

#[derive(Debug, Error)]
pub enum ProductionRecallChannelError {
    #[error("server Recall authority is incomplete or invalid")]
    InvalidServerAuthority,
    #[error("completion authority does not own the active Recall channel")]
    BindingMismatch,
    #[error("Recall producer missed its exact completion tick (expected {expected}, got {actual})")]
    MissedCompletionTick { expected: u64, actual: u64 },
    #[error("persistence could not prepare the Recall terminal")]
    Persistence(#[source] PersistenceError),
    #[error("persistence returned an invalid prepared Recall authority")]
    InvalidPreparedAuthority,
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use crate::{
        AccountId, CoreTerminalCoordinator, CoreTerminalTickSeal, TerminalBinding,
        TerminalCandidate, TerminalKind,
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
        requests: Arc<Mutex<Vec<ProductionRecallCommitRequestV1>>>,
    }

    impl FakePlanner {
        fn fresh() -> Self {
            Self {
                replayed: false,
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn replayed() -> Self {
            Self {
                replayed: true,
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> usize {
            self.requests.lock().expect("requests").len()
        }

        fn last_request(&self) -> ProductionRecallCommitRequestV1 {
            self.requests
                .lock()
                .expect("requests")
                .last()
                .expect("request")
                .clone()
        }
    }

    impl ProductionRecallPlanner for FakePlanner {
        async fn prepare(
            &self,
            request: &ProductionRecallCommitRequestV1,
        ) -> Result<PreparedProductionRecallV1, PersistenceError> {
            self.requests
                .lock()
                .expect("requests")
                .push(request.clone());
            PreparedProductionRecallV1::seal(
                request.clone(),
                request.canonical_hash()?,
                [9; 32],
                self.replayed,
            )
        }
    }

    #[derive(Clone)]
    struct FailOncePlanner {
        attempts: Arc<AtomicUsize>,
        delegate: FakePlanner,
    }

    impl FailOncePlanner {
        fn new() -> Self {
            Self {
                attempts: Arc::new(AtomicUsize::new(0)),
                delegate: FakePlanner::fresh(),
            }
        }
    }

    impl ProductionRecallPlanner for FailOncePlanner {
        async fn prepare(
            &self,
            request: &ProductionRecallCommitRequestV1,
        ) -> Result<PreparedProductionRecallV1, PersistenceError> {
            if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(PersistenceError::InvalidWipeableNamespace);
            }
            self.delegate.prepare(request).await
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn start_authority(server_tick: u64) -> ProductionRecallStartAuthorityV1 {
        ProductionRecallStartAuthorityV1 {
            account_id: [1; 16],
            selected_character_id: [2; 16],
            server_tick,
            pending_item_count: 3,
            pending_material_stack_count: 1,
        }
    }

    fn frame(sequence: u32, client_tick: u64, intent: RecallIntentV1) -> RecallFrameV1 {
        RecallFrameV1 {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence,
            character_id: [2; 16],
            client_tick,
            intent,
        }
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

    #[test]
    fn production_timing_constants_match_simulation_and_session_authority() {
        assert_eq!(
            persistence::PRODUCTION_RECALL_EXPLICIT_CHANNEL_TICKS,
            sim_core::EMERGENCY_RECALL_CHANNEL_TICKS
        );
        assert_eq!(
            persistence::PRODUCTION_RECALL_LINK_LOST_TICKS,
            crate::LINK_LOST_TICKS
        );
        assert_eq!(u64::from(protocol::SIMULATION_HZ), 30);
        assert_eq!(
            persistence::PRODUCTION_RECALL_EXPLICIT_CHANNEL_TICKS * 1_000
                / u64::from(protocol::SIMULATION_HZ),
            400
        );
        assert_eq!(
            persistence::PRODUCTION_RECALL_LINK_LOST_TICKS / u64::from(protocol::SIMULATION_HZ),
            3
        );
        assert_eq!(PRODUCTION_RECALL_MOVEMENT_BASIS_POINTS, 7_500);
    }

    #[test]
    fn start_replay_cancel_and_channel_restrictions_use_server_ticks() {
        let mut channel = ProductionRecallChannel::new(FixedClock(50));
        let pending = channel.handle(
            authenticated(),
            start_authority(100),
            &frame(7, 999, RecallIntentV1::Start),
        );
        assert!(matches!(
            pending,
            RecallResultV1::Pending {
                started_tick: 100,
                completion_tick: 112,
                pending_item_count: 3,
                pending_material_stack_count: 1,
                ..
            }
        ));
        assert_eq!(channel.movement_basis_points(), 7_500);
        assert!(channel.blocks_combat_interaction_and_consumables());
        assert!(!channel.damage_cancels_channel());

        assert_eq!(
            channel.handle(
                authenticated(),
                start_authority(101),
                &frame(7, 999, RecallIntentV1::Start),
            ),
            pending
        );
        assert!(matches!(
            channel.handle(
                authenticated(),
                start_authority(101),
                &frame(7, 998, RecallIntentV1::Start),
            ),
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::IdempotencyConflict,
                ..
            }
        ));
        assert!(matches!(
            channel.handle(
                authenticated(),
                start_authority(111),
                &frame(8, 1_000, RecallIntentV1::Cancel),
            ),
            RecallResultV1::Cancelled {
                started_tick: 100,
                cancelled_tick: 111,
                ..
            }
        ));
        assert!(!channel.is_channeling());
        assert_eq!(channel.movement_basis_points(), 10_000);
    }

    #[test]
    fn cancel_on_completion_tick_is_rejected_and_damage_never_cancels() {
        let mut channel = ProductionRecallChannel::new(FixedClock(50));
        channel.handle(
            authenticated(),
            start_authority(100),
            &frame(7, 100, RecallIntentV1::Start),
        );
        assert!(!channel.damage_cancels_channel());
        assert!(matches!(
            channel.handle(
                authenticated(),
                start_authority(112),
                &frame(8, 112, RecallIntentV1::Cancel),
            ),
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::TerminalLost,
                ..
            }
        ));
        assert!(channel.is_channeling());
    }

    #[tokio::test]
    async fn explicit_completion_prepares_only_at_tick_twelve_with_stable_server_ids() {
        let planner = FakePlanner::fresh();
        let mut channel = ProductionRecallChannel::new(FixedClock(50));
        channel.handle(
            authenticated(),
            start_authority(100),
            &frame(7, 9_999, RecallIntentV1::Start),
        );

        let early = channel
            .evaluate_explicit_tick(&planner, &completion(111))
            .await
            .unwrap();
        assert!(matches!(early, ProductionRecallTickPreparation::Absent(_)));
        assert_eq!(planner.calls(), 0);

        let due = channel
            .evaluate_explicit_tick(&planner, &completion(112))
            .await
            .unwrap();
        let ProductionRecallTickPreparation::Fresh {
            prepared,
            evaluation,
        } = due
        else {
            panic!("tick twelve must prepare");
        };
        assert_eq!(evaluation.producer(), CoreTerminalProducer::EmergencyRecall);
        assert!(evaluation.has_candidate());
        assert_eq!(prepared.request().trigger_started_tick, 100);
        assert_eq!(prepared.request().completion_tick, 112);
        assert_eq!(prepared.request().request_sequence, Some(7));
        assert_eq!(prepared.request().explicit_client_tick, Some(9_999));
        assert_eq!(prepared.request().issued_at_unix_ms, 50);
        assert_ne!(
            prepared.request().mutation_id,
            prepared.request().terminal_id
        );
        assert_eq!(planner.calls(), 1);

        let replay = channel
            .evaluate_explicit_tick(&planner, &completion(112))
            .await
            .unwrap();
        assert!(matches!(
            replay,
            ProductionRecallTickPreparation::Fresh { .. }
        ));
        assert_eq!(planner.calls(), 1);
    }

    #[tokio::test]
    async fn intent_actor_binds_transport_to_one_server_owned_character_snapshot() {
        let actor = ProductionRecallIntentActor::new(
            FixedClock(50),
            [1; 16],
            [2; 16],
            ProductionRecallPendingAuthorityV1 {
                pending_item_count: 3,
                pending_material_stack_count: 1,
            },
        )
        .unwrap();
        let pending = actor
            .handle(
                authenticated(),
                &frame(7, 9_999, RecallIntentV1::Start),
                100,
            )
            .await;
        assert!(matches!(
            pending,
            RecallResultV1::Pending {
                started_tick: 100,
                completion_tick: 112,
                pending_item_count: 3,
                pending_material_stack_count: 1,
                ..
            }
        ));
        assert!(actor.is_channeling().await);
        assert_eq!(actor.movement_basis_points().await, 7_500);
        assert!(actor.blocks_combat_interaction_and_consumables().await);

        actor
            .refresh_pending_authority(ProductionRecallPendingAuthorityV1 {
                pending_item_count: 9,
                pending_material_stack_count: 2,
            })
            .await
            .unwrap();
        assert_eq!(
            actor
                .handle(
                    authenticated(),
                    &frame(7, 9_999, RecallIntentV1::Start),
                    101,
                )
                .await,
            pending
        );
        assert!(matches!(
            actor
                .handle(
                    authenticated(),
                    &frame(8, 10_000, RecallIntentV1::Cancel),
                    111,
                )
                .await,
            RecallResultV1::Cancelled {
                cancelled_tick: 111,
                ..
            }
        ));

        assert!(matches!(
            actor
                .handle(
                    authenticated(),
                    &frame(9, 10_001, RecallIntentV1::Start),
                    200,
                )
                .await,
            RecallResultV1::Pending {
                pending_item_count: 9,
                pending_material_stack_count: 2,
                started_tick: 200,
                completion_tick: 212,
                ..
            }
        ));

        let foreign = AuthenticatedAccount {
            account_id: AccountId::new([10; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        assert!(matches!(
            actor
                .handle(foreign, &frame(10, 10_002, RecallIntentV1::Start), 201)
                .await,
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::ForeignAuthority,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn due_tick_prepare_outage_is_retryable_but_skipping_the_tick_fails_closed() {
        let planner = FailOncePlanner::new();
        let mut channel = ProductionRecallChannel::new(FixedClock(50));
        channel.handle(
            authenticated(),
            start_authority(100),
            &frame(7, 100, RecallIntentV1::Start),
        );

        assert!(matches!(
            channel
                .evaluate_explicit_tick(&planner, &completion(112))
                .await,
            Err(ProductionRecallChannelError::Persistence(
                PersistenceError::InvalidWipeableNamespace
            ))
        ));
        assert!(channel.is_channeling());

        let retried = channel
            .evaluate_explicit_tick(&planner, &completion(112))
            .await
            .expect("same authoritative completion tick remains retryable");
        assert!(matches!(
            retried,
            ProductionRecallTickPreparation::Fresh { .. }
        ));
        assert_eq!(planner.attempts.load(Ordering::SeqCst), 2);
        assert_eq!(planner.delegate.calls(), 1);

        let mut missed = ProductionRecallChannel::new(FixedClock(50));
        missed.handle(
            authenticated(),
            start_authority(100),
            &frame(8, 100, RecallIntentV1::Start),
        );
        assert!(matches!(
            missed
                .evaluate_explicit_tick(&FakePlanner::fresh(), &completion(113))
                .await,
            Err(ProductionRecallChannelError::MissedCompletionTick {
                expected: 112,
                actual: 113
            })
        ));
    }

    #[tokio::test]
    async fn link_lost_is_absent_at_eighty_nine_and_due_at_ninety() {
        let planner = FakePlanner::fresh();
        let early = ProductionLinkLostRecallAuthorityV1 {
            completion: completion(289),
            lost_tick: 200,
            issued_at_unix_ms: 70,
        };
        assert!(matches!(
            evaluate_link_lost_tick(&planner, &early).await.unwrap(),
            ProductionRecallTickPreparation::Absent(_)
        ));
        assert_eq!(planner.calls(), 0);

        let due = ProductionLinkLostRecallAuthorityV1 {
            completion: completion(290),
            ..early
        };
        let prepared = evaluate_link_lost_tick(&planner, &due).await.unwrap();
        let ProductionRecallTickPreparation::Fresh {
            prepared,
            evaluation,
        } = prepared
        else {
            panic!("tick ninety must prepare");
        };
        assert_eq!(
            evaluation.producer(),
            CoreTerminalProducer::DisconnectRecovery
        );
        assert_eq!(
            prepared.request().trigger,
            ProductionRecallTriggerV1::LinkLost
        );
        assert_eq!(prepared.request().trigger_started_tick, 200);
        assert_eq!(prepared.request().completion_tick, 290);
        assert_eq!(prepared.request().request_sequence, None);
        assert_eq!(prepared.request().explicit_client_tick, None);
    }

    #[tokio::test]
    async fn stored_prepare_replay_does_not_enter_a_new_barrier() {
        let planner = FakePlanner::replayed();
        let mut channel = ProductionRecallChannel::new(FixedClock(50));
        channel.handle(
            authenticated(),
            start_authority(100),
            &frame(7, 100, RecallIntentV1::Start),
        );
        assert!(matches!(
            channel
                .evaluate_explicit_tick(&planner, &completion(112))
                .await
                .unwrap(),
            ProductionRecallTickPreparation::CommittedReplay { .. }
        ));
    }

    #[tokio::test]
    async fn lethal_candidate_wins_the_exact_explicit_completion_tick() {
        let planner = FakePlanner::fresh();
        let mut channel = ProductionRecallChannel::new(FixedClock(50));
        channel.handle(
            authenticated(),
            start_authority(100),
            &frame(7, 100, RecallIntentV1::Start),
        );
        let ProductionRecallTickPreparation::Fresh {
            evaluation: recall, ..
        } = channel
            .evaluate_explicit_tick(&planner, &completion(112))
            .await
            .unwrap()
        else {
            panic!("Recall due");
        };
        let binding = TerminalBinding::new([1; 16], [2; 16], [3; 16], [4; 16]).unwrap();
        let lethal = TerminalCandidate::from_server_plan(
            binding,
            [40; 16],
            [41; 16],
            [42; 32],
            [43; 32],
            6,
            112,
            TerminalKind::LethalDeath,
        )
        .unwrap();
        let mut coordinator =
            CoreTerminalCoordinator::new(authenticated(), binding).expect("coordinator");
        for producer in CoreTerminalProducer::ALL {
            let evaluation = match producer {
                CoreTerminalProducer::LethalHealth => {
                    CoreTerminalEvaluation::candidate(producer, binding, 112, 6, lethal.clone())
                }
                CoreTerminalProducer::EmergencyRecall => recall.clone(),
                _ => CoreTerminalEvaluation::absent(producer, binding, 112, 6),
            };
            coordinator.evaluate(evaluation).unwrap();
        }
        let CoreTerminalTickSeal::Prepared(prepared) =
            coordinator.seal_authoritative_tick(112, 6).unwrap()
        else {
            panic!("terminal expected");
        };
        assert_eq!(prepared.winner().kind(), TerminalKind::LethalDeath);
    }

    #[tokio::test]
    async fn identities_are_deterministic_trigger_separated_and_client_tick_independent() {
        let first = FakePlanner::fresh();
        let mut channel = ProductionRecallChannel::new(FixedClock(50));
        channel.handle(
            authenticated(),
            start_authority(100),
            &frame(7, 1, RecallIntentV1::Start),
        );
        channel
            .evaluate_explicit_tick(&first, &completion(112))
            .await
            .unwrap();
        let explicit = first.last_request();

        let second = FakePlanner::fresh();
        let link_lost = ProductionLinkLostRecallAuthorityV1 {
            completion: completion(190),
            lost_tick: 100,
            issued_at_unix_ms: 50,
        };
        evaluate_link_lost_tick(&second, &link_lost).await.unwrap();
        let automatic = second.last_request();
        assert_ne!(explicit.mutation_id, automatic.mutation_id);
        assert_ne!(explicit.terminal_id, automatic.terminal_id);

        let repeated = FakePlanner::fresh();
        let mut repeated_channel = ProductionRecallChannel::new(FixedClock(50));
        repeated_channel.handle(
            authenticated(),
            start_authority(100),
            &frame(7, 9_999, RecallIntentV1::Start),
        );
        repeated_channel
            .evaluate_explicit_tick(&repeated, &completion(112))
            .await
            .unwrap();
        let repeated = repeated.last_request();
        assert_eq!(explicit.mutation_id, repeated.mutation_id);
        assert_eq!(explicit.terminal_id, repeated.terminal_id);
        assert_ne!(
            explicit.canonical_hash().unwrap(),
            repeated.canonical_hash().unwrap(),
            "client tick is diagnostic identity material even though the server terminal IDs remain stable"
        );
    }

    #[test]
    fn malformed_foreign_and_zero_clock_requests_fail_closed() {
        let mut channel = ProductionRecallChannel::new(FixedClock(50));
        let mut malformed = frame(7, 100, RecallIntentV1::Start);
        malformed.client_tick = 0;
        assert!(matches!(
            channel.handle(authenticated(), start_authority(100), &malformed),
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::InvalidRequest,
                ..
            }
        ));

        let oversized_tick = frame(7, MAX_DURABLE_CLIENT_TICK + 1, RecallIntentV1::Start);
        assert!(matches!(
            channel.handle(authenticated(), start_authority(100), &oversized_tick),
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::InvalidRequest,
                ..
            }
        ));

        let production = AuthenticatedAccount {
            account_id: authenticated().account_id,
            namespace: AuthenticatedNamespace::Production,
        };
        assert!(matches!(
            channel.handle(
                production,
                start_authority(100),
                &frame(7, 100, RecallIntentV1::Start),
            ),
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::ForeignAuthority,
                ..
            }
        ));

        let mut zero_clock = ProductionRecallChannel::new(FixedClock(0));
        assert!(matches!(
            zero_clock.handle(
                authenticated(),
                start_authority(100),
                &frame(7, 100, RecallIntentV1::Start),
            ),
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::DatabaseUnavailable,
                ..
            }
        ));
    }
}
