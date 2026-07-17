use std::{
    fs,
    future::Future,
    net::SocketAddr,
    num::NonZeroU64,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use client_bevy::{
    DeathSummaryAction, DeathUiActivity, DeathUiSnapshot, DeathViewClientModel, TerminalDeathPhase,
};
use sqlx::Row;
use tokio::sync::oneshot;

#[path = "support/death_measurement.rs"]
mod death_measurement;
#[path = "support/durable_death.rs"]
mod durable_death_fixture;

use persistence::{
    CaldusExtractionCommit, CaldusExtractionRequest, PersistenceConfig, PostgresPersistence,
    ProductionRecallExpectedVersionsV1, StoredExtractionAuthority, StoredWorldFlowRevisionV1,
    WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    AuthTicket, CharacterLocation, CharacterLocationSnapshot, ClientHello, Compression,
    DEATH_VIEW_SCHEMA_VERSION, DeathViewFrameV1, DeathViewRequestV1, DeathViewResultV1,
    ExtractionCommitFrameV1, ExtractionCommitPayloadV1, ExtractionCommitResultV1,
    HandshakeResponse, ManifestHash, Platform, ProtocolVersion, RESOLUTION_HOLD_SCHEMA_VERSION,
    RecallFrameV1, RecallIntentV1, RecallResultV1, RecallTerminalTriggerV1, ReliableEvent,
    ResolutionHoldActionV1, ResolutionHoldDestinationV1, ResolutionHoldDispositionV1,
    ResolutionHoldItemKindV1, ResolutionHoldItemTransitionV1, ResolutionHoldItemV1,
    ResolutionHoldMutationFrameV1, ResolutionHoldMutationPayloadV1, ResolutionHoldMutationResultV1,
    ResolutionHoldQueryFrameV1, ResolutionHoldQueryResultV1, ResolutionHoldRejectionCodeV1,
    ResolutionHoldStackV1, ResolutionHoldVersionAdvanceV1, ResolutionHoldVersionVectorV1,
    ResolutionHoldVersionsV1, SUCCESSOR_SCHEMA_VERSION, SafeArrival, StoredRecallTerminalResultV1,
    StoredResolutionHoldMutationResultV1, SuccessorCreateFrameV1, SuccessorCreatePayloadV1,
    SuccessorCreateResultV1, SuccessorRejectionCodeV1, TERMINAL_HALL_CONTENT_ID,
    TERMINAL_INVENTORY_SCHEMA_VERSION, TerminalExpectedVersionsV1,
    TerminalInventoryRejectionCodeV1, TerminalVersionAdvanceV1, TerminalVersionVectorV1, WireText,
    WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest, WorldFlowResult,
    WorldTransferCommand, WorldTransferMutation, WorldTransferPayload, WorldTransferResultCode,
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use server_app::{
    AccountId, AdmissionState, AuthenticatedAccount, AuthenticatedNamespace,
    AuthenticationDecision, BoundCoreIdentityServer, CaldusVictoryOwnerCommand,
    CharacterIdGenerator, CoreBargainAuthority, CoreExtractionTerminalAuthority,
    CoreIdentityServerConfig, CoreIdentityServerReport, CoreNonTerminalAdmission,
    CoreOathSelectionAuthority, CoreRecallActorDirectory, CoreRecallAuthoritativeTick,
    CoreRecallTerminalAuthority, CoreRecallTerminalTickOutcome, CoreReliableSequence,
    CoreResolutionHoldAuthority, CoreResolutionHoldIntentAuthority, CoreSafeInventoryAuthority,
    CoreSuccessorAuthority, CoreSuccessorIntentAuthority, CoreTerminalCoordinator,
    CoreTerminalEvaluation, CoreTerminalOtherEvaluationsV1, CoreTerminalProducer,
    CoreTerminalTickSeal, DeathViewService, DisabledDeathViewRepository,
    DisabledProgressionQueryRepository, DisposableCoreJourneyWorldFlow, DurableDeathExecutionError,
    DurableDeathExecutionService, HandshakePolicy, IdentityClock, IdentityService,
    InMemoryAccountRepository, NoopIdentityEventSink, PostgresAccountRepository,
    PostgresCaldusHallTransferCoordinator, PostgresCaldusVictoryCoordinator,
    PostgresDangerEntryAshWalletProviderV3, PostgresDangerEntryInventoryProviderV3,
    PostgresDangerEntryLifeMetricsProviderV3, PostgresDangerEntryOathBargainProviderV3,
    PostgresDeathViewRepository, PostgresDormantWorldFlowCoordinator,
    PostgresProgressionAwardService, PostgresProgressionRestoreProvider,
    PostgresResolutionHoldService, PostgresRewardService, PreparedTerminal, ProductionRecallClock,
    ProductionRecallCompletionAuthorityV1, ProductionRecallDetachOutcome,
    ProductionRecallExecutionService, ProductionRecallIntentActor,
    ProductionRecallPendingAuthorityV1, ProductionRecallPublishedV1, ProgressionQueryService,
    RecoveredProductionRecallActorV1, STORED_TERMINAL_RECEIPT_SCHEMA_V1, SecretRewardEpoch,
    StoredTerminalReceipt, StoredTerminalReceiptV1, SubmitResult, TerminalArbiter, TerminalBinding,
    TerminalCandidate, TerminalKind, WorldFlowGateService, WorldFlowIdGenerator,
    core_recall_completion_outbox, drive_recall_terminal_tick, durable_death_terminal_candidate,
    production_recall_actor_mailbox, recover_committed_death_arbiter,
    recover_committed_recall_actor, serve_core_reliable, serve_handshake,
};
use sim_core::{
    CoreBossParticipant, CoreBossParticipantLock, CoreCaldusAntiCheatState,
    CoreCaldusDefeatPresence, CoreCaldusEligibilityEvidence, CoreCaldusRecallState,
    CoreCaldusSessionState, CoreCaldusVictoryIdentities, EntityId,
};

const ACCOUNT_ID: [u8; 16] = [211; 16];
const CHARACTER_ID: [u8; 16] = [212; 16];
const TRANSFER_ID: [u8; 16] = [213; 16];
const LINEAGE_ID: [u8; 16] = [214; 16];
const RESTORE_ID: [u8; 16] = [215; 16];
const EXTRACTION_RECEIPT_ID: [u8; 16] = [217; 16];
const HALL_ID: &str = "hub.lantern_halls_01";
const WORLD_ID: &str = "world.core_microrealm_01";
const DEATH_LATENCY_SAMPLE_COUNT: usize = 10;

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

async fn reconnect_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence
}

async fn seed_character(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity)
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
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
        "UPDATE accounts SET selected_character_id=$1
         WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(CHARACTER_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
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
    sqlx::query(
        "INSERT INTO character_life_metrics \
         (namespace_id,account_id,character_id,lifetime_ticks,permadeath_combat_ticks, \
          life_metrics_version) VALUES ($1,$2,$3,0,0,1) \
          ON CONFLICT (namespace_id,account_id,character_id) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories
         (namespace_id,account_id,character_id,inventory_version) VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state
         (namespace_id,account_id,character_id,earned_bargain_slots,oath_bargain_version)
         VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id,account_id,balance,wallet_version)
         VALUES ($1,$2,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn seed_empty_resolution_hold_hall(persistence: &PostgresPersistence) {
    seed_character(persistence).await;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE character_world_locations
         SET location_kind=1,location_content_id=$1,safe_arrival_kind=0
         WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
           AND character_version=1 AND location_kind=0",
    )
    .bind(TERMINAL_HALL_CONTENT_ID)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
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

fn death_view_frame(sequence: u32, request: DeathViewRequestV1) -> DeathViewFrameV1 {
    DeathViewFrameV1 {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        sequence,
        content_revision: durable_death_fixture::death_view_revision(),
        request,
    }
}

async fn canonical_death_terminal_signature(
    persistence: &PostgresPersistence,
) -> persistence::StoredCoreDeathTerminalSignatureV1 {
    let signature = persistence
        .load_core_death_terminal_signature_v1(
            durable_death_fixture::ACCOUNT_ID,
            durable_death_fixture::CHARACTER_ID,
        )
        .await
        .unwrap()
        .expect("committed death-terminal signature");
    signature.canonical_bytes().unwrap();
    assert_ne!(signature.digest().unwrap(), [0; 32]);
    signature
}

async fn assert_zero_death_database_residue(persistence: &PostgresPersistence) {
    let residue = death_measurement::PostgresResidueSnapshotV1::capture(persistence)
        .await
        .unwrap();
    assert!(
        residue.is_zero(),
        "death journey retained PostgreSQL work: {residue:?}"
    );
}

async fn assert_complete_death_evidence(persistence: &PostgresPersistence) {
    durable_death_fixture::assert_committed_graph(persistence).await;
    assert_zero_death_database_residue(persistence).await;
}

fn disposable_world_flow(
    persistence: PostgresPersistence,
) -> impl server_app::CoreWorldFlowAuthority {
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let route = PostgresDormantWorldFlowCoordinator::new(
        persistence.clone(),
        FixedAuthority,
        FixedAuthority,
        revision(),
        PostgresProgressionRestoreProvider::new(&progression_content).unwrap(),
        PostgresDangerEntryInventoryProviderV3,
        PostgresDangerEntryOathBargainProviderV3,
        PostgresDangerEntryLifeMetricsProviderV3,
        PostgresDangerEntryAshWalletProviderV3,
    );
    let extraction =
        PostgresCaldusHallTransferCoordinator::new(persistence, FixedAuthority, revision());
    DisposableCoreJourneyWorldFlow::new(route, extraction)
}

fn disabled_progression() -> ProgressionQueryService<DisabledProgressionQueryRepository> {
    let content = sim_content::load_core_development_progression(&content_root()).unwrap();
    ProgressionQueryService::new(DisabledProgressionQueryRepository, &content).unwrap()
}

#[derive(Debug, Clone, Copy)]
struct FixedAuthority;

impl IdentityClock for FixedAuthority {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

impl ProductionRecallClock for FixedAuthority {
    fn unix_millis(&self) -> u64 {
        IdentityClock::unix_millis(self)
    }
}

#[derive(Debug)]
struct RecallRuntimeTick(AtomicU64);

impl RecallRuntimeTick {
    fn set(&self, tick: u64) {
        assert_ne!(tick, 0);
        self.0.store(tick, Ordering::SeqCst);
    }
}

impl CoreRecallAuthoritativeTick for RecallRuntimeTick {
    fn current_tick(&self, _account_id: [u8; 16], _character_id: [u8; 16]) -> NonZeroU64 {
        NonZeroU64::new(self.0.load(Ordering::SeqCst)).expect("test tick remains nonzero")
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

#[derive(Debug, Default)]
struct ScriptedResolutionHoldAuthority {
    mutation_calls: AtomicUsize,
}

impl CoreResolutionHoldIntentAuthority for ScriptedResolutionHoldAuthority {
    #[allow(
        clippy::manual_async_fn,
        reason = "the test authority mirrors the production Send-future transport contract"
    )]
    fn handle_resolution_hold_query<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a ResolutionHoldQueryFrameV1,
    ) -> impl Future<Output = ResolutionHoldQueryResultV1> + Send + 'a {
        async move {
            assert_eq!(authenticated.account_id.as_bytes(), ACCOUNT_ID);
            scripted_resolution_hold_query(frame.sequence, frame.character_id)
        }
    }

    #[allow(
        clippy::manual_async_fn,
        reason = "the test authority mirrors the production Send-future transport contract"
    )]
    fn handle_resolution_hold_mutation<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a ResolutionHoldMutationFrameV1,
    ) -> impl Future<Output = ResolutionHoldMutationResultV1> + Send + 'a {
        async move {
            assert_eq!(authenticated.account_id.as_bytes(), ACCOUNT_ID);
            match self.mutation_calls.fetch_add(1, Ordering::SeqCst) {
                0 => scripted_resolution_hold_mutation(frame.sequence, false),
                1 => scripted_resolution_hold_mutation(frame.sequence, true),
                _ => ResolutionHoldMutationResultV1::Rejected {
                    schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
                    request_sequence: frame.sequence,
                    mutation_id: frame.mutation_id,
                    character_id: frame.character_id,
                    extraction_id: frame.payload.extraction_id,
                    stack_index: frame.payload.stack_index,
                    code: ResolutionHoldRejectionCodeV1::IdempotencyConflict,
                },
            }
        }
    }
}

#[derive(Debug, Default)]
struct ScriptedSuccessorAuthority {
    calls: AtomicUsize,
}

impl CoreSuccessorIntentAuthority for ScriptedSuccessorAuthority {
    #[allow(
        clippy::manual_async_fn,
        reason = "the test authority mirrors the production Send-future transport contract"
    )]
    fn handle_successor_create<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a SuccessorCreateFrameV1,
    ) -> impl Future<Output = SuccessorCreateResultV1> + Send + 'a {
        async move {
            assert_eq!(authenticated.account_id.as_bytes(), ACCOUNT_ID);
            frame.validate().unwrap();
            self.calls.fetch_add(1, Ordering::SeqCst);
            SuccessorCreateResultV1::Rejected {
                schema_version: SUCCESSOR_SCHEMA_VERSION,
                request_sequence: frame.sequence,
                mutation_id: frame.mutation_id,
                death_id: frame.payload.death_id,
                code: SuccessorRejectionCodeV1::DeathNotFound,
            }
        }
    }
}

