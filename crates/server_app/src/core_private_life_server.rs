//! Terminal-first QUIC dispatch for the ordinary Core private-life route.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`-`003`,
//! `TECH-015`, and `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-WORLD-001`, `CONT-HUB-001`/`002`, and `CONT-BOSS-001`/`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-08`, and the M03
//! exit gate). Durable transition reconciliation always precedes response publication.

use std::{
    future::pending,
    sync::Arc,
    time::{Duration, SystemTime},
};

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
    AuthenticatedAccount, AuthenticatedNamespace, CoreBellPortalBinding, CoreBellPortalTransition,
    CoreExtractionIntentAuthority, CoreExtractionTerminalAuthority, CorePrivateHallActorLease,
    CorePrivateLifePreparedBellHandoff, CorePrivateLifeTransportLease,
    CorePrivateMicrorealmBinding, CorePrivateMicrorealmBindingLease,
    CorePrivateMicrorealmDriverObserver, CorePrivateMicrorealmDriverState,
    CorePrivateRouteActorLease, CoreRecallIntentAuthority, CoreRecallTerminalAuthority,
    CoreReliableWriter, CoreReliableWriterError, CoreWorldFlowAuthority, HandshakePolicy,
    dispatch_core_reliable_message, send_gameplay_snapshots,
};

const BELL_DUNGEON_PORTAL_ID: &str = "portal.dungeon.bell_sepulcher";
const BELL_DUNGEON_CONTENT_ID: &str = "dungeon.bell_sepulcher";

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
    actor_generation: Option<SnapshotAuthority>,
    last_observed_tick: u64,
    last_published_bucket: Option<u64>,
    terminal_publication: Option<(u64, u64)>,
}

#[derive(Debug, Default)]
struct CombatPresentationPublisher {
    last_binding: Option<(
        u64,
        protocol::CorePrivateRouteSceneV1,
        Option<protocol::CorePrivateRouteRoomV1>,
        Vec<protocol::CoreCombatActorBindingV1>,
    )>,
}

