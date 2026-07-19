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
    CharacterLocation, RecallFrameV1, RecallIntentV1, RecallResultV1, SafeArrival,
    TERMINAL_HALL_CONTENT_ID, TERMINAL_INVENTORY_SCHEMA_VERSION, TERMINAL_MATERIAL_CAPACITY,
    TERMINAL_PENDING_ITEM_CAPACITY, TerminalInventoryRejectionCodeV1,
};
use thiserror::Error;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreRecallIntentAuthority, CoreRecallIntentReply,
    CoreTerminalEvaluation, CoreTerminalProducer, ProductionRecallPublishedV1,
    RecoveredProductionRecallActorV1, TerminalBinding, production_recall_terminal_candidate,
    validate_published_recall_receipt,
};

pub const PRODUCTION_RECALL_MOVEMENT_BASIS_POINTS: u16 =
    sim_core::EMERGENCY_RECALL_MOVEMENT_BASIS_POINTS;
pub const CORE_RECALL_ACTOR_MAILBOX_CAPACITY: usize = 8;

const MUTATION_ID_CONTEXT: &str = "gravebound.production-recall-channel-mutation.v1";
const TERMINAL_ID_CONTEXT: &str = "gravebound.production-recall-channel-terminal.v1";
const MAX_DURABLE_CLIENT_TICK: u64 = 9_223_372_036_854_775_807;

#[derive(Debug, Clone)]
pub struct CoreRecallActorHandle {
    sender: mpsc::Sender<CoreRecallActorCommand>,
}

#[derive(Debug)]
pub struct CoreRecallActorInbox {
    receiver: mpsc::Receiver<CoreRecallActorCommand>,
}

#[derive(Debug)]
struct CoreRecallActorCommand {
    authenticated: AuthenticatedAccount,
    frame: RecallFrameV1,
    fallback_server_tick: u64,
    reply: oneshot::Sender<CoreRecallIntentReply>,
}

/// Creates one bounded mailbox for one selected character actor. The handle is transport-safe;
/// the inbox remains with the serialized gameplay actor and is retired with it.
#[must_use]
pub fn production_recall_actor_mailbox() -> (CoreRecallActorHandle, CoreRecallActorInbox) {
    let (sender, receiver) = mpsc::channel(CORE_RECALL_ACTOR_MAILBOX_CAPACITY);
    (
        CoreRecallActorHandle { sender },
        CoreRecallActorInbox { receiver },
    )
}

impl CoreRecallIntentAuthority for CoreRecallActorHandle {
    #[allow(
        clippy::manual_async_fn,
        reason = "the desugared public trait contract guarantees a Send future for spawned QUIC workers"
    )]
    fn handle_recall<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a RecallFrameV1,
        fallback_server_tick: u64,
    ) -> impl Future<Output = CoreRecallIntentReply> + Send + 'a {
        async move {
            let rejected = |code| CoreRecallIntentReply {
                server_tick: fallback_server_tick,
                result: RecallResultV1::Rejected {
                    schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
                    request_sequence: frame.sequence,
                    character_id: frame.character_id,
                    code,
                },
            };
            if frame.validate().is_err() || frame.client_tick > MAX_DURABLE_CLIENT_TICK {
                return rejected(TerminalInventoryRejectionCodeV1::InvalidRequest);
            }
            if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
                return rejected(TerminalInventoryRejectionCodeV1::ForeignAuthority);
            }

            let (reply, response) = oneshot::channel();
            let command = CoreRecallActorCommand {
                authenticated,
                frame: *frame,
                fallback_server_tick,
                reply,
            };
            if self.sender.send(command).await.is_err() {
                return rejected(TerminalInventoryRejectionCodeV1::SourceUnavailable);
            }
            response
                .await
                .unwrap_or_else(|_| rejected(TerminalInventoryRejectionCodeV1::SourceUnavailable))
        }
    }
}

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
    pub(crate) fn binding(&self) -> Result<TerminalBinding, ProductionRecallChannelError> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionLinkLostSeedV1 {
    pub lost_tick: u64,
    pub issued_at_unix_ms: u64,
}

