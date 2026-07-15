//! `PostgreSQL` infrastructure for Gravebound durable aggregates.
//!
//! This crate owns connections, migrations, transactions, snapshots, and ledger storage. Product
//! validation and combat rules remain in their authoritative domain crates under GDD `TECH-004`.

use std::{fmt, time::Duration};

use sqlx::{
    PgConnection, PgPool, Postgres, Row, Transaction, migrate::MigrateError,
    postgres::PgPoolOptions,
};
use thiserror::Error;

mod ash_wallet;
mod bargain;
mod bargain_cleanup;
mod bargain_events;
mod bargain_milestone;
mod caldus_victory;
mod combat_loadout;
mod danger_checkpoint;
mod danger_crash_restore;
mod danger_crash_restore_repository;
mod danger_entry_restore;
mod death_live_trace_promotion;
mod death_view_repository;
mod durable_death;
mod durable_death_repository;
mod durable_terminal_recovery;
mod extraction;
mod field_equipment;
mod ground_expiry;
mod identity;
mod items;
mod life_clock_repository;
mod life_deed_repository;
mod lifecycle_signature;
mod live_damage_trace_repository;
mod oath;
mod progression;
mod progression_restore;
mod reward;
mod safe_inventory;
mod world_flow;

pub use ash_wallet::{
    ASH_CURRENCY_ID, ASH_WALLET_CAP, AshMutationCode, AshMutationKind, AshMutationRequest,
    AshWalletTransaction, StoredAshMutationResult, StoredAshWallet,
};
pub use bargain::{
    BargainDecisionTransaction, BargainDecisionTransactionState, StoredActiveBargain,
    StoredBargainCandidate, StoredBargainDecisionResult, StoredBargainLife, StoredBargainOffer,
    StoredBargainSnapshot,
};
pub use bargain_cleanup::{
    BARGAIN_LIFE_CLEANUP_EVENT_SCHEMA_VERSION, BargainLifeCleanupCommand,
    BargainLifeCleanupEventBargainV1, BargainLifeCleanupEventV1, BargainLifeCleanupResult,
    BargainLifeEndReason, cleanup_bargains_for_life_end,
};
pub use bargain_events::{
    BARGAIN_DECLINED_EVENT_SCHEMA_VERSION, BARGAIN_OFFER_EVENT_SCHEMA_VERSION,
    BargainDeclinedEventV1, BargainEventCandidateV1, BargainOfferedEventV1,
};
pub use bargain_milestone::{
    CORE_BARGAIN_LAYOUT_ID, CORE_BARGAIN_MILESTONE_ID, CORE_BARGAIN_SOURCE_ID,
    StagedBargainMilestone, StoredBargainMilestoneLife, StoredBargainMilestoneResult,
};
pub use caldus_victory::{
    CaldusVictoryExitCommit, StoredCaldusVictoryExit, StoredCaldusVictoryOwner,
};
pub use combat_loadout::{
    StoredCombatBargain, StoredCombatBeltStack, StoredCoreCombatLoadout, StoredEquippedWeapon,
};
pub use danger_checkpoint::{
    DangerCheckpointDelete, DangerCheckpointWrite, StoredDangerCheckpoint,
    stage_danger_checkpoint_cleanup,
};
pub use danger_crash_restore::{
    DANGER_CRASH_RESTORE_CONTRACT, DangerCrashAshChange, DangerCrashBargainChange,
    DangerCrashBargainRecordKind, DangerCrashItemChange, DangerCrashItemChangeKind,
    DangerCrashMaterialChange, DangerCrashRestoreCode, DangerCrashRestoreReceipt,
    DangerCrashRestoreRequest, DangerCrashRestoreTransaction, DangerCrashRestoreVersions,
    MAX_CRASH_COMPONENT_CHANGES, MAX_CRASH_ITEM_CHANGES,
};
pub use danger_entry_restore::{
    StoredDangerEntryActiveBargainV3, StoredDangerEntryAshWalletV3,
    StoredDangerEntryInventoryItemV3, StoredDangerEntryInventoryV3, StoredDangerEntryLifeMetricsV3,
    StoredDangerEntryOathBargainV3, stage_danger_entry_ash_wallet_restore_v3,
    stage_danger_entry_inventory_restore_v3, stage_danger_entry_life_metrics_restore_v3,
    stage_danger_entry_oath_bargain_restore_v3,
};
pub use death_live_trace_promotion::{
    DEATH_LIVE_TRACE_PROMOTION_DIGEST_CONTEXT_V1, DEATH_TERMINAL_PAYLOAD_HASH_CONTEXT_V1,
    DurableDeathTraceEntryProvenanceV1, DurableDeathTracePromotionV1,
    canonical_death_terminal_payload_hash_v1,
};
pub use death_view_repository::{
    DeathViewReadError, MAX_DEATH_VIEW_LOST_PER_PAGE, MAX_DEATH_VIEW_MEMORIALS_PER_PAGE,
    MAX_DEATH_VIEW_TRACE_PER_PAGE, StoredDeathMemorialCursorV1, StoredDeathMemorialEntryV1,
    StoredDeathMemorialPageV1, StoredDeathSummaryViewV1, StoredDeathTracePageV1,
    StoredLatestCommittedDeathV1,
};
pub use durable_death::{
    AuthoritativeDeathPlanV1, CORE_DEATH_VIEW_ASSETS_BLAKE3, CORE_DEATH_VIEW_LOCALIZATION_BLAKE3,
    CORE_DEATH_VIEW_RECORDS_BLAKE3, DURABLE_DEATH_CONTRACT, DURABLE_DEATH_SCHEMA_VERSION,
    DURABLE_DEATH_SUMMARY_REVISION, DURABLE_DEATH_TRACE_WINDOW_TICKS, DeathAggregateVersionsV1,
    DeathVersionAdvanceV1, DurableCombatTraceEntryV1, DurableDamageTypeV1, DurableDeathCauseV1,
    DurableDeathCommitRequestV1, DurableDeathContentAuthorityV1, DurableDeathEventV1,
    DurableDeathItemContentAuthorityV1, DurableDeathPresentationAuthorityV1,
    DurableDeathResultCodeV1, DurableDeathSummaryV1, DurableDestructionEntryV1,
    DurableDestructionLocationV1, DurableEchoEnvelopeV1, DurableEchoOutcomeV1, DurableEchoRecordV1,
    DurableEchoStateV1, DurableEchoTransitionReasonV1, DurableEchoTransitionV1,
    DurableEquipmentSlotV1, DurableMemorialRecordV1, DurableNetworkStateV1,
    DurableOrderedContentIdV1, DurableRecallStateV1, DurableSummaryDamageReferenceV1,
    DurableSummaryProjectionEntryV1, DurableSummaryProjectionKindV1, DurableSummaryProjectionsV1,
    DurableTraceStatusV1, MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES,
    MAX_DURABLE_DEATH_PLAN_PAYLOAD_BYTES, MAX_DURABLE_DEATH_RESULT_PAYLOAD_BYTES,
    MAX_DURABLE_DEATH_STATUSES_PER_ENTRY, MAX_DURABLE_DEATH_TRACE_ENTRIES,
    StoredCommittedDeathResultV1, derive_durable_death_bargain_cleanup_event_id,
};
pub use durable_death_repository::DurableDeathTransactionV1;
pub use durable_terminal_recovery::{
    DURABLE_TERMINAL_RECOVERY_SCHEMA_VERSION, StoredCommittedDeathTerminalV1,
};
pub use extraction::{
    CaldusExtractionCommit, CaldusExtractionRequest, CaldusExtractionTransaction,
    CaldusExtractionTransfer, StoredExtractionAuthority, StoredExtractionResult,
    StoredExtractionState, stage_caldus_extraction_transfer,
};
pub use field_equipment::{
    StoredFieldEquipmentCommand, StoredFieldEquipmentItem, StoredFieldEquipmentResult,
    StoredFieldEquipmentSnapshot, StoredFieldEquipmentSource,
};
pub use ground_expiry::{MAX_GROUND_EXPIRY_BATCH, StoredGroundExpiry, StoredGroundExpiryCandidate};
pub use identity::{StoredCharacter, StoredIdentityAggregate, StoredMutation};
pub use items::{
    CORE_ITEM_CONTENT_REVISION, STARTER_INITIALIZER_REVISION, STARTER_ITEM_COUNT,
    StoredStarterInitialization, StoredStarterItem,
};
pub use life_clock_repository::{
    LifeClockCheckpointCommandV1, LifeClockCheckpointRequestV1, LifeClockCheckpointTransactionV1,
    LifeClockContentAuthorityV1, LifeClockDangerAuthorityV1, LifeClockStateV1,
    StoredLifeClockCheckpointV1, StoredLifeClockHeadV1,
};
pub use life_deed_repository::{
    CORE_PROGRESSION_RECORDS_BLAKE3, CORE_WORLD_ASSETS_BLAKE3, CORE_WORLD_LOCALIZATION_BLAKE3,
    CORE_WORLD_RECORDS_BLAKE3, LifeDeedCompletionCommandV2, LifeDeedCompletionRequestV2,
    LifeDeedCompletionTransactionV2, LifeDeedContentAuthorityV2, LifeDeedKindV2,
    LifeDeedProjectionOutcomeV2, StoredLifeDeedCompletionV2, StoredLifeDeedRevocationV2,
};
pub use lifecycle_signature::{
    CORE_ITEM_LIFECYCLE_SIGNATURE_CONTEXT, StoredCoreItemLifecycleSignatureV1,
    StoredLifecycleBossFirstClearV1, StoredLifecycleCapacitiesV1, StoredLifecycleCharacterV1,
    StoredLifecycleEquipmentReceiptV1, StoredLifecycleItemV1, StoredLifecycleLedgerEntryV1,
    StoredLifecycleProgressionV1, StoredLifecycleRewardEntryV1, StoredLifecycleRewardReceiptV1,
    StoredLifecycleSafeInventoryPlacementV1, StoredLifecycleSafeInventoryReceiptV1,
    StoredLifecycleStarterReceiptV1, StoredLifecycleWorldV1, StoredLifecycleXpReceiptV1,
};
pub use live_damage_trace_repository::{
    LIVE_DAMAGE_TRACE_WINDOW_TICKS_V1, LiveDamageTraceCauseV1, LiveDamageTraceContentAuthorityV1,
    LiveDamageTraceDamageTypeV1, LiveDamageTraceDangerAuthorityV1, LiveDamageTraceEntryV1,
    LiveDamageTraceHeadV1, LiveDamageTraceNetworkStateV1, LiveDamageTraceRecallStateV1,
    LiveDamageTraceStatusV1, LiveDamageTraceTickCommandV1, LiveDamageTraceTickRequestV1,
    LiveDamageTraceTickTransactionV1, MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1,
    MAX_LIVE_DAMAGE_TRACE_STATUSES_PER_ENTRY_V1, StoredLiveDamageTraceSnapshotEntryV1,
    StoredLiveDamageTraceSnapshotV1, StoredLiveDamageTraceTickV1,
};
pub use oath::{
    OathSelectionTransaction, OathSelectionTransactionState, StoredCharacterLifeEvent,
    StoredOathCharacter, StoredOathInventory, StoredOathMutationResult,
};
pub use progression::{
    ProgressionAwardTransaction, ProgressionAwardTransactionState, StoredBossFirstClear,
    StoredBossFirstClearState, StoredEncounterLifeState, StoredEncounterRecallState,
    StoredEncounterTrustState, StoredEncounterXpEvidence, StoredLockedProgressionCharacter,
    StoredOrdinaryXpEvidence, StoredProgression, StoredProgressionAwardLocation,
    StoredProgressionContract, StoredProgressionSnapshot, StoredXpAwardResult,
    StoredXpEligibilityEvidence,
};
pub use progression_restore::{
    StoredProgressionCrashRestore, capture_progression_restore, restore_progression_after_crash,
};
pub use reward::{
    RewardPlanningState, RewardTransaction, StoredPendingItem, StoredRewardCommit,
    StoredRewardEntry, StoredRewardItem, StoredRewardOutcome, StoredRewardRequest,
};
pub use safe_inventory::{
    StoredSafeInventoryCommand, StoredSafeInventoryCommandKind, StoredSafeInventoryItem,
    StoredSafeInventoryLocation, StoredSafeInventoryPlacement, StoredSafeInventoryPreflightResult,
    StoredSafeInventoryResult, StoredSafeInventorySnapshot, load_world_flow_safe_inventory,
    stage_world_flow_safe_inventory_preflight,
};
pub use world_flow::{
    StoredDangerEntryRootV3, StoredSafeArrival, StoredWorldFlowCharacter,
    StoredWorldFlowRevisionV1, StoredWorldLocation, StoredWorldTransferReceipt, WorldFlowBegin,
    WorldFlowTransaction, WorldFlowTransactionState, WorldFlowWrite, stage_world_flow_danger_entry,
};