fn scripted_resolution_hold_query(
    request_sequence: u32,
    character_id: [u8; 16],
) -> ResolutionHoldQueryResultV1 {
    let result = ResolutionHoldQueryResultV1::Stored {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        request_sequence,
        character_id,
        versions: ResolutionHoldVersionsV1 {
            account: 1,
            character: 1,
            world: 1,
            inventory: 1,
        },
        storage_resolution_required: true,
        stacks: vec![ResolutionHoldStackV1 {
            extraction_id: [71; 16],
            stack_index: 0,
            template_id: WireText::new("item.armor.parish_leather").unwrap(),
            content_revision: WireText::new(persistence::CORE_ITEM_CONTENT_REVISION).unwrap(),
            item_kind: ResolutionHoldItemKindV1::Equipment,
            items: vec![ResolutionHoldItemV1 {
                item_uid: [72; 16],
                item_version: 1,
            }],
            stack_digest: [73; 32],
            extracted_at_unix_millis: 1_000,
            overflow_deadline_unix_millis: 259_201_000,
            planned_destination: Some(ResolutionHoldDestinationV1::CharacterSafe { slot_index: 0 }),
        }],
    };
    result.validate().unwrap();
    result
}

fn scripted_resolution_hold_mutation(
    request_sequence: u32,
    replayed: bool,
) -> ResolutionHoldMutationResultV1 {
    let result = ResolutionHoldMutationResultV1::Stored {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        request_sequence,
        replayed,
        result: Box::new(StoredResolutionHoldMutationResultV1 {
            mutation_id: [74; 16],
            character_id: CHARACTER_ID,
            extraction_id: [71; 16],
            stack_index: 0,
            action: ResolutionHoldActionV1::Move,
            result_hash: [75; 32],
            committed_at_unix_millis: 2_000,
            versions: ResolutionHoldVersionVectorV1 {
                account: ResolutionHoldVersionAdvanceV1 {
                    before: 1,
                    after: 1,
                },
                character: ResolutionHoldVersionAdvanceV1 {
                    before: 1,
                    after: 2,
                },
                world: ResolutionHoldVersionAdvanceV1 {
                    before: 1,
                    after: 2,
                },
                inventory: ResolutionHoldVersionAdvanceV1 {
                    before: 1,
                    after: 2,
                },
            },
            transitions: vec![ResolutionHoldItemTransitionV1 {
                ordinal: 0,
                item_uid: [72; 16],
                item_version: 2,
                disposition: ResolutionHoldDispositionV1::Moved {
                    destination: ResolutionHoldDestinationV1::CharacterSafe { slot_index: 0 },
                },
            }],
            remaining_hold_stack_count: 0,
            storage_resolution_required: false,
        }),
    };
    result.validate().unwrap();
    result
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

async fn commit_caldus_fixture(persistence: &PostgresPersistence) -> ([u8; 16], [u8; 16]) {
    let participant = CoreBossParticipant {
        entity_id: EntityId::new(1).unwrap(),
        party_slot: 0,
    };
    let lock = CoreBossParticipantLock {
        attempt_ordinal: 1,
        participants: vec![participant],
        maximum_health: 7_200,
    };
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let oath_bargain = sim_content::load_core_development_oaths_bargains(&content_root()).unwrap();
    let rewards = PostgresRewardService::load(
        persistence.clone(),
        &content_root(),
        SecretRewardEpoch::new("core-route-caldus-v1", [0x5a; 32]).unwrap(),
    )
    .unwrap();
    let progression = PostgresProgressionAwardService::new(
        persistence.clone(),
        &progression_content,
        &oath_bargain,
    )
    .unwrap();
    let victory = PostgresCaldusVictoryCoordinator::new(persistence.clone(), rewards, progression);
    victory
        .commit(
            LINEAGE_ID,
            &lock,
            5_400,
            9_000,
            &[CaldusVictoryOwnerCommand {
                participant,
                authenticated: AuthenticatedAccount {
                    account_id: AccountId::new(ACCOUNT_ID).unwrap(),
                    namespace: AuthenticatedNamespace::WipeableTest,
                },
                character_id: CHARACTER_ID,
                expected_progression_version: 1,
                progression_content_revision: ManifestHash::new(
                    progression_content.hashes().records_blake3.clone(),
                )
                .unwrap(),
                eligibility: CoreCaldusEligibilityEvidence {
                    participant,
                    presence_ticks: 5_400,
                    direct_damage: 100,
                    effective_healing_to_others: 0,
                    damage_prevented_on_others: 0,
                    objective_credits: 0,
                    longest_inactivity_ticks: 0,
                    defeat_presence: CoreCaldusDefeatPresence::AliveAndPresent,
                    recall_state: CoreCaldusRecallState::Stayed,
                    session_state: CoreCaldusSessionState::Valid,
                    anti_cheat_state: CoreCaldusAntiCheatState::Valid,
                },
            }],
        )
        .await
        .unwrap();
    let identities = CoreCaldusVictoryIdentities::derive(LINEAGE_ID, &lock).unwrap();
    let extraction = identities.extraction_for(participant).unwrap();
    let revision = revision();
    persistence
        .request_caldus_extraction(&CaldusExtractionRequest {
            account_id: ACCOUNT_ID,
            character_id: CHARACTER_ID,
            extraction_request_id: extraction.request_id.bytes(),
            encounter_id: identities.encounter_id.bytes(),
            instance_lineage_id: LINEAGE_ID,
            entry_restore_point_id: RESTORE_ID,
            exit_instance_id: identities.exit_instance_id.bytes(),
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
            extraction_request_id: extraction.request_id.bytes(),
            extraction_receipt_id: EXTRACTION_RECEIPT_ID,
            authority: StoredExtractionAuthority::WipeableTestEvidence,
        })
        .await
        .unwrap();
    (extraction.request_id.bytes(), EXTRACTION_RECEIPT_ID)
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

async fn connect_endpoint_pair(
    server_endpoint: &quinn::Endpoint,
    client_endpoint: &quinn::Endpoint,
    address: SocketAddr,
) -> (quinn::Connection, quinn::Connection) {
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    (client.unwrap(), server.unwrap())
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

fn death_view_policy() -> HandshakePolicy {
    let mut policy = policy();
    policy
        .feature_flags
        .push(WireText::new(protocol::CORE_DEATH_VIEW_FEATURE_FLAG).unwrap());
    policy
}

fn recall_policy() -> HandshakePolicy {
    let mut policy = policy();
    policy
        .feature_flags
        .push(WireText::new(protocol::CORE_RECALL_TERMINAL_FEATURE_FLAG).unwrap());
    policy
}

fn recall_completion_publication() -> ProductionRecallPublishedV1 {
    ProductionRecallPublishedV1 {
        result: RecallResultV1::Stored {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: Some(1),
            replayed: false,
            result: Box::new(StoredRecallTerminalResultV1 {
                character_id: durable_death_fixture::CHARACTER_ID,
                terminal_id: [81; 16],
                result_hash: [82; 32],
                trigger: RecallTerminalTriggerV1::Explicit,
                committed_at_unix_millis: 51,
                completion_tick: 112,
                destination_content_id: WireText::new(TERMINAL_HALL_CONTENT_ID).unwrap(),
                versions: TerminalVersionVectorV1 {
                    account: terminal_version(5, 5),
                    character: terminal_version(6, 7),
                    world: terminal_version(6, 7),
                    inventory: terminal_version(7, 8),
                    life_clock: terminal_version(8, 9),
                },
                stabilized_item_count: 0,
                stabilized_items_digest: [83; 32],
                destroyed_item_count: 4,
                destroyed_items_digest: [84; 32],
                destroyed_material_stack_count: 2,
                destroyed_materials_digest: [85; 32],
            }),
        },
        hall: CharacterLocationSnapshot {
            character_id: durable_death_fixture::CHARACTER_ID,
            character_version: 7,
            location: CharacterLocation::Safe {
                location_id: WireText::new(TERMINAL_HALL_CONTENT_ID).unwrap(),
                arrival: SafeArrival::HallDefault,
            },
        },
        explicit_client_tick: Some(8_000),
    }
}

fn recovered_recall_fixture(
    authenticated: AuthenticatedAccount,
) -> RecoveredProductionRecallActorV1 {
    let published = recall_completion_publication();
    let RecallResultV1::Stored { result, .. } = &published.result else {
        unreachable!("fixture publication is stored");
    };
    let receipt = StoredTerminalReceipt::from_storage(&StoredTerminalReceiptV1 {
        schema_version: STORED_TERMINAL_RECEIPT_SCHEMA_V1,
        account_id: authenticated.account_id.as_bytes(),
        character_id: result.character_id,
        lineage_id: [86; 16],
        restore_point_id: [87; 16],
        terminal_id: result.terminal_id,
        mutation_id: [88; 16],
        payload_hash: [89; 32],
        server_plan_hash: [90; 32],
        result_hash: result.result_hash,
        expected_state_version: result.versions.character.before,
        post_state_version: result.versions.character.after,
        observed_tick: result.completion_tick,
        committed_tick: result.completion_tick,
        terminal_kind_code: TerminalKind::EmergencyRecall.stable_code(),
    })
    .unwrap();
    RecoveredProductionRecallActorV1 {
        coordinator: CoreTerminalCoordinator::from_stored_receipt(authenticated, receipt).unwrap(),
        published,
    }
}

async fn active_route_recall_completion(
    persistence: &PostgresPersistence,
    server_tick: u64,
) -> ProductionRecallCompletionAuthorityV1 {
    recall_completion_for_active_character(
        persistence,
        ACCOUNT_ID,
        CHARACTER_ID,
        server_tick,
        persistence::PRODUCTION_RECALL_EXPLICIT_CHANNEL_TICKS,
    )
    .await
}

async fn recall_completion_for_active_character(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
    server_tick: u64,
    elapsed_ticks: u64,
) -> ProductionRecallCompletionAuthorityV1 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT account.state_version AS account_version,
                character.character_state_version AS character_version,
                world.character_version AS world_version,
                world.location_kind,world.location_content_id,
                world.instance_lineage_id,world.entry_restore_point_id,
                inventory.inventory_version,
                life.life_metrics_version,life.lifetime_ticks,
                life.permadeath_combat_ticks,
                progression.progression_version,
                oath.oath_bargain_version,ash.wallet_version
         FROM accounts AS account
         JOIN characters AS character
           ON character.namespace_id=account.namespace_id
          AND character.account_id=account.account_id
         JOIN character_world_locations AS world
           ON world.namespace_id=character.namespace_id
          AND world.account_id=character.account_id
          AND world.character_id=character.character_id
         JOIN character_inventories AS inventory
           ON inventory.namespace_id=character.namespace_id
          AND inventory.account_id=character.account_id
          AND inventory.character_id=character.character_id
         JOIN character_life_metrics AS life
           ON life.namespace_id=character.namespace_id
          AND life.account_id=character.account_id
          AND life.character_id=character.character_id
         JOIN character_progression AS progression
           ON progression.namespace_id=character.namespace_id
          AND progression.account_id=character.account_id
          AND progression.character_id=character.character_id
         JOIN character_oath_bargain_state AS oath
           ON oath.namespace_id=character.namespace_id
          AND oath.account_id=character.account_id
          AND oath.character_id=character.character_id
         JOIN ash_wallets AS ash
           ON ash.namespace_id=account.namespace_id
          AND ash.account_id=account.account_id
         WHERE account.namespace_id=$1 AND account.account_id=$2
           AND account.selected_character_id=$3
           AND character.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(row.try_get::<i16, _>("location_kind").unwrap(), 2);
    assert_eq!(
        row.try_get::<String, _>("location_content_id").unwrap(),
        WORLD_ID
    );
    let lineage: [u8; 16] = row
        .try_get::<Vec<u8>, _>("instance_lineage_id")
        .unwrap()
        .try_into()
        .expect("active lineage ID is exactly 16 bytes");
    let restore_point: [u8; 16] = row
        .try_get::<Vec<u8>, _>("entry_restore_point_id")
        .unwrap()
        .try_into()
        .expect("active restore-point ID is exactly 16 bytes");
    let lifetime_ticks = u64::try_from(row.try_get::<i64, _>("lifetime_ticks").unwrap()).unwrap();
    let combat_ticks =
        u64::try_from(row.try_get::<i64, _>("permadeath_combat_ticks").unwrap()).unwrap();
    let content = revision();
    let completion = ProductionRecallCompletionAuthorityV1 {
        account_id,
        character_id,
        instance_lineage_id: lineage,
        entry_restore_point_id: restore_point,
        expected_versions: ProductionRecallExpectedVersionsV1 {
            account: u64::try_from(row.try_get::<i64, _>("account_version").unwrap()).unwrap(),
            character: u64::try_from(row.try_get::<i64, _>("character_version").unwrap()).unwrap(),
            world: u64::try_from(row.try_get::<i64, _>("world_version").unwrap()).unwrap(),
            inventory: u64::try_from(row.try_get::<i64, _>("inventory_version").unwrap()).unwrap(),
            life_metrics: u64::try_from(row.try_get::<i64, _>("life_metrics_version").unwrap())
                .unwrap(),
            progression: u64::try_from(row.try_get::<i64, _>("progression_version").unwrap())
                .unwrap(),
            oath_bargain: u64::try_from(row.try_get::<i64, _>("oath_bargain_version").unwrap())
                .unwrap(),
            ash_wallet: u64::try_from(row.try_get::<i64, _>("wallet_version").unwrap()).unwrap(),
        },
        content_revision: StoredWorldFlowRevisionV1 {
            records_blake3: content.records_blake3.as_str().to_owned(),
            assets_blake3: content.assets_blake3.as_str().to_owned(),
            localization_blake3: content.localization_blake3.as_str().to_owned(),
        },
        server_tick,
        final_lifetime_ticks: lifetime_ticks.checked_add(elapsed_ticks).unwrap(),
        final_permadeath_combat_ticks: combat_ticks.checked_add(elapsed_ticks).unwrap(),
    };
    transaction.rollback().await.unwrap();
    completion
}

fn absent_recall_other_evaluations(
    completion: &ProductionRecallCompletionAuthorityV1,
) -> CoreTerminalOtherEvaluationsV1 {
    let binding = TerminalBinding::new(
        completion.account_id,
        completion.character_id,
        completion.instance_lineage_id,
        completion.entry_restore_point_id,
    )
    .unwrap();
    let absent = |producer| {
        CoreTerminalEvaluation::absent(
            producer,
            binding,
            completion.server_tick,
            completion.expected_versions.character,
        )
    };
    CoreTerminalOtherEvaluationsV1 {
        lethal: absent(CoreTerminalProducer::LethalHealth),
        extraction: absent(CoreTerminalProducer::SuccessfulExtraction),
        fault_restore: absent(CoreTerminalProducer::VerifiedFaultRestoration),
    }
}

const fn terminal_version(before: u64, after: u64) -> TerminalVersionAdvanceV1 {
    TerminalVersionAdvanceV1 { before, after }
}

fn hello() -> ClientHello {
    ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new("m03-core-route-journey-1").unwrap(),
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        auth_ticket: AuthTicket::new(durable_death_fixture::AUTH_TICKET.to_vec()).unwrap(),
        locale: WireText::new("en-US").unwrap(),
    }
}

fn disabled_extraction_frame() -> ExtractionCommitFrameV1 {
    let payload = ExtractionCommitPayloadV1 {
        extraction_request_id: [61; 16],
        expected_versions: TerminalExpectedVersionsV1 {
            account: 1,
            character: 2,
            world: 2,
            inventory: 3,
            life_clock: 4,
        },
        content_revision: revision(),
    };
    ExtractionCommitFrameV1 {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        sequence: 1,
        mutation_id: [62; 16],
        character_id: [63; 16],
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn disabled_recall_frame() -> RecallFrameV1 {
    RecallFrameV1 {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        sequence: 1,
        character_id: [63; 16],
        client_tick: 100,
        intent: RecallIntentV1::Start,
    }
}

fn disabled_resolution_hold_frames() -> (ResolutionHoldQueryFrameV1, ResolutionHoldMutationFrameV1)
{
    let query = ResolutionHoldQueryFrameV1 {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        sequence: 2,
        character_id: [63; 16],
    };
    let payload = ResolutionHoldMutationPayloadV1 {
        extraction_id: [64; 16],
        stack_index: 0,
        action: ResolutionHoldActionV1::Move,
        expected_versions: ResolutionHoldVersionsV1 {
            account: 1,
            character: 2,
            world: 2,
            inventory: 3,
        },
        content_revision: WireText::new(persistence::CORE_ITEM_CONTENT_REVISION).unwrap(),
        expected_stack_digest: [65; 32],
    };
    let mutation = ResolutionHoldMutationFrameV1 {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        sequence: 3,
        mutation_id: [66; 16],
        character_id: [63; 16],
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    };
    (query, mutation)
}

fn empty_resolution_hold_frames() -> (ResolutionHoldQueryFrameV1, ResolutionHoldMutationFrameV1) {
    let query = ResolutionHoldQueryFrameV1 {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        sequence: 1,
        character_id: CHARACTER_ID,
    };
    let payload = ResolutionHoldMutationPayloadV1 {
        extraction_id: [67; 16],
        stack_index: 0,
        action: ResolutionHoldActionV1::Move,
        expected_versions: ResolutionHoldVersionsV1 {
            account: 1,
            character: 1,
            world: 1,
            inventory: 1,
        },
        content_revision: WireText::new(persistence::CORE_ITEM_CONTENT_REVISION).unwrap(),
        expected_stack_digest: [68; 32],
    };
    let mutation = ResolutionHoldMutationFrameV1 {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        sequence: 2,
        mutation_id: [69; 16],
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    };
    (query, mutation)
}

fn scripted_resolution_hold_frame(
    sequence: u32,
    action: ResolutionHoldActionV1,
) -> ResolutionHoldMutationFrameV1 {
    let payload = ResolutionHoldMutationPayloadV1 {
        extraction_id: [71; 16],
        stack_index: 0,
        action,
        expected_versions: ResolutionHoldVersionsV1 {
            account: 1,
            character: 1,
            world: 1,
            inventory: 1,
        },
        content_revision: WireText::new(persistence::CORE_ITEM_CONTENT_REVISION).unwrap(),
        expected_stack_digest: [73; 32],
    };
    ResolutionHoldMutationFrameV1 {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        sequence,
        mutation_id: [74; 16],
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 1_500,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn scripted_successor_frame() -> SuccessorCreateFrameV1 {
    let payload = SuccessorCreatePayloadV1 {
        death_id: [75; 16],
        content_revision: WireText::new(persistence::CORE_ITEM_CONTENT_REVISION).unwrap(),
    };
    SuccessorCreateFrameV1 {
        schema_version: SUCCESSOR_SCHEMA_VERSION,
        sequence: 1,
        mutation_id: [76; 16],
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn production_death_view_hello() -> ClientHello {
    let (_, source_report) = sim_content::load_and_validate(&content_root()).unwrap();
    ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new(server_app::CORE_IDENTITY_BUILD_ID).unwrap(),
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: ManifestHash::new(source_report.package_hash_blake3).unwrap(),
        auth_ticket: AuthTicket::new(durable_death_fixture::AUTH_TICKET.to_vec()).unwrap(),
        locale: WireText::new("en-US").unwrap(),
    }
}

struct ChildCoreIdentityServer {
    child: Child,
    certificate_path: PathBuf,
    readiness_path: PathBuf,
}

impl ChildCoreIdentityServer {
    fn spawn() -> Self {
        let nonce = format!("{}", std::process::id());
        let certificate_path =
            std::env::temp_dir().join(format!("gravebound-m03-death-process-{nonce}-server.der"));
        let readiness_path = std::env::temp_dir().join(format!(
            "gravebound-m03-death-process-{nonce}-readiness.txt"
        ));
        let _ = fs::remove_file(&certificate_path);
        let _ = fs::remove_file(&readiness_path);
        let test_database_url = std::env::var(persistence::TEST_DATABASE_URL_ENV)
            .expect("hosted child process requires TEST_DATABASE_URL");
        let child = Command::new(env!("CARGO_BIN_EXE_server_app"))
            .arg("serve-core-identity")
            .arg("--bind")
            .arg("127.0.0.1:0")
            .arg("--content-root")
            .arg(content_root())
            .arg("--certificate-out")
            .arg(&certificate_path)
            .arg("--readiness-out")
            .arg(&readiness_path)
            .env(persistence::RUNTIME_DATABASE_URL_ENV, test_database_url)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn persistent Core identity child process");
        Self {
            child,
            certificate_path,
            readiness_path,
        }
    }

    async fn readiness(&mut self) -> (SocketAddr, Vec<u8>) {
        for _ in 0..400 {
            if let (Ok(address), Ok(certificate)) = (
                fs::read_to_string(&self.readiness_path),
                fs::read(&self.certificate_path),
            ) && !certificate.is_empty()
            {
                return (
                    address
                        .trim()
                        .parse()
                        .expect("published child socket address"),
                    certificate,
                );
            }
            if let Some(status) = self.child.try_wait().expect("poll child server") {
                panic!("Core identity child exited before readiness: {status}");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("Core identity child did not publish its certificate before timeout");
    }

    fn stop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.certificate_path);
        let _ = fs::remove_file(&self.readiness_path);
    }
}

impl Drop for ChildCoreIdentityServer {
    fn drop(&mut self) {
        self.stop();
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

#[allow(
    clippy::too_many_lines,
    reason = "the first authenticated QUIC session and deliberate response loss stay contiguous"
)]
async fn run_lost_death_summary_session(persistence: &PostgresPersistence) -> DeathViewResultV1 {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = disposable_world_flow(persistence.clone());
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        PostgresDeathViewRepository::new(persistence.clone()),
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_terminal = CoreRecallTerminalAuthority::disabled();
    let authenticated = durable_death_fixture::authenticated_account();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_session = async {
        serve_handshake(
            &server,
            &death_view_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("committed-death-loss-session").unwrap(),
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_terminal,
            authenticated,
            1,
            20_000,
        )
        .await
        .unwrap();
        // The client deliberately sends STOP_SENDING for this response. The read has already
        // resolved from committed PostgreSQL state, so either a completed write or transport loss
        // is acceptable and neither can change domain state.
        let _lost = serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_terminal,
            authenticated,
            2,
            20_000,
        )
        .await;
    };
    let client_session = async {
        assert!(matches!(
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap(),
            HandshakeResponse::Accepted(server)
                if server.feature_flags.iter().any(
                    |flag| flag.as_str() == protocol::CORE_DEATH_VIEW_FEATURE_FLAG
                )
        ));
        let (_, latest) = bot_client::perform_death_view(
            &client,
            death_view_frame(1, DeathViewRequestV1::LatestCommitted),
        )
        .await
        .unwrap();
        bot_client::submit_death_view_without_response(
            &client,
            death_view_frame(
                2,
                DeathViewRequestV1::Summary {
                    death_id: durable_death_fixture::DEATH_ID,
                    lost_start_ordinal: 0,
                    lost_limit: 8,
                },
            ),
        )
        .await
        .unwrap();
        latest
    };
    let ((), latest) = tokio::join!(server_session, client_session);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"lost death response");
    server_endpoint.wait_idle().await;
    latest
}

#[allow(
    clippy::too_many_lines,
    reason = "the restarted authenticated QUIC projection sequence stays contiguous for audit"
)]
async fn run_restarted_death_read_session(
    persistence: &PostgresPersistence,
) -> (
    DeathViewResultV1,
    DeathViewResultV1,
    DeathViewResultV1,
    DeathViewResultV1,
) {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = disposable_world_flow(persistence.clone());
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        PostgresDeathViewRepository::new(persistence.clone()),
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_terminal = CoreRecallTerminalAuthority::disabled();
    let authenticated = durable_death_fixture::authenticated_account();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_session = async {
        serve_handshake(
            &server,
            &death_view_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("committed-death-restart-session").unwrap(),
        )
        .await
        .unwrap();
        for response_sequence in 1..=4 {
            serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &death_views,
                &oath,
                &bargain,
                &safe_inventory,
                &CoreResolutionHoldAuthority::disabled(),
                &CoreSuccessorAuthority::disabled(),
                &extraction_terminal,
                &recall_terminal,
                authenticated,
                response_sequence,
                20_000,
            )
            .await
            .unwrap();
        }
    };
    let client_session = async {
        assert!(matches!(
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap(),
            HandshakeResponse::Accepted(server)
                if server.feature_flags.iter().any(
                    |flag| flag.as_str() == protocol::CORE_DEATH_VIEW_FEATURE_FLAG
                )
        ));
        let (_, latest) = bot_client::perform_death_view(
            &client,
            death_view_frame(1, DeathViewRequestV1::LatestCommitted),
        )
        .await
        .unwrap();
        let (_, summary) = bot_client::perform_death_view(
            &client,
            death_view_frame(
                2,
                DeathViewRequestV1::Summary {
                    death_id: durable_death_fixture::DEATH_ID,
                    lost_start_ordinal: 0,
                    lost_limit: 8,
                },
            ),
        )
        .await
        .unwrap();
        let (_, memorial) = bot_client::perform_death_view(
            &client,
            death_view_frame(
                3,
                DeathViewRequestV1::MemorialPage {
                    after: None,
                    limit: 8,
                },
            ),
        )
        .await
        .unwrap();
        let (_, trace) = bot_client::perform_death_view(
            &client,
            death_view_frame(
                4,
                DeathViewRequestV1::TracePage {
                    death_id: durable_death_fixture::DEATH_ID,
                    start_ordinal: 0,
                    limit: 8,
                },
            ),
        )
        .await
        .unwrap();
        (latest, summary, memorial, trace)
    };
    let ((), views) = tokio::join!(server_session, client_session);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"restart death reads complete");
    server_endpoint.wait_idle().await;
    views
}

