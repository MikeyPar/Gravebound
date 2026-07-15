use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use persistence::{
    DangerCheckpointWrite, PersistenceConfig, PostgresPersistence, StoredWorldFlowRevisionV1,
    WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    AccountBootstrapFrame, AccountBootstrapRequest, AccountBootstrapResult, AccountErrorCode,
    AuthTicket, BargainContentRevisionV1, BargainDecision, BargainDecisionFrame,
    BargainDecisionPayload, BargainDecisionResult, BargainOfferCell, BargainResultCode,
    BargainViewFrame, CharacterMutationFrame, CharacterMutationPayload, ClientHello, Compression,
    HandshakeResponse, InitialOathSelectionFrame, InitialOathSelectionPayload, ManifestHash,
    OathContentRevisionV1, OathResultCode, OathSelectionState, OathViewFrame, Platform,
    ProgressionQueryFrame, ProgressionResult, ProtocolVersion, WireText, WorldFlowFrame,
    WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferMutation,
    WorldTransferPayload, WorldTransferResultCode,
};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, BoundCoreIdentityServer,
    CharacterIdGenerator, CoreCharacterCombatFactory, CoreCheckpointBinding,
    CoreDangerCheckpointService, CoreIdentityServerConfig, CoreIdentityServerReport, CoreLifeKey,
    CoreLiveBindingId, CoreLiveDirectory, CoreResumeOutcome, EntryCaptureContext,
    EntryRestoreProvider, IdentityClock, IdentityService, LOCAL_SERVER_NAME,
    LocalServerRuntimeError, NoopIdentityEventSink, PostgresAccountRepository,
    PostgresGroundExpiryService, PostgresProgressionAwardService,
    PostgresProgressionRestoreProvider, PostgresRewardService, ProgressionAwardCode,
    ProgressionAwardCommand, ProgressionAwardEvidence, ProgressionAwardPayload, RewardGrantContext,
    RewardGrantTransaction, RewardPlacement, SecretRewardEpoch,
};
use sim_core::{
    ArenaGeometry, BellDebtCheckpoint, CombatAction, EncounterXpEvidence, ProjectileCollisionWorld,
    RewardLifeState, RewardRecallState, RewardTrustState, SimulationVector, TilePoint,
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

fn bargain_revision(content_root: &Path) -> BargainContentRevisionV1 {
    let compiled = sim_content::load_core_development_oaths_bargains(content_root).unwrap();
    let hashes = compiled.hashes();
    BargainContentRevisionV1 {
        records_blake3: ManifestHash::new(hashes.records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(hashes.assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(hashes.localization_blake3.clone()).unwrap(),
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

#[allow(clippy::too_many_lines)] // Explicit cross-domain fixture rows keep the authority boundary auditable.
async fn stage_real_quic_bargain_offer(
    persistence: &PostgresPersistence,
    content_root: &Path,
    ticket: &[u8],
    character_id: [u8; 16],
) -> ([u8; 16], [u8; 16]) {
    const LINEAGE_ID: [u8; 16] = [75; 16];
    const RESTORE_ID: [u8; 16] = [76; 16];
    const REWARD_ID: [u8; 16] = [77; 16];
    let hash = blake3::hash(ticket);
    let account_id = <[u8; 16]>::try_from(&hash.as_bytes()[..16]).unwrap();
    let progression = sim_content::load_core_development_progression(content_root).unwrap();
    let oath_bargain = sim_content::load_core_development_oaths_bargains(content_root).unwrap();
    let world = sim_content::load_core_development_world_flow(content_root).unwrap();
    let restores = PostgresProgressionRestoreProvider::new(&progression).unwrap();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let versions: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT a.state_version, c.character_state_version, p.progression_version, \
                i.inventory_version, ob.oath_bargain_version FROM accounts a \
         JOIN characters c USING (namespace_id, account_id) \
         JOIN character_progression p USING (namespace_id, account_id, character_id) \
         JOIN character_inventories i USING (namespace_id, account_id, character_id) \
         JOIN character_oath_bargain_state ob USING (namespace_id, account_id, character_id) \
         WHERE a.namespace_id = $1 AND a.account_id = $2 AND c.character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let hashes = world.hashes();
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id, account_id, character_id, \
         lineage_id, content_id, layout_id, lineage_state, records_blake3, assets_blake3, \
         localization_blake3) VALUES ($1, $2, $3, $4, 'world.core_microrealm_01', \
         'layout.core_private_life_01', 0, $5, $6, $7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_entry_restore_points (namespace_id, account_id, character_id, \
         restore_point_id, lineage_id, source_location_id, restore_location_id, \
         snapshot_contract_version, account_version, character_version, progression_version, \
         inventory_version, oath_bargain_version, life_metrics_version, component_mask, composite_digest, \
         restore_state, records_blake3, assets_blake3, localization_blake3) \
         VALUES ($1, $2, $3, $4, $5, 'hub.lantern_halls_01', 'hub.lantern_halls_01', \
         2, $6, $7, $8, $9, $10, 1, 15, $11, 0, $12, $13, $14)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(RESTORE_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(versions.0)
    .bind(versions.1)
    .bind(versions.2)
    .bind(versions.3)
    .bind(versions.4)
    .bind([91_u8; 32].as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .execute(transaction.connection())
    .await
    .unwrap();
    restores
        .capture(
            &mut transaction,
            EntryCaptureContext {
                account_id,
                character_id,
                restore_point_id: RESTORE_ID,
                mutation_id: [9; 16],
                safe_placement_count: 0,
            },
        )
        .await
        .unwrap();
    let inventory = persistence::stage_danger_entry_inventory_restore_v2(
        &mut transaction,
        account_id,
        character_id,
        RESTORE_ID,
        [9; 16],
        0,
    )
    .await
    .unwrap();
    let oath = persistence::stage_danger_entry_oath_bargain_restore_v2(
        &mut transaction,
        account_id,
        character_id,
        RESTORE_ID,
    )
    .await
    .unwrap();
    let life = persistence::stage_danger_entry_life_metrics_restore_v2(
        &mut transaction,
        account_id,
        character_id,
        RESTORE_ID,
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_entry_restore_points SET inventory_version = $1, \
         oath_bargain_version = $2, life_metrics_version = $3 \
         WHERE namespace_id = $4 AND restore_point_id = $5",
    )
    .bind(i64::try_from(inventory.post_inventory_version).unwrap())
    .bind(i64::try_from(oath.oath_bargain_version).unwrap())
    .bind(i64::try_from(life.life_metrics_version).unwrap())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RESTORE_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    let danger_version = versions.1 + 1;
    sqlx::query(
        "UPDATE characters SET character_state_version = $1 WHERE namespace_id = $2 \
         AND account_id = $3 AND character_id = $4",
    )
    .bind(danger_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_world_locations SET character_version = $1, location_kind = 2, \
         location_content_id = 'world.core_microrealm_01', safe_arrival_kind = NULL, \
         instance_lineage_id = $2, entry_restore_point_id = $3 WHERE namespace_id = $4 \
         AND account_id = $5 AND character_id = $6",
    )
    .bind(danger_version)
    .bind(LINEAGE_ID.as_slice())
    .bind(RESTORE_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let payload = ProgressionAwardPayload {
        character_id,
        expected_progression_version: u64::try_from(versions.2).unwrap(),
        source_content_id: "miniboss.sepulcher_knight".to_owned(),
        progression_content_revision: ManifestHash::new(
            progression.hashes().records_blake3.clone(),
        )
        .unwrap(),
        evidence: ProgressionAwardEvidence::Encounter(EncounterXpEvidence {
            active_ticks: 600,
            present_ticks: 600,
            longest_inactivity_ticks: 0,
            encounter_contribution_reference_health: 4_200,
            direct_damage: 420,
            effective_healing_to_others: 0,
            damage_prevented_on_others: 0,
            qualifying_objective_credits: 0,
            life_state: RewardLifeState::Living,
            recall_state: RewardRecallState::Eligible,
            trust_state: RewardTrustState::Valid,
        }),
    };
    let awards =
        PostgresProgressionAwardService::new(persistence.clone(), &progression, &oath_bargain)
            .unwrap();
    let awarded = awards
        .award(
            AuthenticatedAccount {
                account_id: AccountId::new(account_id).unwrap(),
                namespace: AuthenticatedNamespace::WipeableTest,
            },
            &ProgressionAwardCommand {
                reward_event_id: REWARD_ID,
                payload_hash: payload.canonical_hash(),
                payload,
            },
        )
        .await;
    assert_eq!(awarded.code, ProgressionAwardCode::Accepted);
    (REWARD_ID, account_id)
}

async fn assert_persistent_bargain_route(
    persistence: &PostgresPersistence,
    connection: &quinn::Connection,
    content_root: &Path,
    ticket: &[u8],
    character_id: [u8; 16],
) -> (BargainDecisionFrame, BargainDecisionResult) {
    let (offer_id, _) =
        stage_real_quic_bargain_offer(persistence, content_root, ticket, character_id).await;
    let revision = bargain_revision(content_root);
    let (_, view) = bot_client::perform_bargain_view(
        connection,
        BargainViewFrame {
            sequence: 6,
            character_id,
            content_revision: revision.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(view.code, BargainResultCode::Available);
    let offer = view.projection.unwrap().offer.unwrap();
    let bargain_id = offer
        .cells
        .iter()
        .find_map(|cell| match cell {
            BargainOfferCell::Available { bargain_id, .. } => Some(bargain_id.clone()),
            BargainOfferCell::Unavailable => None,
        })
        .expect("Core offer has an available Bargain");
    let payload = BargainDecisionPayload {
        character_id,
        offer_id,
        decision: BargainDecision::Select { bargain_id },
        content_revision: revision,
        confirmed: true,
    };
    let frame = BargainDecisionFrame {
        mutation_id: [78; 16],
        expected_oath_bargain_version: 3,
        payload_hash: payload.canonical_hash(),
        issued_at_unix_millis: current_unix_millis(),
        payload,
    };
    let (_, selected) = bot_client::perform_bargain_decision(connection, frame.clone())
        .await
        .unwrap();
    assert_eq!(selected.code, BargainResultCode::Accepted);
    assert_eq!(
        selected
            .projection
            .as_ref()
            .unwrap()
            .active_bargain_ids
            .len(),
        1
    );
    (frame, selected)
}

async fn stage_complete_core_combat_package(
    persistence: &PostgresPersistence,
    content_root: &Path,
    ticket: &[u8],
    character_id: [u8; 16],
) {
    let hash = blake3::hash(ticket);
    let account_id = <[u8; 16]>::try_from(&hash.as_bytes()[..16]).unwrap();
    let catalog = sim_content::load_core_development_oaths_bargains(content_root).unwrap();
    let hashes = catalog.hashes();
    let bargains = [
        protocol::CINDER_HUNGER_ID,
        protocol::BELL_DEBT_ID,
        protocol::LANTERN_ASH_ID,
    ];
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "DELETE FROM character_active_bargains WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    for (index, bargain_id) in bargains.iter().enumerate() {
        let ordinal = i16::try_from(index + 1).unwrap();
        let created_version = i64::from(ordinal) + 3;
        let offer_id = [u8::try_from(81 + index).unwrap(); 16];
        let reward_id = [u8::try_from(84 + index).unwrap(); 16];
        let lineage_id = [75_u8; 16];
        let restore_id = [76_u8; 16];
        sqlx::query(
            "INSERT INTO bargain_offers (namespace_id, account_id, character_id, offer_id, \
             source_reward_event_id, source_content_id, source_layout_id, instance_lineage_id, \
             entry_restore_point_id, content_version, records_blake3, assets_blake3, \
             localization_blake3, offer_state, selected_bargain_id, \
             created_oath_bargain_version, resolved_oath_bargain_version, resolved_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 1, $14, $15, $16, \
             transaction_timestamp())",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(offer_id.as_slice())
        .bind(reward_id.as_slice())
        .bind(persistence::CORE_BARGAIN_SOURCE_ID)
        .bind(persistence::CORE_BARGAIN_LAYOUT_ID)
        .bind(lineage_id.as_slice())
        .bind(restore_id.as_slice())
        .bind(catalog.revision_label())
        .bind(&hashes.records_blake3)
        .bind(&hashes.assets_blake3)
        .bind(&hashes.localization_blake3)
        .bind(*bargain_id)
        .bind(created_version)
        .bind(created_version + 1)
        .execute(transaction.connection())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO character_active_bargains (namespace_id, account_id, character_id, \
             bargain_id, acquisition_ordinal, acquired_by_offer_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(*bargain_id)
        .bind(ordinal)
        .bind(offer_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    sqlx::query(
        "UPDATE character_oath_bargain_state SET earned_bargain_slots = 3, \
         oath_bargain_version = 7, updated_at = transaction_timestamp() \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_progression SET level = 10, current_health = 100, \
         updated_at = transaction_timestamp() WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    stage_two_slot_belt(persistence, account_id, character_id).await;
}

async fn stage_two_slot_belt(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let moved = sqlx::query(
        "UPDATE item_instances SET slot_index = 1, updated_at = transaction_timestamp() \
         WHERE namespace_id = $1 AND item_uid = (SELECT item_uid FROM item_instances \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 \
         AND template_id = $4 AND location_kind = 1 AND slot_index = 0 \
         ORDER BY item_uid LIMIT 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(server_app::STARTER_TONIC_ID)
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(moved, 1);
    transaction.commit().await.unwrap();
}

#[derive(Debug, PartialEq, Eq)]
struct PersistedCombatSignature {
    oath_bargain_version: u64,
    level: u16,
    maximum_health: u32,
    bargain_modifiers: sim_core::ResolvedCoreBargainModifiers,
    maximum_health_multiplier_basis_points: u32,
    oath: Option<sim_core::GraveArbalistOath>,
    outgoing_direct_damage_basis_points: u32,
    belt: sim_core::TonicBelt,
    belt_policy: sim_core::TonicBeltPolicy,
}

async fn assert_persisted_combat_factory(
    persistence: &PostgresPersistence,
    content_root: &Path,
    ticket: &[u8],
    character_id: [u8; 16],
) -> PersistedCombatSignature {
    let hash = blake3::hash(ticket);
    let account_id = <[u8; 16]>::try_from(&hash.as_bytes()[..16]).unwrap();
    let snapshot = persistence
        .core_combat_loadout_snapshot(account_id, character_id)
        .await
        .unwrap()
        .unwrap();
    let choices = sim_content::load_core_development_oaths_bargains(content_root).unwrap();
    assert!(snapshot.oath_bargain_version > 0);
    assert!(
        snapshot
            .active_bargains
            .iter()
            .all(|bargain| { bargain.acquiring_offer_content_version == choices.revision_label() })
    );
    if snapshot.active_bargains.len() == 3 {
        assert!(snapshot.belt_slots.iter().all(|slot| matches!(
            slot,
            Some(stack) if stack.template_id == "consumable.red_tonic" && stack.quantity == 1
        )));
    } else {
        assert!(matches!(
            &snapshot.belt_slots,
            [Some(stack), None]
                if stack.template_id == "consumable.red_tonic" && stack.quantity == 2
        ));
    }
    let factory = CoreCharacterCombatFactory::load(persistence.clone(), content_root).unwrap();
    let combat = factory.build(account_id, character_id).await.unwrap();
    assert_eq!(
        combat.oath_bargain_version,
        u64::try_from(snapshot.oath_bargain_version).unwrap()
    );
    assert_eq!(
        combat.bargains.definitions().len(),
        snapshot.active_bargains.len()
    );
    assert_eq!(
        combat.state.oath(),
        Some(sim_core::GraveArbalistOath::LongVigil)
    );
    if snapshot.active_bargains.len() == 3 {
        assert!(combat.bargains.bell_debt().is_some());
        assert!(combat.bargains.lantern_ash().is_some());
        assert_eq!(
            combat.bargain_modifiers.ordinary_attack_rate_basis_points,
            8_500
        );
        assert_eq!(
            combat.bargain_modifiers.outgoing_direct_damage_basis_points,
            11_800
        );
        assert_eq!(
            combat
                .bargain_modifiers
                .maximum_health_multiplier_basis_points,
            8_800
        );
        assert_eq!(
            combat
                .bargain_modifiers
                .potion_healing_multiplier_basis_points,
            14_000
        );
        assert_eq!(combat.bargain_modifiers.active_belt_slots, 1);
        assert_eq!(combat.maximum_health_multiplier_basis_points, 7_920);
        assert_eq!(
            combat.consumables.belt().slots(),
            &[sim_core::BeltSlot::RedTonic(1); 2]
        );
        assert!(combat.consumables.belt_policy().is_active(0));
        assert!(!combat.consumables.belt_policy().is_active(1));
    }
    assert_eq!(combat.level, 10);
    PersistedCombatSignature {
        oath_bargain_version: combat.oath_bargain_version,
        level: combat.level,
        maximum_health: combat.maximum_health,
        bargain_modifiers: combat.bargain_modifiers,
        maximum_health_multiplier_basis_points: combat.maximum_health_multiplier_basis_points,
        oath: combat.state.oath(),
        outgoing_direct_damage_basis_points: combat.state.outgoing_direct_damage_basis_points(),
        belt: *combat.consumables.belt(),
        belt_policy: combat.consumables.belt_policy(),
    }
}

fn core_checkpoint_binding(content_root: &Path) -> CoreCheckpointBinding {
    let world = sim_content::load_core_development_world_flow(content_root).unwrap();
    let hashes = world.hashes();
    CoreCheckpointBinding::new(StoredWorldFlowRevisionV1 {
        records_blake3: hashes.records_blake3.clone(),
        assets_blake3: hashes.assets_blake3.clone(),
        localization_blake3: hashes.localization_blake3.clone(),
    })
    .unwrap()
}

fn checkpoint_world() -> ProjectileCollisionWorld {
    let arena = ArenaGeometry {
        id: "arena.postgres_checkpoint".to_owned(),
        width_milli_tiles: 100_000,
        height_milli_tiles: 100_000,
        shell_thickness_milli_tiles: 1_000,
        player_spawn: TilePoint::new(4_000, 12_000),
        boss_spawn: TilePoint::new(80_000, 80_000),
        pillars: vec![],
        anchors: vec![],
    }
    .validated()
    .unwrap();
    ProjectileCollisionWorld::new(&arena, vec![]).unwrap()
}

async fn persist_pending_bell_checkpoint(
    persistence: &PostgresPersistence,
    content_root: &Path,
    ticket: &[u8],
    character_id: [u8; 16],
) -> BellDebtCheckpoint {
    const LINEAGE_ID: [u8; 16] = [75; 16];
    let account_hash = blake3::hash(ticket);
    let account_id = <[u8; 16]>::try_from(&account_hash.as_bytes()[..16]).unwrap();
    let factory = CoreCharacterCombatFactory::load(persistence.clone(), content_root).unwrap();
    let combat = factory.build(account_id, character_id).await.unwrap();
    let key = CoreLifeKey::new(account_id, character_id, LINEAGE_ID).unwrap();
    let mut directory = CoreLiveDirectory::default();
    directory
        .insert(
            key,
            CoreLiveBindingId::new(1).unwrap(),
            "room.sepulcher",
            core_checkpoint_binding(content_root),
            combat,
        )
        .unwrap();
    let world = checkpoint_world();
    for _ in 0..200 {
        directory
            .get_mut(key)
            .unwrap()
            .with_combat_mutation(|combat| {
                combat.state.step(
                    CombatAction {
                        primary_held: true,
                        primary_press_sequence: 1,
                        ..CombatAction::default()
                    },
                    SimulationVector::new(4.0, 7.0),
                    &world,
                )
            })
            .unwrap()
            .unwrap();
        if directory
            .get(key)
            .unwrap()
            .combat()
            .state
            .has_pending_bell_repeat()
        {
            break;
        }
    }
    let expected = directory
        .get(key)
        .unwrap()
        .combat()
        .state
        .export_bell_debt_checkpoint()
        .unwrap();
    assert!(expected.has_pending_repeat());
    let service = CoreDangerCheckpointService::new(persistence.clone());
    assert_eq!(
        service
            .flush_lifecycle_boundary(directory.get_mut(key).unwrap())
            .await
            .unwrap(),
        Some(DangerCheckpointWrite::Created)
    );
    expected
}

async fn assert_pending_bell_checkpoint_resumes(
    persistence: &PostgresPersistence,
    content_root: &Path,
    ticket: &[u8],
    character_id: [u8; 16],
    expected: &BellDebtCheckpoint,
) {
    const LINEAGE_ID: [u8; 16] = [75; 16];
    let account_hash = blake3::hash(ticket);
    let account_id = <[u8; 16]>::try_from(&account_hash.as_bytes()[..16]).unwrap();
    let factory = CoreCharacterCombatFactory::load(persistence.clone(), content_root).unwrap();
    let combat = factory.build(account_id, character_id).await.unwrap();
    let key = CoreLifeKey::new(account_id, character_id, LINEAGE_ID).unwrap();
    let mut directory = CoreLiveDirectory::default();
    directory
        .insert(
            key,
            CoreLiveBindingId::new(2).unwrap(),
            "room.sepulcher",
            core_checkpoint_binding(content_root),
            combat,
        )
        .unwrap();
    let service = CoreDangerCheckpointService::new(persistence.clone());
    assert!(matches!(
        service
            .resume_latest(directory.get_mut(key).unwrap())
            .await
            .unwrap(),
        CoreResumeOutcome::Restored { checkpoint_tick } if checkpoint_tick > 0
    ));
    assert_eq!(
        directory
            .get(key)
            .unwrap()
            .combat()
            .state
            .export_bell_debt_checkpoint()
            .unwrap(),
        *expected
    );
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
#[allow(clippy::too_many_lines)] // The pre/post-restart network journey remains visible end to end.
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
    let (bargain_frame, bargain_result) = assert_persistent_bargain_route(
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
        panic!("restored durable snapshot expected, received {restored:?}")
    };
    assert_eq!(restored.characters.len(), 1);
    assert_persisted_oath_view(&connection, &content_root, character_id).await;
    let (_, replayed_bargain) = bot_client::perform_bargain_decision(&connection, bargain_frame)
        .await
        .unwrap();
    assert_eq!(replayed_bargain, bargain_result);
    let (_, bargain_view) = bot_client::perform_bargain_view(
        &connection,
        BargainViewFrame {
            sequence: 4,
            character_id,
            content_revision: bargain_revision(&content_root),
        },
    )
    .await
    .unwrap();
    assert_eq!(bargain_view.code, BargainResultCode::NoOffer);
    let bargain_projection = bargain_view.projection.unwrap();
    assert_eq!(bargain_projection.active_bargain_ids.len(), 1);
    assert_eq!(bargain_projection.earned_bargain_slots, 1);
    assert_eq!(bargain_projection.oath_bargain_version, 4);
    stage_complete_core_combat_package(&persistence, &content_root, ticket, character_id).await;
    let combat_before_restart =
        assert_persisted_combat_factory(&persistence, &content_root, ticket, character_id).await;
    let bell_before_restart =
        persist_pending_bell_checkpoint(&persistence, &content_root, ticket, character_id).await;
    connection.close(0_u32.into(), b"combat package restart");
    shutdown.send(()).unwrap();
    assert!(task.await.unwrap().unwrap().persistence_enabled);
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
    let combat_after_restart =
        assert_persisted_combat_factory(&persistence, &content_root, ticket, character_id).await;
    assert_eq!(combat_after_restart, combat_before_restart);
    assert_pending_bell_checkpoint_resumes(
        &persistence,
        &content_root,
        ticket,
        character_id,
        &bell_before_restart,
    )
    .await;
    connection.close(0_u32.into(), b"complete");
    shutdown.send(()).unwrap();
    assert!(task.await.unwrap().unwrap().persistence_enabled);
    endpoint.wait_idle().await;
    persistence.close().await;
}