pub const TEST_DATABASE_URL_ENV: &str = "TEST_DATABASE_URL";
pub const RUNTIME_DATABASE_URL_ENV: &str = "GRAVEBOUND_DATABASE_URL";
pub const DESTRUCTIVE_TEST_OPT_IN_ENV: &str = "GRAVEBOUND_ALLOW_DESTRUCTIVE_DATABASE_TESTS";
pub const WIPEABLE_CORE_NAMESPACE: &str = "test.core";
pub const EXPECTED_SCHEMA_VERSION: i64 = 51;
pub const DEFAULT_MAX_CONNECTIONS: u32 = 8;
pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Secret-bearing database URL. Its `Debug` and `Display` output are always redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretDatabaseUrl(String);

impl SecretDatabaseUrl {
    pub fn new(value: impl Into<String>) -> Result<Self, PersistenceConfigError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(PersistenceConfigError::EmptyDatabaseUrl);
        }
        if !value.starts_with("postgres://") && !value.starts_with("postgresql://") {
            return Err(PersistenceConfigError::UnsupportedDatabaseScheme);
        }
        Ok(Self(value))
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretDatabaseUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretDatabaseUrl(<redacted>)")
    }
}

impl fmt::Display for SecretDatabaseUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted PostgreSQL URL>")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistenceConfig {
    pub database_url: SecretDatabaseUrl,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

impl PersistenceConfig {
    pub fn from_runtime_environment() -> Result<Self, PersistenceConfigError> {
        let database_url = std::env::var(RUNTIME_DATABASE_URL_ENV)
            .map_err(|_| PersistenceConfigError::MissingRuntimeDatabaseUrl)?;
        Ok(Self::with_database_url(SecretDatabaseUrl::new(
            database_url,
        )?))
    }

    pub fn from_test_environment() -> Result<Self, PersistenceConfigError> {
        let database_url = std::env::var(TEST_DATABASE_URL_ENV)
            .map_err(|_| PersistenceConfigError::MissingTestDatabaseUrl)?;
        Ok(Self {
            database_url: SecretDatabaseUrl::new(database_url)?,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
        })
    }

    pub const fn with_database_url(database_url: SecretDatabaseUrl) -> Self {
        Self {
            database_url,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
        }
    }