#[allow(
    clippy::too_many_lines,
    reason = "the child-process handshake and four authenticated projections form one restart proof"
)]
async fn run_child_process_death_read_session() -> (
    DeathViewResultV1,
    DeathViewResultV1,
    DeathViewResultV1,
    DeathViewResultV1,
) {
    let ticket = AuthTicket::new(durable_death_fixture::AUTH_TICKET.to_vec()).unwrap();
    let expected_account = server_app::core_account_id_from_auth_ticket(&ticket).unwrap();
    assert_eq!(
        expected_account.as_bytes(),
        durable_death_fixture::ACCOUNT_ID
    );
    let mut child = ChildCoreIdentityServer::spawn();
    let (address, certificate) = child.readiness().await;
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(certificate))
        .expect("trust child-process certificate");
    let config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(config);
    let connection = endpoint
        .connect(address, "localhost")
        .unwrap()
        .await
        .expect("connect to restarted Core identity process");
    assert!(matches!(
        bot_client::perform_handshake(&connection, production_death_view_hello())
            .await
            .unwrap(),
        HandshakeResponse::Accepted(server)
            if server.feature_flags.iter().any(
                |flag| flag.as_str() == protocol::CORE_DEATH_VIEW_FEATURE_FLAG
            )
    ));
    let (_, latest) = bot_client::perform_death_view(
        &connection,
        death_view_frame(1, DeathViewRequestV1::LatestCommitted),
    )
    .await
    .unwrap();
    let (_, summary) = bot_client::perform_death_view(
        &connection,
        death_view_frame(
            2,
            DeathViewRequestV1::Summary {
                death_id: durable_death_fixture::DEATH_ID,
                lost_start_ordinal: 0,
                lost_limit: 8,
            },
        ),
    )
    .await
    .unwrap();
    let (_, memorial) = bot_client::perform_death_view(
        &connection,
        death_view_frame(
            3,
            DeathViewRequestV1::MemorialPage {
                after: None,
                limit: 8,
            },
        ),
    )
    .await
    .unwrap();
    let (_, trace) = bot_client::perform_death_view(
        &connection,
        death_view_frame(
            4,
            DeathViewRequestV1::TracePage {
                death_id: durable_death_fixture::DEATH_ID,
                start_ordinal: 0,
                limit: 8,
            },
        ),
    )
    .await
    .unwrap();
    connection.close(0_u32.into(), b"child-process death reads complete");
    endpoint.wait_idle().await;
    child.stop();
    (latest, summary, memorial, trace)
}

