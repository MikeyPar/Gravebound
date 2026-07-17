use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use persistence::{
    CORE_ITEM_CONTENT_REVISION, PersistenceConfig, PersistenceError, PostgresPersistence,
    WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    AccountBootstrapFrame, AccountBootstrapRequest, AccountBootstrapResult, AuthTicket,
    CORE_SAFE_INVENTORY_FEATURE_FLAG, ClientHello, Compression, DeathViewContentRevisionV1,
    HandshakeResponse, ManifestHash, Platform, ProgressionProjection, ProgressionQueryFrame,
    ProgressionResult, ProtocolVersion, SafeInventoryDestinationV1, SafeInventoryResultCodeV1,
    SafeInventoryTransferFrameV1, SafeInventoryTransferKindV1, SafeInventoryTransferPayloadV1,
    WireText, WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest, WorldFlowResult,
    WorldTransferCommand, WorldTransferMutation, WorldTransferPayload, WorldTransferResultCode,
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::PrivatePkcs8KeyDer;
use server_app::{
    AccountId, AdmissionState, AuthenticatedAccount, AuthenticatedNamespace,
    AuthenticationDecision, CharacterIdGenerator, CoreBargainAuthority,
    CoreExtractionTerminalAuthority, CoreOathSelectionAuthority, CoreRecallTerminalAuthority,
    CoreResolutionHoldAuthority, CoreSafeInventoryAuthority, DeathViewService,
    DisabledDeathViewRepository, FieldEquipmentConfirmCommand, FieldEquipmentPreviewSource,
    HandshakePolicy, IdentityClock, IdentityService, NoopIdentityEventSink,
    PostgresAccountRepository, PostgresFieldEquipmentService, PostgresProgressionAwardService,
    PostgresProgressionQueryRepository, PostgresRewardService, PostgresSafeInventoryService,
    PostgresWorldFlowLocationRepository, ProgressionAwardCode, ProgressionAwardCommand,
    ProgressionAwardEvidence, ProgressionAwardPayload, ProgressionQueryService, RewardGrantContext,
    RewardGrantTransaction, RewardPlacement, SafeInventoryServiceError, SecretRewardEpoch,
    WorldFlowGateService, initialize_postgres_starter, serve_core_reliable, serve_handshake,
};
use sim_core::{EncounterXpEvidence, RewardLifeState, RewardRecallState, RewardTrustState};

// The mandatory PostgreSQL job shares one database across integration binaries. Keep this
// fixture's identities disjoint so cleanup never depends on another test's foreign-key graph.
const ACCOUNT_ID: [u8; 16] = [210; 16];
const CHARACTER_ID: [u8; 16] = [211; 16];
const ITEM_UID: [u8; 16] = [212; 16];
const CREATION_REQUEST_ID: [u8; 16] = [213; 16];
const MUTATION_ID: [u8; 16] = [214; 16];

fn content_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn death_view_revision() -> DeathViewContentRevisionV1 {
    let content = sim_content::load_core_development_death_view(&content_root()).unwrap();
    DeathViewContentRevisionV1 {
        records_blake3: ManifestHash::new(content.hashes().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(content.hashes().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(content.hashes().localization_blake3.clone())
            .unwrap(),
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
        [230; 16]
    }
}

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

async fn clear_fixture(transaction: &mut persistence::PersistenceTransaction<'_>) {
    sqlx::query(
        "DELETE FROM field_equipment_mutations WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
}

async fn seed_fixture(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    clear_fixture(&mut transaction).await;
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity) \
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id, \
         level,oath_id,life_state,security_state,character_state_version) \
         VALUES ($1,$2,$3,1,'class.grave_arbalist',1,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(CHARACTER_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories \
         (namespace_id,account_id,character_id,inventory_version) VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id,account_id,character_id,total_xp,level, \
         current_health,progression_version) VALUES ($1,$2,$3,0,1,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id,account_id,character_id, \
         earned_bargain_slots,oath_bargain_version) VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,location_content_id,safe_arrival_kind) \
         VALUES ($1,$2,$3,1,1,'hub.lantern_halls_01',0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
         content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
         unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
         salvage_band,salvage_value) VALUES ($1,$2,$3,$4,'item.weapon.crossbow.pine_crossbow', \
         $5,0,1,0,0,$6,0,0,1,0,5,0,0,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ITEM_UID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(CREATION_REQUEST_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    initialize_postgres_starter(persistence, ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
}

async fn insert_equipment(
    transaction: &mut persistence::PersistenceTransaction<'_>,
    item_uid: [u8; 16],
    character_id: Option<[u8; 16]>,
    location_kind: i16,
    slot_index: i16,
) {
    sqlx::query(
        "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
         content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
         unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
         salvage_band,salvage_value) VALUES ($1,$2,$3,$4,'item.weapon.crossbow.pine_crossbow', \
         $5,0,1,0,0,$2,0,0,1,0,$6,$7,0,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item_uid.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(character_id.map(|id| id.to_vec()))
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(location_kind)
    .bind(slot_index)
    .execute(transaction.connection())
    .await
    .unwrap();
}

async fn seed_final_vault_slot_race(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    insert_equipment(&mut transaction, [215; 16], Some(CHARACTER_ID), 5, 1).await;
    for slot in 0_i16..159 {
        let item_uid = (30_000_u128 + u128::try_from(slot).unwrap()).to_be_bytes();
        insert_equipment(&mut transaction, item_uid, None, 6, slot).await;
    }
    transaction.commit().await.unwrap();
}

fn transfer_frame() -> SafeInventoryTransferFrameV1 {
    transfer_frame_at(2)
}

fn transfer_frame_at(expected_inventory_version: u64) -> SafeInventoryTransferFrameV1 {
    let payload = SafeInventoryTransferPayloadV1 {
        kind: SafeInventoryTransferKindV1::CharacterSafeToVault,
        source_slot_index: 0,
        expected_account_version: 1,
        expected_inventory_version,
    };
    SafeInventoryTransferFrameV1 {
        mutation_id: MUTATION_ID,
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn conflicting_transfer_frame_at(expected_inventory_version: u64) -> SafeInventoryTransferFrameV1 {
    let mut frame = transfer_frame_at(expected_inventory_version);
    frame.payload.source_slot_index = 1;
    frame.payload_hash = frame.payload.canonical_hash();
    frame
}

fn stale_transfer_frame() -> SafeInventoryTransferFrameV1 {
    let mut frame = transfer_frame_at(3);
    frame.mutation_id = [231; 16];
    frame
}

fn foreign_character_transfer_frame() -> SafeInventoryTransferFrameV1 {
    let mut frame = transfer_frame_at(4);
    frame.mutation_id = [232; 16];
    frame.character_id = [233; 16];
    frame
}

fn caldus_progression_command(
    progression_content: &sim_content::CoreDevelopmentProgression,
) -> ProgressionAwardCommand {
    let payload = ProgressionAwardPayload {
        character_id: CHARACTER_ID,
        expected_progression_version: 1,
        source_content_id: "boss.sir_caldus".to_owned(),
        progression_content_revision: ManifestHash::new(
            progression_content.hashes().records_blake3.clone(),
        )
        .unwrap(),
        evidence: ProgressionAwardEvidence::Encounter(EncounterXpEvidence {
            active_ticks: 5_400,
            present_ticks: 5_400,
            longest_inactivity_ticks: 0,
            encounter_contribution_reference_health: 7_200,
            direct_damage: 100,
            effective_healing_to_others: 0,
            damage_prevented_on_others: 0,
            qualifying_objective_credits: 0,
            life_state: RewardLifeState::Living,
            recall_state: RewardRecallState::Eligible,
            trust_state: RewardTrustState::Valid,
        }),
    };
    ProgressionAwardCommand {
        reward_event_id: [225; 16],
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

async fn stage_progression_reward_and_equipment(persistence: &PostgresPersistence) {
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let oath_bargain_content =
        sim_content::load_core_development_oaths_bargains(&content_root()).unwrap();
    let command = caldus_progression_command(&progression_content);
    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let progression = PostgresProgressionAwardService::new(
        persistence.clone(),
        &progression_content,
        &oath_bargain_content,
    )
    .unwrap();
    let awarded = progression.award(authenticated, &command).await;
    assert_eq!(awarded.code, ProgressionAwardCode::Accepted);
    assert_eq!(progression.award(authenticated, &command).await, awarded);

    let rewards = PostgresRewardService::load(
        persistence.clone(),
        &content_root(),
        SecretRewardEpoch::new("m03-04g-lifecycle", [0x5a; 32]).unwrap(),
    )
    .unwrap();
    let context = RewardGrantContext {
        reward_request_id: [226; 16],
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        source_instance_id: [227; 16],
        reward_table_id: "reward.boss_caldus",
        current_tick: 9_000,
    };
    let RewardGrantTransaction::Fresh { result, .. } = rewards.grant(context).await.unwrap() else {
        panic!("first lifecycle reward must be fresh")
    };
    assert!(matches!(
        rewards.grant(context).await.unwrap(),
        RewardGrantTransaction::Replay { result: ref replay, .. } if replay == &result
    ));
    let source_slot_index = result
        .items
        .iter()
        .find_map(|item| match (&item.placement, item.item_level) {
            (RewardPlacement::RunBackpack { slot_index }, Some(_)) => Some(*slot_index),
            _ => None,
        })
        .expect("Caldus lifecycle reward must contain RunBackpack equipment");
    let equipment =
        PostgresFieldEquipmentService::load(persistence.clone(), &content_root()).unwrap();
    let source = FieldEquipmentPreviewSource::RunBackpack {
        slot_index: source_slot_index,
    };
    let preview = equipment
        .preview(ACCOUNT_ID, CHARACTER_ID, source, 9_001)
        .await
        .unwrap();
    let confirmation = FieldEquipmentConfirmCommand {
        command_id: [228; 16],
        source,
        preview_hash: preview.mutation.preview_hash,
        now_tick: 9_001,
    };
    let committed = equipment
        .confirm(ACCOUNT_ID, CHARACTER_ID, confirmation)
        .await
        .unwrap();
    assert!(!committed.result.replayed);
    assert!(
        equipment
            .confirm(ACCOUNT_ID, CHARACTER_ID, confirmation)
            .await
            .unwrap()
            .result
            .replayed
    );
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
        required_client_build: WireText::new("m03-safe-inventory-journey-1").unwrap(),
        required_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        content_bundle_version: WireText::new("core-dev").unwrap(),
        region_id: WireText::new("loopback").unwrap(),
        feature_flags: vec![WireText::new(CORE_SAFE_INVENTORY_FEATURE_FLAG).unwrap()],
        admission: AdmissionState::Available,
    }
}

fn hello() -> ClientHello {
    ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new("m03-safe-inventory-journey-1").unwrap(),
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        auth_ticket: AuthTicket::new(b"disposable-safe-inventory".to_vec()).unwrap(),
        locale: WireText::new("en-US").unwrap(),
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct QuicTransferOptions {
    drop_first_safe_response: bool,
    close_database_before_first_safe_request: bool,
}

#[derive(Debug)]
struct QuicJourneyOutcome {
    transfers: Vec<protocol::SafeInventoryTransferResultV1>,
    login_to_control: Duration,
    mutation_round_trips: Vec<Duration>,
}

#[allow(
    clippy::too_many_lines,
    reason = "the disposable QUIC composition and client journey stay visible as one audit boundary"
)]
async fn run_quic_transfer_journey(
    persistence: &PostgresPersistence,
    frames: &[SafeInventoryTransferFrameV1],
    options: QuicTransferOptions,
) -> QuicJourneyOutcome {
    let identity = IdentityService::new(
        PostgresAccountRepository::new(persistence.clone()),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow_content =
        sim_content::load_core_development_world_flow(&content_root()).unwrap();
    let world_flow_revision = WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(world_flow_content.hashes().records_blake3.clone())
            .unwrap(),
        assets_blake3: ManifestHash::new(world_flow_content.hashes().assets_blake3.clone())
            .unwrap(),
        localization_blake3: ManifestHash::new(
            world_flow_content.hashes().localization_blake3.clone(),
        )
        .unwrap(),
    };
    let world_flow = WorldFlowGateService::new(
        PostgresWorldFlowLocationRepository::new(persistence.clone()),
        FixedAuthority,
        world_flow_revision.clone(),
    );
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let progression_revision =
        ManifestHash::new(progression_content.hashes().records_blake3.clone()).unwrap();
    let progression = ProgressionQueryService::new(
        PostgresProgressionQueryRepository::new(persistence.clone(), &progression_content).unwrap(),
        &progression_content,
    )
    .unwrap();
    let death_views = DeathViewService::new(DisabledDeathViewRepository, death_view_revision());
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::persistent(PostgresSafeInventoryService::new(
        persistence.clone(),
    ));
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_terminal = CoreRecallTerminalAuthority::disabled();
    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let login_started = Instant::now();
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
            WireText::new("safe-inventory-session").unwrap(),
        )
        .await
        .unwrap();
        for response_sequence in 1..=frames.len() + 3 {
            let result = serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &death_views,
                &oath,
                &bargain,
                &safe_inventory,
                &CoreResolutionHoldAuthority::disabled(),
                &extraction_terminal,
                &recall_terminal,
                authenticated,
                u32::try_from(response_sequence).unwrap(),
                0,
            )
            .await;
            if !options.drop_first_safe_response || response_sequence != 4 {
                result.unwrap();
            }
        }
    };
    let client_journey = async {
        assert!(matches!(
            bot_client::perform_handshake(&client, hello()).await.unwrap(),
            HandshakeResponse::Accepted(server) if server.feature_flags.iter().any(
                |flag| flag.as_str() == CORE_SAFE_INVENTORY_FEATURE_FLAG
            )
        ));
        let (_, bootstrap) = bot_client::perform_account_bootstrap(
            &client,
            AccountBootstrapFrame {
                sequence: 1,
                request: AccountBootstrapRequest::Bootstrap,
                content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            bootstrap,
            AccountBootstrapResult::Snapshot(snapshot)
                if snapshot.selected_character_id == Some(CHARACTER_ID)
                    && snapshot.characters.len() == 1
        ));
        let (_, progression) = bot_client::perform_progression_query(
            &client,
            ProgressionQueryFrame {
                sequence: 2,
                character_id: CHARACTER_ID,
                progression_content_revision: progression_revision,
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            progression,
            ProgressionResult::Snapshot {
                projection: ProgressionProjection {
                    level: 4,
                    total_xp: 675,
                    current_health: 120,
                    progression_version: 2,
                    ..
                },
                ..
            }
        ));
        let login_to_control = login_started.elapsed();
        let world_payload = WorldTransferPayload {
            content_revision: world_flow_revision,
            command: WorldTransferCommand::UsePortal {
                portal_id: WireText::new("station.realm_gate").unwrap(),
            },
        };
        let (_, world_result) = bot_client::perform_world_flow(
            &client,
            WorldFlowFrame {
                sequence: 3,
                request: WorldFlowRequest::Transfer(WorldTransferMutation {
                    mutation_id: [229; 16],
                    character_id: CHARACTER_ID,
                    expected_character_version: 1,
                    issued_at_unix_millis: 1,
                    payload_hash: world_payload.canonical_hash(),
                    payload: world_payload,
                }),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            world_result,
            WorldFlowResult::Transfer {
                accepted: false,
                code: WorldTransferResultCode::StageDisabled,
                transfer_id: None,
                ..
            }
        ));
        if options.close_database_before_first_safe_request {
            persistence.clone().close().await;
        }
        let mut results = Vec::with_capacity(frames.len());
        let mut mutation_round_trips = Vec::with_capacity(frames.len());
        for (index, frame) in frames.iter().enumerate() {
            if options.drop_first_safe_response && index == 0 {
                bot_client::submit_safe_inventory_without_response(&client, *frame)
                    .await
                    .unwrap();
                continue;
            }
            let mutation_started = Instant::now();
            let (_, result) = bot_client::perform_safe_inventory_transfer(&client, *frame)
                .await
                .unwrap();
            mutation_round_trips.push(mutation_started.elapsed());
            results.push(result);
        }
        (results, login_to_control, mutation_round_trips)
    };
    let ((), (transfers, login_to_control, mutation_round_trips)) =
        tokio::join!(server_journey, client_journey);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"safe-inventory journey complete");
    server_endpoint.wait_idle().await;
    QuicJourneyOutcome {
        transfers,
        login_to_control,
        mutation_round_trips,
    }
}

async fn run_quic_transfers(
    persistence: &PostgresPersistence,
    frames: &[SafeInventoryTransferFrameV1],
    options: QuicTransferOptions,
) -> Vec<protocol::SafeInventoryTransferResultV1> {
    run_quic_transfer_journey(persistence, frames, options)
        .await
        .transfers
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn real_quic_safe_inventory_replays_across_a_new_endpoint() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    stage_progression_reward_and_equipment(&persistence).await;
    let lifecycle_frame = transfer_frame_at(4);
    let lifecycle_conflict = conflicting_transfer_frame_at(4);
    let initial = run_quic_transfers(
        &persistence,
        &[
            stale_transfer_frame(),
            foreign_character_transfer_frame(),
            lifecycle_frame,
            lifecycle_frame,
            lifecycle_conflict,
        ],
        QuicTransferOptions::default(),
    )
    .await;
    assert_eq!(initial[0].code, SafeInventoryResultCodeV1::VersionMismatch);
    assert_eq!(
        initial[1].code,
        SafeInventoryResultCodeV1::HallBindingRequired
    );
    for rejected in &initial[..2] {
        rejected.validate().unwrap();
        assert!(!rejected.replayed);
        assert_eq!(rejected.result_hash, [0; 32]);
        assert_eq!(
            (rejected.account_version, rejected.inventory_version),
            (0, 0)
        );
        assert!(rejected.placements.is_empty());
    }
    assert_eq!(initial[2].code, SafeInventoryResultCodeV1::Accepted);
    assert!(!initial[2].replayed);
    assert!(initial[3].replayed);
    assert_eq!(initial[2].result_hash, initial[3].result_hash);
    assert_eq!(
        initial[4].code,
        SafeInventoryResultCodeV1::IdempotencyConflict
    );
    assert!(!initial[4].replayed);
    assert_eq!(initial[4].result_hash, [0; 32]);
    assert!(initial[4].placements.is_empty());
    let before_reconnect = persistence
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    let before_reconnect_bytes = before_reconnect.canonical_bytes().unwrap();
    let before_reconnect_digest = before_reconnect.digest().unwrap();

    let reconnected = run_quic_transfers(
        &persistence,
        &[lifecycle_frame, lifecycle_conflict],
        QuicTransferOptions::default(),
    )
    .await;
    assert_eq!(reconnected[0].code, SafeInventoryResultCodeV1::Accepted);
    assert!(reconnected[0].replayed);
    assert_eq!(
        reconnected[1].code,
        SafeInventoryResultCodeV1::IdempotencyConflict
    );
    let after_reconnect = persistence
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    assert_eq!(after_reconnect, before_reconnect);
    assert_eq!(
        after_reconnect.canonical_bytes().unwrap(),
        before_reconnect_bytes
    );
    assert_eq!(after_reconnect.digest().unwrap(), before_reconnect_digest);
    persistence.close().await;

    let restarted = disposable_database().await;
    let replay = run_quic_transfers(
        &restarted,
        &[lifecycle_frame, lifecycle_conflict],
        QuicTransferOptions::default(),
    )
    .await;
    assert_eq!(replay[0].code, SafeInventoryResultCodeV1::Accepted);
    assert!(replay[0].replayed);
    assert_eq!(replay[0].result_hash, initial[2].result_hash);
    assert_eq!(
        replay[1].code,
        SafeInventoryResultCodeV1::IdempotencyConflict
    );
    let after_restart = restarted
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    assert_eq!(after_restart, before_reconnect);
    assert_eq!(
        after_restart.canonical_bytes().unwrap(),
        before_reconnect_bytes
    );
    assert_eq!(after_restart.digest().unwrap(), before_reconnect_digest);
    assert_committed_state(&restarted).await;
    restarted.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn dropped_quic_response_retries_the_stored_lifecycle_result() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    stage_progression_reward_and_equipment(&persistence).await;
    let frame = transfer_frame_at(4);
    let dropped = run_quic_transfers(
        &persistence,
        &[frame],
        QuicTransferOptions {
            drop_first_safe_response: true,
            ..QuicTransferOptions::default()
        },
    )
    .await;
    assert!(dropped.is_empty());
    let retry = run_quic_transfers(&persistence, &[frame], QuicTransferOptions::default()).await;
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].code, SafeInventoryResultCodeV1::Accepted);
    assert!(retry[0].replayed);
    assert_eq!(retry[0].placements.len(), 1);
    let signature = persistence
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    assert_eq!(signature.safe_inventory_receipts.len(), 1);
    assert_eq!(
        signature.safe_inventory_receipts[0].result_hash,
        retry[0].result_hash
    );
    assert_committed_state(&persistence).await;
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn duplicate_quic_sessions_converge_on_one_inventory_commit() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    stage_progression_reward_and_equipment(&persistence).await;
    let frame = transfer_frame_at(4);
    let left_frames = [frame];
    let right_frames = [frame];
    let (left, right) = tokio::join!(
        run_quic_transfers(&persistence, &left_frames, QuicTransferOptions::default()),
        run_quic_transfers(&persistence, &right_frames, QuicTransferOptions::default()),
    );
    assert_eq!(left.len(), 1);
    assert_eq!(right.len(), 1);
    assert_eq!(left[0].code, SafeInventoryResultCodeV1::Accepted);
    assert_eq!(right[0].code, SafeInventoryResultCodeV1::Accepted);
    assert_eq!(
        usize::from(left[0].replayed) + usize::from(right[0].replayed),
        1
    );
    assert_eq!(left[0].result_hash, right[0].result_hash);
    let signature = persistence
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    assert_eq!(signature.safe_inventory_receipts.len(), 1);
    assert_committed_state(&persistence).await;
    persistence.close().await;
}

fn duration_stats(samples: &mut [Duration]) -> (Duration, Duration, Duration) {
    assert!(!samples.is_empty());
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let p95_index = (samples.len() * 95).div_ceil(100).saturating_sub(1);
    (median, samples[p95_index], samples[samples.len() - 1])
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn completes_25_item_lifecycle_journeys_within_login_budget() {
    let persistence = disposable_database().await;
    let mut login_samples = Vec::with_capacity(25);
    let mut mutation_samples = Vec::with_capacity(25);
    for journey_index in 0..25 {
        seed_fixture(&persistence).await;
        stage_progression_reward_and_equipment(&persistence).await;
        let outcome = run_quic_transfer_journey(
            &persistence,
            &[transfer_frame_at(4)],
            QuicTransferOptions::default(),
        )
        .await;
        assert_eq!(outcome.transfers.len(), 1, "journey {journey_index}");
        assert_eq!(
            outcome.transfers[0].code,
            SafeInventoryResultCodeV1::Accepted,
            "journey {journey_index}"
        );
        assert!(!outcome.transfers[0].replayed, "journey {journey_index}");
        assert_eq!(
            outcome.mutation_round_trips.len(),
            1,
            "journey {journey_index}"
        );
        let signature = persistence
            .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await
            .unwrap();
        assert_eq!(signature.safe_inventory_receipts.len(), 1);
        assert_eq!(signature.progression.as_ref().unwrap().level, 4);
        assert_eq!(signature.progression.as_ref().unwrap().total_xp, 675);

        let mut cleanup = persistence.begin_transaction().await.unwrap();
        let idle_transactions: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() \
             AND state='idle in transaction'",
        )
        .fetch_one(cleanup.connection())
        .await
        .unwrap();
        cleanup.rollback().await.unwrap();
        assert_eq!(idle_transactions, 0, "journey {journey_index}");
        login_samples.push(outcome.login_to_control);
        mutation_samples.extend(outcome.mutation_round_trips);
    }
    let (login_median, login_p95, login_max) = duration_stats(&mut login_samples);
    let (mutation_median, mutation_p95, mutation_max) = duration_stats(&mut mutation_samples);
    eprintln!(
        "GB-M03-04G journeys=25 login_ms median={:.3} p95={:.3} max={:.3}; \
         mutation_ms median={:.3} p95={:.3} max={:.3}",
        login_median.as_secs_f64() * 1_000.0,
        login_p95.as_secs_f64() * 1_000.0,
        login_max.as_secs_f64() * 1_000.0,
        mutation_median.as_secs_f64() * 1_000.0,
        mutation_p95.as_secs_f64() * 1_000.0,
        mutation_max.as_secs_f64() * 1_000.0,
    );
    assert!(login_median < Duration::from_secs(30));
    persistence.close().await;
}

fn assert_state_free_service_unavailable(result: &protocol::SafeInventoryTransferResultV1) {
    result.validate().unwrap();
    assert_eq!(result.code, SafeInventoryResultCodeV1::ServiceUnavailable);
    assert!(!result.replayed);
    assert_eq!(result.result_hash, [0; 32]);
    assert_eq!((result.account_version, result.inventory_version), (0, 0));
    assert!(result.placements.is_empty());
}

async fn update_receipt_placement_count(persistence: &PostgresPersistence, count: i16) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE safe_inventory_mutations SET placement_count=$1 WHERE namespace_id=$2 \
         AND account_id=$3 AND mutation_id=$4",
    )
    .bind(count)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(MUTATION_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();
}

async fn update_receipt_result_hash(persistence: &PostgresPersistence, hash: [u8; 32]) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE safe_inventory_mutations SET result_hash=$1 WHERE namespace_id=$2 \
         AND account_id=$3 AND mutation_id=$4",
    )
    .bind(hash.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(MUTATION_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();
}

async fn update_fixture_item_content_revision(
    persistence: &PostgresPersistence,
    content_revision: &str,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE item_instances SET content_revision=$1 WHERE namespace_id=$2 AND item_uid=$3",
    )
    .bind(content_revision)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ITEM_UID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();
}