impl CombatPresentationPublisher {
    async fn publish(
        &mut self,
        writer: &CoreReliableWriter,
        state: &CorePrivateMicrorealmDriverState,
    ) -> Result<(), CorePrivateLifeServerError> {
        let Some((observation, route)) = combat_presentation_observation(state) else {
            return Ok(());
        };
        let binding = (
            route.actor_generation,
            route.scene,
            route.room,
            observation.presentation_actors.clone(),
        );
        let bindings_changed = self.last_binding.as_ref() != Some(&binding);
        if !bindings_changed && observation.presentation_telegraphs.is_empty() {
            return Ok(());
        }
        let frame = protocol::CoreCombatPresentationStateV1 {
            schema_version: protocol::CORE_COMBAT_PRESENTATION_SCHEMA_VERSION,
            content_revision: route.content_revision.clone(),
            actor_generation: route.actor_generation,
            route_state_version: route.state_version,
            scene: route.scene,
            room: route.room,
            server_tick: observation.tick,
            actors: observation.presentation_actors.clone(),
            telegraphs: observation.presentation_telegraphs.clone(),
        };
        frame
            .validate()
            .map_err(|_| CorePrivateLifeServerError::Presentation)?;
        writer
            .send_event(
                observation.tick,
                ReliableEvent::CoreCombatPresentationState(Box::new(frame)),
            )
            .await?;
        self.last_binding = Some(binding);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotAuthority {
    Hall(u64),
    Danger(u64),
}

impl SnapshotPublisher {
    fn prepare(
        &mut self,
        observation: &CorePrivateGameplayObservation,
        authority: SnapshotAuthority,
        terminal: bool,
    ) -> Result<Option<Vec<protocol::SnapshotChunk>>, CorePrivateLifeServerError> {
        let generation_changed = self.actor_generation != Some(authority);
        if !generation_changed && observation.tick < self.last_observed_tick {
            return Err(CorePrivateLifeServerError::SnapshotTickRegressed);
        }
        if generation_changed {
            self.actor_generation = Some(authority);
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
    let mut presentation_publisher = CombatPresentationPublisher::default();
    publish_route(&process, &writer, &route, 0).await?;
    publish_consumable_state(&process, &writer, transport, &route).await?;
    publish_latest_route_snapshot(
        &connection,
        &process,
        authenticated,
        transport,
        &route,
        driver.as_ref(),
        &mut snapshot_publisher,
        &writer,
        &mut presentation_publisher,
    )
    .await?;

    let result = run_connection_loop(
        &connection,
        &process,
        authenticated,
        transport,
        &writer,
        &mut route,
        &mut driver,
        &mut snapshot_publisher,
        &mut presentation_publisher,
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
    presentation_publisher: &mut CombatPresentationPublisher,
) -> Result<(), CorePrivateLifeServerError> {
    let mut terminal_refresh = tokio::time::interval(Duration::from_millis(50));
    terminal_refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut hall_tick = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_micros(33_333),
        Duration::from_micros(33_333),
    );
    hall_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = hall_tick.tick(), if matches!(route, ConnectionRoute::Hall { .. }) => {
                let ConnectionRoute::Hall { actor, .. } = route else { unreachable!() };
                let tick = process.hall().advance_tick(authenticated, *actor, transport)?;
                if let Some(result) = tick.interaction {
                    writer
                        .send_event(
                            tick.observation.tick,
                            ReliableEvent::HallInteractionResult(result),
                        )
                        .await?;
                }
                publish_hall_snapshot(connection, snapshot_publisher, &tick.observation)?;
            }
            _ = terminal_refresh.tick(), if matches!(route, ConnectionRoute::Danger(_)) => {
                if let Some(disposition) = process
                    .install_delivered_extraction_hall(authenticated, transport, writer)
                    .await?
                {
                    *route = ConnectionRoute::from_disposition(disposition);
                    sync_driver_observation(route, driver);
                    publish_route(process, writer, route, 0).await?;
                }
            }
            observation = next_driver_observation(driver) => {
                let observation = observation?;
                if observation_allows_route_publication(&observation) {
                    publish_route(process, writer, route, observation_tick(&observation)).await?;
                }
                // Route authority must lead presentation authority on the same reliable
                // stream so the client never observes bindings for a future route version.
                presentation_publisher.publish(writer, &observation).await?;
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
                publish_latest_route_snapshot(
                    connection,
                    process,
                    authenticated,
                    transport,
                    route,
                    driver.as_ref(),
                    snapshot_publisher,
                    writer,
                    presentation_publisher,
                ).await?;
            }
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "initial publication joins transport, route, snapshot, and presentation authorities"
)]
async fn publish_latest_route_snapshot(
    connection: &quinn::Connection,
    process: &CorePrivateLifeProcess,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    route: &ConnectionRoute,
    driver: Option<&DriverObservation>,
    publisher: &mut SnapshotPublisher,
    writer: &Arc<CoreReliableWriter>,
    presentation_publisher: &mut CombatPresentationPublisher,
) -> Result<(), CorePrivateLifeServerError> {
    match route {
        ConnectionRoute::Hall { actor, .. } => {
            let observation = process
                .hall()
                .observation(authenticated, *actor, transport)?;
            publish_hall_snapshot(connection, publisher, &observation)?;
        }
        ConnectionRoute::Danger(_) => {
            if let Some(driver) = driver {
                let observation = driver.observer.latest();
                // A danger snapshot is not safe to consume until the complete actor binding set
                // for the same route context has led it on the reliable stream. This also covers
                // the initial attach and reliable route transitions, before the first driver tick.
                presentation_publisher.publish(writer, &observation).await?;
                publish_observation_snapshot(connection, publisher, &observation)?;
            }
        }
        ConnectionRoute::Bootstrap => {}
    }
    Ok(())
}

fn publish_hall_snapshot(
    connection: &quinn::Connection,
    publisher: &mut SnapshotPublisher,
    observation: &CorePrivateGameplayObservation,
) -> Result<(), CorePrivateLifeServerError> {
    if let Some(chunks) = publisher.prepare(
        observation,
        SnapshotAuthority::Hall(observation.actor_generation),
        false,
    )? {
        send_gameplay_snapshots(connection, chunks)?;
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
    if let Some(chunks) = publisher.prepare(
        observation,
        SnapshotAuthority::Danger(observation.actor_generation),
        terminal,
    )? {
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

fn combat_presentation_observation(
    state: &CorePrivateMicrorealmDriverState,
) -> Option<(
    &CorePrivateGameplayObservation,
    &protocol::CorePrivateRouteStateV1,
)> {
    match state {
        CorePrivateMicrorealmDriverState::Running { step, .. } => {
            Some((&step.observation, &step.route))
        }
        CorePrivateMicrorealmDriverState::TerminalPending { lethal_step, .. } => {
            Some((&lethal_step.observation, &lethal_step.route))
        }
        CorePrivateMicrorealmDriverState::FixedDungeonRunning { frame, .. }
        | CorePrivateMicrorealmDriverState::FixedDungeonRewardPending { frame, .. } => {
            Some((&frame.observation, &frame.route))
        }
        CorePrivateMicrorealmDriverState::FixedDungeonTerminalPending { lethal_frame, .. } => {
            Some((&lethal_frame.observation, &lethal_frame.route))
        }
        CorePrivateMicrorealmDriverState::CaldusRunning { frame, .. }
        | CorePrivateMicrorealmDriverState::CaldusRewardPending { frame, .. } => {
            Some((&frame.observation, &frame.route))
        }
        CorePrivateMicrorealmDriverState::CaldusTerminalPending { lethal_frame, .. } => {
            Some((&lethal_frame.observation, &lethal_frame.route))
        }
        _ => None,
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the negotiated normal-route dispatcher keeps each bounded message arm in one auditable authority match"
)]
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
        WireMessage::CoreConsumableUseFrame(frame) => {
            let (result, state) = match route {
                ConnectionRoute::Danger(binding) => {
                    process
                        .use_consumable(transport, binding.lease.route_lease(), &frame)
                        .await?
                }
                ConnectionRoute::Bootstrap | ConnectionRoute::Hall { .. } => (
                    protocol::CoreConsumableUseResultV1 {
                        schema_version: protocol::CORE_CONSUMABLE_SCHEMA_VERSION,
                        mutation_id: frame.mutation_id,
                        code: protocol::CoreConsumableResultCodeV1::AuthorityMismatch,
                        consumed_item_uid: None,
                        state: None,
                    },
                    None,
                ),
            };
            writer
                .send_response(send, 0, ReliableEvent::CoreConsumableUseResult(result))
                .await?;
            if let Some(state) = state {
                writer
                    .send_event(0, ReliableEvent::CoreConsumableState(state))
                    .await?;
            }
        }
        WireMessage::HallInteractionFrame(frame) => {
            let result = match route {
                ConnectionRoute::Hall { actor, .. } => {
                    process
                        .hall()
                        .handle_interaction(authenticated, *actor, transport, &frame)?
                }
                ConnectionRoute::Bootstrap | ConnectionRoute::Danger(_) => {
                    protocol::HallInteractionResultV1 {
                        schema_version: protocol::HALL_INTERACTION_SCHEMA_VERSION,
                        request_sequence: frame.sequence,
                        code: protocol::HallInteractionResultCodeV1::InvalidState,
                        station: None,
                        held_ticks: 0,
                        required_ticks: 0,
                    }
                }
            };
            writer
                .send_response(send, 0, ReliableEvent::HallInteractionResult(result))
                .await?;
        }
        WireMessage::ActionFrame(frame) => {
            let code = dispatch_private_action(process, transport, route, &frame).await;
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
            if code == ActionResultCode::Accepted && frame.action == protocol::ActionKind::Interact
            {
                publish_route(process, writer, route, 0).await?;
            }
        }
        WireMessage::WorldFlowFrame(frame) => {
            dispatch_world_flow(
                send,
                &frame,
                process,
                authenticated,
                transport,
                writer,
                route,
            )
            .await?;
        }
        WireMessage::BargainViewFrame(frame) => {
            dispatch_bargain_view(send, &frame, process, authenticated, writer, route).await?;
        }
        WireMessage::BargainDecisionFrame(frame) => {
            dispatch_bargain_decision(
                send,
                &frame,
                process,
                authenticated,
                transport,
                writer,
                route,
            )
            .await?;
        }
        WireMessage::ExtractionCommitFrame(frame) => {
            dispatch_extraction(send, &frame, process, authenticated, transport, writer).await?;
        }
        WireMessage::RecallFrame(frame) => {
            dispatch_recall(send, &frame, process, authenticated, transport, writer).await?;
        }
        WireMessage::SafeStorageQueryFrame(frame) => {
            let authorized = match (&*route, frame.surface) {
                (ConnectionRoute::Hall { actor, .. }, protocol::SafeStorageSurfaceV1::Vault) => {
                    process.hall().panel_authorizes(
                        authenticated,
                        *actor,
                        transport,
                        protocol::HallStationV1::Vault,
                    )
                }
                (ConnectionRoute::Hall { actor, .. }, protocol::SafeStorageSurfaceV1::Overflow) => {
                    process.hall().panel_authorizes(
                        authenticated,
                        *actor,
                        transport,
                        protocol::HallStationV1::Overflow,
                    )
                }
                _ => false,
            };
            let result = if authorized {
                process.safe_storage().query(authenticated, &frame).await
            } else {
                protocol::SafeStorageQueryResultV1::Rejected {
                    schema_version: protocol::SAFE_STORAGE_SCHEMA_VERSION,
                    sequence: frame.sequence,
                    code: crate::safe_storage::unauthorized_panel_code(matches!(
                        route,
                        ConnectionRoute::Hall { .. }
                    )),
                }
            };
            writer
                .send_response(
                    send,
                    0,
                    ReliableEvent::SafeStorageQueryResult(Box::new(result)),
                )
                .await?;
        }
        message => {
            if let WireMessage::SafeInventoryTransferFrame(frame) = &message
                && let Some(replay) = process
                    .safe_inventory()
                    .exact_replay(authenticated, frame)
                    .await
            {
                writer
                    .send_response(send, 0, ReliableEvent::SafeInventoryTransferResult(replay))
                    .await?;
                publish_route(process, writer, route, 0).await?;
                return Ok(());
            }
            if let Some(event) =
                hall_panel_rejection(process, authenticated, transport, route, &message)
            {
                writer.send_response(send, 0, event).await?;
                return Ok(());
            }
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
            let stored_successor = is_stored_successor(&dispatch.event);
            let resolved_resolution_hold = is_resolved_resolution_hold(&dispatch.event);
            if stored_successor {
                reconcile_stored_successor(process, authenticated, transport, writer, route)
                    .await?;
            } else if refresh_after && matches!(route, ConnectionRoute::Bootstrap) {
                *route = ConnectionRoute::from_disposition(
                    process
                        .refresh_transport(authenticated, transport, writer)
                        .await?,
                );
            }
            if resolved_resolution_hold && matches!(route, ConnectionRoute::Bootstrap) {
                // The durable result must reach the client before Hall is exposed. A dropped Hall
                // publication is recovered by normal bootstrap; the stored mutation remains exact.
                writer
                    .send_response(send, dispatch.server_tick, dispatch.event)
                    .await?;
                *route = ConnectionRoute::from_disposition(
                    process
                        .refresh_transport(authenticated, transport, writer)
                        .await?,
                );
                publish_route(process, writer, route, 0).await?;
                return Ok(());
            }
            writer
                .send_response(send, dispatch.server_tick, dispatch.event)
                .await?;
            publish_route(process, writer, route, 0).await?;
        }
    }
    Ok(())
}

fn hall_panel_rejection(
    process: &CorePrivateLifeProcess,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    route: &ConnectionRoute,
    message: &WireMessage,
) -> Option<ReliableEvent> {
    let ConnectionRoute::Hall { actor, .. } = route else {
        return None;
    };
    let authorized = |station| {
        process
            .hall()
            .panel_authorizes(authenticated, *actor, transport, station)
    };
    match message {
        WireMessage::DeathViewFrame(frame)
            if !authorized(protocol::HallStationV1::MemorialWall) =>
        {
            Some(ReliableEvent::DeathViewResult(Box::new(
                protocol::DeathViewResultV1::Error {
                    schema_version: protocol::DEATH_VIEW_SCHEMA_VERSION,
                    request_sequence: frame.sequence,
                    code: protocol::DeathViewResultCodeV1::FeatureDisabled,
                },
            )))
        }
        WireMessage::OathViewFrame(frame) if !authorized(protocol::HallStationV1::OathShrine) => {
            Some(ReliableEvent::OathViewResult(protocol::OathViewResult {
                sequence: frame.sequence,
                code: protocol::OathResultCode::LocationRequired,
                projection: None,
            }))
        }
        WireMessage::InitialOathSelectionFrame(frame)
            if !authorized(protocol::HallStationV1::OathShrine) =>
        {
            Some(ReliableEvent::InitialOathSelectionResult(
                protocol::InitialOathSelectionResult {
                    mutation_id: frame.mutation_id,
                    code: protocol::OathResultCode::LocationRequired,
                    projection: None,
                },
            ))
        }
        WireMessage::SafeInventoryTransferFrame(frame)
            if !authorized(match frame.payload.kind {
                protocol::SafeInventoryTransferKindV1::OverflowToCharacterSafe => {
                    protocol::HallStationV1::Overflow
                }
                protocol::SafeInventoryTransferKindV1::CharacterSafeToVault
                | protocol::SafeInventoryTransferKindV1::VaultToCharacterSafe
                | protocol::SafeInventoryTransferKindV1::CharacterSafeToRunBackpack => {
                    protocol::HallStationV1::Vault
                }
            }) =>
        {
            Some(ReliableEvent::SafeInventoryTransferResult(
                protocol::SafeInventoryTransferResultV1 {
                    mutation_id: frame.mutation_id,
                    character_id: frame.character_id,
                    code: protocol::SafeInventoryResultCodeV1::HallBindingRequired,
                    replayed: false,
                    result_hash: [0; protocol::SAFE_INVENTORY_RESULT_HASH_BYTES],
                    account_version: 0,
                    inventory_version: 0,
                    placements: Vec::new(),
                },
            ))
        }
        _ => None,
    }
}

fn is_stored_successor(event: &ReliableEvent) -> bool {
    matches!(
        event,
        ReliableEvent::SuccessorCreateResult(result)
            if matches!(
                result.as_ref(),
                protocol::SuccessorCreateResultV1::Stored { .. }
            )
    )
}

fn is_resolved_resolution_hold(event: &ReliableEvent) -> bool {
    matches!(
        event,
        ReliableEvent::ResolutionHoldMutationResult(result)
            if matches!(
                result.as_ref(),
                protocol::ResolutionHoldMutationResultV1::Stored {
                    result,
                    ..
                } if !result.storage_resolution_required
            )
    )
}

async fn publish_consumable_state(
    process: &CorePrivateLifeProcess,
    writer: &CoreReliableWriter,
    transport: CorePrivateLifeTransportLease,
    route: &ConnectionRoute,
) -> Result<(), CorePrivateLifeServerError> {
    let ConnectionRoute::Danger(binding) = route else {
        return Ok(());
    };
    let state = process
        .consumable_state(transport, binding.lease.route_lease())
        .await?;
    writer
        .send_event(0, ReliableEvent::CoreConsumableState(state))
        .await?;
    Ok(())
}

/// Retires the exact terminal danger task after successor persistence succeeds, then rebuilds the
/// continuing transport from durable authority before the stored response can expose a Play
/// action. A dropped response is harmless: retry returns the same stored successor and this
/// reconciliation is already complete.
async fn reconcile_stored_successor(
    process: &CorePrivateLifeProcess,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    writer: &Arc<CoreReliableWriter>,
    route: &mut ConnectionRoute,
) -> Result<(), CorePrivateLifeServerError> {
    if let ConnectionRoute::Danger(binding) = route {
        process.sessions().unbind_microrealm(binding.lease).await?;
        *route = ConnectionRoute::Bootstrap;
    }
    if matches!(route, ConnectionRoute::Bootstrap) {
        *route = ConnectionRoute::from_disposition(
            process
                .refresh_transport(authenticated, transport, writer)
                .await?,
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_world_flow(
    send: quinn::SendStream,
    frame: &protocol::WorldFlowFrame,
    process: &Arc<CorePrivateLifeProcess>,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    writer: &Arc<CoreReliableWriter>,
    route: &mut ConnectionRoute,
) -> Result<(), CorePrivateLifeServerError> {
    let bell_handoff = prepare_bell_handoff(process, transport, route, frame).await?;
    let transition = transition_kind(frame);
    let result = match route {
        ConnectionRoute::Hall { actor, .. } => {
            process
                .hall_world_flow(*actor, transport)
                .handle_world_flow(authenticated, frame)
                .await
        }
        ConnectionRoute::Bootstrap | ConnectionRoute::Danger(_) => {
            process
                .world_flow()
                .handle_world_flow(authenticated, frame)
                .await
        }
    };
    reconcile_bell_handoff(process, authenticated, route, frame, &result, bell_handoff).await?;
    if accepted_transfer(&result) {
        reconcile_transition(process, authenticated, transport, writer, route, transition).await?;
    }
    writer
        .send_response(send, 0, ReliableEvent::WorldFlowResult(result))
        .await?;
    publish_route(process, writer, route, 0).await?;
    // Route authority is always sequenced first; a client must never apply Belt authority to an
    // older Hall generation. This also covers the ordinary Realm Gate Hall -> danger transition.
    publish_consumable_state(process, writer, transport, route).await?;
    Ok(())
}

async fn dispatch_bargain_view(
    send: quinn::SendStream,
    frame: &protocol::BargainViewFrame,
    process: &CorePrivateLifeProcess,
    authenticated: AuthenticatedAccount,
    writer: &CoreReliableWriter,
    route: &ConnectionRoute,
) -> Result<(), CorePrivateLifeServerError> {
    let result = if route_is_b4_rest(process, route)? {
        process.bargain().view(authenticated, frame).await
    } else {
        protocol::BargainViewResult {
            sequence: frame.sequence,
            code: protocol::BargainResultCode::LocationRequired,
            projection: None,
        }
    };
    writer
        .send_response(send, 0, ReliableEvent::BargainViewResult(result))
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_bargain_decision(
    send: quinn::SendStream,
    frame: &protocol::BargainDecisionFrame,
    process: &CorePrivateLifeProcess,
    authenticated: AuthenticatedAccount,
    transport: CorePrivateLifeTransportLease,
    writer: &CoreReliableWriter,
    route: &ConnectionRoute,
) -> Result<(), CorePrivateLifeServerError> {
    let authority = if route_is_b4_rest(process, route)? {
        process
            .bargain()
            .decide_with_rest_resolution(authenticated, frame)
            .await
    } else {
        crate::CoreBargainDecisionAuthorityResult {
            response: protocol::BargainDecisionResult {
                mutation_id: frame.mutation_id,
                code: protocol::BargainResultCode::LocationRequired,
                projection: None,
            },
            rest_resolution: None,
        }
    };
    if matches!(
        authority.response.code,
        protocol::BargainResultCode::Accepted | protocol::BargainResultCode::Refused
    ) {
        let durable = authority
            .rest_resolution
            .ok_or(CorePrivateLifeServerError::ControlUnavailable)?;
        process
            .sessions()
            .resolve_fixed_dungeon_rest(transport, durable)
            .await?;
    }
    writer
        .send_response(
            send,
            0,
            ReliableEvent::BargainDecisionResult(authority.response),
        )
        .await?;
    publish_route(process, writer, route, 0).await?;
    Ok(())
}

async fn prepare_bell_handoff(
    process: &CorePrivateLifeProcess,
    transport: CorePrivateLifeTransportLease,
    route: &ConnectionRoute,
    frame: &protocol::WorldFlowFrame,
) -> Result<Option<CorePrivateLifePreparedBellHandoff>, CorePrivateLifeServerError> {
    if !is_bell_transfer(frame) {
        return Ok(None);
    }
    let Some(route_lease) = route.route_lease() else {
        return Ok(None);
    };
    let state = process.route_snapshot(route_lease)?;
    if !state.readiness.bell_portal_available.is_available() {
        return Ok(None);
    }
    Ok(Some(
        process.sessions().prepare_bell_handoff(transport).await?,
    ))
}

async fn reconcile_bell_handoff(
    process: &CorePrivateLifeProcess,
    authenticated: AuthenticatedAccount,
    route: &ConnectionRoute,
    frame: &protocol::WorldFlowFrame,
    result: &WorldFlowResult,
    prepared: Option<CorePrivateLifePreparedBellHandoff>,
) -> Result<(), CorePrivateLifeServerError> {
    if !is_bell_transfer(frame) {
        return Ok(());
    }
    if !accepted_transfer(result) {
        if let Some(prepared) = prepared {
            prepared.abort().await?;
        }
        return Ok(());
    }
    if let Some(prepared) = prepared {
        let transition = bell_transition(authenticated, frame, result)
            .ok_or(CorePrivateLifeServerError::ControlUnavailable)?;
        process.commit_bell_handoff(prepared, transition).await?;
        return Ok(());
    }
    if danger_runtime_has_fixed_dungeon(route) {
        return Ok(());
    }
    Err(CorePrivateLifeServerError::ControlUnavailable)
}

fn is_bell_transfer(frame: &protocol::WorldFlowFrame) -> bool {
    matches!(
        &frame.request,
        WorldFlowRequest::Transfer(protocol::WorldTransferMutation {
            payload: protocol::WorldTransferPayload {
                command: WorldTransferCommand::UsePortal { portal_id },
                ..
            },
            ..
        }) if portal_id.as_str() == BELL_DUNGEON_PORTAL_ID
    )
}

fn bell_transition(
    authenticated: AuthenticatedAccount,
    frame: &protocol::WorldFlowFrame,
    result: &WorldFlowResult,
) -> Option<CoreBellPortalTransition> {
    let WorldFlowRequest::Transfer(mutation) = &frame.request else {
        return None;
    };
    let WorldFlowResult::Transfer {
        accepted: true,
        code: WorldTransferResultCode::Accepted,
        snapshot:
            Some(protocol::CharacterLocationSnapshot {
                character_id,
                character_version,
                location:
                    protocol::CharacterLocation::Danger {
                        location_id,
                        instance_lineage_id,
                        entry_restore_point_id,
                    },
            }),
        transfer_id: Some(transfer_id),
        ..
    } = result
    else {
        return None;
    };
    (*character_id == mutation.character_id
        && location_id.as_str() == BELL_DUNGEON_CONTENT_ID
        && *character_version == mutation.expected_character_version.checked_add(1)?)
    .then(|| {
        let binding = CoreBellPortalBinding {
            account_id: authenticated.account_id.as_bytes(),
            character_id: mutation.character_id,
            mutation_id: mutation.mutation_id,
            instance_lineage_id: *instance_lineage_id,
            entry_restore_point_id: *entry_restore_point_id,
            character_version: mutation.expected_character_version,
            content_revision: mutation.payload.content_revision.clone(),
        };
        CoreBellPortalTransition {
            binding,
            transfer_id: *transfer_id,
            destination_character_version: *character_version,
        }
    })
}

fn danger_runtime_has_fixed_dungeon(route: &ConnectionRoute) -> bool {
    let ConnectionRoute::Danger(binding) = route else {
        return false;
    };
    matches!(
        binding.observer.latest(),
        CorePrivateMicrorealmDriverState::FixedDungeonReady { .. }
            | CorePrivateMicrorealmDriverState::FixedDungeonRunning { .. }
            | CorePrivateMicrorealmDriverState::FixedDungeonRewardPending { .. }
            | CorePrivateMicrorealmDriverState::FixedDungeonTerminalPending { .. }
            | CorePrivateMicrorealmDriverState::CaldusRunning { .. }
            | CorePrivateMicrorealmDriverState::CaldusRewardPending { .. }
            | CorePrivateMicrorealmDriverState::CaldusTerminalPending { .. }
            | CorePrivateMicrorealmDriverState::CaldusExitReady { .. }
    )
}

fn route_is_b4_rest(
    process: &CorePrivateLifeProcess,
    route: &ConnectionRoute,
) -> Result<bool, CorePrivateLifeServerError> {
    let Some(lease) = route.route_lease() else {
        return Ok(false);
    };
    let state = process.route_snapshot(lease)?;
    Ok(
        state.scene == protocol::CorePrivateRouteSceneV1::BellSepulcher
            && state.room == Some(protocol::CorePrivateRouteRoomV1::BellRestB4)
            && state.phase == protocol::CorePrivateRoutePhaseV1::Rest,
    )
}

async fn dispatch_private_action(
    process: &CorePrivateLifeProcess,
    transport: CorePrivateLifeTransportLease,
    route: &ConnectionRoute,
    frame: &protocol::ActionFrame,
) -> ActionResultCode {
    if !matches!(route, ConnectionRoute::Danger(_)) {
        return ActionResultCode::InvalidState;
    }
    let accepted = match frame.action {
        protocol::ActionKind::Interact => process
            .sessions()
            .advance_fixed_dungeon(transport)
            .await
            .is_ok(),
        protocol::ActionKind::Ability1Press | protocol::ActionKind::Ability2Press => process
            .sessions()
            .submit_microrealm_action(transport, frame)
            .await
            .is_ok(),
        protocol::ActionKind::RecallStart | protocol::ActionKind::RecallCancel => false,
    };
    if accepted {
        ActionResultCode::Accepted
    } else {
        ActionResultCode::InvalidState
    }
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
    #[error("combat presentation projection was invalid")]
    Presentation,
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

    fn bell_frame() -> protocol::WorldFlowFrame {
        let payload = protocol::WorldTransferPayload {
            content_revision: protocol::WorldFlowContentRevisionV1 {
                records_blake3: protocol::ManifestHash::new("1".repeat(64)).unwrap(),
                assets_blake3: protocol::ManifestHash::new("2".repeat(64)).unwrap(),
                localization_blake3: protocol::ManifestHash::new("3".repeat(64)).unwrap(),
            },
            command: WorldTransferCommand::UsePortal {
                portal_id: WireText::new(BELL_DUNGEON_PORTAL_ID).unwrap(),
            },
        };
        protocol::WorldFlowFrame {
            sequence: 7,
            request: WorldFlowRequest::Transfer(protocol::WorldTransferMutation {
                mutation_id: [4; 16],
                character_id: [5; 16],
                expected_character_version: 9,
                issued_at_unix_millis: 10,
                payload_hash: payload.canonical_hash(),
                payload,
            }),
        }
    }

    fn accepted_bell_result(character_version: u64) -> WorldFlowResult {
        WorldFlowResult::Transfer {
            request_sequence: 7,
            mutation_id: [4; 16],
            accepted: true,
            code: WorldTransferResultCode::Accepted,
            snapshot: Some(protocol::CharacterLocationSnapshot {
                character_id: [5; 16],
                character_version,
                location: protocol::CharacterLocation::Danger {
                    location_id: WireText::new(BELL_DUNGEON_CONTENT_ID).unwrap(),
                    instance_lineage_id: [6; 16],
                    entry_restore_point_id: [7; 16],
                },
            }),
            transfer_id: Some([8; 16]),
        }
    }

    #[test]
    fn bell_transition_is_exactly_request_and_durable_result_bound() {
        let authenticated = AuthenticatedAccount {
            account_id: crate::AccountId::new([9; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        let frame = bell_frame();
        assert!(is_bell_transfer(&frame));
        let transition = bell_transition(authenticated, &frame, &accepted_bell_result(10))
            .expect("exact accepted result binds a transition");
        assert_eq!(transition.binding.account_id, [9; 16]);
        assert_eq!(transition.binding.mutation_id, [4; 16]);
        assert_eq!(transition.binding.instance_lineage_id, [6; 16]);
        assert_eq!(transition.binding.entry_restore_point_id, [7; 16]);
        assert_eq!(transition.transfer_id, [8; 16]);
        assert_eq!(transition.destination_character_version, 10);

        assert!(bell_transition(authenticated, &frame, &accepted_bell_result(11)).is_none());
    }

    #[test]
    fn snapshot_publisher_is_15_hz_generation_bound_and_terminal_complete() {
        let mut publisher = SnapshotPublisher::default();
        let observation = |tick, generation| {
            core_private_gameplay_observation_test_fixture(tick, generation, 7, 5)
        };

        let first = publisher
            .prepare(&observation(1, 1), SnapshotAuthority::Danger(1), false)
            .expect("first snapshot")
            .expect("first committed frame publishes");
        assert_eq!(first[0].sequence, 1);
        assert_eq!(first[0].server_tick, 1);

        let second = publisher
            .prepare(&observation(2, 1), SnapshotAuthority::Danger(1), false)
            .expect("next snapshot")
            .expect("next 15 Hz bucket publishes");
        assert_eq!(second[0].sequence, 2);
        assert!(
            publisher
                .prepare(&observation(3, 1), SnapshotAuthority::Danger(1), false)
                .expect("coalesced snapshot")
                .is_none()
        );
        assert!(
            publisher
                .prepare(&observation(4, 1), SnapshotAuthority::Danger(1), false)
                .expect("second bucket")
                .is_some()
        );

        let lethal = publisher
            .prepare(&observation(4, 1), SnapshotAuthority::Danger(1), true)
            .expect("terminal snapshot")
            .expect("same-tick terminal state must publish");
        assert_eq!(lethal[0].sequence, 4);
        assert!(
            publisher
                .prepare(&observation(4, 1), SnapshotAuthority::Danger(1), true)
                .expect("terminal replay")
                .is_none()
        );

        let reconnect_generation = publisher
            .prepare(&observation(1, 2), SnapshotAuthority::Danger(2), false)
            .expect("next generation")
            .expect("new generation publishes immediately");
        assert_eq!(reconnect_generation[0].sequence, 5);
        assert_eq!(reconnect_generation[0].server_tick, 1);
    }
}