#[allow(
    clippy::too_many_lines,
    reason = "the end-to-end authority sequence stays contiguous for route-bypass auditing"
)]
async fn run_reliable_core_journey(persistence: &PostgresPersistence) -> Duration {
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let route = PostgresDormantWorldFlowCoordinator::new(
        persistence.clone(),
        FixedAuthority,
        FixedAuthority,
        revision(),
        PostgresProgressionRestoreProvider::new(&progression_content).unwrap(),
        PostgresDangerEntryInventoryProviderV3,
        PostgresDangerEntryOathBargainProviderV3,
        PostgresDangerEntryLifeMetricsProviderV3,
        PostgresDangerEntryAshWalletProviderV3,
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
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_terminal = CoreRecallTerminalAuthority::disabled();
    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let (server_endpoint, client_endpoint, address) = endpoints();
    let login_started = Instant::now();
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
        for response_sequence in 1..=6 {
            serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &death_views,
                &oath,
                &bargain,
                &safe_inventory,
                &CoreResolutionHoldAuthority::disabled(),
                &CoreSuccessorAuthority::disabled(),
                &extraction_terminal,
                &recall_terminal,
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
        let hall_request = route_frame(
            1,
            [224; 16],
            1,
            WorldTransferCommand::EnterHallFromCharacterSelect,
        );
        let _discarded_committed_response =
            bot_client::perform_world_flow(&client, hall_request.clone())
                .await
                .unwrap();
        let (_, hall) = bot_client::perform_world_flow(
            &client,
            WorldFlowFrame {
                sequence: 2,
                ..hall_request
            },
        )
        .await
        .unwrap();
        assert_accepted(&hall, 2, HALL_ID);
        let login_to_control = login_started.elapsed();
        let mut mismatched_danger = route_frame(
            3,
            [225; 16],
            2,
            WorldTransferCommand::UsePortal {
                portal_id: WireText::new("station.realm_gate").unwrap(),
            },
        );
        let WorldFlowRequest::Transfer(mutation) = &mut mismatched_danger.request else {
            unreachable!();
        };
        mutation.payload.content_revision.assets_blake3 =
            ManifestHash::new("f".repeat(64)).unwrap();
        mutation.payload_hash = mutation.payload.canonical_hash();
        let (_, mismatch) = bot_client::perform_world_flow(&client, mismatched_danger)
            .await
            .unwrap();
        assert!(matches!(
            mismatch,
            WorldFlowResult::Transfer {
                accepted: false,
                code: WorldTransferResultCode::ContentMismatch,
                ..
            }
        ));
        let (_, danger) = bot_client::perform_world_flow(
            &client,
            route_frame(
                4,
                [226; 16],
                2,
                WorldTransferCommand::UsePortal {
                    portal_id: WireText::new("station.realm_gate").unwrap(),
                },
            ),
        )
        .await
        .unwrap();
        assert_accepted(&danger, 3, WORLD_ID);
        let (extraction_request_id, extraction_receipt_id) =
            commit_caldus_fixture(persistence).await;
        let extraction_request = route_frame(
            5,
            [227; 16],
            3,
            WorldTransferCommand::UseCommittedExtraction {
                portal_id: WireText::new("portal.exit.dungeon.bell_sepulcher").unwrap(),
                extraction_request_id,
                extraction_receipt_id,
            },
        );
        let (_, hall_return) = bot_client::perform_world_flow(&client, extraction_request.clone())
            .await
            .unwrap();
        assert_accepted(&hall_return, 4, HALL_ID);
        let (_, extraction_replay) = bot_client::perform_world_flow(
            &client,
            WorldFlowFrame {
                sequence: 6,
                ..extraction_request
            },
        )
        .await
        .unwrap();
        assert_accepted(&extraction_replay, 4, HALL_ID);
        login_to_control
    };
    let ((), login_to_control) = tokio::join!(server_journey, client_journey);

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
    login_to_control
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one connection proves all adjacent terminal capabilities remain absent and fail closed"
)]
async fn reliable_quic_rejects_disabled_extraction_and_hold_before_domain_access() {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = WorldFlowGateService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        revision(),
    );
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_terminal = CoreRecallTerminalAuthority::disabled();
    let authenticated = durable_death_fixture::authenticated_account();
    let frame = disabled_extraction_frame();
    let (hold_query, hold_mutation) = disabled_resolution_hold_frames();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_session = async {
        serve_handshake(
            &server,
            &policy(),
            AuthenticationDecision::Accepted,
            WireText::new("disabled-extraction-session").unwrap(),
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_terminal,
            authenticated,
            1,
            100,
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_terminal,
            authenticated,
            2,
            100,
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_terminal,
            authenticated,
            3,
            100,
        )
        .await
        .unwrap();
    };
    let client_session = async {
        let HandshakeResponse::Accepted(server_hello) =
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap()
        else {
            panic!("Core handshake must succeed");
        };
        assert!(
            server_hello
                .feature_flags
                .iter()
                .all(|flag| flag.as_str() != protocol::CORE_EXTRACTION_TERMINAL_FEATURE_FLAG)
        );
        assert!(
            server_hello
                .feature_flags
                .iter()
                .all(|flag| flag.as_str() != protocol::CORE_RESOLUTION_HOLD_FEATURE_FLAG)
        );
        let event = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::ExtractionCommitFrame(frame),
        )
        .await
        .unwrap();
        assert!(matches!(
            event.event,
            ReliableEvent::ExtractionCommitResult(result)
                if matches!(
                    *result,
                    ExtractionCommitResultV1::Rejected {
                        code: TerminalInventoryRejectionCodeV1::FeatureDisabled,
                        ..
                    }
                )
        ));
        let event = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::ResolutionHoldQueryFrame(hold_query),
        )
        .await
        .unwrap();
        assert!(matches!(
            event.event,
            ReliableEvent::ResolutionHoldQueryResult(result)
                if matches!(
                    *result,
                    ResolutionHoldQueryResultV1::Rejected {
                        code: ResolutionHoldRejectionCodeV1::FeatureDisabled,
                        ..
                    }
                )
        ));
        let event = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::ResolutionHoldMutationFrame(hold_mutation),
        )
        .await
        .unwrap();
        assert!(matches!(
            event.event,
            ReliableEvent::ResolutionHoldMutationResult(result)
                if matches!(
                    *result,
                    ResolutionHoldMutationResultV1::Rejected {
                        code: ResolutionHoldRejectionCodeV1::FeatureDisabled,
                        ..
                    }
                )
        ));
    };
    tokio::join!(server_session, client_session);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"disabled extraction complete");
    server_endpoint.wait_idle().await;
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one connection proves negotiated authentication, successor dispatch, wire projection, and cleanup"
)]
async fn reliable_quic_dispatches_authenticated_successor_frame() {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = WorldFlowGateService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        revision(),
    );
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let successor = ScriptedSuccessorAuthority::default();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_terminal = CoreRecallTerminalAuthority::disabled();
    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let frame = scripted_successor_frame();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();
    let mut successor_policy = policy();
    successor_policy
        .feature_flags
        .push(WireText::new(protocol::CORE_SUCCESSOR_FEATURE_FLAG).unwrap());

    let server_session = async {
        serve_handshake(
            &server,
            &successor_policy,
            AuthenticationDecision::Accepted,
            WireText::new("scripted-successor-session").unwrap(),
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &successor,
            &extraction_terminal,
            &recall_terminal,
            authenticated,
            1,
            100,
        )
        .await
        .unwrap();
    };
    let client_session = async {
        let HandshakeResponse::Accepted(server_hello) =
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap()
        else {
            panic!("Core handshake must succeed");
        };
        assert!(
            server_hello
                .feature_flags
                .iter()
                .any(|flag| flag.as_str() == protocol::CORE_SUCCESSOR_FEATURE_FLAG)
        );
        let (event, result) = bot_client::perform_successor_create(&client, frame.clone())
            .await
            .unwrap();
        assert_eq!(event.sequence, 1);
        assert_eq!(event.server_tick, 100);
        result.validate().unwrap();
        assert!(matches!(
            result,
            SuccessorCreateResultV1::Rejected {
                request_sequence: 1,
                mutation_id,
                death_id,
                code: SuccessorRejectionCodeV1::DeathNotFound,
                ..
            } if mutation_id == [76; 16] && death_id == [75; 16]
        ));
    };
    tokio::join!(server_session, client_session);
    assert_eq!(successor.calls.load(Ordering::SeqCst), 1);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"scripted successor complete");
    server_endpoint.wait_idle().await;
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one connection proves positive Hold query, fresh Move, replay, and altered conflict ordering"
)]
async fn reliable_quic_dispatches_resolution_hold_move_replay_and_conflict() {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = WorldFlowGateService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        revision(),
    );
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let resolution_hold = ScriptedResolutionHoldAuthority::default();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_terminal = CoreRecallTerminalAuthority::disabled();
    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let query = ResolutionHoldQueryFrameV1 {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        sequence: 1,
        character_id: CHARACTER_ID,
    };
    let fresh = scripted_resolution_hold_frame(2, ResolutionHoldActionV1::Move);
    let replay = scripted_resolution_hold_frame(3, ResolutionHoldActionV1::Move);
    let altered = scripted_resolution_hold_frame(4, ResolutionHoldActionV1::DestroyConfirmed);
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();
    let mut hold_policy = policy();
    hold_policy
        .feature_flags
        .push(WireText::new(protocol::CORE_RESOLUTION_HOLD_FEATURE_FLAG).unwrap());

    let server_session = async {
        serve_handshake(
            &server,
            &hold_policy,
            AuthenticationDecision::Accepted,
            WireText::new("scripted-resolution-hold-session").unwrap(),
        )
        .await
        .unwrap();
        for response_sequence in 1..=4 {
            serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &death_views,
                &oath,
                &bargain,
                &safe_inventory,
                &resolution_hold,
                &CoreSuccessorAuthority::disabled(),
                &extraction_terminal,
                &recall_terminal,
                authenticated,
                response_sequence,
                100,
            )
            .await
            .unwrap();
        }
    };
    let client_session = async {
        let HandshakeResponse::Accepted(server_hello) =
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap()
        else {
            panic!("Core handshake must succeed");
        };
        assert!(
            server_hello
                .feature_flags
                .iter()
                .any(|flag| flag.as_str() == protocol::CORE_RESOLUTION_HOLD_FEATURE_FLAG)
        );

        let event = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::ResolutionHoldQueryFrame(query),
        )
        .await
        .unwrap();
        assert!(matches!(
            event.event,
            ReliableEvent::ResolutionHoldQueryResult(result)
                if matches!(
                    *result,
                    ResolutionHoldQueryResultV1::Stored {
                        storage_resolution_required: true,
                        ref stacks,
                        ..
                    } if stacks.len() == 1
                        && stacks[0].planned_destination
                            == Some(ResolutionHoldDestinationV1::CharacterSafe {
                                slot_index: 0,
                            })
                )
        ));

        let event = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::ResolutionHoldMutationFrame(fresh),
        )
        .await
        .unwrap();
        let ReliableEvent::ResolutionHoldMutationResult(result) = event.event else {
            panic!("fresh Hold Move must use its dedicated result kind");
        };
        let ResolutionHoldMutationResultV1::Stored {
            replayed: false,
            result: fresh_result,
            ..
        } = *result
        else {
            panic!("first Hold Move must return fresh stored authority");
        };
        assert!(!fresh_result.storage_resolution_required);
        assert_eq!(fresh_result.transitions[0].item_version, 2);

        let event = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::ResolutionHoldMutationFrame(replay),
        )
        .await
        .unwrap();
        let ReliableEvent::ResolutionHoldMutationResult(result) = event.event else {
            panic!("replayed Hold Move must use its dedicated result kind");
        };
        let ResolutionHoldMutationResultV1::Stored {
            replayed: true,
            result: replayed_result,
            ..
        } = *result
        else {
            panic!("second exact Hold Move must return replayed stored authority");
        };
        assert_eq!(fresh_result, replayed_result);

        let event = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::ResolutionHoldMutationFrame(altered),
        )
        .await
        .unwrap();
        assert!(matches!(
            event.event,
            ReliableEvent::ResolutionHoldMutationResult(result)
                if matches!(
                    *result,
                    ResolutionHoldMutationResultV1::Rejected {
                        code: ResolutionHoldRejectionCodeV1::IdempotencyConflict,
                        ..
                    }
                )
        ));
    };
    tokio::join!(server_session, client_session);
    assert_eq!(resolution_hold.mutation_calls.load(Ordering::SeqCst), 3);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"scripted ResolutionHold session complete");
    server_endpoint.wait_idle().await;
}

