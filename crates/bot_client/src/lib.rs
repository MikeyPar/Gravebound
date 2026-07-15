//! Headless Gravebound network-client boundary.
//!
//! `bot_client` will exercise the real protocol and player journey without rendering. It may
//! choose inputs, but it cannot author gameplay outcomes or bypass server authority.

mod journey;

pub use journey::*;

use protocol::{
    AccountBootstrapFrame, AccountBootstrapResult, BargainDecisionFrame, BargainDecisionResult,
    BargainViewFrame, BargainViewResult, CharacterMutationFrame, CharacterMutationResult,
    ClientHello, ControlEvent, DeathViewFrameV1, DeathViewResultV1, HandshakeResponse,
    InitialOathSelectionFrame, InitialOathSelectionResult, InputFrame, OathViewFrame,
    OathViewResult, ProgressionQueryFrame, ProgressionResult, ProtocolVersion,
    RELIABLE_FRAME_LIMIT, ReliableEvent, ReliableEventFrame, SIMULATION_HZ,
    SafeInventoryTransferFrameV1, SafeInventoryTransferResultV1, SessionControlFrame,
    SessionControlResult, SnapshotChunk, WireMessage, WorldFlowFrame, WorldFlowResult,
    decode_frame, encode_frame,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BotFoundation {
    pub protocol: ProtocolVersion,
    pub expected_server_hz: u16,
    pub local_simulation_hz: u32,
}

impl BotFoundation {
    #[must_use]
    pub const fn m02() -> Self {
        Self {
            protocol: ProtocolVersion::current(),
            expected_server_hz: SIMULATION_HZ,
            local_simulation_hz: sim_core::TICKS_PER_SECOND,
        }
    }

    pub fn validate(self) -> Result<(), BotFoundationError> {
        if self.local_simulation_hz != u32::from(self.expected_server_hz) {
            return Err(BotFoundationError::SimulationRateMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotDoctorReport {
    pub protocol: ProtocolVersion,
    pub expected_server_hz: u16,
    pub transport_enabled: bool,
    pub journey_enabled: bool,
}

pub async fn run_doctor() -> Result<BotDoctorReport, BotFoundationError> {
    let foundation = BotFoundation::m02();
    foundation.validate()?;
    tokio::task::yield_now().await;
    Ok(BotDoctorReport {
        protocol: foundation.protocol,
        expected_server_hz: foundation.expected_server_hz,
        transport_enabled: true,
        journey_enabled: true,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BotFoundationError {
    #[error("bot and authoritative server simulation rates differ")]
    SimulationRateMismatch,
}

/// Performs the bounded, reliable handshake on the caller's established QUIC connection.
pub async fn perform_handshake(
    connection: &quinn::Connection,
    hello: ClientHello,
) -> Result<HandshakeResponse, BotTransportError> {
    let request = encode_frame(&WireMessage::ClientHello(hello))?;
    let (mut send, mut receive) = connection
        .open_bi()
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    send.write_all(&request)
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    let response = receive
        .read_to_end(RELIABLE_FRAME_LIMIT)
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    match decode_frame(&response)? {
        WireMessage::HandshakeResponse(response) => Ok(response),
        _ => Err(BotTransportError::UnexpectedMessage),
    }
}

pub fn send_input_datagram(
    connection: &quinn::Connection,
    input: InputFrame,
) -> Result<(), BotTransportError> {
    let frame = encode_frame(&WireMessage::InputFrame(input))?;
    connection
        .send_datagram(frame.into())
        .map_err(|error| BotTransportError::Quic(error.to_string()))
}

pub async fn receive_snapshot_datagram(
    connection: &quinn::Connection,
) -> Result<SnapshotChunk, BotTransportError> {
    let frame = connection
        .read_datagram()
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    match decode_frame(&frame)? {
        WireMessage::SnapshotChunk(snapshot) => Ok(snapshot),
        _ => Err(BotTransportError::UnexpectedMessage),
    }
}

pub async fn perform_reliable_gameplay(
    connection: &quinn::Connection,
    message: WireMessage,
) -> Result<ReliableEventFrame, BotTransportError> {
    if message.uses_datagram() {
        return Err(BotTransportError::UnexpectedMessage);
    }
    let request = encode_frame(&message)?;
    let (mut send, mut receive) = connection
        .open_bi()
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    send.write_all(&request)
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    let response = receive
        .read_to_end(RELIABLE_FRAME_LIMIT)
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    match decode_frame(&response)? {
        WireMessage::ReliableEvent(event) => Ok(event),
        _ => Err(BotTransportError::UnexpectedMessage),
    }
}

pub async fn perform_session_control(
    connection: &quinn::Connection,
    frame: SessionControlFrame,
) -> Result<(ReliableEventFrame, SessionControlResult), BotTransportError> {
    let request = encode_frame(&WireMessage::SessionControlFrame(frame))?;
    let (mut send, mut receive) = connection
        .open_bi()
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    send.write_all(&request)
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    let response = receive
        .read_to_end(RELIABLE_FRAME_LIMIT)
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    let WireMessage::ReliableEvent(event) = decode_frame(&response)? else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    let ReliableEvent::Control(ControlEvent::SessionResult(result)) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

pub async fn perform_account_bootstrap(
    connection: &quinn::Connection,
    frame: AccountBootstrapFrame,
) -> Result<(ReliableEventFrame, AccountBootstrapResult), BotTransportError> {
    let event =
        perform_reliable_gameplay(connection, WireMessage::AccountBootstrapFrame(frame)).await?;
    let ReliableEvent::AccountBootstrapResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

pub async fn perform_character_mutation(
    connection: &quinn::Connection,
    frame: CharacterMutationFrame,
) -> Result<(ReliableEventFrame, CharacterMutationResult), BotTransportError> {
    let event =
        perform_reliable_gameplay(connection, WireMessage::CharacterMutationFrame(frame)).await?;
    let ReliableEvent::CharacterMutationResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

pub async fn perform_world_flow(
    connection: &quinn::Connection,
    frame: WorldFlowFrame,
) -> Result<(ReliableEventFrame, WorldFlowResult), BotTransportError> {
    let event = perform_reliable_gameplay(connection, WireMessage::WorldFlowFrame(frame)).await?;
    let ReliableEvent::WorldFlowResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

pub async fn perform_progression_query(
    connection: &quinn::Connection,
    frame: ProgressionQueryFrame,
) -> Result<(ReliableEventFrame, ProgressionResult), BotTransportError> {
    let event =
        perform_reliable_gameplay(connection, WireMessage::ProgressionQueryFrame(frame)).await?;
    let ReliableEvent::ProgressionResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

pub async fn perform_oath_view(
    connection: &quinn::Connection,
    frame: OathViewFrame,
) -> Result<(ReliableEventFrame, OathViewResult), BotTransportError> {
    let event = perform_reliable_gameplay(connection, WireMessage::OathViewFrame(frame)).await?;
    let ReliableEvent::OathViewResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

pub async fn perform_initial_oath_selection(
    connection: &quinn::Connection,
    frame: InitialOathSelectionFrame,
) -> Result<(ReliableEventFrame, InitialOathSelectionResult), BotTransportError> {
    let event =
        perform_reliable_gameplay(connection, WireMessage::InitialOathSelectionFrame(frame))
            .await?;
    let ReliableEvent::InitialOathSelectionResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

pub async fn perform_bargain_view(
    connection: &quinn::Connection,
    frame: BargainViewFrame,
) -> Result<(ReliableEventFrame, BargainViewResult), BotTransportError> {
    let event = perform_reliable_gameplay(connection, WireMessage::BargainViewFrame(frame)).await?;
    let ReliableEvent::BargainViewResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

pub async fn perform_bargain_decision(
    connection: &quinn::Connection,
    frame: BargainDecisionFrame,
) -> Result<(ReliableEventFrame, BargainDecisionResult), BotTransportError> {
    let event =
        perform_reliable_gameplay(connection, WireMessage::BargainDecisionFrame(frame)).await?;
    let ReliableEvent::BargainDecisionResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

pub async fn perform_safe_inventory_transfer(
    connection: &quinn::Connection,
    frame: SafeInventoryTransferFrameV1,
) -> Result<(ReliableEventFrame, SafeInventoryTransferResultV1), BotTransportError> {
    let event =
        perform_reliable_gameplay(connection, WireMessage::SafeInventoryTransferFrame(frame))
            .await?;
    let ReliableEvent::SafeInventoryTransferResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.clone()))
}

/// Reads one authenticated durable-death projection through the ordinary reliable route.
pub async fn perform_death_view(
    connection: &quinn::Connection,
    frame: DeathViewFrameV1,
) -> Result<(ReliableEventFrame, DeathViewResultV1), BotTransportError> {
    let event = perform_reliable_gameplay(connection, WireMessage::DeathViewFrame(frame)).await?;
    let ReliableEvent::DeathViewResult(result) = &event.event else {
        return Err(BotTransportError::UnexpectedMessage);
    };
    Ok((event.clone(), result.as_ref().clone()))
}

/// Submits one reliable safe-inventory mutation and intentionally abandons its response stream.
/// Integration policy uses this to prove that a committed response loss converges through the
/// ordinary mutation retry path; callers must retain and retry the exact mutation identity.
pub async fn submit_safe_inventory_without_response(
    connection: &quinn::Connection,
    frame: SafeInventoryTransferFrameV1,
) -> Result<(), BotTransportError> {
    let request = encode_frame(&WireMessage::SafeInventoryTransferFrame(frame))?;
    let (mut send, receive) = connection
        .open_bi()
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    send.write_all(&request)
        .await
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| BotTransportError::Quic(error.to_string()))?;
    drop(receive);
    Ok(())
}

#[derive(Debug, Error)]
pub enum BotTransportError {
    #[error("QUIC handshake transport failed: {0}")]
    Quic(String),
    #[error("handshake codec failed: {0}")]
    Codec(#[from] protocol::WireCodecError),
    #[error("server sent a non-handshake response on the handshake stream")]
    UnexpectedMessage,
}

#[cfg(test)]
mod tests {
    use protocol::{
        ENTITY_STATE_ALIVE, ENTITY_STATE_ELIGIBLE, EntityKind, EntitySnapshot, MutationResult,
        MutationResultCode, ReliableEvent, ReliableEventFrame, SessionControlRequest,
        SessionControlResult, SessionControlResultCode, SessionDestination, SnapshotChunk,
        WireText,
    };

    use super::*;

    fn entity(
        entity_id: u64,
        kind: EntityKind,
        position: (i32, i32),
        health: (u32, u32),
        state_flags: u32,
    ) -> EntitySnapshot {
        EntitySnapshot {
            entity_id,
            kind,
            x_milli_tiles: position.0,
            y_milli_tiles: position.1,
            velocity_x_milli_tiles_per_second: 0,
            velocity_y_milli_tiles_per_second: 0,
            source_entity_id: if kind == EntityKind::FriendlyProjectile {
                protocol::M02_PLAYER_ENTITY_ID_BASE
            } else {
                0
            },
            source_input_sequence: u32::from(kind == EntityKind::FriendlyProjectile),
            source_projectile_ordinal: 0,
            current_health: health.0,
            maximum_health: health.1,
            state_flags,
        }
    }

    fn chunk(
        sequence: u32,
        chunk_index: u16,
        chunk_count: u16,
        entities: Vec<EntitySnapshot>,
    ) -> SnapshotChunk {
        SnapshotChunk {
            sequence,
            server_tick: u64::from(sequence) * 2,
            state_version: u64::from(sequence) + 10,
            acknowledged_input_sequence: sequence,
            chunk_index,
            chunk_count,
            entities,
        }
    }

    fn bind_bot(bot: &mut JourneyBot) {
        bot.apply_reliable_event(&ReliableEventFrame {
            sequence: 1,
            server_tick: 0,
            event: ReliableEvent::Control(ControlEvent::SessionResult(SessionControlResult {
                request_sequence: 1,
                accepted: true,
                code: SessionControlResultCode::Joined,
                session_id: WireText::new("m02-session-test").unwrap(),
                destination: SessionDestination::CombatInstance,
                server_tick: 0,
                state_version: 1,
                server_monotonic_micros: 1,
                replaced_previous_transport: false,
                controlled_entity_id: Some(protocol::M02_PLAYER_ENTITY_ID_BASE),
            })),
        })
        .unwrap();
    }

    #[test]
    fn bot_foundation_matches_server_tick_contract() {
        assert_eq!(BotFoundation::m02().validate(), Ok(()));
    }

    #[tokio::test]
    async fn doctor_reports_transport_and_snapshot_driven_journey() {
        let report = run_doctor().await.expect("M02 bot foundation doctor");
        assert_eq!(report.protocol, ProtocolVersion::current());
        assert_eq!(report.expected_server_hz, 30);
        assert!(report.transport_enabled);
        assert!(report.journey_enabled);
    }

    #[test]
    fn snapshot_assembly_is_order_independent_bounded_and_fail_closed() {
        let player = entity(
            10_000,
            EntityKind::Player,
            (4_000, 12_000),
            (120, 120),
            ENTITY_STATE_ALIVE | ENTITY_STATE_ELIGIBLE,
        );
        let enemy = entity(
            20_000,
            EntityKind::Enemy,
            (3_000, 8_000),
            (30, 40),
            ENTITY_STATE_ALIVE,
        );
        let mut assembler = BotSnapshotAssembler::default();
        assert!(
            assembler
                .ingest(chunk(1, 1, 2, vec![enemy.clone()]))
                .unwrap()
                .is_none()
        );
        assert!(
            assembler
                .ingest(chunk(1, 1, 2, vec![enemy.clone()]))
                .unwrap()
                .is_none()
        );
        let complete = assembler
            .ingest(chunk(1, 0, 2, vec![player.clone()]))
            .unwrap()
            .expect("complete snapshot");
        assert_eq!(
            complete
                .entities
                .iter()
                .map(|value| value.entity_id)
                .collect::<Vec<_>>(),
            vec![10_000, 20_000]
        );
        assert!(
            assembler
                .ingest(chunk(1, 0, 2, vec![player.clone()]))
                .unwrap()
                .is_none()
        );

        let mut inconsistent = BotSnapshotAssembler::default();
        inconsistent
            .ingest(chunk(2, 0, 2, vec![player.clone()]))
            .unwrap();
        let mut wrong = chunk(2, 1, 2, vec![enemy.clone()]);
        wrong.state_version += 1;
        assert!(matches!(
            inconsistent.ingest(wrong),
            Err(BotJourneyError::InconsistentSnapshotMetadata)
        ));

        let mut duplicate_entity = BotSnapshotAssembler::default();
        duplicate_entity
            .ingest(chunk(3, 0, 2, vec![player.clone()]))
            .unwrap();
        assert!(matches!(
            duplicate_entity.ingest(chunk(3, 1, 2, vec![player])),
            Err(BotJourneyError::DuplicateSnapshotEntity)
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One policy journey keeps protocol state transitions auditable.
    fn bot_steers_fights_collects_and_preserves_session_across_reconnect() {
        let player = entity(
            10_000,
            EntityKind::Player,
            (4_000, 12_000),
            (120, 120),
            ENTITY_STATE_ALIVE | ENTITY_STATE_ELIGIBLE,
        );
        let enemy = entity(
            20_000,
            EntityKind::Enemy,
            (3_000, 8_000),
            (30, 40),
            ENTITY_STATE_ALIVE,
        );
        let projectile = entity(
            30_000,
            EntityKind::FriendlyProjectile,
            (3_900, 11_600),
            (0, 0),
            ENTITY_STATE_ALIVE,
        );
        let mut bot = JourneyBot::default();
        bind_bot(&mut bot);
        bot.ingest_snapshot(chunk(1, 0, 1, vec![player.clone(), enemy, projectile]))
            .unwrap();
        let combat = bot.next_input().unwrap();
        assert!(combat.held_primary);
        assert_eq!(combat.primary_sequence, 1);
        assert_eq!((combat.movement_x_milli, combat.movement_y_milli), (0, 0));
        assert!(combat.aim_y_milli < 0);
        assert!(bot.evidence().saw_enemy_damage);
        assert!(bot.evidence().saw_friendly_projectile);

        let pickup = entity(
            44,
            EntityKind::PersonalPickup,
            (3_000, 12_000),
            (0, 0),
            ENTITY_STATE_ALIVE | ENTITY_STATE_ELIGIBLE,
        );
        bot.ingest_snapshot(chunk(2, 0, 1, vec![player, pickup]))
            .unwrap();
        let approach = bot.next_input().unwrap();
        assert!(!approach.held_primary);
        assert_eq!(
            (approach.movement_x_milli, approach.movement_y_milli),
            (-1_000, 0)
        );
        let request = bot
            .next_pickup_request()
            .unwrap()
            .expect("in-range pickup request");
        assert_eq!(request.pickup_id, 44);
        bot.apply_reliable_event(&ReliableEventFrame {
            sequence: 1,
            server_tick: 4,
            event: ReliableEvent::MutationResult(MutationResult {
                mutation_id: request.mutation_id,
                accepted: true,
                code: MutationResultCode::Accepted,
                state_version: 20,
            }),
        })
        .unwrap();
        assert_eq!(bot.evidence().mutations_accepted, 1);

        let joined = SessionControlResult {
            request_sequence: 1,
            accepted: true,
            code: SessionControlResultCode::Joined,
            session_id: WireText::new("m02-session-1").unwrap(),
            destination: SessionDestination::CombatInstance,
            server_tick: 4,
            state_version: 20,
            server_monotonic_micros: 10,
            replaced_previous_transport: false,
            controlled_entity_id: Some(protocol::M02_PLAYER_ENTITY_ID_BASE),
        };
        bot.apply_reliable_event(&ReliableEventFrame {
            sequence: 2,
            server_tick: 4,
            event: ReliableEvent::Control(ControlEvent::SessionResult(joined)),
        })
        .unwrap();
        let reconnect = bot.next_reconnect(20).unwrap();
        assert!(matches!(
            reconnect.request,
            SessionControlRequest::Reconnect { ref prior_session_id }
                if prior_session_id.as_str() == "m02-session-1"
        ));
        let reattached = SessionControlResult {
            request_sequence: reconnect.sequence,
            accepted: true,
            code: SessionControlResultCode::Reattached,
            session_id: WireText::new("m02-session-1").unwrap(),
            destination: SessionDestination::CombatInstance,
            server_tick: 4,
            state_version: 20,
            server_monotonic_micros: 20,
            replaced_previous_transport: false,
            controlled_entity_id: Some(protocol::M02_PLAYER_ENTITY_ID_BASE),
        };
        bot.apply_reliable_event(&ReliableEventFrame {
            sequence: 3,
            server_tick: 4,
            event: ReliableEvent::Control(ControlEvent::SessionResult(reattached)),
        })
        .unwrap();
        assert_eq!(bot.evidence().reconnects_accepted, 1);
    }

    #[test]
    fn terminal_snapshot_finality_and_sequence_exhaustion_fail_closed() {
        let dead = entity(10_000, EntityKind::Player, (0, 0), (0, 120), 0);
        let mut bot = JourneyBot::default();
        bind_bot(&mut bot);
        bot.ingest_snapshot(chunk(1, 0, 1, vec![dead])).unwrap();
        assert_eq!(bot.terminal_outcome(), BotTerminalOutcome::Dead);
        assert!(matches!(
            bot.next_input(),
            Err(BotJourneyError::TerminalJourney)
        ));

        let mut exhausted = JourneyBot::default();
        exhausted.set_input_sequence_for_test(u32::MAX);
        assert!(matches!(
            exhausted.next_input(),
            Err(BotJourneyError::SequenceExhausted)
        ));
    }
}
