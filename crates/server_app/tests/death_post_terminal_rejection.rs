//! Hosted production-service rejection proof after committed permadeath.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001` requires every later character
//!   action to reject after final death;
//! - `Gravebound_Content_Production_Spec_v1.md`: Core item, Bargain, Hall, and world identities
//!   remain the only executable content authority;
//! - `Gravebound_Development_Roadmap_v1.md`: `GB-M03-02D`, `GB-M03-06C`, and the M03
//!   atomicity/nonduplication gates require typed fail-closed behavior with no posthumous writes.

use std::path::{Path, PathBuf};

use persistence::{
    DurableDeathTransactionV1, PersistenceConfig, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    BargainContentRevisionV1, BargainDecision, BargainDecisionFrame, BargainDecisionPayload,
    BargainResultCode, ManifestHash, SafeInventoryResultCodeV1, SafeInventoryTransferFrameV1,
    SafeInventoryTransferKindV1, SafeInventoryTransferPayloadV1, WireText,
    WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest, WorldFlowResult,
    WorldTransferCommand, WorldTransferMutation, WorldTransferPayload, WorldTransferResultCode,
};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, CoreSafeInventoryAuthority,
    IdentityClock, PostgresBargainService, PostgresDangerEntryAshWalletProviderV3,
    PostgresDangerEntryInventoryProviderV3, PostgresDangerEntryLifeMetricsProviderV3,
    PostgresDangerEntryOathBargainProviderV3, PostgresDormantWorldFlowCoordinator,
    PostgresProgressionAwardService, PostgresProgressionRestoreProvider,
    PostgresSafeInventoryService, ProgressionAwardCode, ProgressionAwardCommand,
    ProgressionAwardEvidence, ProgressionAwardPayload, WorldFlowIdGenerator,
};
use sim_core::NormalXpEvidence;
use sqlx::Row;

#[path = "support/durable_death.rs"]
mod durable_death_fixture;

const OFFER_ID: [u8; 16] = [170; 16];
const BARGAIN_MUTATION_ID: [u8; 16] = [171; 16];
const SAFE_INVENTORY_MUTATION_ID: [u8; 16] = [172; 16];
const PROGRESSION_REWARD_ID: [u8; 16] = [173; 16];
const WORLD_MUTATION_ID: [u8; 16] = [174; 16];

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn persistence_config() -> PersistenceConfig {
    PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL")
}