#[tokio::test]
async fn reliable_quic_rejects_disabled_recall_before_domain_access() {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = WorldFlowGateService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        revision(),
    );
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_terminal = CoreRecallTerminalAuthority::disabled();
    let authenticated = durable_death_fixture::authenticated_account();
    let frame = disabled_recall_frame();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_session = async {
        serve_handshake(
            &server,
            &policy(),
            AuthenticationDecision::Accepted,
            WireText::new("disabled-recall-session").unwrap(),
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_terminal,
            authenticated,
            1,
            100,
        )
        .await
        .unwrap();
    };
    let client_session = async {
        let HandshakeResponse::Accepted(server_hello) =
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap()
        else {
            panic!("Core handshake must succeed");
        };
        assert!(
            server_hello
                .feature_flags
                .iter()
                .all(|flag| flag.as_str() != protocol::CORE_RECALL_TERMINAL_FEATURE_FLAG)
        );
        let event = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::RecallFrame(frame),
        )
        .await
        .unwrap();
        assert!(matches!(
            event.event,
            ReliableEvent::RecallResult(result)
                if matches!(
                    *result,
                    RecallResultV1::Rejected {
                        code: TerminalInventoryRejectionCodeV1::FeatureDisabled,
                        ..
                    }
                )
        ));
    };
    tokio::join!(server_session, client_session);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"disabled Recall complete");
    server_endpoint.wait_idle().await;
}

#[allow(
    clippy::too_many_lines,
    reason = "the authenticated persistent query and typed mutation rejection share one QUIC session"
)]
async fn run_empty_resolution_hold_quic(
    persistence: &PostgresPersistence,
    include_missing_stack_mutation: bool,
) -> ResolutionHoldQueryResultV1 {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = disposable_world_flow(persistence.clone());
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let resolution_hold = CoreResolutionHoldAuthority::persistent(
        PostgresResolutionHoldService::new(persistence.clone()),
    );
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_terminal = CoreRecallTerminalAuthority::disabled();
    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let (query, mutation) = empty_resolution_hold_frames();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();
    let mut hold_policy = policy();
    hold_policy
        .feature_flags
        .push(WireText::new(protocol::CORE_RESOLUTION_HOLD_FEATURE_FLAG).unwrap());

    let server_session = async {
        serve_handshake(
            &server,
            &hold_policy,
            AuthenticationDecision::Accepted,
            WireText::new("persistent-resolution-hold-session").unwrap(),
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &resolution_hold,
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_terminal,
            authenticated,
            1,
            100,
        )
        .await
        .unwrap();
        if include_missing_stack_mutation {
            serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &death_views,
                &oath,
                &bargain,
                &safe_inventory,
                &resolution_hold,
                &CoreSuccessorAuthority::disabled(),
                &extraction_terminal,
                &recall_terminal,
                authenticated,
                2,
                100,
            )
            .await
            .unwrap();
        }
    };
    let client_session = async {
        let HandshakeResponse::Accepted(server_hello) =
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap()
        else {
            panic!("Core handshake must succeed");
        };
        assert!(
            server_hello
                .feature_flags
                .iter()
                .any(|flag| flag.as_str() == protocol::CORE_RESOLUTION_HOLD_FEATURE_FLAG)
        );
        let event = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::ResolutionHoldQueryFrame(query),
        )
        .await
        .unwrap();
        let ReliableEvent::ResolutionHoldQueryResult(result) = event.event else {
            panic!("persistent Hold query must use its dedicated result kind");
        };
        let result = *result;
        assert!(matches!(
            &result,
            ResolutionHoldQueryResultV1::Stored {
                versions: ResolutionHoldVersionsV1 {
                    account: 1,
                    character: 1,
                    world: 1,
                    inventory: 1,
                },
                storage_resolution_required: false,
                stacks,
                ..
            } if stacks.is_empty()
        ));
        if include_missing_stack_mutation {
            let event = bot_client::perform_reliable_gameplay(
                &client,
                protocol::WireMessage::ResolutionHoldMutationFrame(mutation),
            )
            .await
            .unwrap();
            assert!(matches!(
                event.event,
                ReliableEvent::ResolutionHoldMutationResult(result)
                    if matches!(
                        *result,
                        ResolutionHoldMutationResultV1::Rejected {
                            code: ResolutionHoldRejectionCodeV1::NoHeldStack,
                            ..
                        }
                    )
            ));
        }
        result
    };
    let ((), result) = tokio::join!(server_session, client_session);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"persistent ResolutionHold session complete");
    server_endpoint.wait_idle().await;
    result
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "the authenticated Start/Cancel QUIC journey remains contiguous for wire-level audit"
)]
async fn reliable_quic_dispatches_recall_to_one_actor_owned_channel() {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = WorldFlowGateService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        revision(),
    );
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let authenticated = durable_death_fixture::authenticated_account();
    let recall_actor = ProductionRecallIntentActor::new(
        FixedAuthority,
        authenticated.account_id.as_bytes(),
        durable_death_fixture::CHARACTER_ID,
        ProductionRecallPendingAuthorityV1 {
            pending_item_count: 4,
            pending_material_stack_count: 2,
        },
    )
    .unwrap();
    let (recall_handle, mut recall_inbox) = production_recall_actor_mailbox();
    let start = RecallFrameV1 {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        sequence: 1,
        character_id: durable_death_fixture::CHARACTER_ID,
        client_tick: 8_000,
        intent: RecallIntentV1::Start,
    };
    let cancel = RecallFrameV1 {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        sequence: 2,
        character_id: durable_death_fixture::CHARACTER_ID,
        client_tick: 8_001,
        intent: RecallIntentV1::Cancel,
    };
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_session = async {
        serve_handshake(
            &server,
            &recall_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("active-recall-session").unwrap(),
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_handle,
            authenticated,
            1,
            9_000,
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_handle,
            authenticated,
            2,
            9_001,
        )
        .await
        .unwrap();
    };
    let client_session = async {
        let HandshakeResponse::Accepted(server_hello) =
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap()
        else {
            panic!("Core handshake must succeed");
        };
        assert!(
            server_hello
                .feature_flags
                .iter()
                .any(|flag| flag.as_str() == protocol::CORE_RECALL_TERMINAL_FEATURE_FLAG)
        );
        let pending = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::RecallFrame(start),
        )
        .await
        .unwrap();
        assert_eq!(pending.server_tick, 100);
        assert!(matches!(
            pending.event,
            ReliableEvent::RecallResult(result)
                if matches!(
                    *result,
                    RecallResultV1::Pending {
                        started_tick: 100,
                        completion_tick: 112,
                        pending_item_count: 4,
                        pending_material_stack_count: 2,
                        ..
                    }
                )
        ));

        let cancelled = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::RecallFrame(cancel),
        )
        .await
        .unwrap();
        assert_eq!(cancelled.server_tick, 111);
        assert!(matches!(
            cancelled.event,
            ReliableEvent::RecallResult(result)
                if matches!(
                    *result,
                    RecallResultV1::Cancelled {
                        started_tick: 100,
                        cancelled_tick: 111,
                        ..
                    }
                )
        ));
    };
    let actor_session = async {
        assert!(recall_inbox.serve_next(&recall_actor, 100).await);
        assert!(recall_inbox.serve_next(&recall_actor, 111).await);
    };
    tokio::join!(actor_session, server_session, client_session);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"active Recall complete");
    server_endpoint.wait_idle().await;
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "the server-pushed Recall completion journey remains contiguous for wire-level audit"
)]
async fn reliable_quic_pushes_committed_recall_without_a_second_client_request() {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = WorldFlowGateService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        revision(),
    );
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let authenticated = durable_death_fixture::authenticated_account();
    let recall_actor = ProductionRecallIntentActor::new(
        FixedAuthority,
        authenticated.account_id.as_bytes(),
        durable_death_fixture::CHARACTER_ID,
        ProductionRecallPendingAuthorityV1 {
            pending_item_count: 4,
            pending_material_stack_count: 2,
        },
    )
    .unwrap();
    let (recall_handle, mut recall_inbox) = production_recall_actor_mailbox();
    let (completion_outbox, mut completion_inbox) = core_recall_completion_outbox();
    let start = RecallFrameV1 {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        sequence: 1,
        character_id: durable_death_fixture::CHARACTER_ID,
        client_tick: 8_000,
        intent: RecallIntentV1::Start,
    };
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_session = async {
        serve_handshake(
            &server,
            &recall_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("active-recall-push-session").unwrap(),
        )
        .await
        .unwrap();
        let mut sequence = CoreReliableSequence::new();
        let response_sequence = sequence.next_sequence().unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_handle,
            authenticated,
            response_sequence,
            9_000,
        )
        .await
        .unwrap();
        let delivery = completion_inbox
            .send_next(&server, &mut sequence)
            .await
            .unwrap()
            .expect("queued Recall completion");
        assert_eq!(delivery.frame.sequence, 2);
        assert_eq!(delivery.frame.server_tick, 112);
        assert_eq!(sequence.last_sequence(), 2);
        assert_eq!(delivery.hall, recall_completion_publication().hall);
    };
    let client_session = async {
        let HandshakeResponse::Accepted(server_hello) =
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap()
        else {
            panic!("Core handshake must succeed");
        };
        assert!(
            server_hello
                .feature_flags
                .iter()
                .any(|flag| flag.as_str() == protocol::CORE_RECALL_TERMINAL_FEATURE_FLAG)
        );
        let pending = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::RecallFrame(start),
        )
        .await
        .unwrap();
        assert_eq!(pending.sequence, 1);
        assert_eq!(pending.server_tick, 100);
        assert!(matches!(
            pending.event,
            ReliableEvent::RecallResult(result)
                if matches!(
                    *result,
                    RecallResultV1::Pending {
                        started_tick: 100,
                        completion_tick: 112,
                        ..
                    }
                )
        ));

        let pushed = bot_client::receive_server_reliable(&client).await.unwrap();
        assert_eq!(pushed.sequence, 2);
        assert_eq!(pushed.server_tick, 112);
        let ReliableEvent::RecallResult(result) = pushed.event else {
            panic!("server push must contain Recall completion");
        };
        let RecallResultV1::Stored {
            request_sequence: Some(1),
            replayed: false,
            result,
            ..
        } = result.as_ref()
        else {
            panic!("server push must contain the fresh stored Recall");
        };
        assert_eq!(result.completion_tick, 112);
        assert_eq!(result.trigger, RecallTerminalTriggerV1::Explicit);
        assert_eq!(
            result.destination_content_id.as_str(),
            TERMINAL_HALL_CONTENT_ID
        );
    };
    let actor_session = async {
        assert!(recall_inbox.serve_next(&recall_actor, 100).await);
        completion_outbox
            .try_publish(recall_completion_publication())
            .unwrap();
    };
    tokio::join!(actor_session, server_session, client_session);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"pushed Recall complete");
    server_endpoint.wait_idle().await;
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "the abandoned-push and reconnect replay remain one contiguous transport contract"
)]
async fn reliable_quic_abandoned_recall_push_replays_on_exact_reconnect() {
    let authenticated = durable_death_fixture::authenticated_account();
    let recovered = recovered_recall_fixture(authenticated);
    let expected = recovered.published.clone();
    let recall_actor = ProductionRecallIntentActor::new(
        FixedAuthority,
        authenticated.account_id.as_bytes(),
        durable_death_fixture::CHARACTER_ID,
        ProductionRecallPendingAuthorityV1 {
            pending_item_count: 0,
            pending_material_stack_count: 0,
        },
    )
    .unwrap();
    recall_actor
        .restore_committed_recall(&recovered)
        .await
        .unwrap();

    let (completion_outbox, mut completion_inbox) = core_recall_completion_outbox();
    completion_outbox.try_publish(expected.clone()).unwrap();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();
    let abandoned_server = async {
        serve_handshake(
            &server,
            &recall_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("abandoned-recall-push").unwrap(),
        )
        .await
        .unwrap();
        let mut sequence = CoreReliableSequence::new();
        let _delivery = completion_inbox.send_next(&server, &mut sequence).await;
    };
    let abandoning_client = async {
        let HandshakeResponse::Accepted(_) = bot_client::perform_handshake(&client, hello())
            .await
            .unwrap()
        else {
            panic!("Core handshake must succeed");
        };
        let receive = client.accept_uni().await.unwrap();
        drop(receive);
    };
    tokio::join!(abandoned_server, abandoning_client);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"abandoned Recall push");
    server_endpoint.wait_idle().await;

    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = WorldFlowGateService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        revision(),
    );
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let (recall_handle, mut recall_inbox) = production_recall_actor_mailbox();
    let exact_start = RecallFrameV1 {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        sequence: 1,
        character_id: durable_death_fixture::CHARACTER_ID,
        client_tick: 8_000,
        intent: RecallIntentV1::Start,
    };
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();
    let replay_server = async {
        serve_handshake(
            &server,
            &recall_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("reconnected-recall-session").unwrap(),
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            &CoreResolutionHoldAuthority::disabled(),
            &CoreSuccessorAuthority::disabled(),
            &extraction_terminal,
            &recall_handle,
            authenticated,
            1,
            9_000,
        )
        .await
        .unwrap();
    };
    let replay_client = async {
        let HandshakeResponse::Accepted(_) = bot_client::perform_handshake(&client, hello())
            .await
            .unwrap()
        else {
            panic!("Core handshake must succeed");
        };
        let replay = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::RecallFrame(exact_start),
        )
        .await
        .unwrap();
        assert_eq!(replay.sequence, 1);
        assert_eq!(replay.server_tick, 200);
        let ReliableEvent::RecallResult(result) = replay.event else {
            panic!("exact reconnect must return Recall");
        };
        let RecallResultV1::Stored {
            request_sequence: Some(1),
            replayed: true,
            result,
            ..
        } = result.as_ref()
        else {
            panic!("exact reconnect must return stored replay");
        };
        let RecallResultV1::Stored {
            result: expected_result,
            ..
        } = &expected.result
        else {
            unreachable!("fixture is stored");
        };
        assert_eq!(result.terminal_id, expected_result.terminal_id);
        assert_eq!(result.result_hash, expected_result.result_hash);
        assert_eq!(result.completion_tick, 112);
    };
    let replay_actor = async {
        assert!(recall_inbox.serve_next(&recall_actor, 200).await);
    };
    tokio::join!(replay_actor, replay_server, replay_client);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"Recall replay complete");
    server_endpoint.wait_idle().await;
}