    fn validate(&self) -> Result<(), PersistenceConfigError> {
        if self.max_connections == 0 {
            return Err(PersistenceConfigError::ZeroConnections);
        }
        if self.acquire_timeout.is_zero() {
            return Err(PersistenceConfigError::ZeroAcquireTimeout);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PersistenceConfigError {
    #[error("GRAVEBOUND_DATABASE_URL is required for durable server mode")]
    MissingRuntimeDatabaseUrl,
    #[error("TEST_DATABASE_URL is required for real PostgreSQL integration tests")]
    MissingTestDatabaseUrl,
    #[error("PostgreSQL database URL cannot be empty")]
    EmptyDatabaseUrl,
    #[error("database URL must use the postgres or postgresql scheme")]
    UnsupportedDatabaseScheme,
    #[error("PostgreSQL pool must allow at least one connection")]
    ZeroConnections,
    #[error("PostgreSQL connection acquire timeout must be nonzero")]
    ZeroAcquireTimeout,
}

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error(transparent)]
    Config(#[from] PersistenceConfigError),
    #[error("PostgreSQL connection or query failed")]
    Database(#[from] sqlx::Error),
    #[error("PostgreSQL migration failed")]
    Migration(#[source] MigrateError),
    #[error("database schema version {actual} is incompatible; expected {expected}")]
    IncompatibleSchema { expected: i64, actual: i64 },
    #[error("required wipeable namespace is missing or not wipeable")]
    InvalidWipeableNamespace,
    #[error("destructive PostgreSQL tests require explicit opt-in")]
    DestructiveTestOptInRequired,
    #[error("destructive PostgreSQL tests require database gravebound_test or gravebound_test_*")]
    UnsafeTestDatabaseName,
    #[error("stored identity aggregate violates the approved schema")]
    CorruptStoredIdentity,
    #[error("stored world-flow aggregate violates the approved schema")]
    CorruptStoredWorldFlow,
    #[error("stored danger-entry restore graph violates the v2 authority contract")]
    CorruptStoredDangerEntryRestore,
    #[error("a fresh world-flow transaction must append one typed result")]
    WorldFlowResultRequired,
    #[error("world-flow character does not exist for the authenticated account")]
    WorldFlowCharacterNotFound,
    #[error("progression character does not exist for the authenticated account")]
    ProgressionCharacterNotFound,
    #[error("stored progression or XP award violates the approved schema")]
    CorruptStoredProgression,
    #[error("a fresh progression award transaction must append one typed result")]
    ProgressionAwardResultRequired,
    #[error("progression restore point does not exist for the character")]
    ProgressionRestorePointNotFound,
    #[error("a committed death, extraction, or other final resolution superseded crash restore")]
    ProgressionRestoreSuperseded,
    #[error("stored Oath selection violates the approved schema")]
    CorruptStoredOath,
    #[error("Oath character does not exist for the authenticated account")]
    OathCharacterNotFound,
    #[error("a fresh Oath transaction must append one typed result")]
    OathSelectionResultRequired,
    #[error("an accepted Oath transaction must append one character-life event")]
    OathSelectionEventRequired,
    #[error("item character does not exist for the authenticated account")]
    ItemCharacterNotFound,
    #[error("stored item state violates the approved durable item contract")]
    CorruptStoredItems,
    #[error("item request ID was reused with different canonical material")]
    ItemIdempotencyConflict,
    #[error("field equipment command expected a different inventory or item version")]
    FieldEquipmentVersionMismatch,
    #[error("field equipment source or replacement destination no longer matches")]
    FieldEquipmentBindingMismatch,
    #[error("stored safe-inventory state violates the approved durable contract")]
    CorruptStoredSafeInventory,
    #[error("stored lifecycle signature violates the approved canonical contract")]
    CorruptStoredLifecycleSignature,
    #[error("safe-inventory account does not exist")]
    SafeInventoryAccountNotFound,
    #[error("safe-inventory transfer requires the selected living character in Lantern Halls")]
    SafeInventoryHallBindingMismatch,
    #[error("safe-inventory transfer is blocked by an unresolved inventory mutation")]
    SafeInventoryUnresolvedMutation,
    #[error("safe-inventory mutation ID was reused with different canonical material")]
    SafeInventoryIdempotencyConflict,
    #[error("safe-inventory command expected different aggregate or item versions")]
    SafeInventoryVersionMismatch,
    #[error("safe-inventory source or normalized placements no longer match")]
    SafeInventoryBindingMismatch,
    #[error("safe-inventory destination lacks capacity for the complete transfer")]
    SafeInventoryStorageFull,
    #[error("stored Ash wallet or currency ledger violates the approved contract")]
    CorruptStoredAsh,
    #[error("Ash wallet account does not exist")]
    AshAccountNotFound,
    #[error("Ash mutation ID was reused with different canonical material")]
    AshIdempotencyConflict,
    #[error("stored Bargain offer or life state violates the approved contract")]
    CorruptStoredBargain,
    #[error("Bargain character does not exist for the authenticated account")]
    BargainCharacterNotFound,
    #[error("Bargain offer does not exist for the authenticated character")]
    BargainOfferNotFound,
    #[error("a fresh Bargain decision transaction must append one typed result")]
    BargainDecisionResultRequired,
    #[error("an accepted Bargain selection must append one character-life event")]
    BargainSelectionEventRequired,
    #[error("authoritative reward planning failed before commit")]
    RewardPlanningFailed,
    #[error("stored danger checkpoint violates its bounded durable contract")]
    CorruptStoredDangerCheckpoint,
    #[error("danger checkpoint character or aggregate binding does not exist")]
    DangerCheckpointCharacterNotFound,
    #[error("danger checkpoint was superseded by authoritative aggregate state")]
    StaleDangerCheckpoint,
    #[error("danger checkpoint tick was replayed with different material")]
    DangerCheckpointReplayConflict,
    #[error("danger checkpoint cleanup requires a committed safe location")]
    DangerCheckpointFinalizationNotCommitted,
    #[error("stored danger crash restoration violates the approved terminal contract")]
    CorruptStoredDangerCrashRestore,
    #[error("stored durable death request, plan, or result violates the approved contract")]
    CorruptStoredDurableDeath,
    #[error("durable death account or character does not exist for the authenticated owner")]
    DurableDeathOwnerNotFound,
    #[error(
        "durable death expected different aggregate versions (account {account}, character {character}, progression {progression}, inventory {inventory}, oath/bargain {oath_bargain}, life metrics {life_metrics})"
    )]
    DurableDeathVersionMismatch {
        account: u64,
        character: u64,
        progression: u64,
        inventory: u64,
        oath_bargain: u64,
        life_metrics: u64,
    },
    #[error(
        "durable death no longer matches selected-character, danger-root, or custody authority"
    )]
    DurableDeathBindingMismatch,
    #[error("durable death references content outside the promoted server authority")]
    DurableDeathContentMismatch,
    #[error("durable death lost terminal arbitration to an already committed outcome")]
    DurableDeathTerminalSuperseded,
    #[error("final-death identity was replayed with different canonical material")]
    DurableDeathIdempotencyConflict,
    #[error("final-death live-trace promotion was replayed with different canonical material")]
    DurableDeathTracePromotionConflict,
    #[error("danger crash restoration account or character does not exist")]
    DangerCrashRestoreOwnerNotFound,
    #[error("danger crash restoration root does not exist for the bound character")]
    DangerCrashRestorePointNotFound,
    #[error("danger crash restoration cannot unambiguously compensate danger-bound Ash")]
    DangerCrashRestoreAmbiguousAsh,
    #[error("Bargain life-end cleanup input or stored state is corrupt")]
    CorruptBargainCleanup,
    #[error("Bargain life-end cleanup expected a different aggregate version")]
    BargainCleanupVersionMismatch,
    #[error("stored Caldus victory or exit state violates its bounded durable contract")]
    CorruptCaldusVictory,
    #[error("Caldus victory identity was replayed with different canonical material")]
    CaldusVictoryIdempotencyConflict,
    #[error("Caldus exit creation requires every eligible reward to be durably terminal")]
    CaldusRewardNotTerminal,
    #[error("a durable Caldus reward terminal does not match the eligible owner binding")]
    CaldusRewardTerminalMismatch,
    #[error("stored extraction request or receipt violates the approved bounded contract")]
    CorruptStoredExtraction,
    #[error("extraction identity was replayed with different canonical material")]
    ExtractionIdempotencyConflict,
    #[error("extraction does not match the active Caldus exit and danger binding")]
    ExtractionBindingMismatch,
    #[error("extraction lost the race to crash restore or another final resolution")]
    ExtractionSuperseded,
    #[error("Hall transfer requires the matching committed extraction receipt")]
    ExtractionReceiptRequired,
    #[error("committed extraction receipt was already consumed by another transfer")]
    ExtractionAlreadyTransferred,
    #[error("stored live-deed evidence violates the approved v2 contract")]
    CorruptStoredLifeDeed,
    #[error("live-deed account, character, or aggregate authority does not exist")]
    LifeDeedOwnerNotFound,
    #[error("live-deed completion identity was replayed with changed canonical material")]
    LifeDeedIdempotencyConflict,
    #[error("live-deed completion requires the selected living normal-security danger character")]
    LifeDeedBindingMismatch,
    #[error("live-deed completion does not match the promoted Core content authority")]
    LifeDeedContentMismatch,
    #[error("live-deed completion lacks a terminal reward and progression result")]
    LifeDeedRewardNotTerminal,
    #[error("live-deed completion reward authority is inconsistent or ineligible")]
    LifeDeedRewardMismatch,
    #[error("live-deed character version mismatch: expected {expected}, durable {actual}")]
    LifeDeedCharacterVersionMismatch { expected: u64, actual: u64 },
    #[error("live-deed metrics version mismatch: expected {expected}, durable {actual}")]
    LifeDeedMetricsVersionMismatch {
        expected: u64,
        actual: u64,
        projection_digest: [u8; 32],
    },
    #[error("stored life-clock evidence violates the approved contract-1 authority")]
    CorruptStoredLifeClock,
    #[error("life-clock account, character, or aggregate authority does not exist")]
    LifeClockOwnerNotFound,
    #[error("life-clock checkpoint identity was replayed with changed canonical material")]
    LifeClockIdempotencyConflict,
    #[error("life-clock checkpoint requires the selected living normal-security character")]
    LifeClockBindingMismatch,
    #[error("life-clock checkpoint does not match the promoted Core content authority")]
    LifeClockContentMismatch,
    #[error("life-clock character version mismatch: expected {expected}, durable {actual}")]
    LifeClockCharacterVersionMismatch { expected: u64, actual: u64 },
    #[error("life-clock metrics version mismatch: expected {expected}, durable {actual}")]
    LifeClockMetricsVersionMismatch { expected: u64, actual: u64 },
    #[error("life-clock interval is discontinuous: expected end tick {expected}, got {actual}")]
    LifeClockTickDiscontinuity { expected: u64, actual: u64 },
    #[error("life-clock LinkLost interval exceeds the exact 90-tick vulnerable window")]
    LifeClockLinkLostWindowExpired,
    #[error("life-clock reached the exact LinkLost deadline and requires terminal resolution")]
    LifeClockTerminalResolutionRequired,
    #[error("stored live damage trace violates the retained contract-1 authority")]
    CorruptStoredLiveDamageTrace,
    #[error("live damage trace account, character, or aggregate authority does not exist")]
    LiveDamageTraceOwnerNotFound,
    #[error("live damage trace tick identity was replayed with changed canonical material")]
    LiveDamageTraceIdempotencyConflict,
    #[error("live damage trace requires the selected living normal-security danger character")]
    LiveDamageTraceBindingMismatch,
    #[error("live damage trace does not match the promoted Core content authority")]
    LiveDamageTraceContentMismatch,
    #[error("live damage trace character version mismatch: expected {expected}, durable {actual}")]
    LiveDamageTraceCharacterVersionMismatch { expected: u64, actual: u64 },
    #[error("live damage trace tick is not ordered after {previous}: attempted {attempted}")]
    LiveDamageTraceTickOrder { previous: u64, attempted: u64 },
    #[error("live damage trace predecessor does not match the retained active-root head")]
    LiveDamageTracePredecessorMismatch,
    #[error("live damage trace exceeds the bounded 4096-entry current window")]
    LiveDamageTraceCapacityExceeded,
    #[error("live damage trace root is terminal and rejects later standalone ingestion")]
    LiveDamageTraceTerminal,
    #[error("lethal damage evidence must be staged inside the atomic death transaction")]
    LiveDamageTraceTerminalStagingRequired,
}

impl PersistenceError {
    /// Whether a transport-level failure could have occurred after `PostgreSQL` accepted `COMMIT`.
    /// Domain, constraint, decode, pool-acquisition, and explicit rollback errors are known not to
    /// have produced an acknowledgement and must force an authority reload instead of blind retry.
    #[must_use]
    pub const fn may_have_ambiguous_commit_outcome(&self) -> bool {
        matches!(
            self,
            Self::Database(
                sqlx::Error::Io(_) | sqlx::Error::Protocol(_) | sqlx::Error::WorkerCrashed
            )
        )
    }
}

/// Returns whether `PostgreSQL` explicitly permits the complete transaction to be retried.
///
/// Both serialization failures and deadlock victims are safe to replay from the transaction
/// boundary. Retrying only `40001` leaves otherwise-correct concurrent writers dependent on
/// which participant `PostgreSQL` selects as the `40P01` deadlock victim.
pub(crate) fn is_retryable_transaction_failure(error: &PersistenceError) -> bool {
    matches!(
        error,
        PersistenceError::Database(sqlx::Error::Database(database))
            if database.code().as_deref().is_some_and(is_retryable_postgres_code)
    )
}