async fn safe_transfer_durable_counts(persistence: &PostgresPersistence) -> (i64, i64, i64) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let counts = sqlx::query_as(
        "SELECT (SELECT count(*) FROM safe_inventory_mutations WHERE namespace_id=$1 \
         AND account_id=$2 AND mutation_id=$3),(SELECT count(*) FROM safe_inventory_placements \
         WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$3),(SELECT count(*) FROM \
         item_ledger_events WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$3)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(MUTATION_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    counts
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn database_outage_returns_a_state_free_quic_rejection() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    stage_progression_reward_and_equipment(&persistence).await;
    let before_outage = persistence
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    let results = run_quic_transfers(
        &persistence,
        &[transfer_frame_at(4)],
        QuicTransferOptions {
            close_database_before_first_safe_request: true,
            ..QuicTransferOptions::default()
        },
    )
    .await;
    assert_eq!(results.len(), 1);
    assert_state_free_service_unavailable(&results[0]);

    let restarted = disposable_database().await;
    let after_outage = restarted
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    assert_eq!(after_outage, before_outage);
    restarted.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn foreign_item_content_revision_fails_closed_before_storage_mutation() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    stage_progression_reward_and_equipment(&persistence).await;
    let baseline = persistence
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    let alternate_revision = format!("core-dev.blake3.{}", "f".repeat(64));
    assert_ne!(alternate_revision, CORE_ITEM_CONTENT_REVISION);
    update_fixture_item_content_revision(&persistence, &alternate_revision).await;
    assert!(matches!(
        persistence
            .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredLifecycleSignature)
    ));

    let frame = transfer_frame_at(4);
    let rejected = run_quic_transfers(&persistence, &[frame], QuicTransferOptions::default()).await;
    assert_state_free_service_unavailable(&rejected[0]);
    assert_eq!(safe_transfer_durable_counts(&persistence).await, (0, 0, 0));

    update_fixture_item_content_revision(&persistence, CORE_ITEM_CONTENT_REVISION).await;
    assert_eq!(
        persistence
            .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await
            .unwrap(),
        baseline
    );
    let accepted = run_quic_transfers(&persistence, &[frame], QuicTransferOptions::default()).await;
    assert_eq!(accepted[0].code, SafeInventoryResultCodeV1::Accepted);
    assert!(!accepted[0].replayed);
    assert_committed_state(&persistence).await;
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn corrupt_receipt_fails_closed_over_quic_without_a_second_mutation() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    stage_progression_reward_and_equipment(&persistence).await;
    let frame = transfer_frame_at(4);
    let committed =
        run_quic_transfers(&persistence, &[frame], QuicTransferOptions::default()).await;
    assert_eq!(committed[0].code, SafeInventoryResultCodeV1::Accepted);
    let before_corruption = persistence
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();

    update_receipt_placement_count(&persistence, 2).await;
    assert!(matches!(
        persistence
            .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredLifecycleSignature)
    ));

    let rejected = run_quic_transfers(&persistence, &[frame], QuicTransferOptions::default()).await;
    assert_eq!(rejected.len(), 1);
    assert_state_free_service_unavailable(&rejected[0]);
    assert_eq!(safe_transfer_durable_counts(&persistence).await, (1, 1, 1));
    update_receipt_placement_count(&persistence, 1).await;

    let corrupted_hash = [222_u8; 32];
    assert_ne!(corrupted_hash, committed[0].result_hash);
    update_receipt_result_hash(&persistence, corrupted_hash).await;
    let hash_rejected =
        run_quic_transfers(&persistence, &[frame], QuicTransferOptions::default()).await;
    assert_state_free_service_unavailable(&hash_rejected[0]);
    update_receipt_result_hash(&persistence, committed[0].result_hash).await;
    let replay = run_quic_transfers(&persistence, &[frame], QuicTransferOptions::default()).await;
    assert_eq!(replay[0].code, SafeInventoryResultCodeV1::Accepted);
    assert!(replay[0].replayed);
    assert_eq!(replay[0].result_hash, committed[0].result_hash);
    let after_repairs = persistence
        .core_item_lifecycle_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    assert_eq!(after_repairs, before_corruption);
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn safe_inventory_service_derives_placement_and_replays_after_restart() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    let service = PostgresSafeInventoryService::new(persistence.clone());
    let frame = transfer_frame();
    let committed = service.transfer_frame(ACCOUNT_ID, &frame).await.unwrap();
    assert!(!committed.replayed);
    assert_eq!(
        (committed.account_version, committed.inventory_version),
        (2, 3)
    );
    assert_eq!(committed.placements.len(), 1);
    assert_eq!(
        committed.placements[0].destination,
        SafeInventoryDestinationV1::Vault { slot_index: 0 }
    );
    assert_eq!(committed.placements[0].item_version, 2);
    persistence.close().await;

    let restarted = disposable_database().await;
    let restarted_service = PostgresSafeInventoryService::new(restarted.clone());
    let replay = restarted_service
        .transfer_frame(ACCOUNT_ID, &frame)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.result_hash, committed.result_hash);

    let mut changed = frame;
    changed.payload.source_slot_index = 1;
    changed.payload_hash = changed.payload.canonical_hash();
    assert!(matches!(
        restarted_service.transfer_frame(ACCOUNT_ID, &changed).await,
        Err(SafeInventoryServiceError::IdempotencyConflict)
    ));
    assert_committed_state(&restarted).await;
    restarted.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn concurrent_claims_for_final_vault_slot_have_one_winner() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    seed_final_vault_slot_race(&persistence).await;
    let service = PostgresSafeInventoryService::new(persistence.clone());
    let first_payload = SafeInventoryTransferPayloadV1 {
        kind: SafeInventoryTransferKindV1::CharacterSafeToVault,
        source_slot_index: 0,
        expected_account_version: 1,
        expected_inventory_version: 2,
    };
    let second_payload = SafeInventoryTransferPayloadV1 {
        source_slot_index: 1,
        ..first_payload
    };
    let first = SafeInventoryTransferFrameV1 {
        mutation_id: [216; 16],
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 1,
        payload_hash: first_payload.canonical_hash(),
        payload: first_payload,
    };
    let second = SafeInventoryTransferFrameV1 {
        mutation_id: [217; 16],
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 2,
        payload_hash: second_payload.canonical_hash(),
        payload: second_payload,
    };
    let (left, right) = tokio::join!(
        service.transfer_frame(ACCOUNT_ID, &first),
        service.transfer_frame(ACCOUNT_ID, &second),
    );
    assert!(matches!(
        (&left, &right),
        (Ok(_), Err(SafeInventoryServiceError::StaleVersion))
            | (Err(SafeInventoryServiceError::StaleVersion), Ok(_))
    ));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM item_instances WHERE namespace_id=$1 AND account_id=$2 \
         AND location_kind=6),(SELECT count(*) FROM item_instances WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 AND location_kind=5), \
         (SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 \
         AND mutation_id IN ($4,$5)),(SELECT count(*) FROM safe_inventory_mutations \
         WHERE namespace_id=$1 AND account_id=$2 AND mutation_id IN ($4,$5))",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind([216_u8; 16].as_slice())
    .bind([217_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(counts, (160, 1, 1, 1));
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn injected_ledger_failure_rolls_back_item_versions_and_receipt() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    let mut setup = persistence.begin_transaction().await.unwrap();
    sqlx::query("DROP TRIGGER IF EXISTS safe_inventory_test_failure ON item_ledger_events")
        .execute(setup.connection())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS gravebound_test_fail_safe_inventory_ledger()")
        .execute(setup.connection())
        .await
        .unwrap();
    sqlx::query(
        "CREATE FUNCTION gravebound_test_fail_safe_inventory_ledger() RETURNS trigger \
         LANGUAGE plpgsql AS $$ BEGIN IF NEW.mutation_id = decode(repeat('dc',16),'hex') THEN \
         RAISE EXCEPTION 'injected safe inventory ledger failure'; END IF; RETURN NEW; END; $$",
    )
    .execute(setup.connection())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER safe_inventory_test_failure BEFORE INSERT ON item_ledger_events \
         FOR EACH ROW EXECUTE FUNCTION gravebound_test_fail_safe_inventory_ledger()",
    )
    .execute(setup.connection())
    .await
    .unwrap();
    setup.commit().await.unwrap();

    let payload = SafeInventoryTransferPayloadV1 {
        kind: SafeInventoryTransferKindV1::CharacterSafeToVault,
        source_slot_index: 0,
        expected_account_version: 1,
        expected_inventory_version: 2,
    };
    let frame = SafeInventoryTransferFrameV1 {
        mutation_id: [220; 16],
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    };
    assert!(matches!(
        PostgresSafeInventoryService::new(persistence.clone())
            .transfer_frame(ACCOUNT_ID, &frame)
            .await,
        Err(SafeInventoryServiceError::Persistence)
    ));

    let mut verify = persistence.begin_transaction().await.unwrap();
    sqlx::query("DROP TRIGGER safe_inventory_test_failure ON item_ledger_events")
        .execute(verify.connection())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION gravebound_test_fail_safe_inventory_ledger()")
        .execute(verify.connection())
        .await
        .unwrap();
    let state: (Option<Vec<u8>>, i16, i16, i64, i64, i64) = sqlx::query_as(
        "SELECT x.character_id,x.location_kind,x.slot_index,x.item_version,a.state_version, \
         i.inventory_version FROM item_instances x JOIN accounts a USING(namespace_id,account_id) \
         JOIN character_inventories i USING(namespace_id,account_id,character_id) \
         WHERE x.namespace_id=$1 AND x.account_id=$2 AND x.item_uid=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .fetch_one(verify.connection())
    .await
    .unwrap();
    let durable_rows: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 \
         AND account_id=$2 AND mutation_id=$3) + (SELECT count(*) FROM safe_inventory_mutations \
         WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$3)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind([220_u8; 16].as_slice())
    .fetch_one(verify.connection())
    .await
    .unwrap();
    verify.commit().await.unwrap();
    assert_eq!(state, (Some(CHARACTER_ID.to_vec()), 5, 0, 1, 1, 2));
    assert_eq!(durable_rows, 0);
}

async fn assert_committed_state(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let item: (Option<Vec<u8>>, i16, i16, i64) = sqlx::query_as(
        "SELECT character_id,security_state,location_kind,item_version FROM item_instances \
         WHERE namespace_id=$1 AND account_id=$2 AND item_uid=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let ledger_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 \
         AND item_uid=$3 AND mutation_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .bind(MUTATION_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(item, (None, 0, 6, 2));
    assert_eq!(ledger_count, 1);
}
