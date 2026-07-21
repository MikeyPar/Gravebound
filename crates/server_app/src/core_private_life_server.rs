//! Terminal-first QUIC dispatch for the ordinary Core private-life route.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`-`003`,
//! `TECH-015`, and `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-WORLD-001`, `CONT-HUB-001`/`002`, and `CONT-BOSS-001`/`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-08`, and the M03
//! exit gate). Durable transition reconciliation always precedes response publication.

use std::{future::pending, sync::Arc, time::SystemTime};

use protocol::{
    ActionResultCode, HandshakeResponse, RELIABLE_FRAME_LIMIT, ReliableEvent, WireMessage,
    WireText, WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferResultCode,
    decode_frame, encode_frame,
};
use thiserror::Error;

use crate::core_private_gameplay_observation::{
    CorePrivateGameplayObservation, CorePrivateGameplayObservationError,
};
use crate::core_private_life_process::{
    CorePrivateLifeProcess, CorePrivateLifeProcessDisposition, CorePrivateLifeProcessError,
};
use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreExtractionIntentAuthority,
    CoreExtractionTerminalAuthority, CorePrivateHallActorLease, CorePrivateLifeTransportLease,
    CorePrivateMicrorealmBinding, CorePrivateMicrorealmBindingLease,
    CorePrivateMicrorealmDriverObserver, CorePrivateMicrorealmDriverState,
    CorePrivateRouteActorLease, CoreRecallIntentAuthority, CoreRecallTerminalAuthority,
    CoreReliableWriter, CoreReliableWriterError, CoreWorldFlowAuthority, HandshakePolicy,
    dispatch_core_reliable_message, send_gameplay_snapshots,
};

#[derive(Debug)]
enum ConnectionRoute {
    Bootstrap,
    Hall {
        actor: CorePrivateHallActorLease,
        route: CorePrivateRouteActorLease,
    },
    Danger(CorePrivateMicrorealmBinding),
}

impl ConnectionRoute {
    fn from_disposition(disposition: CorePrivateLifeProcessDisposition) -> Self {
        match disposition {
            CorePrivateLifeProcessDisposition::Hall { actor, route, .. } => {
                Self::Hall { actor, route }
            }
            CorePrivateLifeProcessDisposition::Danger(binding) => Self::Danger(binding),
            CorePrivateLifeProcessDisposition::Bootstrap(_) => Self::Bootstrap,
        }
    }

    const fn route_lease(&self) -> Option<CorePrivateRouteActorLease> {
        match self {
            Self::Hall { route, .. } => Some(*route),
            Self::Danger(binding) => Some(binding.lease.route_lease()),
            Self::Bootstrap => None,
        }
    }

    fn driver_observation(&self) -> Option<DriverObservation> {
        match self {
            Self::Danger(binding) => Some(DriverObservation {
                binding: binding.lease,
                observer: binding.observer.clone(),
            }),
            Self::Bootstrap | Self::Hall { .. } => None,
        }
    }
}

#[derive(Debug)]
struct DriverObservation {
    binding: CorePrivateMicrorealmBindingLease,
    observer: CorePrivateMicrorealmDriverObserver,
}

#[derive(Debug, Default)]
struct SnapshotPublisher {
    sequence: u32,
    actor_generation: Option<u64>,
    last_observed_tick: u64,
    last_published_bucket: Option<u64>,
    terminal_publication: Option<(u64, u64)>,
}

