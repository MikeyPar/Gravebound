use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use persistence::{PersistenceConfig, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};
use protocol::{
    AccountBootstrapFrame, AccountBootstrapRequest, AccountBootstrapResult, AccountErrorCode,
    AuthTicket, CharacterMutationFrame, CharacterMutationPayload, ClientHello, Compression,
    HandshakeResponse, InitialOathSelectionFrame, InitialOathSelectionPayload, ManifestHash,
    OathContentRevisionV1, OathResultCode, OathSelectionState, OathViewFrame, Platform,
    ProgressionQueryFrame, ProgressionResult, ProtocolVersion, WireText, WorldFlowFrame,
    WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferMutation,
    WorldTransferPayload, WorldTransferResultCode,
};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, BoundCoreIdentityServer,
    CharacterIdGenerator, CoreCharacterCombatFactory, CoreIdentityServerConfig,
    CoreIdentityServerReport, IdentityClock, IdentityService, LOCAL_SERVER_NAME,
    LocalServerRuntimeError, NoopIdentityEventSink, PostgresAccountRepository,
    PostgresGroundExpiryService, PostgresRewardService, RewardGrantContext, RewardGrantTransaction,
    RewardPlacement, SecretRewardEpoch,
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

impl SequentialIds {
    fn starting_at(next: u8) -> Self {
        assert_ne!(next, 0, "test character IDs must be nonzero");
        Self(AtomicU8::new(next - 1))
    }
}

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
    service_starting_at(persistence, 1)
}

fn service_starting_at(
    persistence: PostgresPersistence,
    next_character_id: u8,
) -> IdentityService<PostgresAccountRepository, FixedClock, SequentialIds, NoopIdentityEventSink> {
    IdentityService::new(
        PostgresAccountRepository::new(persistence),
        FixedClock,
        SequentialIds::starting_at(next_character_id),
        NoopIdentityEventSink,
        manifest(),
    )
}

type TestIdentityService =
    IdentityService<PostgresAccountRepository, FixedClock, SequentialIds, NoopIdentityEventSink>;
type StarterRow = (Vec<u8>, String, i16, i16, i16, i16, i32);

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

