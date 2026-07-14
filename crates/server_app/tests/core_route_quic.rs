use std::{path::PathBuf, sync::Arc};

use persistence::{
    CaldusExtractionCommit, CaldusExtractionRequest, PersistenceConfig, PersistenceTransaction,
    PostgresPersistence, StoredExtractionAuthority, StoredWorldFlowRevisionV1,
    WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    AuthTicket, CharacterLocation, ClientHello, Compression, HandshakeResponse, ManifestHash,
    Platform, ProtocolVersion, SafeArrival, WireText, WorldFlowContentRevisionV1, WorldFlowFrame,
    WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferMutation,
    WorldTransferPayload, WorldTransferResultCode,
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::PrivatePkcs8KeyDer;
use server_app::{
    AccountId, AdmissionState, AuthenticatedAccount, AuthenticatedNamespace,
    AuthenticationDecision, BeltStackV1, CharacterIdGenerator, CoreBargainAuthority,
    CoreOathSelectionAuthority, DisabledProgressionQueryRepository, DisposableCoreJourneyWorldFlow,
    EntryCaptureContext, EntryRestoreProvider, HandshakePolicy, IdentityClock, IdentityService,
    InMemoryAccountRepository, InventorySecurityRestoreV1, NoopIdentityEventSink,
    OathBargainRestoreV1, PostgresCaldusHallTransferCoordinator,
    PostgresDormantWorldFlowCoordinator, PostgresProgressionRestoreProvider,
    ProgressionQueryService, RestorePointError, WorldFlowIdGenerator, serve_core_reliable,
    serve_handshake,
};

const ACCOUNT_ID: [u8; 16] = [211; 16];
const CHARACTER_ID: [u8; 16] = [212; 16];
const TRANSFER_ID: [u8; 16] = [213; 16];
const LINEAGE_ID: [u8; 16] = [214; 16];
const RESTORE_ID: [u8; 16] = [215; 16];
const EXTRACTION_REQUEST_ID: [u8; 16] = [216; 16];
const EXTRACTION_RECEIPT_ID: [u8; 16] = [217; 16];
const HALL_ID: &str = "hub.lantern_halls_01";
const WORLD_ID: &str = "world.core_microrealm_01";

fn content_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence.reset_disposable_identity_data().await.unwrap();
    persistence
}

