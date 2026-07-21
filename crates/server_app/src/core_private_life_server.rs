//! Terminal-first QUIC dispatch for the ordinary Core private-life route.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`-`003`,
//! `TECH-015`, and `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-WORLD-001`, `CONT-HUB-001`/`002`, and `CONT-BOSS-001`/`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-08`, and the M03
//! exit gate). Durable transition reconciliation always precedes response publication.

use std::{sync::Arc, time::SystemTime};

use protocol::{
    ActionResultCode, HandshakeResponse, RELIABLE_FRAME_LIMIT, ReliableEvent, WireMessage,
    WireText, WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferResultCode,
    decode_frame, encode_frame,
};
use thiserror::Error;

use crate::core_private_life_process::{
    CorePrivateLifeProcess, CorePrivateLifeProcessDisposition, CorePrivateLifeProcessError,
};
use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreExtractionIntentAuthority,
    CoreExtractionTerminalAuthority, CorePrivateHallActorLease, CorePrivateLifeTransportLease,
    CorePrivateMicrorealmBinding, CorePrivateRouteActorLease, CoreRecallIntentAuthority,
    CoreRecallTerminalAuthority, CoreReliableWriter, CoreReliableWriterError,
    CoreWorldFlowAuthority, HandshakePolicy, dispatch_core_reliable_message,
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
    let mut last_published_route_version = 0;
    publish_route(&process, &writer, &route, &mut last_published_route_version).await?;

    let result = run_connection_loop(
        &connection,
        &process,
        authenticated,
        transport,
        &writer,
        &mut route,
        &mut last_published_route_version,
    )
    .await;
    let detached = process.detach_transport(transport, unix_millis()?).await;
    match (result, detached) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Ok(()), Ok(_)) => Ok(true),
    }
}

async fn run_connection_loop(
    connection: &quinn::Connection,
    process: &Arc<CorePrivateLifeProcess>,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    writer: &Arc<CoreReliableWriter>,
    route: &mut ConnectionRoute,
    last_published_route_version: &mut u64,
) -> Result<(), CorePrivateLifeServerError> {
    loop {
        tokio::select! {
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
                    last_published_route_version,
                ).await?;
            }
        }
    }
    Ok(())
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
    last_published_route_version: &mut u64,
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
            publish_route(process, writer, route, last_published_route_version).await?;
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
            publish_route(process, writer, route, last_published_route_version).await?;
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
    last_published_route_version: &mut u64,
) -> Result<(), CorePrivateLifeServerError> {
    let Some(lease) = route.route_lease() else {
        return Ok(());
    };
    let snapshot = process.route_snapshot(lease)?;
    if snapshot.state_version <= *last_published_route_version {
        return Ok(());
    }
    *last_published_route_version = snapshot.state_version;
    writer
        .send_event(0, ReliableEvent::CorePrivateRouteState(Box::new(snapshot)))
        .await?;
    Ok(())
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
    #[error(transparent)]
    Process(#[from] CorePrivateLifeProcessError),
    #[error(transparent)]
    Hall(#[from] crate::core_private_hall_runtime::CorePrivateHallError),
    #[error(transparent)]
    Session(#[from] crate::CorePrivateLifeSessionError),
    #[error(transparent)]
    Reliable(#[from] CoreReliableWriterError),
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
