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
mod extraction;
mod field_equipment;
mod ground_expiry;
mod identity;
mod items;
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
    StoredSafeInventoryLocation, StoredSafeInventoryPlacement, StoredSafeInventoryResult,
    StoredSafeInventorySnapshot,
};
pub use world_flow::{
    StoredDangerEntryRootV1, StoredSafeArrival, StoredWorldFlowCharacter,
    StoredWorldFlowRevisionV1, StoredWorldLocation, StoredWorldTransferReceipt, WorldFlowBegin,
    WorldFlowTransaction, WorldFlowTransactionState, WorldFlowWrite, stage_world_flow_danger_entry,
};

pub const TEST_DATABASE_URL_ENV: &str = "TEST_DATABASE_URL";
pub const RUNTIME_DATABASE_URL_ENV: &str = "GRAVEBOUND_DATABASE_URL";
pub const DESTRUCTIVE_TEST_OPT_IN_ENV: &str = "GRAVEBOUND_ALLOW_DESTRUCTIVE_DATABASE_TESTS";
pub const WIPEABLE_CORE_NAMESPACE: &str = "test.core";
pub const EXPECTED_SCHEMA_VERSION: i64 = 30;
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
