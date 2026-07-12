use std::path::{Path, PathBuf};
use std::sync::Arc;

use bot_client::{BotBehavior, BotTerminalOutcome, JourneyBot};
use protocol::{
    ActionKind, AuthTicket, ClientHello, Compression, HandshakeResponse, ManifestHash, Platform,
    ProtocolVersion, SessionDestination, WireMessage, WireText,
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::PrivatePkcs8KeyDer;
use server_app::{
    AdmissionState, AuthenticationDecision, HandshakePolicy, InputDisposition, ManagedSession,
    SessionDirectory, SessionOwnerId, SessionPhase, TransportId, close_transport,
    receive_managed_gameplay_input, send_gameplay_snapshots, serve_handshake,
    serve_managed_gameplay_reliable, serve_session_control,
};

const MAX_FIGHT_AND_PICKUP_TICKS: u64 = 1_200;
const MAX_DEATH_TICKS: u64 = 5_000;

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn policy() -> HandshakePolicy {
    HandshakePolicy {
        required_protocol: ProtocolVersion::current(),
        required_client_build: WireText::new("bot-journey-1").unwrap(),
        required_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        content_bundle_version: WireText::new("fp.1.0.0").unwrap(),
        region_id: WireText::new("loopback").unwrap(),
        feature_flags: vec![WireText::new("m02-journey").unwrap()],
        admission: AdmissionState::Available,
    }
}

fn hello() -> ClientHello {
    ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new("bot-journey-1").unwrap(),
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        auth_ticket: AuthTicket::new(b"redacted-loopback-ticket".to_vec()).unwrap(),
        locale: WireText::new("en-US").unwrap(),
    }
}

fn endpoints() -> (quinn::Endpoint, quinn::Endpoint, std::net::SocketAddr) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let certificate = cert.der().clone();
    let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
    let server_config =
        quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
            .unwrap();
    let server_endpoint =
        quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let server_address = server_endpoint.local_addr().unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).unwrap();
    let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    client_endpoint.set_default_client_config(client_config);
    (server_endpoint, client_endpoint, server_address)
}

async fn connect_pair(
    server_endpoint: &quinn::Endpoint,
    client_endpoint: &quinn::Endpoint,
    server_address: std::net::SocketAddr,
) -> (quinn::Connection, quinn::Connection) {
    let client = client_endpoint
        .connect(server_address, "localhost")
        .unwrap();
    let server = async {
        server_endpoint
            .accept()
            .await
            .expect("open server endpoint")
            .await
            .expect("accepted QUIC connection")
    };
    let (client, server) = tokio::join!(client, server);
    (client.expect("connected QUIC client"), server)
}

async fn exchange_handshake(client: &quinn::Connection, server: &quinn::Connection) {
    let policy = policy();
    let server_exchange = serve_handshake(
        server,
        &policy,
        AuthenticationDecision::Accepted,
        WireText::new("provisional-bot-session").unwrap(),
    );
    let client_exchange = bot_client::perform_handshake(client, hello());
    let (server_result, client_result) = tokio::join!(server_exchange, client_exchange);
    let server_result = server_result.expect("server handshake");
    let client_result = client_result.expect("bot handshake");
    assert_eq!(server_result, client_result);
    assert!(matches!(client_result, HandshakeResponse::Accepted(_)));
}

#[allow(clippy::too_many_arguments)] // Explicit endpoints and identities make authority auditable.
async fn exchange_control(
    bot: &mut JourneyBot,
    client: &quinn::Connection,
    server: &quinn::Connection,
    directory: &mut SessionDirectory,
    owner: SessionOwnerId,
    transport: TransportId,
    frame: protocol::SessionControlFrame,
    monotonic_micros: u64,
) -> server_app::LifecycleResponse {
    let content_root = content_root();
    let server_exchange = serve_session_control(
        server,
        directory,
        owner,
        transport,
        &content_root,
        monotonic_micros,
    );
    let client_exchange = bot_client::perform_session_control(client, frame);
    let (server_result, client_result) = tokio::join!(server_exchange, client_exchange);
    let server_result = server_result.expect("server control");
    let (event, result) = client_result.expect("bot control");
    assert_eq!(server_result.event, event);
    bot.apply_reliable_event(&event).expect("apply control");
    assert_eq!(result.session_id, bot.logical_session_id().unwrap().clone());
    server_result
}