fn world_flow_revision(content_root: &Path) -> protocol::WorldFlowContentRevisionV1 {
    let compiled = sim_content::load_core_development_world_flow(content_root).unwrap();
    protocol::WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(compiled.hashes().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(compiled.hashes().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(compiled.hashes().localization_blake3.clone())
            .unwrap(),
    }
}

fn oath_revision(content_root: &Path) -> OathContentRevisionV1 {
    let compiled = sim_content::load_core_development_oaths_bargains(content_root).unwrap();
    OathContentRevisionV1 {
        records_blake3: ManifestHash::new(compiled.hashes().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(compiled.hashes().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(compiled.hashes().localization_blake3.clone())
            .unwrap(),
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

async fn world_flow_row_count(persistence: &PostgresPersistence) -> i64 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM character_instance_lineages) + \
                (SELECT count(*) FROM character_entry_restore_points) + \
                (SELECT count(*) FROM character_world_transfer_results)",
    )
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    count
}

async fn starter_item_uids(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Vec<Vec<u8>> {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let rows: Vec<StarterRow> = sqlx::query_as(
        "SELECT item_uid, template_id, provenance_kind, salvage_band, location_kind, \
         slot_index, salvage_value FROM item_instances WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 ORDER BY roll_index, unit_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].1, server_app::STARTER_WEAPON_ID);
    assert_eq!(rows[1].1, server_app::STARTER_RELIC_ID);
    assert_eq!(rows[2].1, server_app::STARTER_TONIC_ID);
    assert_eq!(rows[3].1, server_app::STARTER_TONIC_ID);
    assert_eq!(
        (rows[0].2, rows[0].3, rows[0].4, rows[0].5, rows[0].6),
        (0, 0, 0, 0, 0)
    );
    assert_eq!(
        (rows[1].2, rows[1].3, rows[1].4, rows[1].5, rows[1].6),
        (0, 0, 0, 1, 0)
    );
    assert_eq!(
        (rows[2].2, rows[2].3, rows[2].4, rows[2].5, rows[2].6),
        (4, 0, 1, 0, 0)
    );
    assert_eq!(
        (rows[3].2, rows[3].3, rows[3].4, rows[3].5, rows[3].6),
        (4, 0, 1, 0, 0)
    );
    rows.into_iter().map(|row| row.0).collect()
}

async fn remove_starter_fixture(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "DELETE FROM starter_initializer_results WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM item_instances WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_inventories SET inventory_version = 1 WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_durable_reward_and_expiry_lifecycle(
    persistence: &PostgresPersistence,
    character_id: [u8; 16],
) {
    let rewards = PostgresRewardService::load(
        persistence.clone(),
        &content_root(),
        SecretRewardEpoch::new("test-epoch", [0x5a; 32]).unwrap(),
    )
    .unwrap();
    let mut last_context = None;
    let mut last_result = None;
    for ordinal in 0_u8..4 {
        let context = RewardGrantContext {
            reward_request_id: [101 + ordinal; 16],
            account_id: [91; 16],
            character_id,
            source_instance_id: [88; 16],
            reward_table_id: "reward.boss_caldus",
            current_tick: 1_000,
        };
        let RewardGrantTransaction::Fresh { result, durable } =
            rewards.grant(context).await.unwrap()
        else {
            panic!("first reward request must be fresh")
        };
        assert!(!durable.replayed);
        last_context = Some(context);
        last_result = Some(result);
    }
    let context = last_context.unwrap();
    let expected = last_result.unwrap();
    let rotated_rewards = PostgresRewardService::load(
        persistence.clone(),
        &content_root(),
        SecretRewardEpoch::new("test-epoch-rotated", [0xa5; 32]).unwrap(),
    )
    .unwrap();
    let RewardGrantTransaction::Replay { result, durable } =
        rotated_rewards.grant(context).await.unwrap()
    else {
        panic!("identical reward request must replay")
    };
    assert!(durable.replayed);
    assert_eq!(result, expected);
    assert!(result.items.iter().any(|item| matches!(
        item.placement,
        RewardPlacement::PersonalGround {
            expires_at_tick: 2_800,
            ..
        }
    )));

    let expiry = PostgresGroundExpiryService::new(persistence.clone());
    assert!(
        expiry
            .expire_due([88; 16], 2_799, 256)
            .await
            .unwrap()
            .is_empty()
    );
    let expired = expiry.expire_due([88; 16], 2_800, 256).await.unwrap();
    assert!(!expired.is_empty());
    assert!(
        expiry
            .expire_due([88; 16], 2_800, 256)
            .await
            .unwrap()
            .is_empty()
    );

    let mut verification = persistence.begin_transaction().await.unwrap();
    let destroyed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_instances WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 AND location_kind = 4 AND security_state = 3 \
         AND destruction_reason = 'ground_expired'",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([91_u8; 16].as_slice())
    .bind(character_id.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    let expiry_ledger: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 AND event_kind = 2 \
         AND reason = 'ground_expired'",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([91_u8; 16].as_slice())
    .bind(character_id.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    verification.rollback().await.unwrap();
    assert_eq!(destroyed, i64::try_from(expired.len()).unwrap());
    assert_eq!(expiry_ledger, destroyed);
}

async fn assert_persistent_world_flow_is_fail_closed(
    persistence: &PostgresPersistence,
    connection: &quinn::Connection,
    content_root: &Path,
    created: &protocol::CharacterMutationResult,
) {
    let snapshot = created.snapshot.as_ref().unwrap();
    let character_id = snapshot.characters[0].character_id;
    let select_payload = CharacterMutationPayload::Select { character_id };
    let (_, selected) = bot_client::perform_character_mutation(
        connection,
        CharacterMutationFrame {
            mutation_id: [72; 16],
            expected_account_version: snapshot.account_version,
            payload_hash: select_payload.canonical_hash(),
            issued_at_unix_millis: current_unix_millis(),
            payload: select_payload,
        },
    )
    .await
    .unwrap();
    assert!(selected.accepted);
    let progression = sim_content::load_core_development_progression(content_root).unwrap();
    let (_, progression_result) = bot_client::perform_progression_query(
        connection,
        ProgressionQueryFrame {
            sequence: 4,
            character_id,
            progression_content_revision: ManifestHash::new(
                progression.hashes().records_blake3.clone(),
            )
            .unwrap(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        progression_result,
        ProgressionResult::Snapshot {
            projection: protocol::ProgressionProjection {
                level: 1,
                total_xp: 0,
                current_health: 120,
                maximum_health: 120,
                progression_version: 1,
                ..
            },
            ..
        }
    ));
    assert_eq!(world_flow_row_count(persistence).await, 0);

    let payload = WorldTransferPayload {
        content_revision: world_flow_revision(content_root),
        command: WorldTransferCommand::EnterHallFromCharacterSelect,
    };
    let (_, result) = bot_client::perform_world_flow(
        connection,
        WorldFlowFrame {
            sequence: 3,
            request: WorldFlowRequest::Transfer(WorldTransferMutation {
                mutation_id: [73; 16],
                character_id,
                expected_character_version: 1,
                issued_at_unix_millis: current_unix_millis(),
                payload_hash: payload.canonical_hash(),
                payload,
            }),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        result,
        WorldFlowResult::Transfer {
            accepted: false,
            code: WorldTransferResultCode::StageDisabled,
            transfer_id: None,
            snapshot: Some(_),
            ..
        }
    ));
    assert_eq!(world_flow_row_count(persistence).await, 0);
}

async fn assert_persistent_oath_route(
    persistence: &PostgresPersistence,
    connection: &quinn::Connection,
    content_root: &Path,
    ticket: &[u8],
    character_id: [u8; 16],
) {
    let hash = blake3::hash(ticket);
    let account_id = &hash.as_bytes()[..16];
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE characters SET level = 10 WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id)
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_progression SET total_xp = 2700, level = 10, current_health = 156, \
         progression_version = progression_version + 1 WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id)
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_world_locations SET location_kind = 1, \
         location_content_id = 'hub.lantern_halls_01', safe_arrival_kind = 0, \
         safe_spawn_id = NULL, instance_lineage_id = NULL, entry_restore_point_id = NULL \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id)
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let revision = oath_revision(content_root);
    let (_, view) = bot_client::perform_oath_view(
        connection,
        OathViewFrame {
            sequence: 5,
            character_id,
            content_revision: revision.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(view.code, OathResultCode::Available);
    assert!(matches!(
        view.projection.as_ref().map(|value| &value.state),
        Some(OathSelectionState::Eligible { current_level: 10 })
    ));

    let payload = InitialOathSelectionPayload {
        character_id,
        oath_id: WireText::new(protocol::LONG_VIGIL_ID).unwrap(),
        content_revision: revision,
        confirmed: true,
    };
    let frame = InitialOathSelectionFrame {
        mutation_id: [74; 16],
        expected_character_version: 1,
        payload_hash: payload.canonical_hash(),
        issued_at_unix_millis: current_unix_millis(),
        payload,
    };
    let (_, selected) = bot_client::perform_initial_oath_selection(connection, frame.clone())
        .await
        .unwrap();
    assert_eq!(selected.code, OathResultCode::Accepted);
    assert!(matches!(
        selected.projection.as_ref().map(|value| &value.state),
        Some(OathSelectionState::Selected { oath_id, .. })
            if oath_id.as_str() == protocol::LONG_VIGIL_ID
    ));
    let (_, replayed) = bot_client::perform_initial_oath_selection(connection, frame)
        .await
        .unwrap();
    assert_eq!(replayed, selected);
    assert_persisted_combat_factory(persistence, content_root, ticket, character_id).await;
}

async fn assert_persisted_combat_factory(
    persistence: &PostgresPersistence,
    content_root: &Path,
    ticket: &[u8],
    character_id: [u8; 16],
) {
    let hash = blake3::hash(ticket);
    let account_id = <[u8; 16]>::try_from(&hash.as_bytes()[..16]).unwrap();
    let factory = CoreCharacterCombatFactory::load(persistence.clone(), content_root).unwrap();
    let combat = factory.build(account_id, character_id).await.unwrap();
    assert_eq!(
        combat.state.oath(),
        Some(sim_core::GraveArbalistOath::LongVigil)
    );
    assert_eq!(combat.maximum_health_multiplier_basis_points, 9_000);
    assert_eq!(combat.level, 10);
}

async fn assert_persisted_oath_view(
    connection: &quinn::Connection,
    content_root: &Path,
    character_id: [u8; 16],
) {
    let (_, oath) = bot_client::perform_oath_view(
        connection,
        OathViewFrame {
            sequence: 3,
            character_id,
            content_revision: oath_revision(content_root),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        oath.projection.as_ref().map(|value| &value.state),
        Some(OathSelectionState::Selected { oath_id, .. })
            if oath_id.as_str() == protocol::LONG_VIGIL_ID
    ));
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

async fn assert_concurrent_stale_writer(persistence: &PostgresPersistence) {
    let concurrent_first = service_starting_at(persistence.clone(), 31);
    let concurrent_second = service_starting_at(persistence.clone(), 32);
    let AccountBootstrapResult::Snapshot(empty) = concurrent_first
        .bootstrap(Some(account(93)), &bootstrap())
        .await
    else {
        panic!("concurrency fixture account expected")
    };
    assert_eq!(empty.account_version, 1);
    let concurrent_payload = CharacterMutationPayload::Create {
        class_id: WireText::new(protocol::GRAVE_ARBALIST_CLASS_ID).unwrap(),
    };
    let first_frame = mutation(11, 1, concurrent_payload.clone());
    let second_frame = mutation(12, 1, concurrent_payload);
    let (first_result, second_result) = tokio::join!(
        concurrent_first.mutate(Some(account(93)), &first_frame),
        concurrent_second.mutate(Some(account(93)), &second_frame),
    );
    assert_eq!(
        usize::from(first_result.accepted) + usize::from(second_result.accepted),
        1
    );
    let rejected = if first_result.accepted {
        &second_result
    } else {
        &first_result
    };
    assert_eq!(rejected.error, Some(AccountErrorCode::StateVersionMismatch));
    let AccountBootstrapResult::Snapshot(committed) = concurrent_first
        .bootstrap(Some(account(93)), &bootstrap())
        .await
    else {
        panic!("concurrent commit snapshot expected")
    };
    assert_eq!(committed.account_version, 2);
    assert_eq!(committed.characters.len(), 1);
}

async fn assert_corrupt_result_fails_closed(
    persistence: &PostgresPersistence,
    identity: &TestIdentityService,
) {
    let mut corruption = persistence.begin_transaction().await.unwrap();
    let corrupted_rows = sqlx::query(
        "UPDATE account_mutation_results SET result_payload = $1 \
         WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(vec![0_u8])
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([91_u8; 16].as_slice())
    .execute(corruption.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(corrupted_rows, 2);
    corruption.commit().await.unwrap();
    assert_eq!(
        identity.bootstrap(Some(account(91)), &bootstrap()).await,
        AccountBootstrapResult::Error(AccountErrorCode::ServiceUnavailable)
    );
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
    let original_starter_uids = starter_item_uids(&persistence, [91; 16], [1; 16]).await;
    assert_eq!(
        first_process.mutate(Some(account(91)), &create).await,
        created
    );
    let character_id = created.snapshot.as_ref().unwrap().characters[0].character_id;
    remove_starter_fixture(&persistence, [91; 16], character_id).await;
    let AccountBootstrapResult::Snapshot(_) = first_process
        .bootstrap(Some(account(91)), &bootstrap())
        .await
    else {
        panic!("existing-character starter backfill must succeed")
    };
    assert_eq!(
        starter_item_uids(&persistence, [91; 16], character_id).await,
        original_starter_uids
    );
    assert_durable_reward_and_expiry_lifecycle(&persistence, character_id).await;
    let selected = first_process
        .mutate(
            Some(account(91)),
            &mutation(2, 2, CharacterMutationPayload::Select { character_id }),
        )
        .await;
    assert!(selected.accepted);
    assert_concurrent_stale_writer(&persistence).await;
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
    assert_corrupt_result_fails_closed(&restarted_persistence, &restarted).await;
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
    let HandshakeResponse::Accepted(server_hello) =
        bot_client::perform_handshake(&connection, wire_hello(&content_root, ticket))
            .await
            .unwrap()
    else {
        panic!("persistent Core handshake must be accepted")
    };
    assert!(
        server_hello
            .feature_flags
            .iter()
            .all(|flag| flag.as_str() != protocol::CORE_WORLD_FLOW_FEATURE_FLAG)
    );
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
    assert_persistent_world_flow_is_fail_closed(&persistence, &connection, &content_root, &created)
        .await;
    let character_id = created.snapshot.as_ref().unwrap().characters[0].character_id;
    assert_persistent_oath_route(
        &persistence,
        &connection,
        &content_root,
        ticket,
        character_id,
    )
    .await;
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
    // Reliable request sequences restart with the new connection; durable account state does not.
    let (_, restored) =
        bot_client::perform_account_bootstrap(&connection, wire_bootstrap(&content_root, 1))
            .await
            .unwrap();
    let AccountBootstrapResult::Snapshot(restored) = restored else {
        panic!("restored durable snapshot expected")
    };
    assert_eq!(restored.characters.len(), 1);
    assert_persisted_oath_view(&connection, &content_root, character_id).await;
    assert_persisted_combat_factory(&persistence, &content_root, ticket, character_id).await;
    connection.close(0_u32.into(), b"complete");
    shutdown.send(()).unwrap();
    assert!(task.await.unwrap().unwrap().persistence_enabled);
    endpoint.wait_idle().await;
    persistence.close().await;
}
