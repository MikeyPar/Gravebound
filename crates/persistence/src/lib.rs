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

mod ground_expiry;
mod identity;
mod items;
mod oath;
mod progression;
mod progression_restore;
mod reward;
mod world_flow;

pub use ground_expiry::{MAX_GROUND_EXPIRY_BATCH, StoredGroundExpiry, StoredGroundExpiryCandidate};
pub use identity::{StoredCharacter, StoredIdentityAggregate, StoredMutation};
pub use items::{
    STARTER_INITIALIZER_REVISION, STARTER_ITEM_COUNT, StoredStarterInitialization,
    StoredStarterItem,
};
pub use oath::{
    OathSelectionTransaction, OathSelectionTransactionState, StoredCharacterLifeEvent,
    StoredOathCharacter, StoredOathInventory, StoredOathMutationResult,
};
pub use progression::{
    ProgressionAwardTransaction, ProgressionAwardTransactionState, StoredBossFirstClear,
    StoredBossFirstClearState, StoredEncounterLifeState, StoredEncounterRecallState,
    StoredEncounterTrustState, StoredEncounterXpEvidence, StoredLockedProgressionCharacter,
    StoredOrdinaryXpEvidence, StoredProgression, StoredProgressionContract,
    StoredProgressionSnapshot, StoredXpAwardResult, StoredXpEligibilityEvidence,
};
pub use progression_restore::{
    StoredProgressionCrashRestore, capture_progression_restore, restore_progression_after_crash,
};
pub use reward::{
    RewardPlanningState, RewardTransaction, StoredPendingItem, StoredRewardCommit,
    StoredRewardEntry, StoredRewardItem, StoredRewardOutcome, StoredRewardRequest,
};
pub use world_flow::{
    StoredSafeArrival, StoredWorldFlowCharacter, StoredWorldFlowRevisionV1, StoredWorldLocation,
    StoredWorldTransferReceipt, WorldFlowTransaction, WorldFlowTransactionState,
};

pub const TEST_DATABASE_URL_ENV: &str = "TEST_DATABASE_URL";
pub const RUNTIME_DATABASE_URL_ENV: &str = "GRAVEBOUND_DATABASE_URL";
pub const DESTRUCTIVE_TEST_OPT_IN_ENV: &str = "GRAVEBOUND_ALLOW_DESTRUCTIVE_DATABASE_TESTS";
pub const WIPEABLE_CORE_NAMESPACE: &str = "test.core";
pub const EXPECTED_SCHEMA_VERSION: i64 = 12;
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
    #[error("authoritative reward planning failed before commit")]
    RewardPlanningFailed,
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
}
