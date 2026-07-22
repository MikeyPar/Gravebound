//! Lossless acknowledged handoff of committed private-route events.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`SIM-004`, `SIM-010`,
//! `COM-002`, `DTH-001`, and `DTH-010`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-010`, `CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-06`, and `GB-M03-08`).
//!
//! Presentation uses a coalescing `watch` channel. Terminal authority cannot. This capacity-one
//! channel transfers every committed simulation frame and durable route control to one pre-bound
//! terminal owner. It requires an exact acknowledgement before the driver exposes the simulation
//! tick, publishes presentation, returns a control result, or advances.

use std::{num::NonZeroU64, sync::Arc};

use protocol::{
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1,
    CorePrivateRouteSceneV1, CorePrivateRouteStateV1,
};
use sim_core::{
    DeathTraceNetworkState, DeathTraceRecallState, DeathTraceStatus, MAX_DEATH_TRACE_STATUS_TICKS,
    MAX_DEATH_TRACE_STATUSES, Tick, TilePoint,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::{
    CoreBellPortalTransition, CoreDurableB3Resolution, CoreDurableBargainRestResolution,
    CoreDurableCaldusResolution, CorePrivateCaldusRewardCommit, CorePrivateDangerEntryAuthority,
    CorePrivateFixedDungeonB3RewardCommit, CorePrivateFixedDungeonRestCommit,
    CorePrivateMicrorealmFaultKind, CorePrivatePlayerDamageFactV1, CorePrivateRouteActorLease,
    StoredTerminalReceipt, TerminalBinding, TerminalKind,
};

const FEED_CAPACITY: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateTerminalSceneV1 {
    Microrealm,
    FixedDungeon,
    Caldus,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorePrivateTerminalFrameV1 {
    pub delivery_sequence: NonZeroU64,
    pub tick: Tick,
    pub route: CorePrivateRouteStateV1,
    pub player_position: TilePoint,
    pub context: CorePrivateTerminalTickContextV1,
    pub damage: Arc<[CorePrivatePlayerDamageFactV1]>,
    pub player_died: bool,
}

/// Terminal context sampled at the same pre-simulation boundary as retained player input.
///
/// Core currently has no authoritative player-status system. The status collection therefore
/// remains empty in production until that authority exists, while the bounded field preserves the
/// exact append-only death-trace contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateTerminalTickContextV1 {
    pub network_state: DeathTraceNetworkState,
    pub recall_state: DeathTraceRecallState,
    pub statuses: Arc<[DeathTraceStatus]>,
}

impl Default for CorePrivateTerminalTickContextV1 {
    fn default() -> Self {
        Self {
            network_state: DeathTraceNetworkState::Connected,
            recall_state: DeathTraceRecallState::Inactive,
            statuses: Arc::from([]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateTerminalFrameDisposition {
    Continue,
    TerminalOwned { kind: TerminalKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateTerminalRouteControlKindV1 {
    BellDungeonEntered,
    FixedDungeonAdvanced,
    B3RewardCommitted,
    B4RestResolved,
    CaldusRewardCommitted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorePrivateTerminalRouteControlAuthorityV1 {
    BellDungeonEntered {
        transition: CoreBellPortalTransition,
    },
    FixedDungeonAdvanced {
        transition: sim_content::CoreFixedDungeonAdvance,
    },
    B3RewardCommitted {
        durable: CoreDurableB3Resolution,
        commit: CorePrivateFixedDungeonB3RewardCommit,
    },
    B4RestResolved {
        durable: CoreDurableBargainRestResolution,
        commit: CorePrivateFixedDungeonRestCommit,
    },
    CaldusRewardCommitted {
        durable: CoreDurableCaldusResolution,
        commit: CorePrivateCaldusRewardCommit,
    },
}

impl CorePrivateTerminalRouteControlAuthorityV1 {
    #[must_use]
    pub const fn kind(&self) -> CorePrivateTerminalRouteControlKindV1 {
        match self {
            Self::BellDungeonEntered { .. } => {
                CorePrivateTerminalRouteControlKindV1::BellDungeonEntered
            }
            Self::FixedDungeonAdvanced { .. } => {
                CorePrivateTerminalRouteControlKindV1::FixedDungeonAdvanced
            }
            Self::B3RewardCommitted { .. } => {
                CorePrivateTerminalRouteControlKindV1::B3RewardCommitted
            }
            Self::B4RestResolved { .. } => CorePrivateTerminalRouteControlKindV1::B4RestResolved,
            Self::CaldusRewardCommitted { .. } => {
                CorePrivateTerminalRouteControlKindV1::CaldusRewardCommitted
            }
        }
    }

    fn route(&self) -> Option<&CorePrivateRouteStateV1> {
        match self {
            Self::BellDungeonEntered { .. } | Self::FixedDungeonAdvanced { .. } => None,
            Self::B3RewardCommitted { commit, .. } => Some(&commit.route),
            Self::B4RestResolved { commit, .. } => Some(&commit.route),
            Self::CaldusRewardCommitted { commit, .. } => Some(&commit.route),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateTerminalRouteControlV1 {
    pub delivery_sequence: NonZeroU64,
    /// Inherited simulation tick; controls do not advance time. Lifetime and permadeath clocks
    /// remain independent continuously advancing authorities owned by the terminal runtime.
    pub simulation_tick: Tick,
    pub authority: CorePrivateTerminalRouteControlAuthorityV1,
    pub route: CorePrivateRouteStateV1,
}

/// A server-owned runtime failure observed after the preceding frame/control was acknowledged.
/// It advances the terminal barrier by one boundary without inventing a simulation frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateTerminalVerifiedFaultV1 {
    pub delivery_sequence: NonZeroU64,
    pub tick: Tick,
    pub route: CorePrivateRouteStateV1,
    pub kind: CorePrivateMicrorealmFaultKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CorePrivateTerminalDeliveryV1 {
    Frame(Box<CorePrivateTerminalFrameV1>),
    RouteControl(Box<CorePrivateTerminalRouteControlV1>),
    VerifiedFault(Box<CorePrivateTerminalVerifiedFaultV1>),
}

impl CorePrivateTerminalDeliveryV1 {
    const fn delivery_sequence(&self) -> NonZeroU64 {
        match self {
            Self::Frame(frame) => frame.delivery_sequence,
            Self::RouteControl(control) => control.delivery_sequence,
            Self::VerifiedFault(fault) => fault.delivery_sequence,
        }
    }

    const fn tick(&self) -> Tick {
        match self {
            Self::Frame(frame) => frame.tick,
            Self::RouteControl(control) => control.simulation_tick,
            Self::VerifiedFault(fault) => fault.tick,
        }
    }

    const fn route(&self) -> &CorePrivateRouteStateV1 {
        match self {
            Self::Frame(frame) => &frame.route,
            Self::RouteControl(control) => &control.route,
            Self::VerifiedFault(fault) => &fault.route,
        }
    }

    const fn requires_terminal(&self) -> bool {
        matches!(self, Self::VerifiedFault(_))
            || matches!(self, Self::Frame(frame) if frame.player_died)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CorePrivateTerminalFrameAck {
    delivery_sequence: NonZeroU64,
    tick: Tick,
    route_state_version: u64,
    disposition: CorePrivateTerminalFrameDisposition,
}

#[derive(Debug)]
struct CorePrivateTerminalFrameRequest {
    delivery: CorePrivateTerminalDeliveryV1,
    acknowledgement_tx: oneshot::Sender<CorePrivateTerminalFrameAck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateTerminalFeedBinding {
    terminal: TerminalBinding,
    route_lease: CorePrivateRouteActorLease,
    content_revision: CorePrivateRouteContentRevisionV1,
}

impl CorePrivateTerminalFeedBinding {
    #[must_use]
    pub fn from_danger_entry(authority: &CorePrivateDangerEntryAuthority) -> Self {
        Self {
            terminal: authority.terminal(),
            route_lease: authority.route_lease(),
            content_revision: authority.route_content_revision().clone(),
        }
    }

    #[cfg(test)]
    pub fn new(
        terminal: TerminalBinding,
        route_lease: CorePrivateRouteActorLease,
        content_revision: CorePrivateRouteContentRevisionV1,
        entry_restore_point_id: [u8; 16],
    ) -> Result<Self, CorePrivateTerminalFeedError> {
        if route_lease.account_id() != *terminal.account_id()
            || route_lease.character_id() != *terminal.character_id()
            || entry_restore_point_id != *terminal.restore_point_id()
            || content_revision.validate().is_err()
        {
            return Err(CorePrivateTerminalFeedError::InvalidBinding);
        }
        Ok(Self {
            terminal,
            route_lease,
            content_revision,
        })
    }

    #[must_use]
    pub const fn terminal(&self) -> TerminalBinding {
        self.terminal
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CorePrivateTerminalFeedCursor {
    tick: Tick,
    route: Option<CorePrivateRouteStateV1>,
    equal_version_control: Option<CorePrivateTerminalRouteControlKindV1>,
}

#[derive(Debug)]
pub struct CorePrivateTerminalFrameSender {
    binding: Option<CorePrivateTerminalFeedBinding>,
    sender: Option<mpsc::Sender<CorePrivateTerminalFrameRequest>>,
    next_delivery_sequence: u64,
    cursor: CorePrivateTerminalFeedCursor,
}

impl CorePrivateTerminalFrameSender {
    #[must_use]
    pub const fn unbound() -> Self {
        Self {
            binding: None,
            sender: None,
            next_delivery_sequence: 1,
            cursor: CorePrivateTerminalFeedCursor {
                tick: Tick(0),
                route: None,
                equal_version_control: None,
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn resume_cursor_for_test(&mut self, tick: Tick, route: CorePrivateRouteStateV1) {
        self.cursor = CorePrivateTerminalFeedCursor {
            tick,
            route: Some(route),
            equal_version_control: None,
        };
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub async fn deliver(
        &mut self,
        scene: CorePrivateTerminalSceneV1,
        route: CorePrivateRouteStateV1,
        tick: Tick,
        player_position: TilePoint,
        damage: Vec<CorePrivatePlayerDamageFactV1>,
        player_died: bool,
    ) -> Result<CorePrivateTerminalFrameDisposition, CorePrivateTerminalFeedError> {
        self.deliver_with_context(
            scene,
            route,
            tick,
            player_position,
            CorePrivateTerminalTickContextV1::default(),
            damage,
            player_died,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn deliver_with_context(
        &mut self,
        scene: CorePrivateTerminalSceneV1,
        route: CorePrivateRouteStateV1,
        tick: Tick,
        player_position: TilePoint,
        context: CorePrivateTerminalTickContextV1,
        damage: Vec<CorePrivatePlayerDamageFactV1>,
        player_died: bool,
    ) -> Result<CorePrivateTerminalFrameDisposition, CorePrivateTerminalFeedError> {
        let Some(binding) = self.binding.as_ref() else {
            return if damage.is_empty() && !player_died {
                Ok(CorePrivateTerminalFrameDisposition::Continue)
            } else {
                Err(CorePrivateTerminalFeedError::Unbound)
            };
        };
        validate_frame(
            binding,
            scene,
            &route,
            tick,
            &context,
            &damage,
            player_died,
            &self.cursor,
        )?;
        let delivery_sequence = NonZeroU64::new(self.next_delivery_sequence)
            .ok_or(CorePrivateTerminalFeedError::SequenceExhausted)?;
        let frame = CorePrivateTerminalFrameV1 {
            delivery_sequence,
            tick,
            route,
            player_position,
            context,
            damage: damage.into(),
            player_died,
        };
        self.deliver_validated(CorePrivateTerminalDeliveryV1::Frame(Box::new(frame)))
            .await
    }

    pub async fn deliver_route_control(
        &mut self,
        authority: CorePrivateTerminalRouteControlAuthorityV1,
        route: CorePrivateRouteStateV1,
        tick: Tick,
    ) -> Result<CorePrivateTerminalFrameDisposition, CorePrivateTerminalFeedError> {
        let Some(binding) = self.binding.as_ref() else {
            return Ok(CorePrivateTerminalFrameDisposition::Continue);
        };
        validate_route_control(binding, &authority, &route, tick, &self.cursor)?;
        let delivery_sequence = NonZeroU64::new(self.next_delivery_sequence)
            .ok_or(CorePrivateTerminalFeedError::SequenceExhausted)?;
        self.deliver_validated(CorePrivateTerminalDeliveryV1::RouteControl(Box::new(
            CorePrivateTerminalRouteControlV1 {
                delivery_sequence,
                simulation_tick: tick,
                authority,
                route,
            },
        )))
        .await
    }

    /// Delivers one typed, server-observed fault at the next unsealed terminal boundary.
    /// The last acknowledged route is the only authority accepted; callers cannot provide a
    /// replacement route, tick, character, or restore point.
    pub async fn deliver_verified_fault(
        &mut self,
        kind: CorePrivateMicrorealmFaultKind,
    ) -> Result<CorePrivateTerminalFrameDisposition, CorePrivateTerminalFeedError> {
        if matches!(
            kind,
            CorePrivateMicrorealmFaultKind::TerminalAuthority
                | CorePrivateMicrorealmFaultKind::IndeterminateAuthority
        ) {
            return Err(CorePrivateTerminalFeedError::InvalidVerifiedFault);
        }
        let Some(binding) = self.binding.as_ref() else {
            return Ok(CorePrivateTerminalFrameDisposition::Continue);
        };
        let route = self
            .cursor
            .route
            .clone()
            .ok_or(CorePrivateTerminalFeedError::FaultBeforeFirstFrame)?;
        validate_bound_route(binding, &route)?;
        let tick = self
            .cursor
            .tick
            .checked_next()
            .ok_or(CorePrivateTerminalFeedError::SequenceExhausted)?;
        let delivery_sequence = NonZeroU64::new(self.next_delivery_sequence)
            .ok_or(CorePrivateTerminalFeedError::SequenceExhausted)?;
        self.deliver_validated(CorePrivateTerminalDeliveryV1::VerifiedFault(Box::new(
            CorePrivateTerminalVerifiedFaultV1 {
                delivery_sequence,
                tick,
                route,
                kind,
            },
        )))
        .await
    }

    async fn deliver_validated(
        &mut self,
        delivery: CorePrivateTerminalDeliveryV1,
    ) -> Result<CorePrivateTerminalFrameDisposition, CorePrivateTerminalFeedError> {
        let delivery_sequence = delivery.delivery_sequence();
        let tick = delivery.tick();
        let route_state_version = delivery.route().state_version;
        let disposition_contract = match &delivery {
            CorePrivateTerminalDeliveryV1::Frame(frame) if frame.player_died => 0_u8,
            CorePrivateTerminalDeliveryV1::Frame(_) => 1,
            CorePrivateTerminalDeliveryV1::RouteControl(_) => 2,
            CorePrivateTerminalDeliveryV1::VerifiedFault(_) => 3,
        };
        let route = delivery.route().clone();
        let equal_version_control = match &delivery {
            CorePrivateTerminalDeliveryV1::RouteControl(control)
                if self
                    .cursor
                    .route
                    .as_ref()
                    .is_some_and(|previous| previous.state_version == route_state_version) =>
            {
                Some(control.authority.kind())
            }
            CorePrivateTerminalDeliveryV1::VerifiedFault(_) => self.cursor.equal_version_control,
            _ if self
                .cursor
                .route
                .as_ref()
                .is_some_and(|previous| previous.state_version == route_state_version) =>
            {
                self.cursor.equal_version_control
            }
            _ => None,
        };
        let sender = self
            .sender
            .as_ref()
            .ok_or(CorePrivateTerminalFeedError::Unbound)?;
        let (acknowledgement_tx, acknowledgement_rx) = oneshot::channel();
        sender
            .send(CorePrivateTerminalFrameRequest {
                delivery,
                acknowledgement_tx,
            })
            .await
            .map_err(|_| CorePrivateTerminalFeedError::Closed)?;
        let acknowledgement = acknowledgement_rx
            .await
            .map_err(|_| CorePrivateTerminalFeedError::AcknowledgementDropped)?;
        if acknowledgement.delivery_sequence != delivery_sequence
            || acknowledgement.tick != tick
            || acknowledgement.route_state_version != route_state_version
        {
            return Err(CorePrivateTerminalFeedError::AcknowledgementMismatch);
        }
        let valid_disposition = matches!(
            (disposition_contract, acknowledgement.disposition),
            (
                0,
                CorePrivateTerminalFrameDisposition::TerminalOwned {
                    kind: TerminalKind::LethalDeath,
                }
            ) | (1 | 2, CorePrivateTerminalFrameDisposition::Continue)
                | (
                    1 | 3,
                    CorePrivateTerminalFrameDisposition::TerminalOwned {
                        kind: TerminalKind::SuccessfulExtraction
                            | TerminalKind::EmergencyRecall
                            | TerminalKind::DisconnectRecovery
                            | TerminalKind::VerifiedServerFaultRestoration,
                    }
                )
        );
        if !valid_disposition {
            return Err(CorePrivateTerminalFeedError::InvalidDisposition);
        }
        self.cursor = CorePrivateTerminalFeedCursor {
            tick,
            route: Some(route),
            equal_version_control,
        };
        self.next_delivery_sequence = self
            .next_delivery_sequence
            .checked_add(1)
            .ok_or(CorePrivateTerminalFeedError::SequenceExhausted)?;
        Ok(acknowledgement.disposition)
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive authority match keeps every control variant's pre/post-route contract adjacent"
)]
fn validate_route_control(
    binding: &CorePrivateTerminalFeedBinding,
    authority: &CorePrivateTerminalRouteControlAuthorityV1,
    route: &CorePrivateRouteStateV1,
    tick: Tick,
    cursor: &CorePrivateTerminalFeedCursor,
) -> Result<(), CorePrivateTerminalFeedError> {
    validate_bound_route(binding, route)?;
    let previous = cursor
        .route
        .as_ref()
        .ok_or(CorePrivateTerminalFeedError::ControlBeforeFirstFrame)?;
    if tick != cursor.tick || route.state_version < previous.state_version {
        return Err(CorePrivateTerminalFeedError::IncoherentRouteControl);
    }
    if route.state_version == previous.state_version && cursor.equal_version_control.is_some() {
        return Err(CorePrivateTerminalFeedError::DuplicateRouteControl);
    }
    if route.schema_version != previous.schema_version
        || route.character_id != previous.character_id
        || route.content_revision != previous.content_revision
        || route.actor_generation != previous.actor_generation
        || route.instance_lineage_id != previous.instance_lineage_id
    {
        return Err(CorePrivateTerminalFeedError::ForeignBinding);
    }
    if !matches!(
        authority,
        CorePrivateTerminalRouteControlAuthorityV1::BellDungeonEntered { .. }
    ) && route.character_version != previous.character_version
    {
        return Err(CorePrivateTerminalFeedError::ForeignBinding);
    }
    if authority
        .route()
        .is_some_and(|committed| committed != route)
    {
        return Err(CorePrivateTerminalFeedError::IncoherentRouteControl);
    }
    let coherent = match authority {
        CorePrivateTerminalRouteControlAuthorityV1::BellDungeonEntered { transition } => {
            previous.scene == CorePrivateRouteSceneV1::CoreMicrorealm
                && previous.phase == CorePrivateRoutePhaseV1::MicrorealmCleared
                && route.scene == CorePrivateRouteSceneV1::BellSepulcher
                && route.room == Some(CorePrivateRouteRoomV1::BellVestibuleB0)
                && route.phase == CorePrivateRoutePhaseV1::DungeonVestibule
                && previous.state_version.checked_add(1) == Some(route.state_version)
                && transition.binding.account_id == *binding.terminal.account_id()
                && transition.binding.character_id == *binding.terminal.character_id()
                && transition.binding.instance_lineage_id == *binding.terminal.lineage_id()
                && transition.binding.entry_restore_point_id == *binding.terminal.restore_point_id()
                && transition.binding.character_version == previous.character_version
                && transition.destination_character_version == route.character_version
                && transition.transfer_id.iter().any(|byte| *byte != 0)
                && transition.binding.mutation_id.iter().any(|byte| *byte != 0)
        }
        CorePrivateTerminalRouteControlAuthorityV1::FixedDungeonAdvanced { transition } => {
            previous.scene == CorePrivateRouteSceneV1::BellSepulcher
                && route.scene == CorePrivateRouteSceneV1::BellSepulcher
                && (route.room != previous.room || route.phase != previous.phase)
                && previous.state_version.checked_add(1) == Some(route.state_version)
                && Some(fixed_room_for_node(transition.from)) == previous.room
                && Some(fixed_room_for_node(transition.to)) == route.room
        }
        CorePrivateTerminalRouteControlAuthorityV1::B3RewardCommitted { durable, commit } => {
            previous.room == Some(CorePrivateRouteRoomV1::BellKnightB3)
                && route.room == previous.room
                && route.phase == CorePrivateRoutePhaseV1::RoomCleared
                && route.state_version == previous.state_version
                && durable.account_id() == *binding.terminal.account_id()
                && durable.character_id() == *binding.terminal.character_id()
                && durable.instance_lineage_id() == *binding.terminal.lineage_id()
                && durable.reward_event_id() == commit.reward_event_id
                && durable.reward_result_hash() == commit.reward_result_hash
                && durable.progression_payload_hash() == commit.progression_payload_hash
                && durable.disposition() == commit.disposition
                && durable.bargain_offer_id() == commit.bargain_offer_id
                && durable.has_no_offer_resolution() == commit.has_no_offer_resolution
        }
        CorePrivateTerminalRouteControlAuthorityV1::B4RestResolved { durable, commit } => {
            previous.room == Some(CorePrivateRouteRoomV1::BellRestB4)
                && route.room == previous.room
                && route.phase == CorePrivateRoutePhaseV1::Rest
                && route.state_version == previous.state_version
                && durable.account_id() == *binding.terminal.account_id()
                && durable.character_id() == *binding.terminal.character_id()
                && durable.instance_lineage_id() == *binding.terminal.lineage_id()
                && durable.entry_restore_point_id() == *binding.terminal.restore_point_id()
                && durable.source_receipt_id() == commit.source_receipt_id
                && durable.offer_id() == commit.offer_id
                && durable.oath_bargain_version() == commit.oath_bargain_version
                && durable.resolution() == commit.resolution
        }
        CorePrivateTerminalRouteControlAuthorityV1::CaldusRewardCommitted { durable, commit } => {
            let handoff = durable.handoff();
            previous.room == Some(CorePrivateRouteRoomV1::CaldusArenaB6)
                && previous.phase == CorePrivateRoutePhaseV1::BossDefeated
                && route.room == previous.room
                && route.phase == CorePrivateRoutePhaseV1::BossExitReady
                && previous.state_version.checked_add(1) == Some(route.state_version)
                && handoff.route_lease().account_id() == *binding.terminal.account_id()
                && handoff.character_id() == *binding.terminal.character_id()
                && handoff.instance_lineage_id() == *binding.terminal.lineage_id()
                && handoff.entry_restore_point_id() == *binding.terminal.restore_point_id()
                && handoff.route_state_version() == previous.state_version
                && handoff.defeat_tick() == tick
                && durable.exit().exit_instance_id == commit.exit.exit_instance_id
                && commit.disposition == crate::CorePrivateCaldusRewardCommitDisposition::Committed
        }
    };
    if coherent {
        Ok(())
    } else {
        Err(CorePrivateTerminalFeedError::IncoherentRouteControl)
    }
}

fn fixed_room_for_node(node: sim_content::CoreFixedDungeonNode) -> CorePrivateRouteRoomV1 {
    use sim_content::CoreFixedDungeonNode as Node;
    match node {
        Node::BellVestibuleB0 => CorePrivateRouteRoomV1::BellVestibuleB0,
        Node::BellCrossB1 => CorePrivateRouteRoomV1::BellCrossB1,
        Node::BellNaveB2 => CorePrivateRouteRoomV1::BellNaveB2,
        Node::BellKnightB3 => CorePrivateRouteRoomV1::BellKnightB3,
        Node::BellRestB4 => CorePrivateRouteRoomV1::BellRestB4,
        Node::BellBridgeB5 => CorePrivateRouteRoomV1::BellBridgeB5,
        Node::CaldusArenaB6 => CorePrivateRouteRoomV1::CaldusArenaB6,
    }
}

fn validate_bound_route(
    binding: &CorePrivateTerminalFeedBinding,
    route: &CorePrivateRouteStateV1,
) -> Result<(), CorePrivateTerminalFeedError> {
    route
        .validate()
        .map_err(|_| CorePrivateTerminalFeedError::InvalidRoute)?;
    if binding.route_lease.account_id() != *binding.terminal.account_id()
        || binding.route_lease.character_id() != *binding.terminal.character_id()
        || route.character_id != binding.route_lease.character_id()
        || route.actor_generation != binding.route_lease.actor_generation()
        || route.content_revision != binding.content_revision
        || route.character_id != *binding.terminal.character_id()
        || route.instance_lineage_id != Some(*binding.terminal.lineage_id())
    {
        return Err(CorePrivateTerminalFeedError::ForeignBinding);
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "all values form one indivisible frame authority validated before allocation or handoff"
)]
fn validate_frame(
    binding: &CorePrivateTerminalFeedBinding,
    scene: CorePrivateTerminalSceneV1,
    route: &CorePrivateRouteStateV1,
    tick: Tick,
    context: &CorePrivateTerminalTickContextV1,
    damage: &[CorePrivatePlayerDamageFactV1],
    player_died: bool,
    cursor: &CorePrivateTerminalFeedCursor,
) -> Result<(), CorePrivateTerminalFeedError> {
    validate_bound_route(binding, route)?;
    validate_tick_context(context)?;
    if tick.0 == 0 || tick.0 != cursor.tick.0.saturating_add(1) {
        return Err(CorePrivateTerminalFeedError::NonSequentialTick);
    }
    if let Some(previous) = &cursor.route {
        if route.state_version < previous.state_version {
            return Err(CorePrivateTerminalFeedError::RouteVersionRegression);
        }
        if route.state_version == previous.state_version && route != previous {
            return Err(CorePrivateTerminalFeedError::EqualVersionRouteDrift);
        }
    }
    validate_scene(scene, route)?;
    let target = damage.first().map(|fact| fact.target_entity_id);
    for (index, fact) in damage.iter().enumerate() {
        let ordinal =
            u32::try_from(index).map_err(|_| CorePrivateTerminalFeedError::CapacityExceeded)?;
        if fact.tick != tick
            || fact.event_ordinal != ordinal
            || Some(fact.target_entity_id) != target
            || (index > 0 && damage[index - 1].post_health != fact.pre_health)
        {
            return Err(CorePrivateTerminalFeedError::IncoherentDamage);
        }
    }
    if damage.iter().filter(|fact| fact.lethal()).count() != usize::from(player_died)
        || damage
            .iter()
            .position(CorePrivatePlayerDamageFactV1::lethal)
            .is_some_and(|index| index + 1 != damage.len())
    {
        return Err(CorePrivateTerminalFeedError::LethalityMismatch);
    }
    Ok(())
}

fn validate_tick_context(
    context: &CorePrivateTerminalTickContextV1,
) -> Result<(), CorePrivateTerminalFeedError> {
    if context.statuses.len() > MAX_DEATH_TRACE_STATUSES {
        return Err(CorePrivateTerminalFeedError::CapacityExceeded);
    }
    for (index, status) in context.statuses.iter().enumerate() {
        if !is_stable_trace_id(&status.status_id)
            || status.remaining_ticks > MAX_DEATH_TRACE_STATUS_TICKS
            || !(1..=255).contains(&status.stack_count)
            || (index > 0 && context.statuses[index - 1].status_id >= status.status_id)
        {
            return Err(CorePrivateTerminalFeedError::InvalidTickContext);
        }
    }
    Ok(())
}

fn is_stable_trace_id(value: &str) -> bool {
    (3..=96).contains(&value.len())
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'
                        || byte == b'-'
                })
        })
}

fn validate_scene(
    scene: CorePrivateTerminalSceneV1,
    route: &CorePrivateRouteStateV1,
) -> Result<(), CorePrivateTerminalFeedError> {
    let valid = match scene {
        CorePrivateTerminalSceneV1::Microrealm => {
            route.scene == CorePrivateRouteSceneV1::CoreMicrorealm && route.room.is_none()
        }
        CorePrivateTerminalSceneV1::FixedDungeon => {
            route.scene == CorePrivateRouteSceneV1::BellSepulcher
                && matches!(
                    route.room,
                    Some(
                        CorePrivateRouteRoomV1::BellCrossB1
                            | CorePrivateRouteRoomV1::BellNaveB2
                            | CorePrivateRouteRoomV1::BellKnightB3
                            | CorePrivateRouteRoomV1::BellBridgeB5
                    )
                )
        }
        CorePrivateTerminalSceneV1::Caldus => {
            route.scene == CorePrivateRouteSceneV1::BellSepulcher
                && route.room == Some(CorePrivateRouteRoomV1::CaldusArenaB6)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(CorePrivateTerminalFeedError::SceneMismatch)
    }
}

#[derive(Debug)]
pub struct CorePrivateTerminalFrameReceiver {
    binding: TerminalBinding,
    receiver: mpsc::Receiver<CorePrivateTerminalFrameRequest>,
}

impl CorePrivateTerminalFrameReceiver {
    #[must_use]
    pub fn channel(
        binding: CorePrivateTerminalFeedBinding,
    ) -> (CorePrivateTerminalFrameSender, Self) {
        let (sender, receiver) = mpsc::channel(FEED_CAPACITY);
        let terminal = binding.terminal;
        (
            CorePrivateTerminalFrameSender {
                binding: Some(binding),
                sender: Some(sender),
                next_delivery_sequence: 1,
                cursor: CorePrivateTerminalFeedCursor {
                    tick: Tick(0),
                    route: None,
                    equal_version_control: None,
                },
            },
            Self {
                binding: terminal,
                receiver,
            },
        )
    }

    #[must_use]
    pub const fn binding(&self) -> TerminalBinding {
        self.binding
    }

    pub async fn receive(&mut self) -> Option<CorePrivateTerminalFrameDelivery> {
        self.receiver
            .recv()
            .await
            .map(|request| CorePrivateTerminalFrameDelivery {
                delivery: request.delivery,
                binding: self.binding,
                acknowledgement_tx: Some(request.acknowledgement_tx),
            })
    }

    #[cfg(test)]
    pub(crate) fn pending_deliveries(&self) -> usize {
        self.receiver.len()
    }
}

#[derive(Debug)]
pub struct CorePrivateTerminalFrameDelivery {
    delivery: CorePrivateTerminalDeliveryV1,
    binding: TerminalBinding,
    acknowledgement_tx: Option<oneshot::Sender<CorePrivateTerminalFrameAck>>,
}

impl CorePrivateTerminalFrameDelivery {
    #[must_use]
    pub const fn delivery(&self) -> &CorePrivateTerminalDeliveryV1 {
        &self.delivery
    }

    #[must_use]
    pub const fn frame(&self) -> Option<&CorePrivateTerminalFrameV1> {
        match &self.delivery {
            CorePrivateTerminalDeliveryV1::Frame(frame) => Some(frame),
            CorePrivateTerminalDeliveryV1::RouteControl(_)
            | CorePrivateTerminalDeliveryV1::VerifiedFault(_) => None,
        }
    }

    #[must_use]
    pub const fn route_control(&self) -> Option<&CorePrivateTerminalRouteControlV1> {
        match &self.delivery {
            CorePrivateTerminalDeliveryV1::RouteControl(control) => Some(control),
            CorePrivateTerminalDeliveryV1::Frame(_)
            | CorePrivateTerminalDeliveryV1::VerifiedFault(_) => None,
        }
    }

    #[must_use]
    pub const fn verified_fault(&self) -> Option<&CorePrivateTerminalVerifiedFaultV1> {
        match &self.delivery {
            CorePrivateTerminalDeliveryV1::VerifiedFault(fault) => Some(fault),
            CorePrivateTerminalDeliveryV1::Frame(_)
            | CorePrivateTerminalDeliveryV1::RouteControl(_) => None,
        }
    }

    pub fn acknowledge_continue(mut self) -> Result<(), CorePrivateTerminalAcknowledgementError> {
        if self.delivery.requires_terminal() {
            return Err(CorePrivateTerminalAcknowledgementError::InvalidDisposition);
        }
        self.send_acknowledgement(CorePrivateTerminalFrameDisposition::Continue)
    }

    pub fn acknowledge_terminal_owned(
        mut self,
        receipt: &StoredTerminalReceipt,
    ) -> Result<(), CorePrivateTerminalAcknowledgementError> {
        receipt
            .validate()
            .map_err(|_| CorePrivateTerminalAcknowledgementError::InvalidReceipt)?;
        let valid_kind = match &self.delivery {
            CorePrivateTerminalDeliveryV1::Frame(frame) if frame.player_died => {
                receipt.kind() == TerminalKind::LethalDeath
            }
            CorePrivateTerminalDeliveryV1::Frame(_)
            | CorePrivateTerminalDeliveryV1::VerifiedFault(_) => matches!(
                receipt.kind(),
                TerminalKind::SuccessfulExtraction
                    | TerminalKind::EmergencyRecall
                    | TerminalKind::DisconnectRecovery
                    | TerminalKind::VerifiedServerFaultRestoration
            ),
            CorePrivateTerminalDeliveryV1::RouteControl(_) => false,
        };
        if !valid_kind
            || receipt.binding() != self.binding
            || receipt.observed_tick() != self.delivery.tick().0
            || receipt.expected_state_version() != self.delivery.route().character_version
        {
            return Err(CorePrivateTerminalAcknowledgementError::InvalidDisposition);
        }
        self.send_acknowledgement(CorePrivateTerminalFrameDisposition::TerminalOwned {
            kind: receipt.kind(),
        })
    }

    fn send_acknowledgement(
        &mut self,
        disposition: CorePrivateTerminalFrameDisposition,
    ) -> Result<(), CorePrivateTerminalAcknowledgementError> {
        let acknowledgement = CorePrivateTerminalFrameAck {
            delivery_sequence: self.delivery.delivery_sequence(),
            tick: self.delivery.tick(),
            route_state_version: self.delivery.route().state_version,
            disposition,
        };
        self.acknowledgement_tx
            .take()
            .expect("terminal frame delivery owns exactly one acknowledgement")
            .send(acknowledgement)
            .map_err(|_| CorePrivateTerminalAcknowledgementError::DriverGone)
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CorePrivateTerminalFeedError {
    #[error("private terminal-frame feed is not bound")]
    Unbound,
    #[error("private terminal-frame feed is closed")]
    Closed,
    #[error("private terminal-frame acknowledgement was dropped")]
    AcknowledgementDropped,
    #[error("shutdown began with a committed terminal frame unresolved")]
    ShutdownWithUnresolvedFrame,
    #[error("private terminal-frame acknowledgement does not match its delivery")]
    AcknowledgementMismatch,
    #[error("private terminal-frame acknowledgement disposition is invalid")]
    InvalidDisposition,
    #[error("verified server fault cannot precede the first acknowledged route frame")]
    FaultBeforeFirstFrame,
    #[error("verified server fault kind is invalid")]
    InvalidVerifiedFault,
    #[error("private terminal-frame route authority is invalid")]
    InvalidRoute,
    #[error("private terminal-frame owner binding is invalid")]
    InvalidBinding,
    #[error("private terminal-frame binding is foreign")]
    ForeignBinding,
    #[error("private terminal-frame scene does not match route authority")]
    SceneMismatch,
    #[error("private terminal-frame tick is not the next ordered tick")]
    NonSequentialTick,
    #[error("private terminal-frame route state version regressed")]
    RouteVersionRegression,
    #[error("private terminal-frame route changed without a version advance")]
    EqualVersionRouteDrift,
    #[error("private terminal route control arrived before the first simulation frame")]
    ControlBeforeFirstFrame,
    #[error("private terminal route control is incoherent")]
    IncoherentRouteControl,
    #[error("private terminal route control duplicates an acknowledged equal-version event")]
    DuplicateRouteControl,
    #[error("private terminal-frame damage is incoherent")]
    IncoherentDamage,
    #[error("private terminal-frame tick context is invalid")]
    InvalidTickContext,
    #[error("private terminal-frame lethality is incoherent")]
    LethalityMismatch,
    #[error("private terminal-frame capacity was exceeded")]
    CapacityExceeded,
    #[error("private terminal-frame delivery sequence was exhausted")]
    SequenceExhausted,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CorePrivateTerminalAcknowledgementError {
    #[error("private terminal-frame acknowledgement disposition is invalid")]
    InvalidDisposition,
    #[error("private terminal-frame durable receipt is invalid")]
    InvalidReceipt,
    #[error("private terminal-frame driver stopped before acknowledgement")]
    DriverGone,
}