async fn exchange_reliable(
    bot: &mut JourneyBot,
    client: &quinn::Connection,
    server: &quinn::Connection,
    session: &mut ManagedSession,
    transport: TransportId,
    message: WireMessage,
) {
    let server_exchange = serve_managed_gameplay_reliable(server, session, transport);
    let client_exchange = bot_client::perform_reliable_gameplay(client, message);
    let (server_result, client_result) = tokio::join!(server_exchange, client_exchange);
    let server_result = server_result.expect("server reliable gameplay");
    let client_result = client_result.expect("bot reliable gameplay");
    assert_eq!(
        server_result,
        WireMessage::ReliableEvent(client_result.clone())
    );
    bot.apply_reliable_event(&client_result)
        .expect("apply reliable result");
}

async fn drive_tick(
    bot: &mut JourneyBot,
    client: &quinn::Connection,
    server: &quinn::Connection,
    session: &mut ManagedSession,
    transport: TransportId,
) {
    let input = bot.next_input().expect("active bot input");
    bot_client::send_input_datagram(client, input).expect("bot input datagram");
    assert_eq!(
        receive_managed_gameplay_input(server, session, transport)
            .await
            .expect("managed input"),
        InputDisposition::Accepted
    );
    let snapshots = session.tick().expect("authority tick");
    let snapshot_count = snapshots.len();
    send_gameplay_snapshots(server, snapshots).expect("snapshot datagrams");
    for _ in 0..snapshot_count {
        let snapshot = bot_client::receive_snapshot_datagram(client)
            .await
            .expect("bot snapshot");
        bot.ingest_snapshot(snapshot).expect("snapshot policy");
    }
    if let Some(request) = bot.next_pickup_request().expect("pickup policy") {
        exchange_reliable(
            bot,
            client,
            server,
            session,
            transport,
            WireMessage::MutationRequest(request),
        )
        .await;
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One real-QUIC journey keeps every authority handoff visible.
async fn real_quic_bot_fights_collects_reconnects_and_manually_recalls() {
    let (server_endpoint, client_endpoint, server_address) = endpoints();
    let owner = SessionOwnerId::new(1).unwrap();
    let first_transport = TransportId::new(1).unwrap();
    let second_transport = TransportId::new(2).unwrap();
    let third_transport = TransportId::new(3).unwrap();
    let mut directory = SessionDirectory::default();
    let mut bot = JourneyBot::default();

    let (first_client, first_server) =
        connect_pair(&server_endpoint, &client_endpoint, server_address).await;
    exchange_handshake(&first_client, &first_server).await;
    let join = bot.next_join(10).unwrap();
    exchange_control(
        &mut bot,
        &first_client,
        &first_server,
        &mut directory,
        owner,
        first_transport,
        join,
        100,
    )
    .await;

    for _ in 0..MAX_FIGHT_AND_PICKUP_TICKS {
        drive_tick(
            &mut bot,
            &first_client,
            &first_server,
            directory.session_mut(owner).unwrap(),
            first_transport,
        )
        .await;
        if bot.evidence().mutations_accepted == 1 {
            break;
        }
    }
    assert_eq!(bot.evidence().mutations_accepted, 1);
    assert!(bot.evidence().saw_enemy_damage);
    assert!(bot.evidence().saw_friendly_projectile);
    assert!(bot.evidence().moved_from_first_position);

    let state_before_reconnect = directory.session(owner).unwrap().state_version();
    let player_before_reconnect = bot.observation().unwrap().player.entity_id;
    let (second_client, second_server) =
        connect_pair(&server_endpoint, &client_endpoint, server_address).await;
    exchange_handshake(&second_client, &second_server).await;
    let reconnect = bot.next_reconnect(20).unwrap();
    let handoff = exchange_control(
        &mut bot,
        &second_client,
        &second_server,
        &mut directory,
        owner,
        second_transport,
        reconnect,
        200,
    )
    .await;
    assert_eq!(handoff.invalidated_transport, Some(first_transport));
    close_transport(
        &first_server,
        server_app::TRANSPORT_REPLACED_CLOSE_CODE,
        b"journey reconnect",
    );
    assert_eq!(
        directory.session(owner).unwrap().state_version(),
        state_before_reconnect
    );
    assert_eq!(
        bot.observation().unwrap().player.entity_id,
        player_before_reconnect
    );

    let recall = bot.next_action(ActionKind::RecallStart).unwrap();
    exchange_reliable(
        &mut bot,
        &second_client,
        &second_server,
        directory.session_mut(owner).unwrap(),
        second_transport,
        WireMessage::ActionFrame(recall),
    )
    .await;
    for _ in 0..sim_core::EMERGENCY_RECALL_CHANNEL_TICKS {
        drive_tick(
            &mut bot,
            &second_client,
            &second_server,
            directory.session_mut(owner).unwrap(),
            second_transport,
        )
        .await;
    }
    assert_eq!(bot.terminal_outcome(), BotTerminalOutcome::Recalled);
    assert!(matches!(
        directory.session(owner).unwrap().phase(),
        SessionPhase::Recalled { .. }
    ));

    let (third_client, third_server) =
        connect_pair(&server_endpoint, &client_endpoint, server_address).await;
    exchange_handshake(&third_client, &third_server).await;
    let resolved_reconnect = bot.next_reconnect(30).unwrap();
    exchange_control(
        &mut bot,
        &third_client,
        &third_server,
        &mut directory,
        owner,
        third_transport,
        resolved_reconnect,
        300,
    )
    .await;
    assert_eq!(bot.terminal_outcome(), BotTerminalOutcome::Recalled);
    assert_eq!(bot.evidence().reconnects_accepted, 2);
    assert_eq!(
        directory.session(owner).unwrap().phase().destination(),
        SessionDestination::LanternHalls
    );
    first_client.close(0_u32.into(), b"journey complete");
    second_client.close(0_u32.into(), b"journey complete");
    third_client.close(0_u32.into(), b"journey complete");
    client_endpoint.wait_idle().await;
}

#[tokio::test]
async fn real_quic_bot_observes_authoritative_death_and_death_final_reconnect() {
    let (server_endpoint, client_endpoint, server_address) = endpoints();
    let owner = SessionOwnerId::new(2).unwrap();
    let first_transport = TransportId::new(10).unwrap();
    let second_transport = TransportId::new(11).unwrap();
    let mut directory = SessionDirectory::default();
    let mut bot = JourneyBot::with_behavior(BotBehavior::AwaitAuthoritativeDeath);
    let (first_client, first_server) =
        connect_pair(&server_endpoint, &client_endpoint, server_address).await;
    exchange_handshake(&first_client, &first_server).await;
    let join = bot.next_join(10).unwrap();
    exchange_control(
        &mut bot,
        &first_client,
        &first_server,
        &mut directory,
        owner,
        first_transport,
        join,
        100,
    )
    .await;
    for _ in 0..MAX_DEATH_TICKS {
        drive_tick(
            &mut bot,
            &first_client,
            &first_server,
            directory.session_mut(owner).unwrap(),
            first_transport,
        )
        .await;
        if bot.terminal_outcome() == BotTerminalOutcome::Dead {
            break;
        }
    }
    let SessionPhase::Dead { committed_tick } = directory.session(owner).unwrap().phase() else {
        panic!("authoritative death was not committed");
    };
    assert_eq!(bot.terminal_outcome(), BotTerminalOutcome::Dead);
    assert_eq!(bot.observation().unwrap().server_tick, committed_tick);
    assert_eq!(bot.observation().unwrap().player.current_health, 0);

    let (second_client, second_server) =
        connect_pair(&server_endpoint, &client_endpoint, server_address).await;
    exchange_handshake(&second_client, &second_server).await;
    let reconnect = bot.next_reconnect(20).unwrap();
    exchange_control(
        &mut bot,
        &second_client,
        &second_server,
        &mut directory,
        owner,
        second_transport,
        reconnect,
        200,
    )
    .await;
    assert_eq!(bot.terminal_outcome(), BotTerminalOutcome::Dead);
    assert_eq!(
        directory.session(owner).unwrap().phase().destination(),
        SessionDestination::DeathFinal
    );
    first_client.close(0_u32.into(), b"death observed");
    second_client.close(0_u32.into(), b"death observed");
    client_endpoint.wait_idle().await;
}