const fn is_retryable_postgres_code(code: &str) -> bool {
    matches!(code.as_bytes(), b"40001" | b"40P01")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessReport {
    pub schema_version: i64,
    pub namespace: &'static str,
    pub wipeable: bool,
}

#[derive(Debug, Clone)]
pub struct PostgresPersistence {
    pool: PgPool,
}

impl PostgresPersistence {
    pub async fn connect(config: &PersistenceConfig) -> Result<Self, PersistenceError> {
        config.validate()?;
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(config.database_url.expose())
            .await
            .map_err(PersistenceError::Database)?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<(), PersistenceError> {
        MIGRATOR
            .run(&self.pool)
            .await
            .map_err(PersistenceError::Migration)
    }

    pub async fn readiness(&self) -> Result<ReadinessReport, PersistenceError> {
        // Running the embedded migrator is idempotent and validates the complete applied history,
        // including checksums and missing/extra versions, before readiness is reported.
        MIGRATOR
            .run(&self.pool)
            .await
            .map_err(PersistenceError::Migration)?;
        let schema_version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0)::bigint FROM _sqlx_migrations WHERE success",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(PersistenceError::Database)?;
        if schema_version != EXPECTED_SCHEMA_VERSION {
            return Err(PersistenceError::IncompatibleSchema {
                expected: EXPECTED_SCHEMA_VERSION,
                actual: schema_version,
            });
        }

        let namespace = sqlx::query(
            "SELECT namespace_id, wipeable FROM gravebound_namespaces WHERE namespace_id = $1",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .fetch_optional(&self.pool)
        .await
        .map_err(PersistenceError::Database)?;
        let Some(namespace) = namespace else {
            return Err(PersistenceError::InvalidWipeableNamespace);
        };
        let namespace_id: String = namespace
            .try_get("namespace_id")
            .map_err(PersistenceError::Database)?;
        let wipeable: bool = namespace
            .try_get("wipeable")
            .map_err(PersistenceError::Database)?;
        if namespace_id != WIPEABLE_CORE_NAMESPACE || !wipeable {
            return Err(PersistenceError::InvalidWipeableNamespace);
        }
        Ok(ReadinessReport {
            schema_version,
            namespace: WIPEABLE_CORE_NAMESPACE,
            wipeable,
        })
    }

    /// Verifies that an explicitly supplied integration database is safe for destructive tests.
    pub async fn verify_disposable_test_database(&self) -> Result<(), PersistenceError> {
        if std::env::var(DESTRUCTIVE_TEST_OPT_IN_ENV).as_deref() != Ok("1") {
            return Err(PersistenceError::DestructiveTestOptInRequired);
        }
        let database_name: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(&self.pool)
            .await
            .map_err(PersistenceError::Database)?;
        if database_name != "gravebound_test" && !database_name.starts_with("gravebound_test_") {
            return Err(PersistenceError::UnsafeTestDatabaseName);
        }
        Ok(())
    }

    /// Clears the approved identity tables only after the destructive-test guard passes.
    pub async fn reset_disposable_identity_data(&self) -> Result<(), PersistenceError> {
        self.verify_disposable_test_database().await?;
        let mut transaction = self.begin_transaction().await?;
        // These rows bind both the character and its danger lineage. Delete the cross-linked
        // children explicitly before the account cascade reaches either side of that binding.
        for statement in [
            "DELETE FROM field_equipment_mutations WHERE namespace_id = $1",
            "DELETE FROM character_extraction_results WHERE namespace_id = $1",
            "DELETE FROM caldus_victory_exit_owners WHERE namespace_id = $1",
            "DELETE FROM caldus_victory_exits WHERE namespace_id = $1",
            "DELETE FROM bargain_decision_results WHERE namespace_id = $1",
            "DELETE FROM character_active_bargains WHERE namespace_id = $1",
            "DELETE FROM bargain_milestone_results WHERE namespace_id = $1",
            "DELETE FROM bargain_offer_candidates WHERE namespace_id = $1",
            "DELETE FROM bargain_offers WHERE namespace_id = $1",
        ] {
            sqlx::query(statement)
                .bind(WIPEABLE_CORE_NAMESPACE)
                .execute(transaction.connection())
                .await
                .map_err(PersistenceError::Database)?;
        }
        sqlx::query("DELETE FROM accounts WHERE namespace_id = $1")
            .bind(WIPEABLE_CORE_NAMESPACE)
            .execute(transaction.connection())
            .await
            .map_err(PersistenceError::Database)?;
        transaction.commit().await
    }

    /// Begins a transaction with the isolation required for durable aggregate mutation.
    pub async fn begin_transaction(&self) -> Result<PersistenceTransaction<'_>, PersistenceError> {
        let mut inner = self
            .pool
            .begin()
            .await
            .map_err(PersistenceError::Database)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *inner)
            .await
            .map_err(PersistenceError::Database)?;
        Ok(PersistenceTransaction { inner })
    }

    pub async fn close(self) {
        self.pool.close().await;
    }
}

/// Owned serializable transaction boundary for repository adapters.
pub struct PersistenceTransaction<'pool> {
    inner: Transaction<'pool, Postgres>,
}