#[allow(
    clippy::too_many_lines,
    reason = "the exact LinkLost boundary, reconnect push, restart recovery, and cleanup remain one auditable branch"
)]
async fn run_postgres_quic_link_lost_recall_branch(
    persistence: &PostgresPersistence,
) -> StoredRecallTerminalResultV1 {
    const LOST_TICK: u64 = 200;
    const EARLY_TICK: u64 = LOST_TICK + persistence::PRODUCTION_RECALL_LINK_LOST_TICKS - 1;
    const DUE_TICK: u64 = LOST_TICK + persistence::PRODUCTION_RECALL_LINK_LOST_TICKS;
    let authenticated = durable_death_fixture::authenticated_account();
    let tick_source = Arc::new(RecallRuntimeTick(AtomicU64::new(LOST_TICK)));
    let actor = Arc::new(
        ProductionRecallIntentActor::new(
            FixedAuthority,
            authenticated.account_id.as_bytes(),
            durable_death_fixture::CHARACTER_ID,
            ProductionRecallPendingAuthorityV1 {
                pending_item_count: 1,
                pending_material_stack_count: 1,
            },
        )
        .unwrap(),
    );
    let directory = Arc::new(CoreRecallActorDirectory::new(Arc::clone(&tick_source)));
    let registration = directory
        .register_actor(authenticated, Arc::clone(&actor))
        .await
        .unwrap();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let (lost_client, lost_server) =
        connect_endpoint_pair(&server_endpoint, &client_endpoint, address).await;
    let lost_transport = directory
        .attach_transport(authenticated, lost_server)
        .await
        .unwrap();
    lost_client.close(0_u32.into(), b"enter LinkLost");
    assert_eq!(
        directory
            .detach_transport(lost_transport.lease, 20_000)
            .await
            .unwrap(),
        ProductionRecallDetachOutcome::LinkLostStarted {
            deadline_tick: DUE_TICK
        }
    );

    let early = recall_completion_for_active_character(
        persistence,
        authenticated.account_id.as_bytes(),
        durable_death_fixture::CHARACTER_ID,
        EARLY_TICK,
        persistence::PRODUCTION_RECALL_LINK_LOST_TICKS - 1,
    )
    .await;
    let binding = TerminalBinding::new(
        early.account_id,
        early.character_id,
        early.instance_lineage_id,
        early.entry_restore_point_id,
    )
    .unwrap();
    let mut coordinator = CoreTerminalCoordinator::new(authenticated, binding).unwrap();
    let executor = ProductionRecallExecutionService::new(persistence.clone());
    assert_eq!(
        drive_recall_terminal_tick(
            actor.as_ref(),
            &mut coordinator,
            persistence,
            &executor,
            &early,
            absent_recall_other_evaluations(&early),
        )
        .await
        .unwrap(),
        CoreRecallTerminalTickOutcome::NoTerminal
    );

    tick_source.set(DUE_TICK);
    let due = recall_completion_for_active_character(
        persistence,
        authenticated.account_id.as_bytes(),
        durable_death_fixture::CHARACTER_ID,
        DUE_TICK,
        persistence::PRODUCTION_RECALL_LINK_LOST_TICKS,
    )
    .await;
    let outcome = drive_recall_terminal_tick(
        actor.as_ref(),
        &mut coordinator,
        persistence,
        &executor,
        &due,
        absent_recall_other_evaluations(&due),
    )
    .await
    .unwrap();
    let CoreRecallTerminalTickOutcome::RecallStored(published) = &outcome else {
        panic!("tick ninety must commit automatic DisconnectRecovery")
    };
    let RecallResultV1::Stored {
        request_sequence: None,
        replayed: false,
        result,
        ..
    } = &published.result
    else {
        panic!("fresh LinkLost publication must carry the stored server-generated result")
    };
    assert_eq!(result.trigger, RecallTerminalTriggerV1::LinkLost);
    assert_eq!(result.completion_tick, DUE_TICK);
    let expected_result = result.as_ref().clone();

    let (reconnected_client, reconnected_server) =
        connect_endpoint_pair(&server_endpoint, &client_endpoint, address).await;
    let reconnected = directory
        .attach_transport(authenticated, reconnected_server)
        .await
        .unwrap();
    assert!(reconnected.invalidated_connection.is_none());
    assert!(
        registration
            .completion_outbox
            .try_publish_outcome(&outcome)
            .unwrap()
    );
    let pushed = tokio::time::timeout(
        Duration::from_secs(5),
        bot_client::receive_server_reliable(&reconnected_client),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(pushed.sequence, 1);
    assert_eq!(pushed.server_tick, DUE_TICK);
    assert!(matches!(
        pushed.event,
        ReliableEvent::RecallResult(result)
            if matches!(
                result.as_ref(),
                RecallResultV1::Stored {
                    request_sequence: None,
                    replayed: false,
                    result,
                    ..
                } if result.as_ref() == &expected_result
            )
    ));

    for connection in directory.begin_shutdown().await {
        connection.close(0_u32.into(), b"LinkLost branch shutdown");
    }
    let report = directory.finish_shutdown().await.unwrap();
    assert_eq!(report.served_actor_commands, 0);
    assert_eq!(report.delivered_completion_publications, 1);
    assert_eq!(report.undelivered_completion_publications, 0);
    assert_eq!(report.abandoned_completion_publications, 0);
    assert!(report.zero_residue);
    reconnected_client.close(0_u32.into(), b"LinkLost branch complete");
    server_endpoint.close(0_u32.into(), b"LinkLost branch complete");
    client_endpoint.wait_idle().await;
    server_endpoint.wait_idle().await;
    assert_zero_death_database_residue(persistence).await;
    expected_result
}

#[allow(
    clippy::too_many_lines,
    reason = "the duplicate handoff, real lethal candidate, terminal commit, and complete cleanup form one auditable branch"
)]
async fn run_postgres_quic_link_lost_lethal_branch(persistence: &PostgresPersistence) {
    let authenticated = durable_death_fixture::authenticated_account();
    let death = durable_death_fixture::prepare_death(persistence.clone()).await;
    let lethal = durable_death_terminal_candidate(&death).unwrap();
    let due_tick = lethal.observed_tick();
    let lost_tick = due_tick
        .checked_sub(persistence::PRODUCTION_RECALL_LINK_LOST_TICKS)
        .unwrap();
    let tick_source = Arc::new(RecallRuntimeTick(AtomicU64::new(lost_tick)));
    let actor = Arc::new(
        ProductionRecallIntentActor::new(
            FixedAuthority,
            authenticated.account_id.as_bytes(),
            durable_death_fixture::CHARACTER_ID,
            ProductionRecallPendingAuthorityV1 {
                pending_item_count: 1,
                pending_material_stack_count: 1,
            },
        )
        .unwrap(),
    );
    let directory = Arc::new(CoreRecallActorDirectory::new(Arc::clone(&tick_source)));
    let registration = directory
        .register_actor(authenticated, Arc::clone(&actor))
        .await
        .unwrap();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let (old_client, old_server) =
        connect_endpoint_pair(&server_endpoint, &client_endpoint, address).await;
    let old = directory
        .attach_transport(authenticated, old_server)
        .await
        .unwrap();
    let (current_client, current_server) =
        connect_endpoint_pair(&server_endpoint, &client_endpoint, address).await;
    let current = directory
        .attach_transport(authenticated, current_server)
        .await
        .unwrap();
    current
        .invalidated_connection
        .expect("duplicate handoff returns the old connection after generation commit")
        .close(0_u32.into(), b"authoritative duplicate handoff");
    assert_eq!(
        directory.detach_transport(old.lease, 30_000).await.unwrap(),
        ProductionRecallDetachOutcome::StaleGenerationIgnored
    );
    old_client.close(0_u32.into(), b"stale transport retired");
    current_client.close(0_u32.into(), b"current transport lost");
    assert_eq!(
        directory
            .detach_transport(current.lease, 30_001)
            .await
            .unwrap(),
        ProductionRecallDetachOutcome::LinkLostStarted {
            deadline_tick: due_tick
        }
    );

    tick_source.set(due_tick);
    let due = recall_completion_for_active_character(
        persistence,
        authenticated.account_id.as_bytes(),
        durable_death_fixture::CHARACTER_ID,
        due_tick,
        persistence::PRODUCTION_RECALL_LINK_LOST_TICKS,
    )
    .await;
    let due_binding = TerminalBinding::new(
        due.account_id,
        due.character_id,
        due.instance_lineage_id,
        due.entry_restore_point_id,
    )
    .unwrap();
    assert_eq!(lethal.binding(), due_binding);
    assert_eq!(
        lethal.expected_state_version(),
        due.expected_versions.character
    );
    let mut others = absent_recall_other_evaluations(&due);
    others.lethal = CoreTerminalEvaluation::candidate(
        CoreTerminalProducer::LethalHealth,
        lethal.binding(),
        due_tick,
        due.expected_versions.character,
        lethal.clone(),
    );
    let mut coordinator = CoreTerminalCoordinator::new(authenticated, lethal.binding()).unwrap();
    let recall_executor = ProductionRecallExecutionService::new(persistence.clone());
    let outcome = drive_recall_terminal_tick(
        actor.as_ref(),
        &mut coordinator,
        persistence,
        &recall_executor,
        &due,
        others,
    )
    .await
    .unwrap();
    assert!(
        !registration
            .completion_outbox
            .try_publish_outcome(&outcome)
            .unwrap()
    );
    let CoreRecallTerminalTickOutcome::OtherTerminalPrepared(prepared) = outcome else {
        panic!("real lethal death must win the exact LinkLost deadline")
    };
    assert_eq!(prepared.winner(), &lethal);
    let committed = DurableDeathExecutionService::new(persistence.clone())
        .execute_coordinated(&mut coordinator, &prepared, &death)
        .await
        .unwrap();
    assert!(!committed.transaction.is_replay());

    let mut verification = persistence.begin_transaction().await.unwrap();
    let recall_results: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_recall_terminal_results_v1
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(authenticated.account_id.as_bytes().as_slice())
    .bind(durable_death_fixture::CHARACTER_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    assert_eq!(recall_results, 0);
    verification.rollback().await.unwrap();
    durable_death_fixture::assert_committed_graph(persistence).await;
    canonical_death_terminal_signature(persistence).await;

    for connection in directory.begin_shutdown().await {
        connection.close(0_u32.into(), b"lethal branch shutdown");
    }
    let report = directory.finish_shutdown().await.unwrap();
    assert_eq!(report.delivered_completion_publications, 0);
    assert_eq!(report.undelivered_completion_publications, 0);
    assert_eq!(report.abandoned_completion_publications, 0);
    assert!(report.zero_residue);
    server_endpoint.close(0_u32.into(), b"lethal branch complete");
    client_endpoint.wait_idle().await;
    server_endpoint.wait_idle().await;
    assert_zero_death_database_residue(persistence).await;
}

/// GDD `TECH-015`/`021`-`023` and `DTH-010`/`011`, Content Spec Core danger/Hall
/// authority, and Roadmap `GB-M03-03`/`08` require automatic Recall and lethal resolution to
/// share one exact tick, survive reconnect/restart, and leave no transport, actor, queue,
/// transaction, or lock residue.
#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn postgres_quic_link_lost_terminal_matrix_is_lethal_first_and_residue_free() {
    let persistence = disposable_database().await;
    durable_death_fixture::seed_danger_root(&persistence).await;
    let expected_recall = run_postgres_quic_link_lost_recall_branch(&persistence).await;
    persistence.close().await;

    let restarted = reconnect_database().await;
    let recovered = recover_committed_recall_actor(
        &restarted,
        durable_death_fixture::authenticated_account(),
        durable_death_fixture::CHARACTER_ID,
    )
    .await
    .unwrap()
    .expect("process restart recovers the committed LinkLost terminal");
    let RecallResultV1::Stored {
        replayed: true,
        result,
        ..
    } = recovered.published.result
    else {
        panic!("restart publication must be the exact stored replay")
    };
    assert_eq!(result.as_ref(), &expected_recall);
    assert_zero_death_database_residue(&restarted).await;

    restarted.reset_disposable_identity_data().await.unwrap();
    durable_death_fixture::seed_danger_root(&restarted).await;
    run_postgres_quic_link_lost_lethal_branch(&restarted).await;
    restarted.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "the real route, Recall commit, pool restart, actor reconstruction, and wire replay form one auditable journey"
)]
async fn reliable_quic_postgres_recall_replays_after_pool_and_actor_restart() {
    const START_TICK: u64 = 100;
    const COMPLETION_TICK: u64 = START_TICK + persistence::PRODUCTION_RECALL_EXPLICIT_CHANNEL_TICKS;
    let persistence = disposable_database().await;
    seed_character(&persistence).await;
    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = disposable_world_flow(persistence.clone());
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let recall_actor = ProductionRecallIntentActor::new(
        FixedAuthority,
        ACCOUNT_ID,
        CHARACTER_ID,
        ProductionRecallPendingAuthorityV1 {
            pending_item_count: 0,
            pending_material_stack_count: 0,
        },
    )
    .unwrap();
    let executor = ProductionRecallExecutionService::new(persistence.clone());
    let (recall_handle, mut recall_inbox) = production_recall_actor_mailbox();
    let start = RecallFrameV1 {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        sequence: 3,
        character_id: CHARACTER_ID,
        client_tick: 8_000,
        intent: RecallIntentV1::Start,
    };
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let fresh_server = async {
        serve_handshake(
            &server,
            &recall_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("postgres-recall-fresh-session").unwrap(),
        )
        .await
        .unwrap();
        let mut sequence = CoreReliableSequence::new();
        for _ in 0..3 {
            let response_sequence = sequence.next_sequence().unwrap();
            serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &death_views,
                &oath,
                &bargain,
                &safe_inventory,
                &CoreResolutionHoldAuthority::disabled(),
                &CoreSuccessorAuthority::disabled(),
                &extraction_terminal,
                &recall_handle,
                authenticated,
                response_sequence,
                9_000,
            )
            .await
            .unwrap();
        }
        assert_eq!(sequence.last_sequence(), 3);
    };
    let fresh_client = async {
        let HandshakeResponse::Accepted(server_hello) =
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap()
        else {
            panic!("active Recall handshake must succeed");
        };
        assert!(
            server_hello
                .feature_flags
                .iter()
                .any(|flag| flag.as_str() == protocol::CORE_RECALL_TERMINAL_FEATURE_FLAG)
        );
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
        let pending = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::RecallFrame(start),
        )
        .await
        .unwrap();
        assert_eq!(pending.sequence, 3);
        assert_eq!(pending.server_tick, START_TICK);
        assert!(matches!(
            pending.event,
            ReliableEvent::RecallResult(result)
                if matches!(
                    *result,
                    RecallResultV1::Pending {
                        request_sequence: 3,
                        started_tick: START_TICK,
                        completion_tick: COMPLETION_TICK,
                        pending_item_count: 0,
                        pending_material_stack_count: 0,
                        ..
                    }
                )
        ));
    };
    let fresh_actor = async {
        assert!(recall_inbox.serve_next(&recall_actor, START_TICK).await);
        let completion = active_route_recall_completion(&persistence, COMPLETION_TICK).await;
        let binding = TerminalBinding::new(
            completion.account_id,
            completion.character_id,
            completion.instance_lineage_id,
            completion.entry_restore_point_id,
        )
        .unwrap();
        let mut coordinator = CoreTerminalCoordinator::new(authenticated, binding).unwrap();
        let outcome = drive_recall_terminal_tick(
            &recall_actor,
            &mut coordinator,
            &persistence,
            &executor,
            &completion,
            absent_recall_other_evaluations(&completion),
        )
        .await
        .unwrap();
        (outcome, coordinator)
    };
    let ((), (), (fresh_outcome, fresh_coordinator)) =
        tokio::join!(fresh_server, fresh_client, fresh_actor);
    let CoreRecallTerminalTickOutcome::RecallStored(fresh_published) = fresh_outcome else {
        panic!("first PostgreSQL Recall must commit exactly once");
    };
    let RecallResultV1::Stored {
        request_sequence: Some(3),
        replayed: false,
        result: fresh_result,
        ..
    } = &fresh_published.result
    else {
        panic!("fresh driver publication must contain the committed Recall");
    };
    assert_eq!(fresh_result.completion_tick, COMPLETION_TICK);
    assert_eq!(fresh_result.stabilized_item_count, 0);
    assert_eq!(fresh_result.destroyed_item_count, 0);
    assert_eq!(fresh_result.destroyed_material_stack_count, 0);
    let stored_before_restart = persistence
        .load_committed_recall_terminal_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap()
        .expect("fresh Recall is durable before delivery");
    assert!(stored_before_restart.owns_current_hall);
    assert_eq!(stored_before_restart.result_hash, fresh_result.result_hash);
    assert_eq!(
        fresh_coordinator.committed_receipt().unwrap().result_hash(),
        &fresh_result.result_hash
    );

    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"committed before Recall delivery");
    server_endpoint.wait_idle().await;
    drop(recall_handle);
    drop(recall_inbox);
    drop(executor);
    drop(world_flow);
    drop(fresh_coordinator);
    drop(recall_actor);
    persistence.close().await;

    let restarted = reconnect_database().await;
    let recovered = recover_committed_recall_actor(&restarted, authenticated, CHARACTER_ID)
        .await
        .unwrap()
        .expect("pool restart reconstructs the current committed Recall actor");
    assert_eq!(recovered.published.hall, fresh_published.hall);
    let RecallResultV1::Stored {
        replayed: true,
        result: recovered_result,
        ..
    } = &recovered.published.result
    else {
        panic!("recovered publication must be marked as replayed");
    };
    assert_eq!(recovered_result.as_ref(), fresh_result.as_ref());
    let restarted_actor = ProductionRecallIntentActor::new(
        FixedAuthority,
        ACCOUNT_ID,
        CHARACTER_ID,
        ProductionRecallPendingAuthorityV1 {
            pending_item_count: 0,
            pending_material_stack_count: 0,
        },
    )
    .unwrap();
    restarted_actor
        .restore_committed_recall(&recovered)
        .await
        .unwrap();
    let (restarted_handle, mut restarted_inbox) = production_recall_actor_mailbox();
    let mut altered = start;
    altered.client_tick += 1;
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = WorldFlowGateService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        revision(),
    );
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let extraction_terminal = CoreExtractionTerminalAuthority::disabled();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();
    let replay_server = async {
        serve_handshake(
            &server,
            &recall_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("postgres-recall-restarted-session").unwrap(),
        )
        .await
        .unwrap();
        for response_sequence in 1..=2 {
            serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &death_views,
                &oath,
                &bargain,
                &safe_inventory,
                &CoreResolutionHoldAuthority::disabled(),
                &CoreSuccessorAuthority::disabled(),
                &extraction_terminal,
                &restarted_handle,
                authenticated,
                response_sequence,
                9_000,
            )
            .await
            .unwrap();
        }
    };
    let replay_client = async {
        let HandshakeResponse::Accepted(_) = bot_client::perform_handshake(&client, hello())
            .await
            .unwrap()
        else {
            panic!("restarted Recall handshake must succeed");
        };
        let exact = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::RecallFrame(start),
        )
        .await
        .unwrap();
        assert_eq!(exact.sequence, 1);
        assert_eq!(exact.server_tick, COMPLETION_TICK + 1);
        let ReliableEvent::RecallResult(exact_result) = exact.event else {
            panic!("exact restart request must return Recall");
        };
        let RecallResultV1::Stored {
            request_sequence: Some(3),
            replayed: true,
            result,
            ..
        } = exact_result.as_ref()
        else {
            panic!("exact restart request must return stored replay");
        };
        assert_eq!(result.as_ref(), fresh_result.as_ref());

        let conflict = bot_client::perform_reliable_gameplay(
            &client,
            protocol::WireMessage::RecallFrame(altered),
        )
        .await
        .unwrap();
        assert_eq!(conflict.sequence, 2);
        assert_eq!(conflict.server_tick, COMPLETION_TICK + 2);
        assert!(matches!(
            conflict.event,
            ReliableEvent::RecallResult(result)
                if matches!(
                    *result,
                    RecallResultV1::Rejected {
                        request_sequence: 3,
                        code: TerminalInventoryRejectionCodeV1::IdempotencyConflict,
                        ..
                    }
                )
        ));
    };
    let replay_actor = async {
        assert!(
            restarted_inbox
                .serve_next(&restarted_actor, COMPLETION_TICK + 1)
                .await
        );
        assert!(
            restarted_inbox
                .serve_next(&restarted_actor, COMPLETION_TICK + 2)
                .await
        );
    };
    tokio::join!(replay_server, replay_client, replay_actor);
    let mut verification = restarted.begin_transaction().await.unwrap();
    let terminal_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_recall_terminal_results_v1
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    assert_eq!(terminal_count, 1);
    verification.rollback().await.unwrap();
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"restarted Recall replay complete");
    server_endpoint.wait_idle().await;
    drop(restarted_handle);
    drop(restarted_inbox);
    drop(restarted_actor);
    restarted.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn reliable_quic_reads_persistent_hold_and_rejects_missing_stack_after_restart() {
    let persistence = disposable_database().await;
    seed_empty_resolution_hold_hall(&persistence).await;
    let before_restart = run_empty_resolution_hold_quic(&persistence, true).await;
    persistence.close().await;

    let restarted = reconnect_database().await;
    let after_restart = run_empty_resolution_hold_quic(&restarted, false).await;
    assert_eq!(before_restart, after_restart);
    restarted.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn reliable_quic_traverses_disposable_core_route_and_committed_extraction() {
    let persistence = disposable_database().await;
    seed_character(&persistence).await;
    let login_to_control = Box::pin(run_reliable_core_journey(&persistence)).await;
    assert!(login_to_control < Duration::from_secs(30));
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn reliable_quic_completes_25_scripted_core_journeys_below_login_budget() {
    let persistence = disposable_database().await;
    let mut login_to_control = Vec::with_capacity(25);
    for _ in 0..25 {
        persistence.reset_disposable_identity_data().await.unwrap();
        seed_character(&persistence).await;
        let elapsed = Box::pin(run_reliable_core_journey(&persistence)).await;
        assert!(elapsed < Duration::from_secs(30));
        login_to_control.push(elapsed);
    }
    login_to_control.sort_unstable();
    let median = login_to_control[login_to_control.len() / 2];
    assert!(median < Duration::from_secs(30));
    println!(
        "GB-M03-03F 25-journey login-to-control: median={}us p95={}us max={}us",
        median.as_micros(),
        login_to_control[23].as_micros(),
        login_to_control[24].as_micros()
    );
    persistence.close().await;
}

fn seal_complete_same_tick_terminal_set(
    lethal: &TerminalCandidate,
) -> (CoreTerminalCoordinator, PreparedTerminal) {
    let tick = lethal.observed_tick();
    let version = lethal.expected_state_version();
    let mut coordinator = CoreTerminalCoordinator::new(
        durable_death_fixture::authenticated_account(),
        lethal.binding(),
    )
    .unwrap();
    // Submit every competing terminal result in reverse priority order. GB-M03-08 will replace
    // the opaque non-death candidates with their extraction/Recall repository plans; this already
    // proves that their shared production barrier cannot outrank the sealed lethal plan.
    for producer in CoreTerminalProducer::ALL.into_iter().rev() {
        let competing = if producer == CoreTerminalProducer::LethalHealth {
            lethal.clone()
        } else {
            let discriminator = 60_u8 + producer.terminal_kind().stable_code();
            TerminalCandidate::from_server_plan(
                lethal.binding(),
                [discriminator; 16],
                [discriminator + 10; 16],
                [discriminator + 20; 32],
                [discriminator + 30; 32],
                version,
                tick,
                producer.terminal_kind(),
            )
            .unwrap()
        };
        coordinator
            .evaluate(CoreTerminalEvaluation::candidate(
                producer,
                lethal.binding(),
                tick,
                version,
                competing,
            ))
            .unwrap();
    }
    let CoreTerminalTickSeal::Prepared(prepared) =
        coordinator.seal_authoritative_tick(tick, version).unwrap()
    else {
        panic!("same-tick terminal set must produce a winner")
    };
    assert_eq!(prepared.winner(), lethal);
    (coordinator, prepared)
}

fn death_client_endpoint(certificate_der: &[u8]) -> quinn::Endpoint {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(certificate_der.to_vec()))
        .unwrap();
    let config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(config);
    endpoint
}

fn assert_measured_identity_shutdown(report: CoreIdentityServerReport) {
    assert_eq!(report.accepted_connections, 1);
    assert_eq!(report.rejected_connections, 0);
    assert_eq!(report.combat_sessions_admitted, 0);
    assert_eq!(report.completed_connection_tasks, 1);
    assert_eq!(report.failed_connection_tasks, 0);
    assert_eq!(report.remaining_connection_tasks, 0);
    assert_eq!(report.remaining_open_connections, 0);
    assert!(report.zero_residue);
    assert!(report.persistence_enabled);
}

#[allow(
    clippy::too_many_lines,
    reason = "one measured journey preserves the exact commit, reducer, replay, and cleanup boundaries"
)]
async fn run_measured_death_journey(
    persistence: &PostgresPersistence,
    presentation: &sim_content::CoreDevelopmentDeathView,
) -> death_measurement::DeathLatencySampleV1 {
    persistence.reset_disposable_identity_data().await.unwrap();
    durable_death_fixture::seed_danger_root(persistence).await;
    let death = durable_death_fixture::prepare_death(persistence.clone()).await;
    let candidate = durable_death_terminal_candidate(&death).unwrap();
    let (mut coordinator, prepared) = seal_complete_same_tick_terminal_set(&candidate);

    let server = BoundCoreIdentityServer::bind_persistent(
        &CoreIdentityServerConfig {
            bind_address: "127.0.0.1:0".parse().unwrap(),
            content_root: content_root(),
        },
        PostgresAccountRepository::new(persistence.clone()),
    )
    .unwrap();
    let address = server.local_address();
    let client_endpoint = death_client_endpoint(server.certificate_der());
    let (shutdown_send, shutdown_receive) = oneshot::channel::<()>();
    let server_task = tokio::spawn(server.serve_until(async {
        let _ = shutdown_receive.await;
    }));
    let connection = client_endpoint
        .connect(address, "localhost")
        .unwrap()
        .await
        .unwrap();
    assert!(matches!(
        bot_client::perform_handshake(&connection, production_death_view_hello())
            .await
            .unwrap(),
        HandshakeResponse::Accepted(server)
            if server.feature_flags.iter().any(
                |flag| flag.as_str() == protocol::CORE_DEATH_VIEW_FEATURE_FLAG
            )
    ));

    let terminal_commit_started = Instant::now();
    let committed = DurableDeathExecutionService::new(persistence.clone())
        .execute_coordinated(&mut coordinator, &prepared, &death)
        .await
        .unwrap();
    let acknowledgement = Instant::now();
    let terminal_commit_latency = terminal_commit_started.elapsed();
    assert!(!committed.transaction.is_replay());

    let mut model = DeathViewClientModel::new(presentation.clone()).unwrap();
    let latest_request = model
        .begin_committed_death_lookup(durable_death_fixture::CHARACTER_ID)
        .unwrap();
    let latest_started = Instant::now();
    let (_, latest) = bot_client::perform_death_view(&connection, latest_request)
        .await
        .unwrap();
    let latest_round_trip_latency = latest_started.elapsed();
    let latest_outcome = model.handle_result(&latest).unwrap();
    let summary_request = latest_outcome
        .follow_up
        .expect("latest committed death starts the summary query");
    let summary_started = Instant::now();
    let (_, summary) = bot_client::perform_death_view(&connection, summary_request)
        .await
        .unwrap();
    let summary_round_trip_latency = summary_started.elapsed();
    model.handle_result(&summary).unwrap();
    assert_eq!(model.terminal().phase(), TerminalDeathPhase::SummaryReady);
    assert!(
        model
            .terminal()
            .action_state(DeathSummaryAction::InspectTrace)
            .is_enabled()
    );
    let snapshot = DeathUiSnapshot::terminal(&model).unwrap();
    assert!(snapshot.summary.is_some());
    assert_eq!(snapshot.activity, DeathUiActivity::Idle);
    let acknowledgement_to_interactive_latency = acknowledgement.elapsed();
    assert!(
        acknowledgement_to_interactive_latency < Duration::from_secs(2),
        "durable acknowledgement to interactive summary exceeded DTH-021: \
         {acknowledgement_to_interactive_latency:?}"
    );

    let signature_started = Instant::now();
    let signature = canonical_death_terminal_signature(persistence).await;
    let canonical_signature_query_latency = signature_started.elapsed();

    let expected_result = committed.transaction.result().clone();
    let mut replay_arbiter = TerminalArbiter::new(candidate.binding());
    assert!(matches!(
        replay_arbiter.submit(candidate),
        SubmitResult::Accepted { .. }
    ));
    let replay_prepared = replay_arbiter
        .prepare(death.request().plan.event.death_tick)
        .unwrap();
    let replay_started = Instant::now();
    let replay = DurableDeathExecutionService::new(persistence.clone())
        .execute_prepared(&mut replay_arbiter, &replay_prepared, &death)
        .await
        .unwrap();
    let exact_replay_latency = replay_started.elapsed();
    assert!(replay.transaction.is_replay());
    assert_eq!(replay.transaction.result(), &expected_result);
    assert_eq!(
        canonical_death_terminal_signature(persistence).await,
        signature
    );

    connection.close(0_u32.into(), b"measured death journey complete");
    client_endpoint.wait_idle().await;
    assert_eq!(client_endpoint.open_connections(), 0);
    shutdown_send.send(()).unwrap();
    assert_measured_identity_shutdown(server_task.await.unwrap().unwrap());
    assert_complete_death_evidence(persistence).await;

    death_measurement::DeathLatencySampleV1 {
        terminal_commit: terminal_commit_latency,
        exact_replay: exact_replay_latency,
        canonical_signature_query: canonical_signature_query_latency,
        latest_round_trip: latest_round_trip_latency,
        summary_round_trip: summary_round_trip_latency,
        acknowledgement_to_interactive: acknowledgement_to_interactive_latency,
        zero_residue: true,
    }
}

