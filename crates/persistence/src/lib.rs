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

mod identity;
mod progression;
mod world_flow;

pub use identity::{StoredCharacter, StoredIdentityAggregate, StoredMutation};
pub use progression::{
    ProgressionAwardTransaction, ProgressionAwardTransactionState, StoredBossFirstClear,
    StoredBossFirstClearState, StoredEncounterLifeState, StoredEncounterRecallState,
    StoredEncounterTrustState, StoredEncounterXpEvidence, StoredLockedProgressionCharacter,
    StoredOrdinaryXpEvidence, StoredProgression, StoredProgressionContract,
    StoredProgressionSnapshot, StoredXpAwardResult, StoredXpEligibilityEvidence,
};
pub use world_flow::{
    StoredSafeArrival, StoredWorldLocation, StoredWorldTransferReceipt, WorldFlowTransactionState,
};

pub const TEST_DATABASE_URL_ENV: &str = "TEST_DATABASE_URL";
pub const RUNTIME_DATABASE_URL_ENV: &str = "GRAVEBOUND_DATABASE_URL";
pub const DESTRUCTIVE_TEST_OPT_IN_ENV: &str = "GRAVEBOUND_ALLOW_DESTRUCTIVE_DATABASE_TESTS";
pub const WIPEABLE_CORE_NAMESPACE: &str = "test.core";
pub const EXPECTED_SCHEMA_VERSION: i64 = 4;
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
    Database(#[source] sqlx::Error),
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
    #[error("world-flow character does not exist for the authenticated account")]
    WorldFlowCharacterNotFound,
    #[error("progression character does not exist for the authenticated account")]
    ProgressionCharacterNotFound,
    #[error("stored progression or XP award violates the approved schema")]
    CorruptStoredProgression,
    #[error("a fresh progression award transaction must append one typed result")]
    ProgressionAwardResultRequired,
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
}
