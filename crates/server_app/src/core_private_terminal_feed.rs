//! Lossless acknowledged handoff of committed private-route frames.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`SIM-004`, `SIM-010`,
//! `COM-002`, `DTH-001`, and `DTH-010`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-010`, `CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-06`, and `GB-M03-08`).
//!
//! Presentation uses a coalescing `watch` channel. Terminal authority cannot. This capacity-one
//! channel transfers every committed simulation frame to one pre-bound terminal owner and requires
//! an exact acknowledgement before the driver exposes the tick, publishes presentation, or advances.

use std::{num::NonZeroU64, sync::Arc};

use protocol::{
    CorePrivateRouteContentRevisionV1, CorePrivateRouteRoomV1, CorePrivateRouteSceneV1,
    CorePrivateRouteStateV1,
};
use sim_core::{Tick, TilePoint};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::{
    CorePrivatePlayerDamageFactV1, CorePrivateRouteActorLease, StoredTerminalReceipt,
    TerminalBinding, TerminalKind,
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
    pub damage: Arc<[CorePrivatePlayerDamageFactV1]>,
    pub player_died: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateTerminalFrameDisposition {
    Continue,
    TerminalOwned { kind: TerminalKind },
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
    frame: CorePrivateTerminalFrameV1,
    acknowledgement_tx: oneshot::Sender<CorePrivateTerminalFrameAck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateTerminalFeedBinding {
    terminal: TerminalBinding,
    route_lease: CorePrivateRouteActorLease,
    content_revision: CorePrivateRouteContentRevisionV1,
}

impl CorePrivateTerminalFeedBinding {
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
            },
        }
    }

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
            damage: damage.into(),
            player_died,
        };
        let sender = self
            .sender
            .as_ref()
            .ok_or(CorePrivateTerminalFeedError::Unbound)?;
        let (acknowledgement_tx, acknowledgement_rx) = oneshot::channel();
        sender
            .send(CorePrivateTerminalFrameRequest {
                frame: frame.clone(),
                acknowledgement_tx,
            })
            .await
            .map_err(|_| CorePrivateTerminalFeedError::Closed)?;
        let acknowledgement = acknowledgement_rx
            .await
            .map_err(|_| CorePrivateTerminalFeedError::AcknowledgementDropped)?;
        if acknowledgement.delivery_sequence != delivery_sequence
            || acknowledgement.tick != tick
            || acknowledgement.route_state_version != frame.route.state_version
        {
            return Err(CorePrivateTerminalFeedError::AcknowledgementMismatch);
        }
        match (player_died, acknowledgement.disposition) {
            (false, CorePrivateTerminalFrameDisposition::Continue)
            | (
                true,
                CorePrivateTerminalFrameDisposition::TerminalOwned {
                    kind: TerminalKind::LethalDeath,
                },
            ) => {}
            _ => return Err(CorePrivateTerminalFeedError::InvalidDisposition),
        }
        self.cursor = CorePrivateTerminalFeedCursor {
            tick,
            route: Some(frame.route.clone()),
        };
        self.next_delivery_sequence = self
            .next_delivery_sequence
            .checked_add(1)
            .ok_or(CorePrivateTerminalFeedError::SequenceExhausted)?;
        Ok(acknowledgement.disposition)
    }
}

fn validate_frame(
    binding: &CorePrivateTerminalFeedBinding,
    scene: CorePrivateTerminalSceneV1,
    route: &CorePrivateRouteStateV1,
    tick: Tick,
    damage: &[CorePrivatePlayerDamageFactV1],
    player_died: bool,
    cursor: &CorePrivateTerminalFeedCursor,
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
                frame: request.frame,
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
    frame: CorePrivateTerminalFrameV1,
    binding: TerminalBinding,
    acknowledgement_tx: Option<oneshot::Sender<CorePrivateTerminalFrameAck>>,
}

impl CorePrivateTerminalFrameDelivery {
    #[must_use]
    pub const fn frame(&self) -> &CorePrivateTerminalFrameV1 {
        &self.frame
    }

    pub fn acknowledge_continue(mut self) -> Result<(), CorePrivateTerminalAcknowledgementError> {
        if self.frame.player_died {
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
        if !self.frame.player_died
            || receipt.kind() != TerminalKind::LethalDeath
            || receipt.binding() != self.binding
            || receipt.observed_tick() != self.frame.tick.0
        {
            return Err(CorePrivateTerminalAcknowledgementError::InvalidDisposition);
        }
        self.send_acknowledgement(CorePrivateTerminalFrameDisposition::TerminalOwned {
            kind: TerminalKind::LethalDeath,
        })
    }

    fn send_acknowledgement(
        &mut self,
        disposition: CorePrivateTerminalFrameDisposition,
    ) -> Result<(), CorePrivateTerminalAcknowledgementError> {
        let acknowledgement = CorePrivateTerminalFrameAck {
            delivery_sequence: self.frame.delivery_sequence,
            tick: self.frame.tick,
            route_state_version: self.frame.route.state_version,
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
    #[error("private terminal-frame damage is incoherent")]
    IncoherentDamage,
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