/// GDD `DTH-001`/`DTH-020` and `TECH-020`-`023`, Content Spec `CONT-ECHO-009` and
/// `CONT-HUB-002`, and Roadmap `GB-M03-02`/`06`/`13` jointly require a committed lethal result to
/// survive lost delivery, a complete process-local authority rebuild, and a newly launched server
/// process without duplicate domain records. The lethal input in this proof is exclusively
/// server-authored; real QUIC carries only authenticated historical reads, so normal player death
/// admission remains disabled.
#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn committed_death_survives_response_loss_and_full_process_restart_over_real_quic() {
    let persistence = disposable_database().await;
    durable_death_fixture::seed_danger_root(&persistence).await;
    let death = durable_death_fixture::prepare_death(persistence.clone()).await;
    let candidate = durable_death_terminal_candidate(&death).unwrap();
    let (mut coordinator, prepared) = seal_complete_same_tick_terminal_set(&candidate);

    // GDD TECH-021/023, Content Spec CONT-ECHO-009, and Roadmap GB-M03-02D/06 require an
    // unresolved terminal to block departure during a real database outage and converge after a
    // freshly bound server authority reconnects. Closing the pool exercises the production writer error,
    // not a fake repository mode.
    let unavailable_writer = persistence.clone();
    persistence.close().await;
    assert!(matches!(
        DurableDeathExecutionService::new(unavailable_writer)
            .execute_coordinated(&mut coordinator, &prepared, &death)
            .await,
        Err(DurableDeathExecutionError::Persistence(_))
    ));
    assert_eq!(
        coordinator.non_terminal_admission(),
        CoreNonTerminalAdmission::BlockedByUnresolvedTerminal
    );

    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    let committed = DurableDeathExecutionService::new(persistence.clone())
        .execute_coordinated(&mut coordinator, &prepared, &death)
        .await
        .unwrap();
    assert!(!committed.transaction.is_replay());
    let expected_result = committed.transaction.result().clone();
    let expected_receipt = coordinator.committed_receipt().unwrap().clone();
    durable_death_fixture::assert_committed_graph(&persistence).await;
    let before_response_loss_signature = canonical_death_terminal_signature(&persistence).await;

    // Capture one projection, abandon the following summary response, and then discard every
    // process-local authority that knew the commit acknowledgement.
    let latest_before_restart = run_lost_death_summary_session(&persistence).await;
    drop(committed);
    drop(coordinator);
    persistence.close().await;

    let restarted = PostgresPersistence::connect(&config).await.unwrap();
    restarted.verify_disposable_test_database().await.unwrap();
    restarted.migrate().await.unwrap();
    let after_rebind_signature = canonical_death_terminal_signature(&restarted).await;
    assert_eq!(
        after_rebind_signature.canonical_bytes().unwrap(),
        before_response_loss_signature.canonical_bytes().unwrap()
    );

    let mut recovered = recover_committed_death_arbiter(
        &restarted,
        durable_death_fixture::ACCOUNT_ID,
        durable_death_fixture::CHARACTER_ID,
    )
    .await
    .unwrap()
    .expect("the committed terminal must reconstruct after server-authority rebind");
    assert_eq!(recovered.committed_receipt(), Some(&expected_receipt));
    assert!(matches!(
        recovered.submit(candidate.clone()),
        SubmitResult::ReplayedCommitted { receipt } if receipt == expected_receipt
    ));

    // A server that lost all in-memory acknowledgement may retry the exact sealed winner. The
    // durable writer returns its original result and the rebuilt arbiter publishes identical bytes.
    let mut replay_arbiter = TerminalArbiter::new(candidate.binding());
    assert!(matches!(
        replay_arbiter.submit(candidate),
        SubmitResult::Accepted { .. }
    ));
    let replay_prepared = replay_arbiter
        .prepare(death.request().plan.event.death_tick)
        .unwrap();
    let replay = DurableDeathExecutionService::new(restarted.clone())
        .execute_prepared(&mut replay_arbiter, &replay_prepared, &death)
        .await
        .unwrap();
    assert!(replay.transaction.is_replay());
    assert_eq!(replay.transaction.result(), &expected_result);
    assert_eq!(replay_arbiter.committed_receipt(), Some(&expected_receipt));
    let after_replay_signature = canonical_death_terminal_signature(&restarted).await;
    assert_eq!(after_replay_signature, before_response_loss_signature);

    let (latest, summary, memorial, trace) = run_restarted_death_read_session(&restarted).await;
    assert_committed_death_view_results(
        &latest_before_restart,
        &latest,
        &summary,
        &memorial,
        &trace,
    );
    assert_eq!(
        canonical_death_terminal_signature(&restarted).await,
        before_response_loss_signature
    );
    let (child_latest, child_summary, child_memorial, child_trace) =
        run_child_process_death_read_session().await;
    assert_committed_death_view_results(
        &latest_before_restart,
        &child_latest,
        &child_summary,
        &child_memorial,
        &child_trace,
    );
    assert_eq!(
        canonical_death_terminal_signature(&restarted).await,
        before_response_loss_signature
    );
    assert_complete_death_evidence(&restarted).await;
    restarted.close().await;
}

