//! Deterministic logical-session ownership for `GB-M02-04`.
//!
//! A transport authorizes ingress but never owns the authoritative gameplay aggregate. All time
//! is expressed in the aggregate's 30 Hz ticks so reconnect tests require no sleeping.

use std::{collections::BTreeMap, path::Path};

use protocol::{
    ControlEvent, InputFrame, ReliableEvent, ReliableEventFrame, SessionControlFrame,
    SessionControlRequest, SessionControlResult, SessionControlResultCode, SessionDestination,
    WireMessage, WireText,
};
use sim_core::{AuthorityInput, AuthorityPhase, EntityId, SharedAuthoritativeArena};
use thiserror::Error;

use crate::{AuthoritativeSession, InputDisposition, SessionError};

pub const LINK_LOST_TICKS: u64 = 90;

macro_rules! nonzero_id {
    ($name:ident, $error:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Result<Self, LifecycleError> {
                if value == 0 {
                    Err(LifecycleError::ZeroIdentity($error))
                } else {
                    Ok(Self(value))
                }
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

nonzero_id!(LogicalSessionId, "logical session");
nonzero_id!(SessionOwnerId, "session owner");
nonzero_id!(TransportId, "transport");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    Connected,
    LinkLost { lost_tick: u64, recall_tick: u64 },
    Recalled { committed_tick: u64 },
    Dead { committed_tick: u64 },
    Closed,
}

impl SessionPhase {
    #[must_use]
    pub const fn destination(self) -> SessionDestination {
        match self {
            Self::Connected | Self::LinkLost { .. } => SessionDestination::CombatInstance,
            Self::Recalled { .. } => SessionDestination::LanternHalls,
            Self::Dead { .. } => SessionDestination::DeathFinal,
            Self::Closed => SessionDestination::Closed,
        }
    }

    #[must_use]
    const fn simulation_active(self) -> bool {
        matches!(self, Self::Connected | Self::LinkLost { .. })
    }

    #[must_use]
    pub const fn is_resolved(self) -> bool {
        matches!(
            self,
            Self::Recalled { .. } | Self::Dead { .. } | Self::Closed
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TransportBinding {
    id: TransportId,
    last_control_sequence: u32,
}

#[derive(Debug)]
pub struct ManagedSession {
    id: LogicalSessionId,
    owner: SessionOwnerId,
    transport: Option<TransportBinding>,
    phase: SessionPhase,
    authority: AuthoritativeSession,
}

impl ManagedSession {
    fn new(
        id: LogicalSessionId,
        owner: SessionOwnerId,
        transport: TransportId,
        first_control_sequence: u32,
        authority: AuthoritativeSession,
    ) -> Self {
        Self {
            id,
            owner,
            transport: Some(TransportBinding {
                id: transport,
                last_control_sequence: first_control_sequence,
            }),
            phase: SessionPhase::Connected,
            authority,
        }
    }

    #[must_use]
    pub const fn id(&self) -> LogicalSessionId {
        self.id
    }

    #[must_use]
    pub const fn owner(&self) -> SessionOwnerId {
        self.owner
    }

    #[must_use]
    pub const fn phase(&self) -> SessionPhase {
        self.phase
    }

    #[must_use]
    pub const fn transport_id(&self) -> Option<TransportId> {
        match self.transport {
            Some(binding) => Some(binding.id),
            None => None,
        }
    }

    #[must_use]
    pub const fn is_simulation_active(&self) -> bool {
        self.phase.simulation_active()
    }

    #[must_use]
    pub const fn authority(&self) -> &AuthoritativeSession {
        &self.authority
    }

    pub fn shared_player_id(&self) -> Result<EntityId, LifecycleError> {
        protocol::M02_PLAYER_ENTITY_ID_BASE
            .checked_add(self.id.get().saturating_sub(1))
            .and_then(EntityId::new)
            .ok_or(LifecycleError::PlayerIdentityExhausted)
    }

    pub(crate) fn take_shared_input(&mut self) -> Result<AuthorityInput, LifecycleError> {
        if !self.phase.simulation_active() {
            return Err(LifecycleError::IngressUnavailable);
        }
        self.authority
            .take_shared_authority_input()
            .map_err(Into::into)
    }

    pub(crate) fn encode_shared_snapshots(
        &mut self,
        arena: &SharedAuthoritativeArena,
        server_tick: u64,
    ) -> Result<Vec<protocol::SnapshotChunk>, LifecycleError> {
        let player_id = self.shared_player_id()?;
        self.authority
            .encode_shared_snapshots(
                server_tick,
                arena.state_version(),
                arena.snapshots_for(player_id)?,
            )
            .map_err(Into::into)
    }

    pub(crate) fn resolve_shared_post_simulation(
        &mut self,
        arena: &mut SharedAuthoritativeArena,
        tick: u64,
    ) -> Result<(), LifecycleError> {
        let player_id = self.shared_player_id()?;
        let shared_phase = arena
            .players()
            .get(&player_id)
            .ok_or(LifecycleError::SharedPlayerMissing)?
            .phase();
        if matches!(shared_phase, AuthorityPhase::Dead { .. }) {
            self.phase = SessionPhase::Dead {
                committed_tick: tick,
            };
            self.transport = None;
            return Ok(());
        }
        if let SessionPhase::LinkLost { recall_tick, .. } = self.phase
            && tick >= recall_tick
        {
            let recall = arena.commit_automatic_recall_at(player_id, sim_core::Tick(tick))?;
            self.phase = SessionPhase::Recalled {
                committed_tick: recall.committed_at.0,
            };
            self.transport = None;
        }
        Ok(())
    }

    #[must_use]
    pub fn server_tick(&self) -> u64 {
        self.authority.arena().player().combat.tick().0
    }

    #[must_use]
    pub const fn state_version(&self) -> u64 {
        self.authority.arena().state_version()
    }

    pub fn submit_input(
        &mut self,
        transport: TransportId,
        frame: &InputFrame,
    ) -> Result<InputDisposition, LifecycleError> {
        self.require_active_transport(transport)?;
        if !matches!(self.phase, SessionPhase::Connected) {
            return Err(LifecycleError::IngressUnavailable);
        }
        self.authority.submit_input(frame).map_err(Into::into)
    }

    pub fn handle_gameplay_reliable(
        &mut self,
        transport: TransportId,
        message: WireMessage,
    ) -> Result<WireMessage, LifecycleError> {
        self.require_active_transport(transport)?;
        if !matches!(self.phase, SessionPhase::Connected) {
            return Err(LifecycleError::IngressUnavailable);
        }
        self.authority.handle_reliable(message).map_err(Into::into)
    }

    pub(crate) fn handle_shared_gameplay_reliable(
        &mut self,
        transport: TransportId,
        message: WireMessage,
        arena: &mut SharedAuthoritativeArena,
    ) -> Result<WireMessage, LifecycleError> {
        self.require_active_transport(transport)?;
        if !matches!(self.phase, SessionPhase::Connected) {
            return Err(LifecycleError::IngressUnavailable);
        }
        let server_tick = arena.wave().tick().0.saturating_sub(1);
        let response = match message {
            WireMessage::ActionFrame(frame) => {
                self.authority.submit_action_at(&frame, server_tick)?
            }
            WireMessage::MutationRequest(request) => self.authority.submit_shared_mutation(
                &request,
                arena,
                self.shared_player_id()?,
                server_tick,
            )?,
            _ => return Err(SessionError::UnexpectedReliableMessage.into()),
        };
        Ok(WireMessage::ReliableEvent(response))
    }

    pub(crate) fn synchronize_shared_control_response(
        &mut self,
        event: &mut ReliableEventFrame,
        server_tick: u64,
        state_version: u64,
    ) -> Result<(), LifecycleError> {
        event.server_tick = server_tick;
        let ReliableEvent::Control(ControlEvent::SessionResult(result)) = &mut event.event else {
            return Err(LifecycleError::InvalidControlFrame);
        };
        result.server_tick = server_tick;
        result.state_version = state_version;
        if result.code == SessionControlResultCode::LeaveAccepted {
            let recall_tick = server_tick
                .checked_add(LINK_LOST_TICKS)
                .ok_or(LifecycleError::TickExhausted)?;
            self.phase = SessionPhase::LinkLost {
                lost_tick: server_tick,
                recall_tick,
            };
        }
        Ok(())
    }

    /// Advances authoritative gameplay before resolving the `LinkLost` deadline. This ordering
    /// makes a death committed on the deadline tick final before automatic Recall.
    pub fn tick(&mut self) -> Result<Vec<protocol::SnapshotChunk>, LifecycleError> {
        if !self.phase.simulation_active() {
            return Ok(Vec::new());
        }
        let snapshots = self.authority.tick()?;
        let tick = self.server_tick();
        let authority_phase = self.authority.arena().phase();
        self.resolve_post_simulation(tick, authority_phase)?;
        Ok(snapshots)
    }

    fn resolve_post_simulation(
        &mut self,
        tick: u64,
        authority_phase: AuthorityPhase,
    ) -> Result<(), LifecycleError> {
        if matches!(authority_phase, AuthorityPhase::Dead { .. }) {
            self.phase = SessionPhase::Dead {
                committed_tick: tick,
            };
            self.transport = None;
            return Ok(());
        }
        if matches!(authority_phase, AuthorityPhase::Recalled { .. }) {
            self.phase = SessionPhase::Recalled {
                committed_tick: tick,
            };
            self.transport = None;
            return Ok(());
        }
        if let SessionPhase::LinkLost { recall_tick, .. } = self.phase
            && tick >= recall_tick
        {
            let recall = self.authority.commit_emergency_recall()?;
            self.phase = SessionPhase::Recalled {
                committed_tick: recall.committed_at.0,
            };
            self.transport = None;
        }
        Ok(())
    }

    pub fn transport_lost(&mut self, transport: TransportId) -> Result<(), LifecycleError> {
        self.require_active_transport(transport)?;
        self.enter_link_lost()
    }

    pub(crate) fn transport_lost_at_shared_tick(
        &mut self,
        transport: TransportId,
        tick: u64,
    ) -> Result<(), LifecycleError> {
        self.require_active_transport(transport)?;
        if !matches!(self.phase, SessionPhase::Connected) {
            return Err(LifecycleError::IngressUnavailable);
        }
        let recall_tick = tick
            .checked_add(LINK_LOST_TICKS)
            .ok_or(LifecycleError::TickExhausted)?;
        self.authority.neutralize_transport_input();
        self.transport = None;
        self.phase = SessionPhase::LinkLost {
            lost_tick: tick,
            recall_tick,
        };
        Ok(())
    }

    fn enter_link_lost(&mut self) -> Result<(), LifecycleError> {
        if !matches!(self.phase, SessionPhase::Connected) {
            return Err(LifecycleError::IngressUnavailable);
        }
        let lost_tick = self.server_tick();
        let recall_tick = lost_tick
            .checked_add(LINK_LOST_TICKS)
            .ok_or(LifecycleError::TickExhausted)?;
        self.authority.neutralize_transport_input();
        self.transport = None;
        self.phase = SessionPhase::LinkLost {
            lost_tick,
            recall_tick,
        };
        Ok(())
    }

    fn prepare_reconnect(
        &self,
        owner: SessionOwnerId,
        transport: TransportId,
        sequence: u32,
    ) -> Result<Option<TransportId>, LifecycleError> {
        if owner != self.owner {
            return Err(LifecycleError::UnauthorizedOwner);
        }
        if matches!(self.phase, SessionPhase::Closed) {
            return Err(LifecycleError::SessionClosed);
        }
        if let Some(binding) = self.transport
            && binding.id == transport
            && sequence <= binding.last_control_sequence
        {
            return Err(LifecycleError::StaleControlSequence);
        }
        Ok(self
            .transport
            .map(|binding| binding.id)
            .filter(|old| *old != transport))
    }

    fn commit_reconnect(&mut self, transport: TransportId, sequence: u32) {
        self.transport = Some(TransportBinding {
            id: transport,
            last_control_sequence: sequence,
        });
        if matches!(self.phase, SessionPhase::LinkLost { .. }) {
            self.phase = SessionPhase::Connected;
        }
    }

    fn prepare_leave(
        &self,
        transport: TransportId,
        sequence: u32,
    ) -> Result<(u64, u64), LifecycleError> {
        self.validate_control_sequence(transport, sequence)?;
        if !matches!(self.phase, SessionPhase::Connected) {
            return Err(LifecycleError::IngressUnavailable);
        }
        let lost_tick = self.server_tick();
        let recall_tick = lost_tick
            .checked_add(LINK_LOST_TICKS)
            .ok_or(LifecycleError::TickExhausted)?;
        Ok((lost_tick, recall_tick))
    }

    fn commit_leave(&mut self, lost_tick: u64, recall_tick: u64) {
        self.authority.neutralize_transport_input();
        self.transport = None;
        self.phase = SessionPhase::LinkLost {
            lost_tick,
            recall_tick,
        };
    }

    fn validate_control_sequence(
        &self,
        transport: TransportId,
        sequence: u32,
    ) -> Result<(), LifecycleError> {
        let binding = self
            .transport
            .as_ref()
            .ok_or(LifecycleError::StaleTransport)?;
        if binding.id != transport {
            return Err(LifecycleError::StaleTransport);
        }
        if sequence <= binding.last_control_sequence {
            return Err(LifecycleError::StaleControlSequence);
        }
        Ok(())
    }

    fn require_active_transport(&self, transport: TransportId) -> Result<(), LifecycleError> {
        match self.transport {
            Some(binding) if binding.id == transport => Ok(()),
            _ => Err(LifecycleError::StaleTransport),
        }
    }

    fn session_id_text(&self) -> Result<WireText<64>, LifecycleError> {
        WireText::new(format!("m02-session-{:016x}", self.id.get()))
            .map_err(|_| LifecycleError::SessionIdEncoding)
    }

    fn result(
        &self,
        request_sequence: u32,
        code: SessionControlResultCode,
        server_monotonic_micros: u64,
        replaced_previous_transport: bool,
    ) -> Result<SessionControlResult, LifecycleError> {
        Ok(SessionControlResult {
            request_sequence,
            accepted: code.is_accepted(),
            code,
            session_id: self.session_id_text()?,
            destination: self.phase.destination(),
            server_tick: self.server_tick(),
            state_version: self.state_version(),
            server_monotonic_micros,
            replaced_previous_transport,
            controlled_entity_id: matches!(
                code,
                SessionControlResultCode::Joined | SessionControlResultCode::Reattached
            )
            .then_some(protocol::M02_PLAYER_ENTITY_ID_BASE + self.id.get() - 1),
        })
    }

    fn emit_result(
        &mut self,
        result: SessionControlResult,
    ) -> Result<ReliableEventFrame, LifecycleError> {
        self.authority
            .emit_control_result(result)
            .map_err(Into::into)
    }

    fn close_for_shutdown(&mut self) -> Result<Option<ReliableEventFrame>, LifecycleError> {
        if matches!(self.phase, SessionPhase::Closed) {
            return Ok(None);
        }
        let event = self
            .transport
            .is_some()
            .then(|| self.authority.emit_shutdown_event())
            .transpose()?;
        self.authority.neutralize_transport_input();
        self.transport = None;
        self.phase = SessionPhase::Closed;
        Ok(event)
    }
}

#[derive(Debug)]
pub struct LifecycleResponse {
    pub event: ReliableEventFrame,
    pub invalidated_transport: Option<TransportId>,
}

#[derive(Debug)]
pub struct DirectoryTickOutput {
    pub owner: SessionOwnerId,
    pub before_tick: u64,
    pub after_tick: u64,
    pub snapshots: Vec<protocol::SnapshotChunk>,
}

#[derive(Debug)]
pub struct SessionDirectory {
    sessions: BTreeMap<SessionOwnerId, ManagedSession>,
    next_session_id: u64,
    accepting: bool,
}

impl Default for SessionDirectory {
    fn default() -> Self {
        Self {
            sessions: BTreeMap::new(),
            next_session_id: 1,
            accepting: true,
        }
    }
}

impl SessionDirectory {
    #[must_use]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    #[must_use]
    pub const fn is_accepting(&self) -> bool {
        self.accepting
    }

    #[must_use]
    pub fn session(&self, owner: SessionOwnerId) -> Option<&ManagedSession> {
        self.sessions.get(&owner)
    }

    pub fn session_mut(&mut self, owner: SessionOwnerId) -> Option<&mut ManagedSession> {
        self.sessions.get_mut(&owner)
    }

    #[must_use]
    pub fn owner_ids(&self) -> Vec<SessionOwnerId> {
        self.sessions.keys().copied().collect()
    }

    /// Advances every simulation-active logical session exactly once in stable owner order.
    pub fn tick_simulation_active(&mut self) -> Result<Vec<DirectoryTickOutput>, LifecycleError> {
        let mut outputs = Vec::new();
        for (owner, session) in &mut self.sessions {
            if !session.is_simulation_active() {
                continue;
            }
            let before_tick = session.server_tick();
            let snapshots = session.tick()?;
            outputs.push(DirectoryTickOutput {
                owner: *owner,
                before_tick,
                after_tick: session.server_tick(),
                snapshots,
            });
        }
        Ok(outputs)
    }

    /// Removes terminal logical sessions after their final protocol evidence has been delivered.
    pub fn retire_resolved(&mut self) -> Vec<SessionOwnerId> {
        let resolved = self.resolved_owner_ids();
        for owner in &resolved {
            let removed = self.remove_resolved(*owner);
            debug_assert!(removed);
        }
        resolved
    }

    #[must_use]
    pub fn resolved_owner_ids(&self) -> Vec<SessionOwnerId> {
        self.sessions
            .iter()
            .filter_map(|(owner, session)| session.phase().is_resolved().then_some(*owner))
            .collect()
    }

    pub fn remove_resolved(&mut self, owner: SessionOwnerId) -> bool {
        if self
            .sessions
            .get(&owner)
            .is_some_and(|session| session.phase().is_resolved())
        {
            self.sessions.remove(&owner);
            true
        } else {
            false
        }
    }

    pub fn handle_control(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
        frame: &SessionControlFrame,
        content_root: &Path,
        server_monotonic_micros: u64,
    ) -> Result<LifecycleResponse, LifecycleError> {
        self.handle_control_with_authority(owner, transport, frame, server_monotonic_micros, || {
            AuthoritativeSession::from_content_root(content_root)
        })
    }

    pub fn handle_control_with_compiled_content(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
        frame: &SessionControlFrame,
        content: &sim_content::AuthorityCombatTestContent,
        server_monotonic_micros: u64,
    ) -> Result<LifecycleResponse, LifecycleError> {
        self.handle_control_with_authority(owner, transport, frame, server_monotonic_micros, || {
            AuthoritativeSession::from_compiled_content(content)
        })
    }

    fn handle_control_with_authority<F>(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
        frame: &SessionControlFrame,
        server_monotonic_micros: u64,
        authority_factory: F,
    ) -> Result<LifecycleResponse, LifecycleError>
    where
        F: FnOnce() -> Result<AuthoritativeSession, crate::SessionError>,
    {
        frame
            .validate()
            .map_err(|_| LifecycleError::InvalidControlFrame)?;
        if !self.accepting {
            return self.rejected_response(
                owner,
                frame,
                SessionControlResultCode::ServerShuttingDown,
                server_monotonic_micros,
            );
        }
        let response = match &frame.request {
            SessionControlRequest::Join => self.join(
                owner,
                transport,
                frame.sequence,
                server_monotonic_micros,
                authority_factory,
            ),
            SessionControlRequest::Reconnect { prior_session_id } => self.reconnect(
                owner,
                transport,
                frame.sequence,
                prior_session_id,
                server_monotonic_micros,
            ),
            SessionControlRequest::Leave => {
                self.leave(owner, transport, frame.sequence, server_monotonic_micros)
            }
        };
        match response {
            Ok(response) => Ok(response),
            Err(error) => match lifecycle_rejection_code(&error) {
                Some(code) => self.rejected_response(owner, frame, code, server_monotonic_micros),
                None => Err(error),
            },
        }
    }

    fn rejected_response(
        &mut self,
        owner: SessionOwnerId,
        frame: &SessionControlFrame,
        code: SessionControlResultCode,
        server_monotonic_micros: u64,
    ) -> Result<LifecycleResponse, LifecycleError> {
        if let Some(session) = self.sessions.get_mut(&owner) {
            let result = session.result(frame.sequence, code, server_monotonic_micros, false)?;
            return Ok(LifecycleResponse {
                event: session.emit_result(result)?,
                invalidated_transport: None,
            });
        }
        let session_id = match &frame.request {
            SessionControlRequest::Reconnect { prior_session_id } => prior_session_id.clone(),
            SessionControlRequest::Join | SessionControlRequest::Leave => {
                WireText::new("unbound-session").map_err(|_| LifecycleError::SessionIdEncoding)?
            }
        };
        let result = SessionControlResult {
            request_sequence: frame.sequence,
            accepted: false,
            code,
            session_id,
            destination: SessionDestination::Closed,
            server_tick: 0,
            state_version: 0,
            server_monotonic_micros,
            replaced_previous_transport: false,
            controlled_entity_id: None,
        };
        Ok(LifecycleResponse {
            event: ReliableEventFrame {
                sequence: 1,
                server_tick: 0,
                event: ReliableEvent::Control(ControlEvent::SessionResult(result)),
            },
            invalidated_transport: None,
        })
    }

    fn join(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
        sequence: u32,
        server_monotonic_micros: u64,
        authority_factory: impl FnOnce() -> Result<AuthoritativeSession, crate::SessionError>,
    ) -> Result<LifecycleResponse, LifecycleError> {
        if let Some(session) = self.sessions.get_mut(&owner) {
            let invalidated = session.prepare_reconnect(owner, transport, sequence)?;
            let result = session.result(
                sequence,
                SessionControlResultCode::Reattached,
                server_monotonic_micros,
                invalidated.is_some(),
            )?;
            let event = session.emit_result(result)?;
            session.commit_reconnect(transport, sequence);
            return Ok(LifecycleResponse {
                event,
                invalidated_transport: invalidated,
            });
        }
        let id = LogicalSessionId::new(self.next_session_id)?;
        let next = self
            .next_session_id
            .checked_add(1)
            .ok_or(LifecycleError::SessionIdExhausted)?;
        let authority = authority_factory()?;
        let mut session = ManagedSession::new(id, owner, transport, sequence, authority);
        let result = session.result(
            sequence,
            SessionControlResultCode::Joined,
            server_monotonic_micros,
            false,
        )?;
        let event = session.emit_result(result)?;
        self.sessions.insert(owner, session);
        self.next_session_id = next;
        Ok(LifecycleResponse {
            event,
            invalidated_transport: None,
        })
    }

    fn reconnect(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
        sequence: u32,
        prior_session_id: &WireText<64>,
        server_monotonic_micros: u64,
    ) -> Result<LifecycleResponse, LifecycleError> {
        let session = self
            .sessions
            .get_mut(&owner)
            .ok_or(LifecycleError::SessionNotFound)?;
        if session.session_id_text()?.as_str() != prior_session_id.as_str() {
            return Err(LifecycleError::SessionNotFound);
        }
        let invalidated = session.prepare_reconnect(owner, transport, sequence)?;
        let result = session.result(
            sequence,
            SessionControlResultCode::Reattached,
            server_monotonic_micros,
            invalidated.is_some(),
        )?;
        let event = session.emit_result(result)?;
        session.commit_reconnect(transport, sequence);
        Ok(LifecycleResponse {
            event,
            invalidated_transport: invalidated,
        })
    }

    fn leave(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
        sequence: u32,
        server_monotonic_micros: u64,
    ) -> Result<LifecycleResponse, LifecycleError> {
        let session = self
            .sessions
            .get_mut(&owner)
            .ok_or(LifecycleError::SessionNotFound)?;
        let (lost_tick, recall_tick) = session.prepare_leave(transport, sequence)?;
        let result = session.result(
            sequence,
            SessionControlResultCode::LeaveAccepted,
            server_monotonic_micros,
            false,
        )?;
        let event = session.emit_result(result)?;
        session.commit_leave(lost_tick, recall_tick);
        Ok(LifecycleResponse {
            event,
            invalidated_transport: Some(transport),
        })
    }

    pub fn begin_shutdown(
        &mut self,
    ) -> Result<Vec<(TransportId, ReliableEventFrame)>, LifecycleError> {
        if !self.accepting {
            return Ok(Vec::new());
        }
        self.accepting = false;
        let mut events = Vec::new();
        for session in self.sessions.values_mut() {
            let transport = session.transport_id();
            if let Some(event) = session.close_for_shutdown()?
                && let Some(transport) = transport
            {
                events.push((transport, event));
            }
        }
        Ok(events)
    }

    pub fn finish_shutdown(&mut self) -> Result<(), LifecycleError> {
        if self.accepting {
            return Err(LifecycleError::ShutdownNotStarted);
        }
        self.sessions.clear();
        Ok(())
    }
}

fn lifecycle_rejection_code(error: &LifecycleError) -> Option<SessionControlResultCode> {
    match error {
        LifecycleError::SessionNotFound => Some(SessionControlResultCode::SessionNotFound),
        LifecycleError::UnauthorizedOwner | LifecycleError::StaleTransport => {
            Some(SessionControlResultCode::Unauthorized)
        }
        LifecycleError::StaleControlSequence => Some(SessionControlResultCode::StaleSequence),
        LifecycleError::IngressUnavailable | LifecycleError::SessionClosed => {
            Some(SessionControlResultCode::SessionResolved)
        }
        LifecycleError::ServerShuttingDown => Some(SessionControlResultCode::ServerShuttingDown),
        _ => None,
    }
}

#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("{0} identity must be nonzero")]
    ZeroIdentity(&'static str),
    #[error("control frame failed protocol validation")]
    InvalidControlFrame,
    #[error("logical session ID exhausted")]
    SessionIdExhausted,
    #[error("logical session ID could not be encoded")]
    SessionIdEncoding,
    #[error("authoritative tick exhausted")]
    TickExhausted,
    #[error("shared player identity exhausted")]
    PlayerIdentityExhausted,
    #[error("logical session has no player in the hosted shared arena")]
    SharedPlayerMissing,
    #[error("logical session was not found")]
    SessionNotFound,
    #[error("authenticated owner does not own this session")]
    UnauthorizedOwner,
    #[error("transport is no longer authoritative for this logical session")]
    StaleTransport,
    #[error("control request sequence is stale")]
    StaleControlSequence,
    #[error("session does not accept gameplay ingress in its current phase")]
    IngressUnavailable,
    #[error("logical session is closed")]
    SessionClosed,
    #[error("server is shutting down")]
    ServerShuttingDown,
    #[error("shutdown must begin before teardown")]
    ShutdownNotStarted,
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    SharedAuthority(#[from] sim_core::SharedAuthorityError),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use protocol::{ControlEvent, ReliableEvent, SessionControlResultCode};

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn owner(value: u64) -> SessionOwnerId {
        SessionOwnerId::new(value).unwrap()
    }

    fn transport(value: u64) -> TransportId {
        TransportId::new(value).unwrap()
    }

    fn control(sequence: u32, request: SessionControlRequest) -> SessionControlFrame {
        SessionControlFrame {
            sequence,
            client_tick: 0,
            client_monotonic_micros: 10,
            request,
        }
    }

    fn result(event: &ReliableEventFrame) -> &SessionControlResult {
        let ReliableEvent::Control(ControlEvent::SessionResult(result)) = &event.event else {
            panic!("session result");
        };
        result
    }

    fn joined_directory() -> (SessionDirectory, WireText<64>) {
        let mut directory = SessionDirectory::default();
        let response = directory
            .handle_control(
                owner(1),
                transport(1),
                &control(1, SessionControlRequest::Join),
                &content_root(),
                100,
            )
            .unwrap();
        assert_eq!(
            result(&response.event).code,
            SessionControlResultCode::Joined
        );
        (directory, result(&response.event).session_id.clone())
    }

    #[test]
    fn exact_link_lost_boundary_reconnects_at_89_and_recalls_at_90() {
        let (mut directory, session_id) = joined_directory();
        let session = directory.session_mut(owner(1)).unwrap();
        session.transport_lost(transport(1)).unwrap();
        for _ in 0..89 {
            session.tick().unwrap();
        }
        assert!(matches!(
            session.phase(),
            SessionPhase::LinkLost {
                lost_tick: 0,
                recall_tick: 90
            }
        ));
        let response = directory
            .handle_control(
                owner(1),
                transport(2),
                &control(
                    1,
                    SessionControlRequest::Reconnect {
                        prior_session_id: session_id,
                    },
                ),
                &content_root(),
                200,
            )
            .unwrap();
        assert_eq!(
            result(&response.event).destination,
            SessionDestination::CombatInstance
        );
        let (mut recall_directory, _) = joined_directory();
        recall_directory
            .session_mut(owner(1))
            .unwrap()
            .transport_lost(transport(1))
            .unwrap();
        for _ in 0..90 {
            recall_directory
                .session_mut(owner(1))
                .unwrap()
                .tick()
                .unwrap();
        }
        assert_eq!(
            recall_directory.session(owner(1)).unwrap().phase(),
            SessionPhase::Recalled { committed_tick: 90 }
        );
        let recalled = recall_directory
            .session(owner(1))
            .unwrap()
            .authority()
            .arena();
        assert_eq!(
            recalled.phase(),
            AuthorityPhase::Recalled {
                committed_at: sim_core::Tick(90)
            }
        );
        assert!(recalled.inventory().equipped().iter().flatten().count() >= 3);
        assert_eq!(
            recalled
                .inventory()
                .belt()
                .slots()
                .iter()
                .copied()
                .map(sim_core::BeltSlot::tonic_count)
                .sum::<u8>(),
            2
        );
        assert!(recalled.inventory().backpack().iter().all(Option::is_none));
    }

    #[test]
    fn authoritative_death_wins_on_the_recall_boundary_tick() {
        let (mut directory, session_id) = joined_directory();
        let session = directory.session_mut(owner(1)).unwrap();
        session.transport_lost(transport(1)).unwrap();
        session
            .resolve_post_simulation(
                90,
                AuthorityPhase::Dead {
                    committed_at: sim_core::Tick(90),
                },
            )
            .unwrap();
        assert_eq!(session.phase(), SessionPhase::Dead { committed_tick: 90 });
        let response = directory
            .handle_control(
                owner(1),
                transport(2),
                &control(
                    1,
                    SessionControlRequest::Reconnect {
                        prior_session_id: session_id,
                    },
                ),
                &content_root(),
                250,
            )
            .unwrap();
        assert_eq!(
            result(&response.event).destination,
            SessionDestination::DeathFinal
        );
    }

    #[test]
    fn reconnect_preserves_advanced_authority_and_reports_resolved_route() {
        let (mut directory, session_id) = joined_directory();
        let input = InputFrame {
            sequence: 1,
            client_tick: 1,
            movement_x_milli: 1_000,
            movement_y_milli: 0,
            aim_x_milli: 1_000,
            aim_y_milli: 0,
            held_primary: false,
            primary_sequence: 0,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        };
        let session = directory.session_mut(owner(1)).unwrap();
        session.submit_input(transport(1), &input).unwrap();
        session.tick().unwrap();
        let version_before_loss = session.state_version();
        let player_id = session.authority().arena().player().target.entity_id;
        session.transport_lost(transport(1)).unwrap();
        for _ in 0..5 {
            session.tick().unwrap();
        }
        let version_before_reconnect = session.state_version();
        assert!(version_before_reconnect > version_before_loss);
        let response = directory
            .handle_control(
                owner(1),
                transport(2),
                &control(
                    1,
                    SessionControlRequest::Reconnect {
                        prior_session_id: session_id,
                    },
                ),
                &content_root(),
                555,
            )
            .unwrap();
        let session = directory.session(owner(1)).unwrap();
        assert_eq!(session.state_version(), version_before_reconnect);
        assert_eq!(
            session.authority().arena().player().target.entity_id,
            player_id
        );
        assert_eq!(result(&response.event).server_monotonic_micros, 555);

        let (mut resolved, resolved_id) = joined_directory();
        let resolved_session = resolved.session_mut(owner(1)).unwrap();
        resolved_session.transport_lost(transport(1)).unwrap();
        for _ in 0..90 {
            resolved_session.tick().unwrap();
        }
        let response = resolved
            .handle_control(
                owner(1),
                transport(2),
                &control(
                    1,
                    SessionControlRequest::Reconnect {
                        prior_session_id: resolved_id,
                    },
                ),
                &content_root(),
                777,
            )
            .unwrap();
        assert_eq!(
            result(&response.event).destination,
            SessionDestination::LanternHalls
        );
    }

    #[test]
    fn duplicate_join_atomically_replaces_transport_and_preserves_authority() {
        let (mut directory, _) = joined_directory();
        let before = directory.session(owner(1)).unwrap().state_version();
        let response = directory
            .handle_control(
                owner(1),
                transport(2),
                &control(1, SessionControlRequest::Join),
                &content_root(),
                300,
            )
            .unwrap();
        assert_eq!(response.invalidated_transport, Some(transport(1)));
        assert!(result(&response.event).replaced_previous_transport);
        assert_eq!(directory.len(), 1);
        assert_eq!(directory.session(owner(1)).unwrap().state_version(), before);
        assert!(matches!(
            directory.session_mut(owner(1)).unwrap().submit_input(
                transport(1),
                &InputFrame {
                    sequence: 1,
                    client_tick: 1,
                    movement_x_milli: 0,
                    movement_y_milli: 0,
                    aim_x_milli: 1_000,
                    aim_y_milli: 0,
                    held_primary: false,
                    primary_sequence: 0,
                    ability_1_sequence: 0,
                    ability_2_sequence: 0,
                }
            ),
            Err(LifecycleError::StaleTransport)
        ));
    }

    #[test]
    fn expected_control_rejections_are_typed_and_nonmutating() {
        let (mut directory, _) = joined_directory();
        let state_version = directory.session(owner(1)).unwrap().state_version();
        let stale = directory
            .handle_control(
                owner(1),
                transport(1),
                &control(1, SessionControlRequest::Join),
                &content_root(),
                400,
            )
            .unwrap();
        assert_eq!(
            result(&stale.event).code,
            SessionControlResultCode::StaleSequence
        );
        assert!(!result(&stale.event).accepted);
        assert_eq!(
            directory.session(owner(1)).unwrap().transport_id(),
            Some(transport(1))
        );
        assert_eq!(
            directory.session(owner(1)).unwrap().state_version(),
            state_version
        );

        let missing = directory
            .handle_control(
                owner(2),
                transport(2),
                &control(
                    1,
                    SessionControlRequest::Reconnect {
                        prior_session_id: WireText::new("missing-session").unwrap(),
                    },
                ),
                &content_root(),
                500,
            )
            .unwrap();
        assert_eq!(
            result(&missing.event).code,
            SessionControlResultCode::SessionNotFound
        );
        assert_eq!(directory.len(), 1);
    }

    #[test]
    fn graceful_shutdown_is_idempotent_nonlethal_and_drains_cleanly() {
        let (mut directory, _) = joined_directory();
        let events = directory.begin_shutdown().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].1.event,
            ReliableEvent::Control(ControlEvent::ServerShuttingDown)
        ));
        assert_eq!(directory.begin_shutdown().unwrap().len(), 0);
        assert!(!directory.is_accepting());
        assert!(matches!(
            directory.session(owner(1)).unwrap().phase(),
            SessionPhase::Closed
        ));
        directory.finish_shutdown().unwrap();
        assert!(directory.is_empty());
    }
}