impl SnapshotPublisher {
    fn prepare(
        &mut self,
        observation: &CorePrivateGameplayObservation,
        terminal: bool,
    ) -> Result<Option<Vec<protocol::SnapshotChunk>>, CorePrivateLifeServerError> {
        let generation_changed = self.actor_generation != Some(observation.actor_generation);
        if !generation_changed && observation.tick < self.last_observed_tick {
            return Err(CorePrivateLifeServerError::SnapshotTickRegressed);
        }
        if generation_changed {
            self.actor_generation = Some(observation.actor_generation);
            self.last_observed_tick = 0;
            self.last_published_bucket = None;
            self.terminal_publication = None;
        }
        self.last_observed_tick = observation.tick;
        let bucket = observation.tick / 2;
        let terminal_key = (observation.actor_generation, observation.tick);
        let publish = self
            .last_published_bucket
            .is_none_or(|published| bucket > published)
            || (terminal && self.terminal_publication != Some(terminal_key));
        if !publish {
            return Ok(None);
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(CorePrivateLifeServerError::SnapshotSequenceExhausted)?;
        let chunks = observation.snapshot_chunks(self.sequence)?;
        self.last_published_bucket = Some(bucket);
        if terminal {
            self.terminal_publication = Some(terminal_key);
        }
        Ok(Some(chunks))
    }
}

pub(crate) async fn serve_core_private_life_connection(
    incoming: quinn::Incoming,
    policy: HandshakePolicy,
    process: Arc<CorePrivateLifeProcess>,
) -> Result<bool, CorePrivateLifeServerError> {
    let connection = incoming.await?;
    let (mut send, mut receive) = connection.accept_bi().await?;
    let request = receive.read_to_end(RELIABLE_FRAME_LIMIT).await?;
    let WireMessage::ClientHello(hello) = decode_frame(&request)? else {
        return Err(CorePrivateLifeServerError::UnexpectedHandshake);
    };
    let response = policy.evaluate(
        &hello,
        crate::AuthenticationDecision::Accepted,
        WireText::new("core-private-life-session")?,
    );
    send.write_all(&encode_frame(&WireMessage::HandshakeResponse(
        response.clone(),
    ))?)
    .await?;
    send.finish()?;
    if !matches!(response, HandshakeResponse::Accepted(_)) {
        return Ok(false);
    }

    let account_id = crate::core_account_id_from_auth_ticket(&hello.auth_ticket)
        .ok_or(CorePrivateLifeServerError::InvalidAccount)?;
    let authenticated = AuthenticatedAccount {
        account_id,
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let issued_at_unix_ms = unix_millis()?;
    let attached = process
        .attach_transport(authenticated, connection.clone(), issued_at_unix_ms)
        .await?;
    let transport = attached.transport;
    let writer = attached.writer;
    let mut route = ConnectionRoute::from_disposition(attached.disposition);
    let mut driver = route.driver_observation();
    let mut snapshot_publisher = SnapshotPublisher::default();
    publish_route(&process, &writer, &route, 0).await?;
    publish_latest_driver_snapshot(&connection, driver.as_ref(), &mut snapshot_publisher)?;

    let result = run_connection_loop(
        &connection,
        &process,
        authenticated,
        transport,
        &writer,
        &mut route,
        &mut driver,
        &mut snapshot_publisher,
    )
    .await;
    let detached = process.detach_transport(transport, unix_millis()?).await;
    match (result, detached) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Ok(()), Ok(_)) => Ok(true),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the connection loop keeps transport, route, and snapshot custody explicit"
)]
async fn run_connection_loop(
    connection: &quinn::Connection,
    process: &Arc<CorePrivateLifeProcess>,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    writer: &Arc<CoreReliableWriter>,
    route: &mut ConnectionRoute,
    driver: &mut Option<DriverObservation>,
    snapshot_publisher: &mut SnapshotPublisher,
) -> Result<(), CorePrivateLifeServerError> {
    loop {
        tokio::select! {
            observation = next_driver_observation(driver) => {
                let observation = observation?;
                if observation_allows_route_publication(&observation) {
                    publish_route(process, writer, route, observation_tick(&observation)).await?;
                }
                publish_observation_snapshot(connection, snapshot_publisher, &observation)?;
            }
            datagram = connection.read_datagram() => {
                let Ok(bytes) = datagram else { break };
                let WireMessage::InputFrame(frame) = decode_frame(&bytes)? else {
                    return Err(CorePrivateLifeServerError::UnexpectedDatagram);
                };
                match route {
                    ConnectionRoute::Hall { actor, .. } => {
                        process.hall().apply_input(authenticated, *actor, transport, &frame)?;
                    }
                    ConnectionRoute::Danger(_) => {
                        process.sessions().submit_microrealm_input(transport, &frame).await?;
                    }
                    ConnectionRoute::Bootstrap => {
                        return Err(CorePrivateLifeServerError::ControlUnavailable);
                    }
                }
            }
            stream = connection.accept_bi() => {
                let Ok((send, mut receive)) = stream else { break };
                let request = receive.read_to_end(RELIABLE_FRAME_LIMIT).await?;
                let message = decode_frame(&request)?;
                dispatch_reliable(
                    send,
                    message,
                    process,
                    authenticated,
                    transport,
                    writer,
                    route,
                ).await?;
                sync_driver_observation(route, driver);
                publish_latest_driver_snapshot(connection, driver.as_ref(), snapshot_publisher)?;
            }
        }
    }
    Ok(())
}