/// GDD `DTH-001` and `DTH-021` require the native summary to become interactive only after durable
/// acknowledgement and within two seconds. Content `CONT-HUB-002` supplies the exact stored
/// presentation, while Roadmap `GB-M03-06` requires measured latency and zero runtime/database
/// residue. This sample set is death-performance evidence, not the roadmap's final 25 full-loop
/// private-character journeys.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn durable_death_reaches_interactive_summary_within_two_seconds_over_real_quic() {
    let persistence = disposable_database().await;
    let presentation = sim_content::load_core_development_death_view(&content_root()).unwrap();
    let mut journeys = Vec::with_capacity(DEATH_LATENCY_SAMPLE_COUNT);
    for _ in 0..DEATH_LATENCY_SAMPLE_COUNT {
        journeys.push(Box::pin(run_measured_death_journey(&persistence, &presentation)).await);
    }
    let hashes = presentation.hashes();
    let evidence = death_measurement::DeathLatencyEvidenceV1::compile(
        &journeys,
        server_app::CORE_IDENTITY_BUILD_ID,
        hashes.records_blake3.clone(),
        hashes.assets_blake3.clone(),
        hashes.localization_blake3.clone(),
    )
    .unwrap();
    assert_eq!(evidence.sample_count, DEATH_LATENCY_SAMPLE_COUNT);
    assert!(evidence.every_summary_interactive_under_two_seconds);
    assert!(evidence.zero_transport_task_session_transaction_and_lock_residue);
    assert!(evidence.accepted);
    assert_ne!(evidence.raw_report_hash_blake3, "0".repeat(64));
    println!(
        "GB_M03_06E_LATENCY_EVIDENCE={}",
        serde_json::to_string(&evidence).unwrap()
    );
    persistence.close().await;
}

fn assert_committed_death_view_results(
    latest_before_restart: &DeathViewResultV1,
    latest: &DeathViewResultV1,
    summary: &DeathViewResultV1,
    memorial: &DeathViewResultV1,
    trace: &DeathViewResultV1,
) {
    assert_eq!(latest, latest_before_restart);
    assert!(matches!(
        latest,
        DeathViewResultV1::Latest {
            death: Some(latest),
            ..
        } if latest.death_id == durable_death_fixture::DEATH_ID
            && latest.presentation_revision == durable_death_fixture::death_view_revision()
    ));
    assert!(matches!(
        summary,
        DeathViewResultV1::Summary { summary, .. }
            if summary.death_id == durable_death_fixture::DEATH_ID
                && summary.echo_outcome == protocol::DeathEchoOutcomeV1::Available
                && summary.lost.len() == 2
                && summary.presentation_revision == durable_death_fixture::death_view_revision()
    ));
    assert!(matches!(
        memorial,
        DeathViewResultV1::MemorialPage { entries, next_cursor: None, .. }
            if entries.len() == 1
                && entries[0].cursor.death_id == durable_death_fixture::DEATH_ID
                && entries[0].presentation_revision
                    == durable_death_fixture::death_view_revision()
    ));
    assert!(matches!(
        trace,
        DeathViewResultV1::TracePage { page, .. }
            if page.death_id == durable_death_fixture::DEATH_ID
                && page.entries.len() == 2
                && page.entries.last().is_some_and(|entry| entry.lethal)
                && page.presentation_revision == durable_death_fixture::death_view_revision()
    ));
}