impl PersistenceTransaction<'_> {
    /// Provides the transaction-scoped connection needed by a typed repository implementation.
    pub fn connection(&mut self) -> &mut PgConnection {
        &mut self.inner
    }

    pub async fn commit(self) -> Result<(), PersistenceError> {
        self.inner
            .commit()
            .await
            .map_err(PersistenceError::Database)
    }

    pub async fn rollback(self) -> Result<(), PersistenceError> {
        self.inner
            .rollback()
            .await
            .map_err(PersistenceError::Database)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_url_is_always_redacted() {
        let url = SecretDatabaseUrl::new("postgres://user:secret@localhost/gravebound").unwrap();
        assert_eq!(format!("{url:?}"), "SecretDatabaseUrl(<redacted>)");
        assert_eq!(url.to_string(), "<redacted PostgreSQL URL>");
        assert!(!format!("{url:?}{url}").contains("secret"));
    }

    #[test]
    fn configuration_fails_closed() {
        assert_eq!(
            SecretDatabaseUrl::new("sqlite://gravebound").unwrap_err(),
            PersistenceConfigError::UnsupportedDatabaseScheme
        );
        let mut config = PersistenceConfig::with_database_url(
            SecretDatabaseUrl::new("postgres://localhost/gravebound").unwrap(),
        );
        config.max_connections = 0;
        assert_eq!(
            config.validate().unwrap_err(),
            PersistenceConfigError::ZeroConnections
        );
    }

    #[test]
    fn transaction_retry_policy_is_narrow_and_covers_deadlock_victims() {
        assert!(is_retryable_postgres_code("40001"));
        assert!(is_retryable_postgres_code("40P01"));
        for terminal_code in ["23505", "23503", "08006", "57014"] {
            assert!(!is_retryable_postgres_code(terminal_code));
        }
    }

    #[test]
    fn only_post_commit_transport_loss_is_ambiguous_to_callers() {
        let transport = PersistenceError::Database(sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "lost acknowledgement",
        )));
        assert!(transport.may_have_ambiguous_commit_outcome());
        assert!(
            !PersistenceError::Database(sqlx::Error::PoolTimedOut)
                .may_have_ambiguous_commit_outcome()
        );
        assert!(
            !PersistenceError::LiveDamageTracePredecessorMismatch
                .may_have_ambiguous_commit_outcome()
        );
    }

    #[test]
    fn world_flow_migration_is_typed_and_contains_no_speculative_domain_payload() {
        let migration = include_str!("../../../migrations/0002_wipeable_world_flow.sql");
        for required in [
            "character_world_locations",
            "character_instance_lineages",
            "character_entry_restore_points",
            "character_world_transfer_results",
            "character_danger_checkpoints",
            "one_active_restore_point_per_character",
            "component_mask = 7",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "CREATE TABLE items", "core.1.0.0"] {
            assert!(
                !migration.contains(prohibited),
                "world-flow root migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn progression_migration_is_normalized_bounded_and_restore_ready() {
        let migration = include_str!("../../../migrations/0003_wipeable_core_progression.sql");
        for required in [
            "character_progression",
            "character_xp_award_results",
            "account_boss_first_clears",
            "entry_restore_progression_v1",
            "progression_level_xp_shape",
            "requested_xp = applied_xp + discarded_xp",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "FLOAT", "DOUBLE PRECISION", "core.1.0.0"] {
            assert!(
                !migration.contains(prohibited),
                "progression migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn durable_death_foundation_is_normalized_terminal_and_echo_atomicity_ready() {
        let migration = include_str!("../../../migrations/0031_durable_death_foundation.sql");
        for required in [
            "character_life_state_core CHECK (life_state IN (0, 1))",
            "progression_current_health_terminal",
            "entry_restore_inventory_v1",
            "entry_restore_inventory_items_v1",
            "Safe -> AtRiskEquipped",
            "character_life_metrics",
            "character_life_deeds",
            "death_events",
            "death_combat_trace_entries",
            "death_combat_trace_statuses",
            "death_destruction_entries",
            "death_summary_snapshots",
            "memorial_records_newest_first",
            "echo_records",
            "one_available_echo_per_account",
            "dormant_echoes_oldest_first",
            "echo_state_transitions",
            "death_mutation_results",
            "death_audit_events",
            "death_outbox_events",
            "permadeath-v1",
            "permadeath",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
            "DROP TABLE",
            "DELETE FROM item_instances",
            "DELETE FROM item_ledger_events",
            "localized_text",
        ] {
            assert!(
                !migration.contains(prohibited),
                "death foundation migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn strict_death_integrity_closes_relational_and_null_authority_gaps() {
        let migration = include_str!("../../../migrations/0032_strict_death_integrity.sql");
        for required in [
            "0032 requires dormant pre-route death and Echo tables",
            "character_run_material_stacks",
            "death_destruction_item_owned",
            "death_destruction_ledger_owned",
            "death_destruction_material_owned",
            "pre_location_kind IS NOT NULL",
            "pre_item_version IS NOT NULL",
            "death_request_identity",
            "death_result_request_owned",
            "death_summary_damage_parent",
            "echo_transition_creation_death_owned",
            "death_outbox_echo_owned",
            "restore_inventory_item_security_transition",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "final_damage <= raw_damage",
            "DROP TABLE",
            "TRUNCATE",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "strict death integrity migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn complete_death_graph_is_terminal_bound_deferred_and_immutable() {
        let migration = include_str!("../../../migrations/0037_complete_death_graph.sql");
        for required in [
            "0037 requires no death/Echo rows",
            "character_roster_life_shape",
            "former_roster_ordinal",
            "death_mutation_id",
            "restore_death_result_owned",
            "death_restore_terminal_owned",
            "echo_expected",
            "enforce_death_destruction_source_v1",
            "enforce_complete_death_graph_v1",
            "death combat trace is incomplete or noncanonical",
            "death destruction graph is incomplete or noncanonical",
            "enforce_echo_history_v1",
            "reject_death_history_mutation_v1",
            "reject_dead_character_insert_v1",
            "death_outbox_publish_only",
            "DEFERRABLE INITIALLY DEFERRED",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM item_instances",
            "DELETE FROM item_ledger_events",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "complete death graph migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn crash_restore_ash_changes_follow_only_the_wipeable_owner_cascade() {
        let migration = include_str!("../../../migrations/0038_crash_restore_ash_wipe_cascade.sql");
        for required in [
            "danger_crash_restore_ash_changes",
            "danger_crash_ash_original_owned",
            "danger_crash_ash_compensation_owned",
            "ON DELETE CASCADE",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "DROP TRIGGER",
            "DROP FUNCTION",
            "TRUNCATE",
            "DELETE FROM",
            "JSON",
            "JSONB",
        ] {
            assert!(
                !migration.contains(prohibited),
                "Ash wipe correction leaked {prohibited}"
            );
        }
    }

    #[test]
    fn durable_death_custody_closure_is_exact_deferred_and_wipeable() {
        let migration = include_str!("../../../migrations/0039_durable_death_custody_closure.sql");
        for required in [
            "0039 requires no death/Echo rows, death-terminal roots, or permadeath custody",
            "pre_oath_bargain_version",
            "post_oath_bargain_version = pre_oath_bargain_version + 1",
            "bargain_cleanup_event_id",
            "death_oath_bargain_state_owned",
            "death_bargain_cleanup_outbox_owned",
            "event_type = 'bargains_cleared_death'",
            "character_active_bargains AS active_bargain",
            "entry_ordinal BETWEEN 0 AND 4095",
            "section.section_kind IN (1, 2)",
            "item_terminal_death_owned",
            "item_ledger_terminal_death_owned",
            "destruction_reason IN ('ground_expired', 'crash_revoked')",
            "reason = 'ground_expired' AND terminal_death_id IS NULL",
            "ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED",
            "CREATE OR REPLACE FUNCTION enforce_complete_death_graph_v1()",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM item_instances",
            "DELETE FROM item_ledger_events",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "durable-death custody migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn lifetime_and_permadeath_combat_clocks_remain_independent() {
        let migration = include_str!("../../../migrations/0040_independent_death_clocks.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "DROP CONSTRAINT entry_restore_life_v3_ticks",
            "captured_lifetime_ticks >= 0",
            "rollback_permadeath_combat_ticks >= 0",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "rollback_permadeath_combat_ticks <= captured_lifetime_ticks",
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM",
        ] {
            assert!(
                !migration.contains(prohibited),
                "independent clock correction leaked {prohibited}"
            );
        }
    }

    #[test]
    fn death_child_insert_window_guards_relation_specific_fields_procedurally() {
        let migration =
            include_str!("../../../migrations/0041_death_child_trigger_relation_guard.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "CREATE OR REPLACE FUNCTION enforce_death_child_insert_window_v1()",
            "IF TG_TABLE_NAME = 'death_outbox_events' THEN",
            "IF NEW.event_type = 'echo_promoted' THEN",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "TG_TABLE_NAME = 'death_outbox_events' AND NEW.event_type",
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM",
        ] {
            assert!(
                !migration.contains(prohibited),
                "relation-safe death trigger correction leaked {prohibited}"
            );
        }
    }

    #[test]
    fn echo_promotion_trigger_authority_is_account_bound() {
        let migration = include_str!("../../../migrations/0042_echo_promotion_account_binding.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "echo_promotion_trigger_account_exact",
            "echo_promotion_outbox_trigger_exact",
            "transition.trigger_death_id = outbox.trigger_death_id",
            "echo.account_id = trigger_death.account_id",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DROP TABLE", "TRUNCATE", "DELETE FROM"] {
            assert!(
                !migration.contains(prohibited),
                "Echo promotion authority migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn live_death_evidence_is_normalized_bounded_and_terminal_guarded() {
        let migration = include_str!("../../../migrations/0043_live_death_evidence.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "character_life_clock_checkpoint_receipts_v1",
            "character_life_deed_completion_receipts_v1",
            "character_live_damage_trace_ticks_v1",
            "character_live_damage_trace_entries_v1",
            "character_live_damage_trace_statuses_v1",
            "post_life_metrics_version = pre_life_metrics_version + 1",
            "UNIQUE (namespace_id, account_id, character_id, authoritative_tick)",
            "danger_entry_life_metrics_version <= pre_life_metrics_version",
            "danger_entry_permadeath_combat_ticks <= pre_permadeath_combat_ticks",
            "life_clock_checkpoint_entry_exact_v1",
            "life_clock_checkpoint_receipt_append_only_v1",
            "life_deed_completion_receipt_append_only_v1",
            "dead_life_clock_checkpoint_insert_v1",
            "dead_life_deed_completion_insert_v1",
            "dead_live_trace_tick_insert_v1",
            "live_trace_tick_graph_complete_v1",
            "live_trace_entry_graph_complete_v1",
            "live_trace_status_graph_complete_v1",
            "window_entry_count > 4096 OR maximum_tick - minimum_tick > 300",
            "live_trace_absent_after_death_v1",
            "death_requires_live_trace_cleanup_v1",
            "AFTER INSERT ON death_events",
            "ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "ALTER TABLE character_life_metrics",
            "ALTER TABLE character_life_deeds",
            "DROP TABLE",
            "TRUNCATE",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "live death-evidence migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn live_evidence_receipts_retain_complete_mutation_authority() {
        let migration =
            include_str!("../../../migrations/0044_live_evidence_mutation_authority.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "0044 requires dormant live death-evidence receipt tables",
            "character_life_clock_checkpoint_receipts_v1",
            "ADD COLUMN issued_at TIMESTAMPTZ NOT NULL",
            "life_clock_checkpoint_issue_order",
            "character_life_deed_completion_receipts_v1",
            "ADD COLUMN expected_character_version BIGINT NOT NULL",
            "life_deed_completion_version_positive",
            "life_deed_completion_issue_order",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "live evidence authority closure leaked {prohibited}"
            );
        }
    }

    #[test]
    fn live_deed_v2_closes_reward_restore_revocation_and_projection_authority() {
        let migration = include_str!("../../../migrations/0045_live_deed_authority_closure.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "character_life_deed_completion_receipts_v2",
            "character_life_deed_revocations_v2",
            "character_life_deed_conflict_audits_v2",
            "source_instance_id BYTEA NOT NULL",
            "progression_records_blake3 TEXT NOT NULL",
            "world_records_blake3 TEXT NOT NULL",
            "world_assets_blake3 TEXT NOT NULL",
            "world_localization_blake3 TEXT NOT NULL",
            "base_xp INTEGER NOT NULL",
            "restore_point_live_deed_v2_unique",
            "xp_live_deed_v2_unique",
            "deed.core.sir_caldus_defeated",
            "deed.core.sepulcher_knight_defeated",
            "miniboss.sepulcher_knight",
            "deed_kind = 2",
            "reward.miniboss_t1",
            "xp.miniboss_t1",
            "reward.boss_caldus",
            "xp.boss_caldus",
            "post_life_metrics_version = pre_life_metrics_version + 1",
            "core-dev.blake3.27818db710b7553520a162f6f8337dcd0419c459d20c6513a7e12c78fed24ebb",
            "051f86a69b9d2a9dd911f0d92bf53b40e460ef13c9058d6f0b1f32f11b226f95",
            "97b7188e26329b9430b7289d1e17d347c9b9472863b7db6bd48501fd3b773158",
            "32ce9fce6f1d49d5cd6cb570fa0590a5ee5644388c2620b67846320d4b2a3759",
            "895c38724abfdef4909751743d91b5cff90d7f073c553bc044601abff4763a26",
            "reward.request_state = 1",
            "reward.source_instance_id = NEW.source_instance_id",
            "caldus_victory_exit_owners",
            "victory.instance_lineage_id = NEW.lineage_id",
            "xp.revoked_by_restore_point_id",
            "life_deed_reward_authority_exact_v2",
            "life_deed_revocation_authority_exact_v2",
            "xp_deed_revocation_pair_exact_v2",
            "life_deed_receipt_projection_exact_v2",
            "life_deed_revocation_projection_exact_v2",
            "life_deed_projection_self_exact_v2",
            "danger_crash_life_deed_v2_shape",
            "danger_crash_life_deed_result_exact_v1",
            "danger_crash_life_deed_children_exact_v1",
            "crash result live deed revocation graph is incomplete or noncanonical",
            "character life deed projection diverges from immutable active receipts",
            "attempted_request_hash <> stored_request_hash",
            "no raw payload or network secret is stored",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "live deed authority closure leaked {prohibited}"
            );
        }
    }

    #[test]
    fn crash_life_deed_revocation_is_complete_versioned_and_forward_only() {
        let migration =
            include_str!("../../../migrations/0046_crash_life_deed_revocation_closure.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "life_deed_revocation_digest BYTEA",
            "life_deed_contract_version = 1",
            "require_new_crash_life_deed_contract_v1",
            "new crash result requires life deed contract 1",
            "CREATE OR REPLACE FUNCTION enforce_danger_crash_life_deed_graph_v1",
            "row_number() OVER (ORDER BY completion_id) - 1",
            "ordered.revoked_at <> stored_committed_at",
            "receipt.restore_point_id = stored_restore",
            "revocation.completion_id IS NULL",
            "crash result live deed revocation graph is incomplete or noncanonical",
            "Migration 0045 introduced the revocation graph before the crash writer existed",
            "Downgrade requires proving no contract-1 result or v2 revocation exists",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "crash deed revocation closure leaked {prohibited}"
            );
        }
    }

    #[test]
    fn authoritative_life_clock_receipts_are_versioned_replay_safe_and_forward_only() {
        let migration =
            include_str!("../../../migrations/0047_authoritative_life_clock_receipts.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "0047 requires the dormant life-clock receipt table",
            "ADD COLUMN contract_version SMALLINT NOT NULL DEFAULT 1",
            "ADD COLUMN expected_character_version BIGINT NOT NULL",
            "advanced_ticks BETWEEN 1 AND 1800",
            "authoritative_tick >= advanced_ticks",
            "character_life_clock_conflict_audits_v1",
            "observed_character_version BIGINT NOT NULL",
            "observed_life_metrics_version BIGINT NOT NULL",
            "attempted_request_hash <> stored_request_hash",
            "life_clock_conflict_audit_append_only_v1",
            "stores hashes and observed versions, never raw payloads or network secrets",
            "Do not drop the expected-version/conflict evidence",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "authoritative life-clock migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn retained_live_trace_ingest_is_authoritative_replayable_and_forward_only() {
        let migration =
            include_str!("../../../migrations/0048_retained_live_damage_trace_ingest.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "character_live_damage_trace_ingest_receipts_v1",
            "character_live_damage_trace_conflict_audits_v1",
            "checkpoint_tick >= 0 AND event_tick > 0",
            "live_trace_ingest_payload_authority_unique",
            "UNIQUE (namespace_id, account_id, character_id, trace_tick_id)",
            "live_trace_payload_retained_receipt_owned_v1",
            "REFERENCES character_entry_restore_points",
            "request_hash BYTEA NOT NULL",
            "result_digest BYTEA NOT NULL",
            "DEFERRABLE INITIALLY DEFERRED",
            "source_sim_entity_id BYTEA",
            "live_trace_source_identity_parity",
            "live_trace_ingest_receipt_append_only_v1",
            "live_trace_conflict_audit_append_only_v1",
            "attempted_request_hash <> stored_request_hash",
            "retained after complete live tick pruning",
            "Never remove retained ingest/conflict evidence",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "retained live trace migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn death_live_trace_promotion_is_mandatory_complete_and_forward_only() {
        let migration = include_str!("../../../migrations/0049_death_live_trace_promotion.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "0049 requires no existing deaths",
            "death_live_trace_sets_v1",
            "death_live_trace_receipt_links_v1",
            "death_live_trace_entry_provenance_v1",
            "death_live_trace_promotion_conflict_audits_v1",
            "terminal_payload_hash BYTEA NOT NULL",
            "REFERENCES character_live_damage_trace_ingest_receipts_v1",
            "REFERENCES death_combat_trace_entries",
            "enforce_death_live_trace_promotion_graph_v1",
            "death_requires_live_trace_promotion_v1",
            "DEFERRABLE INITIALLY DEFERRED",
            "promotion.receipt_count - 1",
            "receipt.lethal_count <> 0",
            "receipt.expected_character_version = death.pre_character_version",
            "receipt.receipt_committed_at IS DISTINCT FROM retained.committed_at",
            "provenance.cause_kind = death.cause_kind",
            "character_live_damage_trace_ingest_receipts_v1 AS retained",
            "enforce_death_live_trace_provenance_source_v1",
            "death_live_trace_provenance_source_exact_v1",
            "EXCEPT",
            "death_live_trace_conflict_authority_v1",
            "attempted_promotion_digest <> stored_promotion_digest",
            "never rewrite migrations 0043 or 0048",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "death live-trace promotion migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn death_live_trace_provenance_diagnostics_are_forward_only_and_exact() {
        let migration =
            include_str!("../../../migrations/0050_death_live_trace_provenance_diagnostics.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "CREATE OR REPLACE FUNCTION enforce_death_live_trace_provenance_source_v1()",
            "has no exact retained live entry",
            "differs from retained live authority",
            "differs from durable trace authority",
            "has divergent durable statuses",
            "status_id COLLATE \"C\"",
            "Never rewrite migration 0049",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DROP TABLE", "TRUNCATE", "DELETE FROM", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "provenance diagnostics migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn death_presentation_authority_is_additive_disjoint_and_fail_closed() {
        let migration = include_str!("../../../migrations/0051_death_presentation_authority.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "SPEC-CONFLICT-009-m03-death-memorial.md",
            "0051 requires no existing death rows",
            "presentation_records_blake3 TEXT NOT NULL",
            "presentation_assets_blake3 TEXT NOT NULL",
            "presentation_localization_blake3 TEXT NOT NULL",
            "death_presentation_revision_exact",
            "never presentation/localization authority",
            "Never drop these columns in place",
            "copy world_* values into presentation_*",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "UPDATE death_events",
            "JSON",
            "JSONB",
        ] {
            assert!(
                !migration.contains(prohibited),
                "death presentation migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn danger_entry_restore_v2_is_component_complete_and_clock_authoritative() {
        let migration = include_str!("../../../migrations/0033_danger_entry_restore_v2.sql");
        for required in [
            "0033 requires no existing danger-entry restore points",
            "restore_contract_v2",
            "snapshot_contract_version = 2",
            "restore_components_v2_complete",
            "component_mask = 15",
            "entry_restore_oath_bargain_v2",
            "entry_restore_active_bargains_v2",
            "entry_restore_life_metrics_v2",
            "rollback_permadeath_combat_ticks",
            "restore_v2_progression_component_required",
            "restore_v2_inventory_component_required",
            "restore_v2_oath_component_required",
            "restore_v2_life_component_required",
            "enforce_entry_restore_inventory_v2_count",
            "enforce_entry_restore_oath_v2_count",
            "DEFERRABLE INITIALLY DEFERRED",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
            "DELETE FROM",
        ] {
            assert!(
                !migration.contains(prohibited),
                "danger-entry restore v2 migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn danger_entry_restore_v3_is_exact_replay_complete_and_forward_only() {
        let migration = include_str!("../../../migrations/0034_exact_danger_crash_restore_v3.sql");
        for required in [
            "0034 requires no existing danger-entry restore points",
            "SPEC-CONFLICT-009/027/028",
            "GDD TECH-015/020/021/023",
            "entry_restore_inventory_item_v1_owned",
            "restore_contract_v3",
            "snapshot_contract_version = 3",
            "restore_components_v3_complete",
            "component_mask = 31",
            "entry_restore_progression_v3",
            "entry_restore_inventory_v3",
            "entry_restore_inventory_items_v3",
            "baseline_item_count BETWEEN 0 AND 64",
            "account_id BYTEA NOT NULL",
            "entry_restore_inventory_v3_item_capture_exact",
            "item.creation_request_id = NEW.creation_request_id",
            "item.item_kind = NEW.item_kind",
            "count(*) > 6",
            "max(item_kind) = 0 AND count(*) > 1",
            "count(DISTINCT (item_kind, template_id, content_revision)) <> 1",
            "danger-entry v3 inventory ordinals are not canonical",
            "entry_restore_oath_bargain_v3",
            "acquired_by_offer_id",
            "entry_restore_life_metrics_v3",
            "entry_restore_ash_wallet_v3",
            "restore_v3_ash_component_required",
            "location_kind = 7",
            "reason = 'consumed'",
            "source_kind = 4",
            "reason = 'crash_restored'",
            "reason = 'crash_revoked'",
            "destruction_reason IS NOT NULL",
            "terminal_restore_point_id IS NOT NULL",
            "revoked_by_restore_point_id IS NOT NULL AND revoked_at IS NOT NULL",
            "reversed_by_mutation_id IS NOT NULL",
            "revoked_by_restore_point_id",
            "reversed_by_restore_point_id",
            "danger_crash_restore_results",
            "restore_v3_crash_result_required",
            "danger_crash_restore_item_changes",
            "danger_crash_restore_material_changes",
            "danger_crash_restore_bargain_changes",
            "danger_crash_restore_ash_changes",
            "revoked_item_count BETWEEN 0 AND 4095",
            "restored_item_count + revoked_item_count BETWEEN 0 AND 4095",
            "change_ordinal BETWEEN 0 AND 4094",
            "item_ledger_crash_resolution_identity",
            "ledger_event_kind = 4 AND ledger_source_kind = 4",
            "danger_crash_item_change_source_exact",
            "danger_crash_material_change_source_exact",
            "danger_crash_bargain_change_source_exact",
            "danger_crash_ash_change_source_exact",
            "danger crash result child order is not canonical",
            "danger-entry v3 component history is immutable",
            "danger-entry v3 snapshot children are immutable",
            "entry_restore_v3_root_terminal_complete",
            "entry_restore_v3_root_immutable",
            "danger-entry v3 root must begin Active",
            "OLD.restore_state = 0",
            "crash-revoked Bargain source history is immutable",
            "ash_crash_binding_immutable",
            "OLD.namespace_id, OLD.restore_point_id",
            "enforce_danger_crash_result_counts_v3",
            "danger crash restore result history is immutable",
            "DEFERRABLE INITIALLY DEFERRED",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM item_instances",
            "DELETE FROM item_ledger_events",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
            "TECH-019",
            "change_ordinal BETWEEN 0 AND 4095",
        ] {
            assert!(
                !migration.contains(prohibited),
                "danger-entry restore v3 migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn crash_restore_request_authority_is_terminal_replay_and_source_bound() {
        let migration =
            include_str!("../../../migrations/0035_crash_restore_request_authority.sql");
        for required in [
            "danger_crash_restore_request_results",
            "danger_crash_restore_conflict_audits",
            "outcome_code BETWEEN 0 AND 4",
            "outcome_code = 0 AND observed_restore_state = 4",
            "attempted_request_hash <> stored_request_hash",
            "danger_crash_request_terminal_source_exact",
            "new crash restoration receipt lacks its normalized result",
            "danger crash request history is immutable",
            "CREATE OR REPLACE FUNCTION enforce_danger_crash_item_change_source_v3",
            "baseline.restore_point_id = NEW.restore_point_id",
            "NEW.change_kind = 1 AND EXISTS",
            "bargain_offer_crash_restore_same_entry",
            "offer.entry_restore_point_id = NEW.restore_point_id",
            "milestone.entry_restore_point_id = NEW.restore_point_id",
            "DEFERRABLE INITIALLY DEFERRED",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DROP TABLE",
            "TRUNCATE",
            "DELETE FROM",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration.contains(prohibited),
                "crash request authority migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn restore_root_immutability_uses_the_typed_world_revision_columns() {
        let migration = include_str!("../../../migrations/0036_fix_restore_root_immutability.sql");
        for required in [
            "CREATE OR REPLACE FUNCTION enforce_entry_restore_v3_root_immutability",
            "NEW.records_blake3",
            "NEW.assets_blake3",
            "NEW.localization_blake3",
            "NEW.restore_state BETWEEN 1 AND 4",
            "NEW.crash_restore_mutation_id IS NULL",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        assert!(!migration.contains("NEW.content_revision"));
        assert!(!migration.contains("OLD.content_revision"));
    }

    #[test]
    fn progression_crash_restore_migration_binds_and_revokes_without_deletion() {
        let migration =
            include_str!("../../../migrations/0012_progression_crash_restore_binding.sql");
        for required in [
            "entry_restore_point_id",
            "revoked_by_restore_point_id",
            "revocation_progression_version",
            "xp_entry_restore_owned",
            "xp_revocation_restore_owned",
            "xp_awards_by_entry_restore",
            "restored_progression_version",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DELETE FROM character_xp_award_results",
            "JSON",
            "JSONB",
            "FLOAT",
        ] {
            assert!(
                !migration.contains(prohibited),
                "progression crash-restore migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn entry_restore_component_root_is_deferred_for_atomic_composite_capture() {
        let migration = include_str!("../../../migrations/0024_defer_entry_restore_components.sql");
        for required in [
            "entry_restore_progression_root_owned",
            "DEFERRABLE INITIALLY DEFERRED",
            "character_entry_restore_points",
            "entry restore progression root constraint is missing",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
    }

    #[test]
    fn caldus_victory_exit_migration_is_reward_terminal_and_inventory_boundary_safe() {
        let migration = include_str!("../../../migrations/0025_caldus_victory_exit_gate.sql");
        for required in [
            "caldus_victory_exits",
            "caldus_victory_exit_owners",
            "reward_requests",
            "character_xp_award_results",
            "eligible_owner_count BETWEEN 1 AND 8",
            "caldus_victory_terminal_hashes_exact",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "pending_inventory",
            "overflow",
            "resolution_hold",
            "JSON",
            "JSONB",
            "FLOAT",
            "DOUBLE PRECISION",
        ] {
            assert!(
                !migration
                    .to_ascii_lowercase()
                    .contains(&prohibited.to_ascii_lowercase()),
                "Caldus victory migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn xp_revocation_shape_rejects_null_three_valued_logic_edges() {
        let migration =
            include_str!("../../../migrations/0013_strict_xp_crash_revocation_shape.sql");
        for required in [
            "DROP CONSTRAINT xp_crash_revocation_shape",
            "entry_restore_point_id IS NOT NULL",
            "revoked_by_restore_point_id IS NOT NULL",
            "revocation_progression_version IS NOT NULL",
            "revoked_by_restore_point_id = entry_restore_point_id",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DELETE", "DROP TABLE", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "strict XP revocation migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn rejected_xp_profile_forward_migration_is_nullable_but_still_bounded() {
        let migration = include_str!("../../../migrations/0004_nullable_rejected_xp_profile.sql");
        for required in [
            "ADD COLUMN normal_living_at_death BOOLEAN",
            "ALTER COLUMN xp_profile_id DROP NOT NULL",
            "DROP CONSTRAINT xp_profile_id_bounded",
            "DROP CONSTRAINT xp_normal_evidence_shape",
            "xp_profile_id IS NULL OR length(xp_profile_id) BETWEEN 3 AND 96",
            "normal_living_at_death IS NOT NULL",
            "normal_living_at_death IS NULL",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "DROP TABLE", "core.1.0.0"] {
            assert!(
                !migration.contains(prohibited),
                "XP-profile correction leaked {prohibited}"
            );
        }
    }

    #[test]
    fn world_flow_revision_forward_migration_preserves_exact_independent_hashes() {
        let migration =
            include_str!("../../../migrations/0005_typed_world_flow_revision_and_arrival.sql");
        for required in [
            "SET safe_arrival_kind = 0",
            "location_kind = 0",
            "records_blake3 TEXT NOT NULL",
            "assets_blake3 TEXT NOT NULL",
            "localization_blake3 TEXT NOT NULL",
            "receipt_world_flow_revision_exact",
            "world-flow revision migration requires dormant world-flow tables",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["md5(", "sha256(", "digest(", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "world-flow revision migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn initial_oath_migration_is_exact_idempotent_and_outbox_backed() {
        let migration = include_str!("../../../migrations/0006_initial_oath_selection.sql");
        for required in [
            "character_oath_id_core",
            "character_initial_oath_level",
            "character_oath_mutation_results",
            "character_life_outbox",
            "oath_selected",
            "one_oath_selected_event_per_character",
            "unpublished_character_life_events",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "FLOAT", "DOUBLE PRECISION"] {
            assert!(
                !migration.contains(prohibited),
                "initial Oath migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn minimal_ash_wallet_migration_is_bounded_replay_first_and_ledger_backed() {
        let migration = include_str!("../../../migrations/0014_minimal_ash_wallet.sql");
        for required in [
            "ash_wallets",
            "ash_mutation_results",
            "currency_ledger_events",
            "currency.ash_shards",
            "ash_wallet_balance_bounded",
            "ash_result_arithmetic",
            "ash_result_rejection_kind",
            "currency_ledger_arithmetic",
            "currency_ledger_account_history",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "FLOAT", "DOUBLE PRECISION", "premium"] {
            assert!(
                !migration.contains(prohibited),
                "Ash wallet migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn ash_wallet_forward_migration_adds_locked_expected_version_results() {
        let migration = include_str!("../../../migrations/0015_ash_expected_wallet_version.sql");
        for required in [
            "expected_wallet_version",
            "pre_wallet_version",
            "ash_result_expected_version_positive",
            "ash_result_expected_version_match",
            "result_code BETWEEN 0 AND 3",
            "result_code IN (0, 3)",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DROP TABLE", "TRUNCATE", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "Ash wallet forward migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn bargain_offer_migration_is_normalized_replay_backed_and_restore_aligned() {
        let migration = include_str!("../../../migrations/0016_deterministic_bargain_offers.sql");
        for required in [
            "character_oath_bargain_state",
            "earned_bargain_slots BETWEEN 0 AND 3",
            "oath_bargain_version",
            "bargain_offers",
            "bargain_offer_candidates",
            "character_active_bargains",
            "bargain_milestone_results",
            "bargain_decision_results",
            "bargain_selected",
            "open_bargain_offers_by_character",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "FLOAT", "DOUBLE PRECISION", "premium"] {
            assert!(
                !migration.contains(prohibited),
                "Bargain offer migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn bargain_offer_source_binding_is_forward_only_and_instance_exact() {
        let migration = include_str!("../../../migrations/0017_bind_bargain_offer_source.sql");
        for required in [
            "0017 requires dormant pre-route Bargain offer tables",
            "miniboss.sepulcher_knight",
            "layout.core_private_life_01",
            "instance_lineage_id",
            "entry_restore_point_id",
            "bargain_offer_lineage_owned",
            "bargain_offer_restore_owned",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DROP TABLE", "TRUNCATE", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "Bargain source binding leaked {prohibited}"
            );
        }
    }

    #[test]
    fn bargain_offer_resolution_versions_allow_later_open_offer_decisions() {
        let migration =
            include_str!("../../../migrations/0018_bargain_offer_resolution_versions.sql");
        for required in [
            "DROP CONSTRAINT bargain_offer_resolution_shape",
            "resolved_oath_bargain_version > created_oath_bargain_version",
            "resolved_oath_bargain_version >= created_oath_bargain_version",
            "resolved_oath_bargain_version = created_oath_bargain_version",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DROP TABLE", "TRUNCATE", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "Bargain resolution migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn core_bargain_milestone_is_source_bound_once_per_life_and_cross_domain_owned() {
        let migration = include_str!("../../../migrations/0019_bind_core_bargain_milestone.sql");
        for required in [
            "milestone.core.sepulcher_knight_first_clear",
            "miniboss.sepulcher_knight",
            "layout.core_private_life_01",
            "bargain_milestone_once_per_life",
            "bargain_milestone_offer_is_source",
            "bargain_milestone_slot_transition_exact",
            "pre_earned_bargain_slots",
            "post_earned_bargain_slots",
            "bargain_milestone_lineage_owned",
            "bargain_milestone_restore_owned",
            "bargain_milestone_ash_result_owned",
            "bargain_milestone_offer_owned",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        assert!(migration.contains("requires dormant pre-route Bargain milestone tables"));
    }

    #[test]
    fn instance_layout_binding_makes_bargain_source_authority_durable() {
        let migration = include_str!("../../../migrations/0020_bind_instance_layout.sql");
        for required in [
            "lineage_layout_id_bounded",
            "lineage_layout_identity",
            "bargain_offer_layout_lineage_owned",
            "bargain_milestone_layout_lineage_owned",
            "instance_lineage_id, source_layout_id",
            "lineage_id, layout_id",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DROP TABLE", "TRUNCATE", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "layout binding leaked {prohibited}"
            );
        }
    }

    #[test]
    fn bell_debt_checkpoint_migration_is_bounded_versioned_and_component_complete() {
        let migration = include_str!("../../../migrations/0021_bell_debt_danger_checkpoint.sql");
        for required in [
            "0021 requires dormant danger checkpoint rows",
            "component_mask = 15",
            "checkpoint_schema_version = 1",
            "octet_length(checkpoint_payload) BETWEEN 1 AND 4096",
            "checkpoint_payload_digest",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DROP TABLE", "TRUNCATE", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "checkpoint migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn bargain_life_cleanup_outbox_is_typed_and_history_preserving() {
        let migration = include_str!("../../../migrations/0022_bargain_life_cleanup_outbox.sql");
        for required in [
            "bargains_cleared_death",
            "bargains_cleared_retirement",
            "one_bargain_cleanup_event_per_life_reason",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "DELETE FROM bargain_offers",
            "DELETE FROM bargain_decision_results",
            "DROP TABLE",
            "TRUNCATE",
            "JSON",
            "JSONB",
        ] {
            assert!(
                !migration.contains(prohibited),
                "cleanup migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn bargain_telemetry_outbox_names_are_exact_and_forward_only() {
        let migration = include_str!("../../../migrations/0023_bargain_telemetry_outbox.sql");
        for required in ["bargain_offered", "bargain_declined"] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DROP TABLE", "TRUNCATE", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "telemetry migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn durable_item_migration_is_unit_normalized_typed_and_replay_backed() {
        let migration = include_str!("../../../migrations/0007_durable_item_lifecycle.sql");
        for required in [
            "character_inventories",
            "starter_initializer_results",
            "reward_requests",
            "reward_result_entries",
            "item_instances",
            "item_ledger_events",
            "one_equipment_per_slot",
            "item_units_by_projected_stack",
            "personal_ground_by_expiry",
            "ground_expired",
            "core-dev[.]blake3",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "FLOAT", "DOUBLE PRECISION", "core.1.0.0"] {
            assert!(
                !migration.contains(prohibited),
                "durable item migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn item_ledger_ownership_allows_only_explicit_wipeable_cascade() {
        let migration = include_str!("../../../migrations/0008_wipeable_item_ledger_ownership.sql");
        for required in [
            "item_ledger_item_owned",
            "item_ledger_character_owned",
            "ON DELETE CASCADE",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["DROP TABLE", "TRUNCATE", "JSON", "JSONB"] {
            assert!(
                !migration.contains(prohibited),
                "ledger ownership migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn item_provenance_and_salvage_are_typed_independent_axes() {
        let migration = include_str!("../../../migrations/0009_item_provenance_and_salvage.sql");
        for required in [
            "provenance_kind",
            "salvage_band",
            "salvage_value",
            "item_creation_provenance_shape",
            "item_zero_salvage_shape",
            "ALTER COLUMN provenance_kind DROP DEFAULT",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "FLOAT", "DOUBLE PRECISION"] {
            assert!(
                !migration.contains(prohibited),
                "item provenance migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn reward_request_can_be_reserved_before_planning_and_only_commits_complete() {
        let migration = include_str!("../../../migrations/0010_reward_request_planning_state.sql");
        for required in [
            "request_state",
            "request_state = 0",
            "plan_hash IS NULL",
            "request_state = 1",
            "post_inventory_version = pre_inventory_version + 1",
            "ALTER COLUMN request_state DROP DEFAULT",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "DROP TABLE", "TRUNCATE"] {
            assert!(
                !migration.contains(prohibited),
                "reward request state migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn empty_reward_commit_does_not_advance_inventory_version() {
        let migration = include_str!("../../../migrations/0011_reward_request_item_count.sql");
        for required in [
            "reward_item_count",
            "reward_item_count BETWEEN 0 AND 64",
            "CASE WHEN reward_item_count = 0 THEN 0 ELSE 1 END",
            "reward_item_count IS NULL",
            "reward_item_count IS NOT NULL",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in ["JSON", "JSONB", "DROP TABLE", "TRUNCATE"] {
            assert!(
                !migration.contains(prohibited),
                "reward item-count migration leaked {prohibited}"
            );
        }
    }

    #[test]
    fn field_equipment_schema_binds_preview_replay_and_exact_source_shape() {
        let migration = include_str!("../../../migrations/0027_field_equipment_mutations.sql");
        for required in [
            "CREATE TABLE field_equipment_mutations",
            "canonical_request_hash BYTEA NOT NULL",
            "preview_hash BYTEA NOT NULL",
            "pre_inventory_version BIGINT NOT NULL",
            "post_inventory_version BIGINT NOT NULL",
            "source_kind SMALLINT NOT NULL",
            "replacement_slot_index SMALLINT",
            "post_inventory_version = pre_inventory_version + 1",
        ] {
            assert!(migration.contains(required), "schema 27 omitted {required}");
        }
        let lowercase = migration.to_ascii_lowercase();
        for forbidden in [
            "vault",
            "character_safe",
            "overflow",
            "resolution_hold",
            "extraction",
            "json",
        ] {
            assert!(
                !lowercase.contains(forbidden),
                "schema 27 leaked {forbidden}"
            );
        }
    }

    #[test]
    fn safe_storage_schema_appends_locations_and_preserves_existing_rows() {
        let migration = include_str!("../../../migrations/0028_character_safe_vault_locations.sql");
        for required in [
            "location_kind BETWEEN 0 AND 6",
            "location_kind = 5 AND character_id IS NOT NULL",
            "slot_index BETWEEN 0 AND 7",
            "location_kind = 6 AND character_id IS NULL",
            "slot_index BETWEEN 0 AND 159",
            "item_account_owned",
            "item_character_custody_owned",
            "one_character_safe_equipment_per_slot",
            "one_vault_equipment_per_slot",
            "Schema-27 rollback requires zero rows in 5 or 6",
        ] {
            assert!(migration.contains(required), "schema 28 omitted {required}");
        }
        let lowercase = migration.to_ascii_lowercase();
        for forbidden in [
            "drop table",
            "truncate",
            "delete from",
            "update item_instances",
            "overflow",
            "resolutionhold",
            "resolution_hold",
            "extraction",
            "json",
        ] {
            assert!(
                !lowercase.contains(forbidden),
                "schema 28 leaked {forbidden}"
            );
        }
    }

    #[test]
    fn safe_inventory_receipt_is_normalized_bounded_and_version_exact() {
        let migration = include_str!("../../../migrations/0029_safe_inventory_mutations.sql");
        for required in [
            "CREATE TABLE safe_inventory_mutations",
            "CREATE TABLE safe_inventory_placements",
            "canonical_request_hash BYTEA NOT NULL",
            "command_kind BETWEEN 0 AND 2",
            "placement_count BETWEEN 1 AND 6",
            "post_inventory_version = pre_inventory_version + 1",
            "post_account_version = pre_account_version + 1",
            "post_account_version = pre_account_version",
            "destination_kind = 2",
            "destination_kind = 5",
            "destination_kind = 6",
            "post_item_version = pre_item_version + 1",
        ] {
            assert!(migration.contains(required), "schema 29 omitted {required}");
        }
        let lowercase = migration.to_ascii_lowercase();
        for forbidden in [
            "json",
            "overflow",
            "resolutionhold",
            "resolution_hold",
            "extraction",
            "drop table",
            "truncate",
        ] {
            assert!(
                !lowercase.contains(forbidden),
                "schema 29 leaked {forbidden}"
            );
        }
    }

    #[test]
    fn safe_inventory_result_code_is_forward_only_and_acceptance_exact() {
        let migration = include_str!("../../../migrations/0030_safe_inventory_result_code.sql");
        for required in [
            "ADD COLUMN result_code SMALLINT NOT NULL DEFAULT 1",
            "result_code = 1",
            "ALTER COLUMN result_code DROP DEFAULT",
        ] {
            assert!(migration.contains(required), "schema 30 omitted {required}");
        }
        let lowercase = migration.to_ascii_lowercase();
        for forbidden in ["drop table", "truncate", "delete from", "update ", "json"] {
            assert!(
                !lowercase.contains(forbidden),
                "schema 30 leaked {forbidden}"
            );
        }
    }
}