async fn seed_character(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity,
         selected_character_id) VALUES ($1,$2,1,2,$3)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id,
         level,oath_id,life_state,security_state,character_state_version)
         VALUES ($1,$2,$3,1,'class.grave_arbalist',1,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id,
         character_version,location_kind,location_content_id,safe_arrival_kind)
         VALUES ($1,$2,$3,1,0,NULL,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id,account_id,character_id,total_xp,level,
         current_health,progression_version) VALUES ($1,$2,$3,0,1,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn revision() -> WorldFlowContentRevisionV1 {
    let world = sim_content::load_core_development_world_flow(&content_root()).unwrap();
    WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(world.hashes().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(world.hashes().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(world.hashes().localization_blake3.clone()).unwrap(),
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedAuthority;

impl IdentityClock for FixedAuthority {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

impl CharacterIdGenerator for FixedAuthority {
    fn next_id(&self) -> [u8; 16] {
        [221; 16]
    }
}

impl WorldFlowIdGenerator for FixedAuthority {
    fn next_transfer_id(&self) -> [u8; 16] {
        TRANSFER_ID
    }

    fn next_lineage_id(&self) -> [u8; 16] {
        LINEAGE_ID
    }

    fn next_restore_point_id(&self) -> [u8; 16] {
        RESTORE_ID
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedInventory;

impl EntryRestoreProvider for FixedInventory {
    type Snapshot = InventorySecurityRestoreV1;

    async fn capture<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        Ok(InventorySecurityRestoreV1 {
            equipment: [None; 4],
            belt: [
                BeltStackV1 {
                    consumable_id: None,
                    unit_uids: vec![],
                },
                BeltStackV1 {
                    consumable_id: None,
                    unit_uids: vec![],
                },
            ],
            inventory_version: 1,
        })
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: server_app::CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedOathBargains;

impl EntryRestoreProvider for FixedOathBargains {
    type Snapshot = OathBargainRestoreV1;

    async fn capture<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        Ok(OathBargainRestoreV1 {
            oath_id: None,
            active_bargain_ids: vec![],
            earned_bargain_slots: 0,
            oath_bargain_version: 1,
        })
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: server_app::CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Ok(())
    }
}

fn route_frame(
    sequence: u32,
    mutation_id: [u8; 16],
    version: u64,
    command: WorldTransferCommand,
) -> WorldFlowFrame {
    let payload = WorldTransferPayload {
        content_revision: revision(),
        command,
    };
    WorldFlowFrame {
        sequence,
        request: WorldFlowRequest::Transfer(WorldTransferMutation {
            mutation_id,
            character_id: CHARACTER_ID,
            expected_character_version: version,
            issued_at_unix_millis: 9_000,
            payload_hash: payload.canonical_hash(),
            payload,
        }),
    }
}

async fn commit_caldus_fixture(persistence: &PostgresPersistence) {
    let revision = revision();
    persistence
        .request_caldus_extraction(&CaldusExtractionRequest {
            account_id: ACCOUNT_ID,
            character_id: CHARACTER_ID,
            extraction_request_id: EXTRACTION_REQUEST_ID,
            encounter_id: [222; 16],
            instance_lineage_id: LINEAGE_ID,
            entry_restore_point_id: RESTORE_ID,
            exit_instance_id: [223; 16],
            attempt_ordinal: 1,
            party_slot: 0,
            participant_entity_id: 1,
            expected_character_version: 3,
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: revision.records_blake3.as_str().to_owned(),
                assets_blake3: revision.assets_blake3.as_str().to_owned(),
                localization_blake3: revision.localization_blake3.as_str().to_owned(),
            },
        })
        .await
        .unwrap();
    persistence
        .commit_caldus_extraction(CaldusExtractionCommit {
            extraction_request_id: EXTRACTION_REQUEST_ID,
            extraction_receipt_id: EXTRACTION_RECEIPT_ID,
            authority: StoredExtractionAuthority::WipeableTestEvidence,
        })
        .await
        .unwrap();
}

fn endpoints() -> (quinn::Endpoint, quinn::Endpoint, std::net::SocketAddr) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let certificate = cert.der().clone();
    let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
    let server_config =
        quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
            .unwrap();
    let server = quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let address = server.local_addr().unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).unwrap();
    let config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    let mut client = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    client.set_default_client_config(config);
    (server, client, address)
}

fn policy() -> HandshakePolicy {
    HandshakePolicy {
        required_protocol: ProtocolVersion::current(),
        required_client_build: WireText::new("m03-core-route-journey-1").unwrap(),
        required_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        content_bundle_version: WireText::new("core-dev").unwrap(),
        region_id: WireText::new("loopback").unwrap(),
        feature_flags: vec![WireText::new("core_world_flow_integration").unwrap()],
        admission: AdmissionState::Available,
    }
}

fn hello() -> ClientHello {
    ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new("m03-core-route-journey-1").unwrap(),
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        auth_ticket: AuthTicket::new(b"disposable-core-route".to_vec()).unwrap(),
        locale: WireText::new("en-US").unwrap(),
    }
}

fn assert_accepted(result: &WorldFlowResult, version: u64, location: &str) {
    assert!(matches!(
        result,
        WorldFlowResult::Transfer {
            accepted: true,
            code: WorldTransferResultCode::Accepted,
            snapshot: Some(snapshot),
            ..
        } if snapshot.character_version == version && match &snapshot.location {
            CharacterLocation::Safe { location_id, arrival: SafeArrival::HallDefault }
            | CharacterLocation::Danger { location_id, .. } => location_id.as_str() == location,
            CharacterLocation::Safe { .. } | CharacterLocation::CharacterSelect { .. } => false,
        }
    ));
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "the end-to-end authority sequence stays contiguous for route-bypass auditing"
)]
async fn reliable_quic_traverses_disposable_core_route_and_committed_extraction() {
    let persistence = disposable_database().await;
    seed_character(&persistence).await;
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let route = PostgresDormantWorldFlowCoordinator::new(
        persistence.clone(),
        FixedAuthority,
        FixedAuthority,
        revision(),
        PostgresProgressionRestoreProvider::new(&progression_content).unwrap(),
        FixedInventory,
        FixedOathBargains,
    );
    let extraction =
        PostgresCaldusHallTransferCoordinator::new(persistence.clone(), FixedAuthority, revision());
    let world_flow = DisposableCoreJourneyWorldFlow::new(route, extraction);
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let progression =
        ProgressionQueryService::new(DisabledProgressionQueryRepository, &progression_content)
            .unwrap();
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_journey = async {
        serve_handshake(
            &server,
            &policy(),
            AuthenticationDecision::Accepted,
            WireText::new("core-route-session").unwrap(),
        )
        .await
        .unwrap();
        for response_sequence in 1..=3 {
            serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &oath,
                &bargain,
                authenticated,
                response_sequence,
                0,
            )
            .await
            .unwrap();
        }
    };
    let client_journey = async {
        assert!(matches!(
            bot_client::perform_handshake(&client, hello()).await.unwrap(),
            HandshakeResponse::Accepted(server) if server.feature_flags.iter().any(
                |flag| flag.as_str() == "core_world_flow_integration"
            )
        ));
        let (_, hall) = bot_client::perform_world_flow(
            &client,
            route_frame(
                1,
                [224; 16],
                1,
                WorldTransferCommand::EnterHallFromCharacterSelect,
            ),
        )
        .await
        .unwrap();
        assert_accepted(&hall, 2, HALL_ID);
        let (_, danger) = bot_client::perform_world_flow(
            &client,
            route_frame(
                2,
                [225; 16],
                2,
                WorldTransferCommand::UsePortal {
                    portal_id: WireText::new("station.realm_gate").unwrap(),
                },
            ),
        )
        .await
        .unwrap();
        assert_accepted(&danger, 3, WORLD_ID);
        commit_caldus_fixture(&persistence).await;
        let (_, hall_return) = bot_client::perform_world_flow(
            &client,
            route_frame(
                3,
                [226; 16],
                3,
                WorldTransferCommand::UseCommittedExtraction {
                    portal_id: WireText::new("portal.exit.dungeon.bell_sepulcher").unwrap(),
                    extraction_request_id: EXTRACTION_REQUEST_ID,
                    extraction_receipt_id: EXTRACTION_RECEIPT_ID,
                },
            ),
        )
        .await
        .unwrap();
        assert_accepted(&hall_return, 4, HALL_ID);
    };
    tokio::join!(server_journey, client_journey);

    assert!(matches!(
        persistence.world_location(ACCOUNT_ID, CHARACTER_ID).await.unwrap(),
        Some(persistence::StoredWorldLocation::Safe {
            character_version: 4,
            location_content_id,
            arrival: persistence::StoredSafeArrival::HallDefault,
        }) if location_content_id == HALL_ID
    ));
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"journey complete");
    server_endpoint.wait_idle().await;
    persistence.close().await;
}