fn authenticated() -> AuthenticatedAccount {
    AuthenticatedAccount {
        account_id: AccountId::new(durable_death_fixture::ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

fn bargain_revision(content: &sim_content::CompiledOathBargainCatalog) -> BargainContentRevisionV1 {
    let hashes = content.hashes();
    BargainContentRevisionV1 {
        records_blake3: ManifestHash::new(hashes.records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(hashes.assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(hashes.localization_blake3.clone()).unwrap(),
    }
}

fn world_revision() -> WorldFlowContentRevisionV1 {
    let content = sim_content::load_core_development_world_flow(&content_root()).unwrap();
    let hashes = content.hashes();
    WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(hashes.records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(hashes.assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(hashes.localization_blake3.clone()).unwrap(),
    }
}

async fn seed_open_bargain_offer(
    persistence: &PostgresPersistence,
    content: &sim_content::CompiledOathBargainCatalog,
) {
    let identity = durable_death_fixture::PRIMARY_IDENTITY;
    let hashes = content.hashes();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO bargain_offers \
         (namespace_id,account_id,character_id,offer_id,source_reward_event_id,content_version, \
          records_blake3,assets_blake3,localization_blake3,offer_state,selected_bargain_id, \
          created_oath_bargain_version,resolved_oath_bargain_version,source_content_id, \
          source_layout_id,instance_lineage_id,entry_restore_point_id) \
         VALUES ($1,$2,$3,$4,$4,'core-dev',$5,$6,$7,0,NULL,1,NULL, \
                 'miniboss.sepulcher_knight','layout.core_private_life_01',$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(OFFER_ID.as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .bind(identity.lineage_id.as_slice())
    .bind(identity.restore_point_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    for (ordinal, bargain_id, score) in [
        (0_i16, "bargain.bell_debt", [1_u8; 32]),
        (1, "bargain.cinder_hunger", [2_u8; 32]),
        (2, "bargain.lantern_ash", [3_u8; 32]),
    ] {
        sqlx::query(
            "INSERT INTO bargain_offer_candidates \
             (namespace_id,account_id,offer_id,candidate_ordinal,bargain_id,score) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
        .bind(OFFER_ID.as_slice())
        .bind(ordinal)
        .bind(bargain_id)
        .bind(score.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
}

fn safe_inventory_frame() -> SafeInventoryTransferFrameV1 {
    let payload = SafeInventoryTransferPayloadV1 {
        kind: SafeInventoryTransferKindV1::CharacterSafeToVault,
        source_slot_index: 0,
        expected_account_version: 2,
        expected_inventory_version: 3,
    };
    SafeInventoryTransferFrameV1 {
        mutation_id: SAFE_INVENTORY_MUTATION_ID,
        character_id: durable_death_fixture::PRIMARY_IDENTITY.character_id,
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn progression_command(
    content: &sim_content::CoreDevelopmentProgression,
) -> ProgressionAwardCommand {
    let payload = ProgressionAwardPayload {
        character_id: durable_death_fixture::PRIMARY_IDENTITY.character_id,
        expected_progression_version: 3,
        source_content_id: "enemy.drowned_pilgrim".into(),
        progression_content_revision: ManifestHash::new(content.hashes().records_blake3.clone())
            .unwrap(),
        evidence: ProgressionAwardEvidence::Ordinary(NormalXpEvidence {
            living_at_enemy_death: true,
            delta_x_milli_tiles: 1_000,
            delta_y_milli_tiles: 0,
            contribution_window_ticks: 300,
            actual_health_damage_to_enemy: 1,
            effective_support_to_qualifying_player: false,
        }),
    };
    ProgressionAwardCommand {
        reward_event_id: PROGRESSION_REWARD_ID,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn bargain_frame(revision: BargainContentRevisionV1) -> BargainDecisionFrame {
    let payload = BargainDecisionPayload {
        character_id: durable_death_fixture::PRIMARY_IDENTITY.character_id,
        offer_id: OFFER_ID,
        decision: BargainDecision::Refuse,
        content_revision: revision,
        confirmed: true,
    };
    BargainDecisionFrame {
        mutation_id: BARGAIN_MUTATION_ID,
        expected_oath_bargain_version: 2,
        payload_hash: payload.canonical_hash(),
        issued_at_unix_millis: 1,
        payload,
    }
}

fn world_frame() -> WorldFlowFrame {
    let payload = WorldTransferPayload {
        content_revision: world_revision(),
        command: WorldTransferCommand::UsePortal {
            portal_id: WireText::new("station.realm_gate").unwrap(),
        },
    };
    WorldFlowFrame {
        sequence: 1,
        request: WorldFlowRequest::Transfer(WorldTransferMutation {
            mutation_id: WORLD_MUTATION_ID,
            character_id: durable_death_fixture::PRIMARY_IDENTITY.character_id,
            expected_character_version: 3,
            issued_at_unix_millis: 1,
            payload_hash: payload.canonical_hash(),
            payload,
        }),
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedClock;

impl IdentityClock for FixedClock {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedWorldIds;

impl WorldFlowIdGenerator for FixedWorldIds {
    fn next_transfer_id(&self) -> [u8; 16] {
        [191; 16]
    }

    fn next_lineage_id(&self) -> [u8; 16] {
        [192; 16]
    }

    fn next_restore_point_id(&self) -> [u8; 16] {
        [193; 16]
    }
}

fn world_coordinator(
    persistence: PostgresPersistence,
    progression: &sim_content::CoreDevelopmentProgression,
) -> PostgresDormantWorldFlowCoordinator<
    FixedWorldIds,
    FixedClock,
    PostgresDangerEntryInventoryProviderV3,
    PostgresDangerEntryOathBargainProviderV3,
    PostgresDangerEntryLifeMetricsProviderV3,
    PostgresDangerEntryAshWalletProviderV3,
> {
    PostgresDormantWorldFlowCoordinator::new(
        persistence,
        FixedWorldIds,
        FixedClock,
        world_revision(),
        PostgresProgressionRestoreProvider::new(progression).unwrap(),
        PostgresDangerEntryInventoryProviderV3,
        PostgresDangerEntryOathBargainProviderV3,
        PostgresDangerEntryLifeMetricsProviderV3,
        PostgresDangerEntryAshWalletProviderV3,
    )
}

fn world_code(result: &WorldFlowResult) -> WorldTransferResultCode {
    match result {
        WorldFlowResult::Transfer { code, .. } | WorldFlowResult::Error { code, .. } => *code,
        WorldFlowResult::Location { .. } => panic!("post-death transfer returned a location view"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PostDeathMutationCounts {
    safe_inventory: i64,
    xp_awards: i64,
    boss_first_clears: i64,
    bargain_milestones: i64,
    bargain_decisions: i64,
    life_outbox: i64,
    world_transfers: i64,
    lineages: i64,
    restore_points: i64,
    item_ledger: i64,
    bargain_offers: i64,
    bargain_candidates: i64,
}

async fn post_death_mutation_counts(persistence: &PostgresPersistence) -> PostDeathMutationCounts {
    let identity = durable_death_fixture::PRIMARY_IDENTITY;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT \
         (SELECT count(*) FROM safe_inventory_mutations WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS safe_inventory, \
         (SELECT count(*) FROM character_xp_award_results WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS xp_awards, \
         (SELECT count(*) FROM account_boss_first_clears WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS boss_first_clears, \
         (SELECT count(*) FROM bargain_milestone_results WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS bargain_milestones, \
         (SELECT count(*) FROM bargain_decision_results WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS bargain_decisions, \
         (SELECT count(*) FROM character_life_outbox WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS life_outbox, \
         (SELECT count(*) FROM character_world_transfer_results WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS world_transfers, \
         (SELECT count(*) FROM character_instance_lineages WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS lineages, \
         (SELECT count(*) FROM character_entry_restore_points WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS restore_points, \
         (SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS item_ledger, \
         (SELECT count(*) FROM bargain_offers WHERE namespace_id=$1 \
          AND account_id=$2 AND character_id=$3) AS bargain_offers, \
         (SELECT count(*) FROM bargain_offer_candidates WHERE namespace_id=$1 \
          AND account_id=$2 AND offer_id=$4) AS bargain_candidates",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(OFFER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    PostDeathMutationCounts {
        safe_inventory: row.get("safe_inventory"),
        xp_awards: row.get("xp_awards"),
        boss_first_clears: row.get("boss_first_clears"),
        bargain_milestones: row.get("bargain_milestones"),
        bargain_decisions: row.get("bargain_decisions"),
        life_outbox: row.get("life_outbox"),
        world_transfers: row.get("world_transfers"),
        lineages: row.get("lineages"),
        restore_points: row.get("restore_points"),
        item_ledger: row.get("item_ledger"),
        bargain_offers: row.get("bargain_offers"),
        bargain_candidates: row.get("bargain_candidates"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "the four production-service outcomes and one shared before/after authority remain contiguous"
)]
async fn committed_death_rejects_production_services_without_posthumous_rows() {
    let persistence = PostgresPersistence::connect(&persistence_config())
        .await
        .unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();

    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let bargain_content =
        sim_content::load_core_development_oaths_bargains(&content_root()).unwrap();
    durable_death_fixture::seed_danger_root(&persistence).await;
    seed_open_bargain_offer(&persistence, &bargain_content).await;
    let prepared = durable_death_fixture::prepare_death(persistence.clone()).await;
    let fresh = persistence
        .transact_durable_death(prepared.request(), prepared.content(), prepared.promotion())
        .await
        .unwrap();
    assert!(matches!(fresh, DurableDeathTransactionV1::Fresh(_)));
    durable_death_fixture::assert_committed_graph(&persistence).await;

    let baseline_signature = persistence
        .load_core_death_terminal_signature_v1(
            durable_death_fixture::ACCOUNT_ID,
            durable_death_fixture::PRIMARY_IDENTITY.character_id,
        )
        .await
        .unwrap()
        .unwrap();
    baseline_signature.canonical_bytes().unwrap();
    let baseline_counts = post_death_mutation_counts(&persistence).await;
    assert_eq!(baseline_counts.bargain_offers, 1);
    assert_eq!(baseline_counts.bargain_candidates, 3);

    let safe_inventory = CoreSafeInventoryAuthority::persistent(PostgresSafeInventoryService::new(
        persistence.clone(),
    ));
    let safe_frame = safe_inventory_frame();
    let safe_first = safe_inventory.transfer(authenticated(), &safe_frame).await;
    let safe_retry = safe_inventory.transfer(authenticated(), &safe_frame).await;
    assert_eq!(safe_first, safe_retry);
    assert_eq!(
        safe_first.code,
        SafeInventoryResultCodeV1::HallBindingRequired
    );

    let progression = PostgresProgressionAwardService::new(
        persistence.clone(),
        &progression_content,
        &bargain_content,
    )
    .unwrap();
    let progression_command = progression_command(&progression_content);
    let progression_first = progression
        .award(authenticated(), &progression_command)
        .await;
    let progression_retry = progression
        .award(authenticated(), &progression_command)
        .await;
    assert_eq!(progression_first, progression_retry);
    assert_eq!(progression_first.code, ProgressionAwardCode::CharacterDead);

    let bargain =
        PostgresBargainService::new(persistence.clone(), FixedClock, &bargain_content).unwrap();
    let bargain_frame = bargain_frame(bargain_revision(&bargain_content));
    let bargain_first = bargain.decide(authenticated(), &bargain_frame).await;
    let bargain_retry = bargain.decide(authenticated(), &bargain_frame).await;
    assert_eq!(bargain_first, bargain_retry);
    assert_eq!(bargain_first.code, BargainResultCode::CharacterDead);

    let world = world_coordinator(persistence.clone(), &progression_content);
    let world_frame = world_frame();
    let world_first = world.handle(authenticated(), &world_frame).await;
    let world_retry = world.handle(authenticated(), &world_frame).await;
    assert_eq!(world_first, world_retry);
    assert_eq!(
        world_code(&world_first),
        WorldTransferResultCode::CharacterDead
    );

    assert_eq!(
        post_death_mutation_counts(&persistence).await,
        baseline_counts
    );
    let after_signature = persistence
        .load_core_death_terminal_signature_v1(
            durable_death_fixture::ACCOUNT_ID,
            durable_death_fixture::PRIMARY_IDENTITY.character_id,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_signature, baseline_signature);

    let death_replay = persistence
        .transact_durable_death(prepared.request(), prepared.content(), prepared.promotion())
        .await
        .unwrap();
    assert!(death_replay.is_replay());
    assert_eq!(death_replay.result(), fresh.result());
    assert_eq!(
        persistence
            .load_core_death_terminal_signature_v1(
                durable_death_fixture::ACCOUNT_ID,
                durable_death_fixture::PRIMARY_IDENTITY.character_id,
            )
            .await
            .unwrap()
            .unwrap(),
        baseline_signature
    );
}