impl ProductionLinkLostSeedV1 {
    fn completion_tick(self) -> Result<u64, ProductionRecallChannelError> {
        if self.lost_tick == 0 || self.issued_at_unix_ms == 0 {
            return Err(ProductionRecallChannelError::InvalidServerAuthority);
        }
        self.lost_tick
            .checked_add(persistence::PRODUCTION_RECALL_LINK_LOST_TICKS)
            .ok_or(ProductionRecallChannelError::InvalidServerAuthority)
    }
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
pub struct ProductionRecallProducerEvaluation {
    pub evaluation: CoreTerminalEvaluation,
    pub prepared: Option<Box<PreparedProductionRecallV1>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionRecallTickBundle {
    Evaluated {
        emergency: Box<ProductionRecallProducerEvaluation>,
        disconnect: Box<ProductionRecallProducerEvaluation>,
    },
    CommittedReplay {
        prepared: Box<PreparedProductionRecallV1>,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PinnedTerminalTickV1 {
    server_tick: u64,
    snapshot_hash: Option<[u8; 32]>,
}

#[derive(Debug)]
struct ProductionRecallIntentActorState<Clock> {
    channel: ProductionRecallChannel<Clock>,
    pending: ProductionRecallPendingAuthorityV1,
    link_lost: Option<ProductionLinkLostSeedV1>,
    link_lost_prepared: Option<Box<PreparedProductionRecallV1>>,
    pinned_terminal: Option<PinnedTerminalTickV1>,
    published: Option<ProductionRecallPublishedV1>,
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
                link_lost: None,
                link_lost_prepared: None,
                pinned_terminal: None,
                published: None,
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
        let mut state = self.state.lock().await;
        if let Some(pinned) = state.pinned_terminal {
            return Err(ProductionRecallChannelError::TerminalTickPinned {
                pinned_tick: pinned.server_tick,
            });
        }
        state.pending = pending;
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
        let rejection = |code| RecallResultV1::Rejected {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: frame.sequence,
            character_id: frame.character_id,
            code,
        };
        if frame.validate().is_err() || frame.client_tick > MAX_DURABLE_CLIENT_TICK {
            return rejection(TerminalInventoryRejectionCodeV1::InvalidRequest);
        }
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || authenticated.account_id.as_bytes() != self.account_id
            || frame.character_id != self.character_id
        {
            return rejection(TerminalInventoryRejectionCodeV1::ForeignAuthority);
        }
        if let Some(published) = state.published.as_ref() {
            let exact_explicit_replay = matches!(frame.intent, RecallIntentV1::Start)
                && matches!(
                        &published.result,
                        RecallResultV1::Stored {
                            request_sequence: Some(sequence),
                            ..
                } if *sequence == frame.sequence
                    )
                && published.explicit_client_tick == Some(frame.client_tick);
            if exact_explicit_replay {
                return replayed_published_result(&published.result);
            }
            let code = if matches!(frame.intent, RecallIntentV1::Start)
                && published.explicit_client_tick.is_some()
            {
                TerminalInventoryRejectionCodeV1::IdempotencyConflict
            } else {
                TerminalInventoryRejectionCodeV1::TerminalLost
            };
            return rejection(code);
        }
        if state.pinned_terminal.is_some() {
            return rejection(TerminalInventoryRejectionCodeV1::UnresolvedMutation);
        }
        let authority = ProductionRecallStartAuthorityV1 {
            account_id: self.account_id,
            selected_character_id: self.character_id,
            server_tick,
            pending_item_count: state.pending.pending_item_count,
            pending_material_stack_count: state.pending.pending_material_stack_count,
        };
        state.channel.handle(authenticated, authority, frame)
    }

    /// Captures transport loss inside the serialized actor turn. An exact duplicate is harmless;
    /// altered loss authority fails closed and cannot move the automatic Recall deadline.
    pub async fn enter_link_lost(
        &self,
        lost_tick: u64,
        issued_at_unix_ms: u64,
    ) -> Result<(), ProductionRecallChannelError> {
        let seed = ProductionLinkLostSeedV1 {
            lost_tick,
            issued_at_unix_ms,
        };
        seed.completion_tick()?;
        let mut state = self.state.lock().await;
        if let Some(pinned) = state.pinned_terminal {
            return Err(ProductionRecallChannelError::TerminalTickPinned {
                pinned_tick: pinned.server_tick,
            });
        }
        match state.link_lost {
            None => {
                state.link_lost = Some(seed);
                state.link_lost_prepared = None;
                Ok(())
            }
            Some(active) if active == seed => Ok(()),
            Some(_) => Err(ProductionRecallChannelError::LinkLostAlreadyActive),
        }
    }

    /// Reattaches only before the exact 90-tick deadline. At or after the deadline the terminal
    /// tick must resolve and cannot be erased by a late transport.
    pub async fn reconnect_before_link_lost_deadline(
        &self,
        reconnect_tick: u64,
    ) -> Result<(), ProductionRecallChannelError> {
        if reconnect_tick == 0 {
            return Err(ProductionRecallChannelError::InvalidServerAuthority);
        }
        let mut state = self.state.lock().await;
        let seed = state
            .link_lost
            .ok_or(ProductionRecallChannelError::LinkLostNotActive)?;
        let deadline_tick = seed.completion_tick()?;
        if reconnect_tick >= deadline_tick || state.pinned_terminal.is_some() {
            return Err(ProductionRecallChannelError::LinkLostDeadlineElapsed {
                deadline_tick,
                reconnect_tick,
            });
        }
        state.link_lost = None;
        state.link_lost_prepared = None;
        Ok(())
    }

    #[must_use]
    pub async fn pinned_terminal_tick(&self) -> Option<u64> {
        self.state
            .lock()
            .await
            .pinned_terminal
            .map(|pinned| pinned.server_tick)
    }

    #[must_use]
    pub async fn published_recall(&self) -> Option<ProductionRecallPublishedV1> {
        self.state.lock().await.published.clone()
    }

    /// Installs restart-recovered Recall authority into a newly reconstructed character actor.
    /// Recovery is accepted only before any live channel or `LinkLost` state exists.
    pub async fn restore_committed_recall(
        &self,
        recovered: &RecoveredProductionRecallActorV1,
    ) -> Result<(), ProductionRecallChannelError> {
        let receipt = recovered
            .coordinator
            .committed_receipt()
            .ok_or(ProductionRecallChannelError::InvalidPublishedAuthority)?;
        if receipt.binding().account_id() != &self.account_id
            || receipt.binding().character_id() != &self.character_id
        {
            return Err(ProductionRecallChannelError::InvalidPublishedAuthority);
        }
        validate_published_recall_receipt(&recovered.published, receipt)
            .map_err(|_| ProductionRecallChannelError::InvalidPublishedAuthority)?;
        let (completion_tick, post_world_version) =
            validate_published_recall(&recovered.published, self.character_id)?;
        let mut state = self.state.lock().await;
        if !matches!(state.channel.state, ProductionRecallChannelState::Inactive)
            || state.link_lost.is_some()
            || state.link_lost_prepared.is_some()
            || state.pinned_terminal.is_some()
            || state.published.is_some()
        {
            return Err(ProductionRecallChannelError::InvalidPublishedAuthority);
        }
        validate_published_hall(&recovered.published, self.character_id, post_world_version)?;
        pin_published_terminal_tick(&mut state.pinned_terminal, completion_tick)?;
        state.published = Some(recovered.published.clone());
        Ok(())
    }

    /// Installs the exact committed protocol/Hall projection before the actor is unfrozen or
    /// retired. Exact duplicate publication is idempotent; any differing publication is corrupt.
    pub(crate) async fn publish_recall(
        &self,
        published: ProductionRecallPublishedV1,
    ) -> Result<(), ProductionRecallChannelError> {
        let (completion_tick, post_world_version) =
            validate_published_recall(&published, self.character_id)?;
        validate_published_hall(&published, self.character_id, post_world_version)?;
        let mut state = self.state.lock().await;
        pin_published_terminal_tick(&mut state.pinned_terminal, completion_tick)?;
        match state.published.as_ref() {
            None => {
                state.published = Some(published);
                Ok(())
            }
            Some(existing) if existing == &published => Ok(()),
            Some(_) => Err(ProductionRecallChannelError::InvalidPublishedAuthority),
        }
    }

    /// Pins a non-Recall terminal candidate to the same immutable actor tick before the shared
    /// coordinator is touched. Exact retry is allowed; tick advancement is rejected.
    pub(crate) async fn pin_terminal_snapshot(
        &self,
        server_tick: u64,
        snapshot_hash: [u8; 32],
    ) -> Result<(), ProductionRecallChannelError> {
        if server_tick == 0 || snapshot_hash == [0; 32] {
            return Err(ProductionRecallChannelError::InvalidServerAuthority);
        }
        let mut state = self.state.lock().await;
        pin_terminal_snapshot(&mut state.pinned_terminal, server_tick, snapshot_hash)
    }

    /// Builds both Recall producer evaluations from one actor-owned completion snapshot. A due
    /// explicit or `LinkLost` completion pins the tick before any repository await. Preparation
    /// failure therefore retries the exact same tick rather than changing the gameplay outcome.
    pub async fn evaluate_terminal_tick<Planner>(
        &self,
        planner: &Planner,
        authority: &ProductionRecallCompletionAuthorityV1,
        snapshot_hash: [u8; 32],
    ) -> Result<ProductionRecallTickBundle, ProductionRecallChannelError>
    where
        Clock: ProductionRecallClock,
        Planner: ProductionRecallPlanner,
    {
        if authority.account_id != self.account_id
            || authority.character_id != self.character_id
            || snapshot_hash == [0; 32]
        {
            return Err(ProductionRecallChannelError::BindingMismatch);
        }
        authority.binding()?;

        let mut state = self.state.lock().await;
        if let Some(pinned) = state.pinned_terminal {
            if pinned.server_tick != authority.server_tick {
                return Err(ProductionRecallChannelError::PinnedTickMismatch {
                    expected: pinned.server_tick,
                    actual: authority.server_tick,
                });
            }
            if pinned
                .snapshot_hash
                .is_some_and(|existing| existing != snapshot_hash)
            {
                return Err(ProductionRecallChannelError::PinnedSnapshotMismatch);
            }
        }

        let explicit_due = state.channel.completion_tick() == Some(authority.server_tick);
        let link_lost_due = state
            .link_lost
            .map(ProductionLinkLostSeedV1::completion_tick)
            .transpose()?
            == Some(authority.server_tick);
        if explicit_due && link_lost_due {
            return Err(ProductionRecallChannelError::MultipleRecallCandidates);
        }
        if explicit_due || link_lost_due {
            pin_terminal_snapshot(
                &mut state.pinned_terminal,
                authority.server_tick,
                snapshot_hash,
            )?;
        }

        let explicit = state
            .channel
            .evaluate_explicit_tick(planner, authority)
            .await?;
        if let ProductionRecallTickPreparation::CommittedReplay { prepared } = explicit {
            return Ok(ProductionRecallTickBundle::CommittedReplay { prepared });
        }

        let disconnect = if let Some(seed) = state.link_lost {
            if let Some(prepared) = state.link_lost_prepared.as_ref() {
                prepared_preparation(
                    prepared.as_ref().clone(),
                    authority,
                    CoreTerminalProducer::DisconnectRecovery,
                )?
            } else {
                let prepared = evaluate_link_lost_tick(
                    planner,
                    &ProductionLinkLostRecallAuthorityV1 {
                        completion: authority.clone(),
                        lost_tick: seed.lost_tick,
                        issued_at_unix_ms: seed.issued_at_unix_ms,
                    },
                )
                .await?;
                if let ProductionRecallTickPreparation::Fresh {
                    prepared: fresh, ..
                } = &prepared
                {
                    state.link_lost_prepared = Some(fresh.clone());
                }
                prepared
            }
        } else {
            ProductionRecallTickPreparation::Absent(
                authority.absent(CoreTerminalProducer::DisconnectRecovery)?,
            )
        };
        if let ProductionRecallTickPreparation::CommittedReplay { prepared } = disconnect {
            if explicit.prepared().is_some() {
                return Err(ProductionRecallChannelError::MultipleRecallCandidates);
            }
            return Ok(ProductionRecallTickBundle::CommittedReplay { prepared });
        }

        let emergency = producer_evaluation(explicit)?;
        let disconnect = producer_evaluation(disconnect)?;
        if emergency.prepared.is_some() && disconnect.prepared.is_some() {
            return Err(ProductionRecallChannelError::MultipleRecallCandidates);
        }
        Ok(ProductionRecallTickBundle::Evaluated {
            emergency: Box::new(emergency),
            disconnect: Box::new(disconnect),
        })
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
        let state = self.state.lock().await;
        if state.pinned_terminal.is_some() {
            0
        } else {
            state.channel.movement_basis_points()
        }
    }

    #[must_use]
    pub async fn blocks_combat_interaction_and_consumables(&self) -> bool {
        let state = self.state.lock().await;
        state.pinned_terminal.is_some() || state.channel.blocks_combat_interaction_and_consumables()
    }
}

impl CoreRecallActorInbox {
    /// Prevents new actor commands while allowing the owner to finish or abandon the bounded
    /// commands already queued. Dropping the inbox after this call resolves every waiting
    /// transport request as `SourceUnavailable` through its closed oneshot.
    pub fn close(&mut self) {
        self.receiver.close();
    }

    #[must_use]
    pub fn queued_command_count(&self) -> usize {
        self.receiver.len()
    }

    /// Processes one bounded transport command inside the owning character actor's serialized
    /// turn. The actor supplies the authoritative tick; the transport fallback tick is never used
    /// for an accepted command.
    pub async fn serve_next<Clock>(
        &mut self,
        actor: &ProductionRecallIntentActor<Clock>,
        authoritative_tick: u64,
    ) -> bool
    where
        Clock: ProductionRecallClock,
    {
        self.serve_next_with_tick(actor, || Some(authoritative_tick))
            .await
    }

    /// Resolves the authoritative tick after a command leaves the mailbox. A command can wait
    /// across several simulation ticks, so reading the tick before `recv` would let queue latency
    /// move gameplay authority backward.
    pub async fn serve_next_with_tick<Clock, Tick>(
        &mut self,
        actor: &ProductionRecallIntentActor<Clock>,
        authoritative_tick: Tick,
    ) -> bool
    where
        Clock: ProductionRecallClock,
        Tick: FnOnce() -> Option<u64>,
    {
        let Some(command) = self.receiver.recv().await else {
            return false;
        };
        let Some(authoritative_tick) = authoritative_tick() else {
            let _ = command.reply.send(CoreRecallIntentReply {
                server_tick: command.fallback_server_tick,
                result: RecallResultV1::Rejected {
                    schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
                    request_sequence: command.frame.sequence,
                    character_id: command.frame.character_id,
                    code: TerminalInventoryRejectionCodeV1::SourceUnavailable,
                },
            });
            return true;
        };
        let result = actor
            .handle(command.authenticated, &command.frame, authoritative_tick)
            .await;
        let _ = command.reply.send(CoreRecallIntentReply {
            server_tick: authoritative_tick,
            result,
        });
        true
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
    const fn completion_tick(&self) -> Option<u64> {
        match &self.state {
            ProductionRecallChannelState::Channeling(channel)
            | ProductionRecallChannelState::Prepared { channel, .. } => {
                Some(channel.completion_tick)
            }
            ProductionRecallChannelState::Inactive => None,
        }
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

fn validate_published_recall(
    published: &ProductionRecallPublishedV1,
    character_id: [u8; 16],
) -> Result<(u64, u64), ProductionRecallChannelError> {
    published
        .result
        .validate()
        .map_err(|_| ProductionRecallChannelError::InvalidPublishedAuthority)?;
    match &published.result {
        RecallResultV1::Stored { result, .. } if result.character_id == character_id => {
            Ok((result.completion_tick, result.versions.world.after))
        }
        _ => Err(ProductionRecallChannelError::InvalidPublishedAuthority),
    }
}

fn validate_published_hall(
    published: &ProductionRecallPublishedV1,
    character_id: [u8; 16],
    post_world_version: u64,
) -> Result<(), ProductionRecallChannelError> {
    if published.hall.character_id != character_id
        || published.hall.character_version != post_world_version
        || !matches!(
            &published.hall.location,
            CharacterLocation::Safe {
                location_id,
                arrival: SafeArrival::HallDefault,
            } if location_id.as_str() == TERMINAL_HALL_CONTENT_ID
        )
    {
        return Err(ProductionRecallChannelError::InvalidPublishedAuthority);
    }
    Ok(())
}

fn pin_terminal_snapshot(
    pinned_terminal: &mut Option<PinnedTerminalTickV1>,
    server_tick: u64,
    snapshot_hash: [u8; 32],
) -> Result<(), ProductionRecallChannelError> {
    match *pinned_terminal {
        None => {
            *pinned_terminal = Some(PinnedTerminalTickV1 {
                server_tick,
                snapshot_hash: Some(snapshot_hash),
            });
            Ok(())
        }
        Some(mut pinned) if pinned.server_tick == server_tick => match pinned.snapshot_hash {
            Some(existing) if existing != snapshot_hash => {
                Err(ProductionRecallChannelError::PinnedSnapshotMismatch)
            }
            Some(_) => Ok(()),
            None => {
                pinned.snapshot_hash = Some(snapshot_hash);
                *pinned_terminal = Some(pinned);
                Ok(())
            }
        },
        Some(pinned) => Err(ProductionRecallChannelError::PinnedTickMismatch {
            expected: pinned.server_tick,
            actual: server_tick,
        }),
    }
}

fn pin_published_terminal_tick(
    pinned_terminal: &mut Option<PinnedTerminalTickV1>,
    server_tick: u64,
) -> Result<(), ProductionRecallChannelError> {
    match *pinned_terminal {
        None => {
            *pinned_terminal = Some(PinnedTerminalTickV1 {
                server_tick,
                snapshot_hash: None,
            });
            Ok(())
        }
        Some(pinned) if pinned.server_tick == server_tick => Ok(()),
        Some(pinned) => Err(ProductionRecallChannelError::PinnedTickMismatch {
            expected: pinned.server_tick,
            actual: server_tick,
        }),
    }
}

fn producer_evaluation(
    preparation: ProductionRecallTickPreparation,
) -> Result<ProductionRecallProducerEvaluation, ProductionRecallChannelError> {
    match preparation {
        ProductionRecallTickPreparation::Absent(evaluation) => {
            Ok(ProductionRecallProducerEvaluation {
                evaluation,
                prepared: None,
            })
        }
        ProductionRecallTickPreparation::Fresh {
            prepared,
            evaluation,
        } => Ok(ProductionRecallProducerEvaluation {
            evaluation,
            prepared: Some(prepared),
        }),
        ProductionRecallTickPreparation::CommittedReplay { .. } => {
            Err(ProductionRecallChannelError::InvalidPreparedAuthority)
        }
    }
}

fn replayed_published_result(result: &RecallResultV1) -> RecallResultV1 {
    match result {
        RecallResultV1::Stored {
            schema_version,
            request_sequence,
            result,
            ..
        } => RecallResultV1::Stored {
            schema_version: *schema_version,
            request_sequence: *request_sequence,
            replayed: true,
            result: result.clone(),
        },
        _ => result.clone(),
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
                validate_prepared_request(&prepared, &request)?;
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
    validate_prepared_request(&prepared, &request)?;
    prepared_preparation(prepared, &authority.completion, producer)
}

fn validate_prepared_request(
    prepared: &PreparedProductionRecallV1,
    request: &ProductionRecallCommitRequestV1,
) -> Result<(), ProductionRecallChannelError> {
    prepared
        .validate()
        .map_err(|_| ProductionRecallChannelError::InvalidPreparedAuthority)?;
    let request_hash = request
        .canonical_hash()
        .map_err(ProductionRecallChannelError::Persistence)?;
    if prepared.request() != request || prepared.canonical_request_hash() != request_hash {
        return Err(ProductionRecallChannelError::InvalidPreparedAuthority);
    }
    Ok(())
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
    #[error("a different Recall LinkLost window is already active")]
    LinkLostAlreadyActive,
    #[error("no Recall LinkLost window is active")]
    LinkLostNotActive,
    #[error(
        "Recall LinkLost reconnect reached or exceeded its deadline (deadline {deadline_tick}, reconnect {reconnect_tick})"
    )]
    LinkLostDeadlineElapsed {
        deadline_tick: u64,
        reconnect_tick: u64,
    },
    #[error("terminal tick {pinned_tick} is pinned until commit or recovery")]
    TerminalTickPinned { pinned_tick: u64 },
    #[error("terminal tick is pinned at {expected}, not {actual}")]
    PinnedTickMismatch { expected: u64, actual: u64 },
    #[error("terminal retry changed the immutable actor/producer snapshot")]
    PinnedSnapshotMismatch,
    #[error("explicit and LinkLost Recall produced more than one terminal candidate")]
    MultipleRecallCandidates,
    #[error("committed Recall publication is invalid or conflicts with actor authority")]
    InvalidPublishedAuthority,
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

    const SNAPSHOT_HASH: [u8; 32] = [77; 32];

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
    async fn actor_pins_due_tick_across_prepare_outage_and_freezes_mutations() {
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
        actor
            .handle(
                authenticated(),
                &frame(7, 9_999, RecallIntentV1::Start),
                100,
            )
            .await;
        let planner = FailOncePlanner::new();

        assert!(matches!(
            actor
                .evaluate_terminal_tick(&planner, &completion(112), SNAPSHOT_HASH)
                .await,
            Err(ProductionRecallChannelError::Persistence(
                PersistenceError::InvalidWipeableNamespace
            ))
        ));
        assert_eq!(actor.pinned_terminal_tick().await, Some(112));
        assert_eq!(actor.movement_basis_points().await, 0);
        assert!(actor.blocks_combat_interaction_and_consumables().await);
        assert!(matches!(
            actor
                .handle(
                    authenticated(),
                    &frame(7, 9_999, RecallIntentV1::Start),
                    112,
                )
                .await,
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::UnresolvedMutation,
                ..
            }
        ));
        assert!(matches!(
            actor
                .refresh_pending_authority(ProductionRecallPendingAuthorityV1 {
                    pending_item_count: 4,
                    pending_material_stack_count: 2,
                })
                .await,
            Err(ProductionRecallChannelError::TerminalTickPinned { pinned_tick: 112 })
        ));
        assert!(matches!(
            actor
                .evaluate_terminal_tick(&planner, &completion(113), SNAPSHOT_HASH)
                .await,
            Err(ProductionRecallChannelError::PinnedTickMismatch {
                expected: 112,
                actual: 113
            })
        ));

        let ProductionRecallTickBundle::Evaluated {
            emergency,
            disconnect,
        } = actor
            .evaluate_terminal_tick(&planner, &completion(112), SNAPSHOT_HASH)
            .await
            .unwrap()
        else {
            panic!("same pinned tick must retry fresh preparation");
        };
        assert!(emergency.prepared.is_some());
        assert!(emergency.evaluation.has_candidate());
        assert!(disconnect.prepared.is_none());
        assert!(!disconnect.evaluation.has_candidate());
        assert_eq!(planner.attempts.load(Ordering::SeqCst), 2);
        assert_eq!(planner.delegate.calls(), 1);
    }

    #[tokio::test]
    async fn actor_owns_link_lost_reconnect_and_exact_tick_ninety_preparation() {
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
        actor.enter_link_lost(200, 70).await.unwrap();
        actor.enter_link_lost(200, 70).await.unwrap();
        assert!(matches!(
            actor.enter_link_lost(201, 70).await,
            Err(ProductionRecallChannelError::LinkLostAlreadyActive)
        ));

        let ProductionRecallTickBundle::Evaluated {
            emergency,
            disconnect,
        } = actor
            .evaluate_terminal_tick(&FakePlanner::fresh(), &completion(289), SNAPSHOT_HASH)
            .await
            .unwrap()
        else {
            panic!("tick eighty-nine must remain nonterminal");
        };
        assert!(!emergency.evaluation.has_candidate());
        assert!(!disconnect.evaluation.has_candidate());
        assert_eq!(actor.pinned_terminal_tick().await, None);
        actor
            .reconnect_before_link_lost_deadline(289)
            .await
            .unwrap();
        assert!(matches!(
            actor.reconnect_before_link_lost_deadline(289).await,
            Err(ProductionRecallChannelError::LinkLostNotActive)
        ));

        actor.enter_link_lost(200, 70).await.unwrap();
        let planner = FakePlanner::fresh();
        let ProductionRecallTickBundle::Evaluated {
            emergency,
            disconnect,
        } = actor
            .evaluate_terminal_tick(&planner, &completion(290), SNAPSHOT_HASH)
            .await
            .unwrap()
        else {
            panic!("tick ninety must prepare disconnect recovery");
        };
        assert!(emergency.prepared.is_none());
        assert!(!emergency.evaluation.has_candidate());
        assert!(disconnect.prepared.is_some());
        assert!(disconnect.evaluation.has_candidate());
        assert_eq!(actor.pinned_terminal_tick().await, Some(290));
        assert_eq!(planner.calls(), 1);
        assert!(matches!(
            actor.reconnect_before_link_lost_deadline(290).await,
            Err(ProductionRecallChannelError::LinkLostDeadlineElapsed {
                deadline_tick: 290,
                reconnect_tick: 290
            })
        ));
    }

    #[tokio::test]
    async fn bounded_actor_mailbox_fails_closed_before_or_after_actor_access() {
        assert_eq!(CORE_RECALL_ACTOR_MAILBOX_CAPACITY, 8);
        let (handle, inbox) = production_recall_actor_mailbox();

        let production = AuthenticatedAccount {
            account_id: authenticated().account_id,
            namespace: AuthenticatedNamespace::Production,
        };
        let foreign = handle
            .handle_recall(production, &frame(1, 100, RecallIntentV1::Start), 700)
            .await;
        assert_eq!(foreign.server_tick, 700);
        assert!(matches!(
            foreign.result,
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::ForeignAuthority,
                ..
            }
        ));

        drop(inbox);
        let unavailable = handle
            .handle_recall(authenticated(), &frame(2, 101, RecallIntentV1::Start), 701)
            .await;
        assert_eq!(unavailable.server_tick, 701);
        assert!(matches!(
            unavailable.result,
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::SourceUnavailable,
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
