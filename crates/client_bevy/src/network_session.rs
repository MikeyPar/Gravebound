//! Native-client connection presentation for `GB-M02-04`.
//!
//! Local time may drive a countdown and retry request, but only a typed server result can route
//! the character to combat, Lantern Halls, or final death.

use protocol::{
    ControlEvent, ReliableEvent, ReliableEventFrame, SessionControlFrame, SessionControlRequest,
    SessionControlResult, SessionControlResultCode, SessionDestination, WireText,
};
use thiserror::Error;

pub const CLIENT_LINK_LOST_MS: u64 = 3_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientConnectionPhase {
    Offline,
    Joining,
    Connected {
        session_id: WireText<64>,
    },
    LinkLost {
        session_id: WireText<64>,
        lost_at_millis: u64,
        deadline_millis: u64,
    },
    Reconnecting {
        session_id: WireText<64>,
    },
    AwaitingAuthoritativeResolution {
        session_id: WireText<64>,
    },
    LanternHalls {
        session_id: WireText<64>,
    },
    DeathFinal {
        session_id: WireText<64>,
    },
    ServerShuttingDown,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConnectionLifecycle {
    phase: ClientConnectionPhase,
    next_control_sequence: u32,
    last_server_tick: u64,
    last_server_monotonic_micros: u64,
}

impl Default for ClientConnectionLifecycle {
    fn default() -> Self {
        Self {
            phase: ClientConnectionPhase::Offline,
            next_control_sequence: 1,
            last_server_tick: 0,
            last_server_monotonic_micros: 0,
        }
    }
}

impl ClientConnectionLifecycle {
    #[must_use]
    pub const fn phase(&self) -> &ClientConnectionPhase {
        &self.phase
    }

    #[must_use]
    pub const fn last_server_tick(&self) -> u64 {
        self.last_server_tick
    }

    #[must_use]
    pub const fn last_server_monotonic_micros(&self) -> u64 {
        self.last_server_monotonic_micros
    }

    pub fn join_request(
        &mut self,
        client_tick: u64,
        client_monotonic_micros: u64,
    ) -> Result<SessionControlFrame, ClientSessionLifecycleError> {
        if !matches!(self.phase, ClientConnectionPhase::Offline) {
            return Err(ClientSessionLifecycleError::InvalidPhase);
        }
        self.phase = ClientConnectionPhase::Joining;
        self.control_frame(
            client_tick,
            client_monotonic_micros,
            SessionControlRequest::Join,
        )
    }

    pub fn transport_lost(&mut self, now_millis: u64) -> Result<(), ClientSessionLifecycleError> {
        let session_id = match &self.phase {
            ClientConnectionPhase::Connected { session_id } => session_id.clone(),
            _ => return Err(ClientSessionLifecycleError::InvalidPhase),
        };
        let deadline_millis = now_millis
            .checked_add(CLIENT_LINK_LOST_MS)
            .ok_or(ClientSessionLifecycleError::ClockExhausted)?;
        self.phase = ClientConnectionPhase::LinkLost {
            session_id,
            lost_at_millis: now_millis,
            deadline_millis,
        };
        Ok(())
    }

    /// Advances presentation only. Deadline expiry deliberately remains unresolved until a server
    /// response establishes Recall or death finality.
    pub fn update_local_clock(
        &mut self,
        now_millis: u64,
    ) -> Result<(), ClientSessionLifecycleError> {
        if let ClientConnectionPhase::LinkLost {
            session_id,
            lost_at_millis,
            deadline_millis,
        } = &self.phase
        {
            if now_millis < *lost_at_millis {
                return Err(ClientSessionLifecycleError::ClockMovedBackward);
            }
            if now_millis >= *deadline_millis {
                self.phase = ClientConnectionPhase::AwaitingAuthoritativeResolution {
                    session_id: session_id.clone(),
                };
            }
        }
        Ok(())
    }

    pub fn reconnect_request(
        &mut self,
        client_tick: u64,
        client_monotonic_micros: u64,
    ) -> Result<SessionControlFrame, ClientSessionLifecycleError> {
        let session_id = match &self.phase {
            ClientConnectionPhase::LinkLost { session_id, .. }
            | ClientConnectionPhase::AwaitingAuthoritativeResolution { session_id } => {
                session_id.clone()
            }
            _ => return Err(ClientSessionLifecycleError::InvalidPhase),
        };
        self.phase = ClientConnectionPhase::Reconnecting {
            session_id: session_id.clone(),
        };
        self.control_frame(
            client_tick,
            client_monotonic_micros,
            SessionControlRequest::Reconnect {
                prior_session_id: session_id,
            },
        )
    }

    pub fn leave_request(
        &mut self,
        client_tick: u64,
        client_monotonic_micros: u64,
    ) -> Result<SessionControlFrame, ClientSessionLifecycleError> {
        if !matches!(self.phase, ClientConnectionPhase::Connected { .. }) {
            return Err(ClientSessionLifecycleError::InvalidPhase);
        }
        self.control_frame(
            client_tick,
            client_monotonic_micros,
            SessionControlRequest::Leave,
        )
    }

    pub fn apply_reliable_event(
        &mut self,
        event: &ReliableEventFrame,
        now_millis: u64,
    ) -> Result<(), ClientSessionLifecycleError> {
        match &event.event {
            ReliableEvent::Control(ControlEvent::SessionResult(result)) => {
                self.apply_session_result(result, now_millis)
            }
            ReliableEvent::Control(ControlEvent::ServerShuttingDown) => {
                self.phase = ClientConnectionPhase::ServerShuttingDown;
                Ok(())
            }
            _ => Err(ClientSessionLifecycleError::UnexpectedEvent),
        }
    }

    fn apply_session_result(
        &mut self,
        result: &SessionControlResult,
        now_millis: u64,
    ) -> Result<(), ClientSessionLifecycleError> {
        if !result.accepted {
            return Err(ClientSessionLifecycleError::Rejected(result.code));
        }
        self.last_server_tick = result.server_tick;
        self.last_server_monotonic_micros = result.server_monotonic_micros;
        self.phase = match result.code {
            SessionControlResultCode::LeaveAccepted => {
                let deadline_millis = now_millis
                    .checked_add(CLIENT_LINK_LOST_MS)
                    .ok_or(ClientSessionLifecycleError::ClockExhausted)?;
                ClientConnectionPhase::LinkLost {
                    session_id: result.session_id.clone(),
                    lost_at_millis: now_millis,
                    deadline_millis,
                }
            }
            SessionControlResultCode::Joined | SessionControlResultCode::Reattached => {
                match result.destination {
                    SessionDestination::CombatInstance => ClientConnectionPhase::Connected {
                        session_id: result.session_id.clone(),
                    },
                    SessionDestination::LanternHalls => ClientConnectionPhase::LanternHalls {
                        session_id: result.session_id.clone(),
                    },
                    SessionDestination::DeathFinal => ClientConnectionPhase::DeathFinal {
                        session_id: result.session_id.clone(),
                    },
                    SessionDestination::Closed => ClientConnectionPhase::Closed,
                }
            }
            _ => return Err(ClientSessionLifecycleError::InvalidResult),
        };
        Ok(())
    }

    fn control_frame(
        &mut self,
        client_tick: u64,
        client_monotonic_micros: u64,
        request: SessionControlRequest,
    ) -> Result<SessionControlFrame, ClientSessionLifecycleError> {
        let sequence = self.next_control_sequence;
        self.next_control_sequence = sequence
            .checked_add(1)
            .ok_or(ClientSessionLifecycleError::SequenceExhausted)?;
        Ok(SessionControlFrame {
            sequence,
            client_tick,
            client_monotonic_micros,
            request,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ClientSessionLifecycleError {
    #[error("connection action is invalid in the current phase")]
    InvalidPhase,
    #[error("local connection clock moved backward")]
    ClockMovedBackward,
    #[error("local connection clock exhausted")]
    ClockExhausted,
    #[error("control request sequence exhausted")]
    SequenceExhausted,
    #[error("unexpected reliable event on the lifecycle seam")]
    UnexpectedEvent,
    #[error("server rejected lifecycle request: {0:?}")]
    Rejected(SessionControlResultCode),
    #[error("server lifecycle result is inconsistent with an accepted request")]
    InvalidResult,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(destination: SessionDestination) -> ReliableEventFrame {
        ReliableEventFrame {
            sequence: 1,
            server_tick: 42,
            event: ReliableEvent::Control(ControlEvent::SessionResult(SessionControlResult {
                request_sequence: 1,
                accepted: true,
                code: SessionControlResultCode::Reattached,
                session_id: WireText::new("session-1").unwrap(),
                destination,
                server_tick: 42,
                state_version: 7,
                server_monotonic_micros: 123_000,
                replaced_previous_transport: false,
                controlled_entity_id: Some(protocol::M02_PLAYER_ENTITY_ID_BASE),
            })),
        }
    }

    #[test]
    fn local_deadline_never_fabricates_recall_or_death() {
        let mut lifecycle = ClientConnectionLifecycle {
            phase: ClientConnectionPhase::Connected {
                session_id: WireText::new("session-1").unwrap(),
            },
            ..ClientConnectionLifecycle::default()
        };
        lifecycle.transport_lost(10).unwrap();
        lifecycle.update_local_clock(3_009).unwrap();
        assert!(matches!(
            lifecycle.phase(),
            ClientConnectionPhase::LinkLost { .. }
        ));
        lifecycle.update_local_clock(3_010).unwrap();
        assert!(matches!(
            lifecycle.phase(),
            ClientConnectionPhase::AwaitingAuthoritativeResolution { .. }
        ));
    }

    #[test]
    fn only_server_result_selects_combat_hall_or_final_death() {
        for (destination, expected) in [
            (SessionDestination::CombatInstance, "combat"),
            (SessionDestination::LanternHalls, "hall"),
            (SessionDestination::DeathFinal, "death"),
        ] {
            let mut lifecycle = ClientConnectionLifecycle {
                phase: ClientConnectionPhase::Reconnecting {
                    session_id: WireText::new("session-1").unwrap(),
                },
                ..ClientConnectionLifecycle::default()
            };
            lifecycle
                .apply_reliable_event(&result(destination), 0)
                .unwrap();
            assert_eq!(
                match lifecycle.phase() {
                    ClientConnectionPhase::Connected { .. } => "combat",
                    ClientConnectionPhase::LanternHalls { .. } => "hall",
                    ClientConnectionPhase::DeathFinal { .. } => "death",
                    _ => "unexpected",
                },
                expected
            );
            assert_eq!(lifecycle.last_server_tick(), 42);
            assert_eq!(lifecycle.last_server_monotonic_micros(), 123_000);
        }
    }
}