fn publish_latest_driver_snapshot(
    connection: &quinn::Connection,
    driver: Option<&DriverObservation>,
    publisher: &mut SnapshotPublisher,
) -> Result<(), CorePrivateLifeServerError> {
    if let Some(driver) = driver {
        publish_observation_snapshot(connection, publisher, &driver.observer.latest())?;
    }
    Ok(())
}

fn publish_observation_snapshot(
    connection: &quinn::Connection,
    publisher: &mut SnapshotPublisher,
    state: &CorePrivateMicrorealmDriverState,
) -> Result<(), CorePrivateLifeServerError> {
    let Some((observation, terminal)) = gameplay_observation(state) else {
        return Ok(());
    };
    if let Some(chunks) = publisher.prepare(observation, terminal)? {
        send_gameplay_snapshots(connection, chunks)?;
    }
    Ok(())
}

fn gameplay_observation(
    state: &CorePrivateMicrorealmDriverState,
) -> Option<(&CorePrivateGameplayObservation, bool)> {
    match state {
        CorePrivateMicrorealmDriverState::Running { step, .. } => Some((&step.observation, false)),
        CorePrivateMicrorealmDriverState::TerminalPending { lethal_step, .. } => {
            Some((&lethal_step.observation, true))
        }
        CorePrivateMicrorealmDriverState::FixedDungeonRunning { frame, .. }
        | CorePrivateMicrorealmDriverState::FixedDungeonRewardPending { frame, .. } => {
            Some((&frame.observation, false))
        }
        CorePrivateMicrorealmDriverState::FixedDungeonTerminalPending { lethal_frame, .. } => {
            Some((&lethal_frame.observation, true))
        }
        CorePrivateMicrorealmDriverState::CaldusRunning { frame, .. }
        | CorePrivateMicrorealmDriverState::CaldusRewardPending { frame, .. } => {
            Some((&frame.observation, false))
        }
        CorePrivateMicrorealmDriverState::CaldusTerminalPending { lethal_frame, .. } => {
            Some((&lethal_frame.observation, true))
        }
        CorePrivateMicrorealmDriverState::Starting
        | CorePrivateMicrorealmDriverState::BellResolutionPending { .. }
        | CorePrivateMicrorealmDriverState::FixedDungeonReady { .. }
        | CorePrivateMicrorealmDriverState::CaldusExitReady { .. }
        | CorePrivateMicrorealmDriverState::Faulted { .. } => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_reliable(
    send: quinn::SendStream,
    message: WireMessage,
    process: &Arc<CorePrivateLifeProcess>,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    writer: &Arc<CoreReliableWriter>,
    route: &mut ConnectionRoute,
) -> Result<(), CorePrivateLifeServerError> {
    match message {
        WireMessage::ActionFrame(frame) => {
            let code = if matches!(route, ConnectionRoute::Danger(_))
                && process
                    .sessions()
                    .submit_microrealm_action(transport, &frame)
                    .await
                    .is_ok()
            {
                ActionResultCode::Accepted
            } else {
                ActionResultCode::InvalidState
            };
            writer
                .send_response(
                    send,
                    0,
                    ReliableEvent::ActionResult {
                        action_sequence: frame.sequence,
                        code,
                    },
                )
                .await?;
        }
        WireMessage::WorldFlowFrame(frame) => {
            let transition = transition_kind(&frame);
            let result = match route {
                ConnectionRoute::Hall { actor, .. } => {
                    process
                        .hall_world_flow(*actor, transport)
                        .handle_world_flow(authenticated, &frame)
                        .await
                }
                ConnectionRoute::Bootstrap | ConnectionRoute::Danger(_) => {
                    process
                        .world_flow()
                        .handle_world_flow(authenticated, &frame)
                        .await
                }
            };
            if accepted_transfer(&result) {
                reconcile_transition(process, authenticated, transport, writer, route, transition)
                    .await?;
            }
            writer
                .send_response(send, 0, ReliableEvent::WorldFlowResult(result))
                .await?;
            publish_route(process, writer, route, 0).await?;
        }
        WireMessage::ExtractionCommitFrame(frame) => {
            dispatch_extraction(send, &frame, process, authenticated, transport, writer).await?;
        }
        WireMessage::RecallFrame(frame) => {
            dispatch_recall(send, &frame, process, authenticated, transport, writer).await?;
        }
        message => {
            let refresh_after = matches!(
                message,
                WireMessage::AccountBootstrapFrame(_)
                    | WireMessage::CharacterMutationFrame(_)
                    | WireMessage::SuccessorCreateFrame(_)
            );
            let disabled_extraction = CoreExtractionTerminalAuthority::disabled();
            let disabled_recall = CoreRecallTerminalAuthority::disabled();
            let dispatch = dispatch_core_reliable_message(
                message,
                process.identity().as_ref(),
                process.world_flow().as_ref(),
                process.progression().as_ref(),
                process.death_views().as_ref(),
                process.oath().as_ref(),
                process.bargain().as_ref(),
                process.safe_inventory().as_ref(),
                process.resolution_hold().as_ref(),
                process.successor().as_ref(),
                &disabled_extraction,
                &disabled_recall,
                authenticated,
                0,
            )
            .await?;
            if refresh_after && matches!(route, ConnectionRoute::Bootstrap) {
                *route = ConnectionRoute::from_disposition(
                    process
                        .refresh_transport(authenticated, transport, writer)
                        .await?,
                );
            }
            writer
                .send_response(send, dispatch.server_tick, dispatch.event)
                .await?;
            publish_route(process, writer, route, 0).await?;
        }
    }
    Ok(())
}

async fn dispatch_extraction(
    send: quinn::SendStream,
    frame: &protocol::ExtractionCommitFrameV1,
    process: &Arc<CorePrivateLifeProcess>,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    writer: &CoreReliableWriter,
) -> Result<(), CorePrivateLifeServerError> {
    let reply = match process.sessions().extraction_lease(transport).await {
        Ok(lease) => {
            process
                .extraction()
                .authority(lease)
                .handle_extraction(authenticated, frame, 0)
                .await
        }
        Err(_) => {
            CoreExtractionTerminalAuthority::disabled()
                .handle_extraction(authenticated, frame, 0)
                .await
        }
    };
    writer
        .send_response(
            send,
            reply.server_tick,
            ReliableEvent::ExtractionCommitResult(Box::new(reply.result)),
        )
        .await?;
    Ok(())
}

async fn dispatch_recall(
    send: quinn::SendStream,
    frame: &protocol::RecallFrameV1,
    process: &Arc<CorePrivateLifeProcess>,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    writer: &CoreReliableWriter,
) -> Result<(), CorePrivateLifeServerError> {
    let reply = match process.sessions().recall_authority(transport).await {
        Ok(authority) => authority.handle_recall(authenticated, frame, 0).await,
        Err(_) => {
            CoreRecallTerminalAuthority::disabled()
                .handle_recall(authenticated, frame, 0)
                .await
        }
    };
    writer
        .send_response(
            send,
            reply.server_tick,
            ReliableEvent::RecallResult(Box::new(reply.result)),
        )
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionKind {
    EnterMicrorealm { character_id: [u8; 16] },
    Refresh,
    None,
}

fn transition_kind(frame: &protocol::WorldFlowFrame) -> TransitionKind {
    let WorldFlowRequest::Transfer(mutation) = &frame.request else {
        return TransitionKind::None;
    };
    match &mutation.payload.command {
        WorldTransferCommand::UsePortal { portal_id }
            if portal_id.as_str() == "station.realm_gate" =>
        {
            TransitionKind::EnterMicrorealm {
                character_id: mutation.character_id,
            }
        }
        WorldTransferCommand::EnterHallFromCharacterSelect
        | WorldTransferCommand::ReturnToCharacterSelect
        | WorldTransferCommand::UseCommittedExtraction { .. } => TransitionKind::Refresh,
        WorldTransferCommand::UsePortal { .. } => TransitionKind::None,
    }
}

fn accepted_transfer(result: &WorldFlowResult) -> bool {
    matches!(
        result,
        WorldFlowResult::Transfer {
            accepted: true,
            code: WorldTransferResultCode::Accepted,
            ..
        }
    )
}

async fn reconcile_transition(
    process: &Arc<CorePrivateLifeProcess>,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    writer: &Arc<CoreReliableWriter>,
    route: &mut ConnectionRoute,
    transition: TransitionKind,
) -> Result<(), CorePrivateLifeServerError> {
    match transition {
        TransitionKind::EnterMicrorealm { character_id } => {
            *route = ConnectionRoute::Danger(
                process
                    .enter_committed_microrealm(authenticated, transport, character_id)
                    .await?,
            );
        }
        TransitionKind::Refresh => {
            *route = ConnectionRoute::from_disposition(
                process
                    .refresh_transport(authenticated, transport, writer)
                    .await?,
            );
        }
        TransitionKind::None => {}
    }
    Ok(())
}

async fn publish_route(
    process: &CorePrivateLifeProcess,
    writer: &CoreReliableWriter,
    route: &ConnectionRoute,
    server_tick: u64,
) -> Result<(), CorePrivateLifeServerError> {
    let Some(lease) = route.route_lease() else {
        return Ok(());
    };
    let snapshot = process.route_snapshot(lease)?;
    writer
        .send_route_event(
            server_tick,
            ReliableEvent::CorePrivateRouteState(Box::new(snapshot)),
        )
        .await?;
    Ok(())
}

async fn next_driver_observation(
    driver: &mut Option<DriverObservation>,
) -> Result<CorePrivateMicrorealmDriverState, CorePrivateLifeServerError> {
    match driver {
        Some(driver) => Ok(driver.observer.changed().await?),
        None => pending().await,
    }
}

fn sync_driver_observation(route: &ConnectionRoute, driver: &mut Option<DriverObservation>) {
    let expected = match route {
        ConnectionRoute::Danger(binding) => Some(binding.lease),
        ConnectionRoute::Bootstrap | ConnectionRoute::Hall { .. } => None,
    };
    if driver.as_ref().map(|current| current.binding) == expected {
        return;
    }
    *driver = route.driver_observation();
}

fn observation_tick(observation: &CorePrivateMicrorealmDriverState) -> u64 {
    match observation {
        CorePrivateMicrorealmDriverState::Running { step, .. } => step.tick.0,
        CorePrivateMicrorealmDriverState::TerminalPending { lethal_step, .. } => lethal_step.tick.0,
        CorePrivateMicrorealmDriverState::BellResolutionPending { final_tick, .. } => final_tick.0,
        CorePrivateMicrorealmDriverState::FixedDungeonReady { ready } => {
            ready.final_microrealm_tick.0
        }
        CorePrivateMicrorealmDriverState::FixedDungeonRunning { frame, .. }
        | CorePrivateMicrorealmDriverState::FixedDungeonRewardPending { frame, .. } => frame.tick.0,
        CorePrivateMicrorealmDriverState::FixedDungeonTerminalPending { lethal_frame, .. } => {
            lethal_frame.tick.0
        }
        CorePrivateMicrorealmDriverState::CaldusRunning { frame, .. }
        | CorePrivateMicrorealmDriverState::CaldusRewardPending { frame, .. } => frame.tick.0,
        CorePrivateMicrorealmDriverState::CaldusTerminalPending { lethal_frame, .. } => {
            lethal_frame.tick.0
        }
        CorePrivateMicrorealmDriverState::Faulted { fault, .. } => fault.last_committed_tick.0,
        CorePrivateMicrorealmDriverState::Starting
        | CorePrivateMicrorealmDriverState::CaldusExitReady { .. } => 0,
    }
}

fn observation_allows_route_publication(observation: &CorePrivateMicrorealmDriverState) -> bool {
    !matches!(
        observation,
        CorePrivateMicrorealmDriverState::FixedDungeonRewardPending { .. }
            | CorePrivateMicrorealmDriverState::CaldusRewardPending { .. }
            | CorePrivateMicrorealmDriverState::CaldusExitReady { .. }
    )
}

fn unix_millis() -> Result<u64, CorePrivateLifeServerError> {
    let millis = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| CorePrivateLifeServerError::Clock)?
        .as_millis();
    u64::try_from(millis).map_err(|_| CorePrivateLifeServerError::Clock)
}

#[derive(Debug, Error)]
pub(crate) enum CorePrivateLifeServerError {
    #[error("private-life server received a non-hello handshake message")]
    UnexpectedHandshake,
    #[error("private-life server could not derive the authenticated account")]
    InvalidAccount,
    #[error("private-life server received a non-input datagram")]
    UnexpectedDatagram,
    #[error("private-life control is unavailable for the durable bootstrap state")]
    ControlUnavailable,
    #[error("private-life server clock is outside the supported range")]
    Clock,
    #[error("private-life snapshot tick regressed within one actor generation")]
    SnapshotTickRegressed,
    #[error("private-life snapshot sequence exhausted")]
    SnapshotSequenceExhausted,
    #[error(transparent)]
    Process(#[from] CorePrivateLifeProcessError),
    #[error(transparent)]
    Hall(#[from] crate::core_private_hall_runtime::CorePrivateHallError),
    #[error(transparent)]
    Session(#[from] crate::CorePrivateLifeSessionError),
    #[error(transparent)]
    Reliable(#[from] CoreReliableWriterError),
    #[error(transparent)]
    Observation(#[from] crate::CorePrivateMicrorealmObservationError),
    #[error(transparent)]
    GameplayObservation(#[from] CorePrivateGameplayObservationError),
    #[error(transparent)]
    Transport(#[from] crate::ServerTransportError),
    #[error(transparent)]
    Bounded(#[from] protocol::BoundedValueError),
    #[error(transparent)]
    Codec(#[from] protocol::WireCodecError),
    #[error(transparent)]
    Connection(#[from] quinn::ConnectionError),
    #[error(transparent)]
    Read(#[from] quinn::ReadToEndError),
    #[error(transparent)]
    Write(#[from] quinn::WriteError),
    #[error(transparent)]
    ClosedStream(#[from] quinn::ClosedStream),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_private_gameplay_observation::core_private_gameplay_observation_test_fixture;

    #[test]
    fn snapshot_publisher_is_15_hz_generation_bound_and_terminal_complete() {
        let mut publisher = SnapshotPublisher::default();
        let observation = |tick, generation| {
            core_private_gameplay_observation_test_fixture(tick, generation, 7, 5)
        };

        let first = publisher
            .prepare(&observation(1, 1), false)
            .expect("first snapshot")
            .expect("first committed frame publishes");
        assert_eq!(first[0].sequence, 1);
        assert_eq!(first[0].server_tick, 1);

        let second = publisher
            .prepare(&observation(2, 1), false)
            .expect("next snapshot")
            .expect("next 15 Hz bucket publishes");
        assert_eq!(second[0].sequence, 2);
        assert!(
            publisher
                .prepare(&observation(3, 1), false)
                .expect("coalesced snapshot")
                .is_none()
        );
        assert!(
            publisher
                .prepare(&observation(4, 1), false)
                .expect("second bucket")
                .is_some()
        );

        let lethal = publisher
            .prepare(&observation(4, 1), true)
            .expect("terminal snapshot")
            .expect("same-tick terminal state must publish");
        assert_eq!(lethal[0].sequence, 4);
        assert!(
            publisher
                .prepare(&observation(4, 1), true)
                .expect("terminal replay")
                .is_none()
        );

        let reconnect_generation = publisher
            .prepare(&observation(1, 2), false)
            .expect("next generation")
            .expect("new generation publishes immediately");
        assert_eq!(reconnect_generation[0].sequence, 5);
        assert_eq!(reconnect_generation[0].server_tick, 1);
    }
}
