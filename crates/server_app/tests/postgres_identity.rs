use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use persistence::{PersistenceConfig, PostgresPersistence};
use protocol::{
    AccountBootstrapFrame, AccountBootstrapRequest, AccountBootstrapResult, AuthTicket,
    CharacterMutationFrame, CharacterMutationPayload, ClientHello, Compression, HandshakeResponse,
    ManifestHash, Platform, ProtocolVersion, WireText,
};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, BoundCoreIdentityServer,
    CharacterIdGenerator, CoreIdentityServerConfig, CoreIdentityServerReport, IdentityClock,
    IdentityService, LOCAL_SERVER_NAME, LocalServerRuntimeError, NoopIdentityEventSink,
    PostgresAccountRepository,
};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy)]
struct FixedClock;

impl IdentityClock for FixedClock {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

#[derive(Debug, Default)]
struct SequentialIds(AtomicU8);

impl CharacterIdGenerator for SequentialIds {
    fn next_id(&self) -> [u8; 16] {
        [self.0.fetch_add(1, Ordering::Relaxed) + 1; 16]
    }
}

fn manifest() -> ManifestHash {
    ManifestHash::new("a".repeat(64)).unwrap()
}

fn account(value: u8) -> AuthenticatedAccount {
    AuthenticatedAccount {
        account_id: AccountId::new([value; 16]).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

fn service(
    persistence: PostgresPersistence,
) -> IdentityService<PostgresAccountRepository, FixedClock, SequentialIds, NoopIdentityEventSink> {
    IdentityService::new(
        PostgresAccountRepository::new(persistence),
        FixedClock,
        SequentialIds::default(),
        NoopIdentityEventSink,
        manifest(),
    )
}

fn bootstrap() -> AccountBootstrapFrame {
    AccountBootstrapFrame {
        sequence: 1,
        request: AccountBootstrapRequest::Bootstrap,
        content_manifest_hash: manifest(),
    }
}

fn mutation(id: u8, version: u64, payload: CharacterMutationPayload) -> CharacterMutationFrame {
    CharacterMutationFrame {
        mutation_id: [id; 16],
        expected_account_version: version,
        payload_hash: payload.canonical_hash(),
        issued_at_unix_millis: 9_000,
        payload,
    }
}

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn wire_hello(content_root: &Path, ticket: &[u8]) -> ClientHello {
    let (_, report) = sim_content::load_and_validate(content_root).unwrap();
    ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new(server_app::CORE_IDENTITY_BUILD_ID).unwrap(),
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: ManifestHash::new(report.package_hash_blake3).unwrap(),
        auth_ticket: AuthTicket::new(ticket.to_vec()).unwrap(),
        locale: WireText::new("en-US").unwrap(),
    }
}

fn wire_bootstrap(content_root: &Path, sequence: u32) -> AccountBootstrapFrame {
    let (_, report) = sim_content::load_and_validate(content_root).unwrap();
    AccountBootstrapFrame {
        sequence,
        request: AccountBootstrapRequest::Bootstrap,
        content_manifest_hash: ManifestHash::new(report.package_hash_blake3).unwrap(),
    }
}

fn current_unix_millis() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn client_endpoint(certificate: rustls::pki_types::CertificateDer<'static>) -> quinn::Endpoint {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).unwrap();
    let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(client_config);
    endpoint
}

type ServerTask =
    tokio::task::JoinHandle<Result<CoreIdentityServerReport, LocalServerRuntimeError>>;

fn start_server(
    content_root: &Path,
    persistence: PostgresPersistence,
) -> (
    std::net::SocketAddr,
    rustls::pki_types::CertificateDer<'static>,
    oneshot::Sender<()>,
    ServerTask,
) {
    let server = BoundCoreIdentityServer::bind_persistent(
        &CoreIdentityServerConfig {
            bind_address: "127.0.0.1:0".parse().unwrap(),
            content_root: content_root.to_path_buf(),
        },
        PostgresAccountRepository::new(persistence),
    )
    .unwrap();
    let address = server.local_address();
    let certificate = rustls::pki_types::CertificateDer::from(server.certificate_der().to_vec());
    let (shutdown_send, shutdown_receive) = oneshot::channel();
    let task = tokio::spawn(server.serve_until(async {
        let _ = shutdown_receive.await;
    }));
    (address, certificate, shutdown_send, task)
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn postgres_identity_survives_service_restart_and_replays_exactly_once() {
    let config = PersistenceConfig::from_test_environment().unwrap();
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.migrate().await.unwrap();
    persistence.reset_disposable_identity_data().await.unwrap();

    let first_process = service(persistence.clone());
    let create = mutation(
        1,
        1,
        CharacterMutationPayload::Create {
            class_id: WireText::new(protocol::GRAVE_ARBALIST_CLASS_ID).unwrap(),
        },
    );
    let created = first_process.mutate(Some(account(91)), &create).await;
    assert!(created.accepted);
    assert_eq!(
        first_process.mutate(Some(account(91)), &create).await,
        created
    );
    let character_id = created.snapshot.as_ref().unwrap().characters[0].character_id;
    let selected = first_process
        .mutate(
            Some(account(91)),
            &mutation(2, 2, CharacterMutationPayload::Select { character_id }),
        )
        .await;
    assert!(selected.accepted);
    drop(first_process);
    persistence.close().await;

    let restarted_persistence = PostgresPersistence::connect(&config).await.unwrap();
    let restarted = service(restarted_persistence.clone());
    let AccountBootstrapResult::Snapshot(snapshot) =
        restarted.bootstrap(Some(account(91)), &bootstrap()).await
    else {
        panic!("durable account snapshot expected")
    };
    assert_eq!(snapshot.account_version, 3);
    assert_eq!(snapshot.characters.len(), 1);
    assert_eq!(snapshot.selected_character_id, Some(character_id));
    let AccountBootstrapResult::Snapshot(isolated) =
        restarted.bootstrap(Some(account(92)), &bootstrap()).await
    else {
        panic!("isolated account snapshot expected")
    };
    assert!(isolated.characters.is_empty());
    restarted_persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn postgres_real_quic_server_restart_preserves_authoritative_roster() {
    let config = PersistenceConfig::from_test_environment().unwrap();
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.migrate().await.unwrap();
    persistence.reset_disposable_identity_data().await.unwrap();
    let content_root = content_root();
    let ticket = b"postgres-real-quic-account";

    let (address, certificate, shutdown, task) = start_server(&content_root, persistence.clone());
    let endpoint = client_endpoint(certificate);
    let connection = endpoint
        .connect(address, LOCAL_SERVER_NAME)
        .unwrap()
        .await
        .unwrap();
    assert!(matches!(
        bot_client::perform_handshake(&connection, wire_hello(&content_root, ticket))
            .await
            .unwrap(),
        HandshakeResponse::Accepted(_)
    ));
    let (_, initial) =
        bot_client::perform_account_bootstrap(&connection, wire_bootstrap(&content_root, 1))
            .await
            .unwrap();
    assert!(
        matches!(initial, AccountBootstrapResult::Snapshot(ref value) if value.characters.is_empty())
    );
    let payload = CharacterMutationPayload::Create {
        class_id: WireText::new(protocol::GRAVE_ARBALIST_CLASS_ID).unwrap(),
    };
    let (_, created) = bot_client::perform_character_mutation(
        &connection,
        CharacterMutationFrame {
            mutation_id: [71; 16],
            expected_account_version: 1,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: current_unix_millis(),
            payload,
        },
    )
    .await
    .unwrap();
    assert!(created.accepted);
    connection.close(0_u32.into(), b"durable restart");
    shutdown.send(()).unwrap();
    let report = task.await.unwrap().unwrap();
    assert!(report.persistence_enabled);
    assert_eq!(report.combat_sessions_admitted, 0);
    endpoint.wait_idle().await;

    let (address, certificate, shutdown, task) = start_server(&content_root, persistence.clone());
    let endpoint = client_endpoint(certificate);
    let connection = endpoint
        .connect(address, LOCAL_SERVER_NAME)
        .unwrap()
        .await
        .unwrap();
    bot_client::perform_handshake(&connection, wire_hello(&content_root, ticket))
        .await
        .unwrap();
    let (_, restored) =
        bot_client::perform_account_bootstrap(&connection, wire_bootstrap(&content_root, 2))
            .await
            .unwrap();
    let AccountBootstrapResult::Snapshot(restored) = restored else {
        panic!("restored durable snapshot expected")
    };
    assert_eq!(restored.characters.len(), 1);
    connection.close(0_u32.into(), b"complete");
    shutdown.send(()).unwrap();
    assert!(task.await.unwrap().unwrap().persistence_enabled);
    endpoint.wait_idle().await;
    persistence.close().await;
}
