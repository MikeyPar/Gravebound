//! Durable telemetry-domain source foundation for `GB-M03-09`.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`TECH-123`,
//! `TEL-001`-`005`), `Gravebound_Content_Production_Spec_v1.md` (`CONT-002` and Core stable
//! IDs), and `Gravebound_Development_Roadmap_v1.md` (`ADR-005`, `GB-M03-09`). Session and crash
//! facts are written through typed commands. Onboarding and loot facts are read here but are
//! inserted only by schema-70/71 projectors inside the owning gameplay transaction.

use std::future::Future;

use sqlx::{PgConnection, Row};
use telemetry::StableTelemetryId;
use thiserror::Error;

use crate::{PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

pub const MAX_M03_TELEMETRY_SOURCE_POLL_V1: usize = 256;
const MAX_TRANSACTION_ATTEMPTS: u8 = 3;

const POLL_DOMAIN_SOURCES_SQL: &str = r"
SELECT family,event_id,account_id,character_id,session_id,build_id,
       content_bundle_version,platform,region_id,environment,cohort_tags,
       occurred_at_millis,commit_order,event_kind,source_id,class_id,source_content_id,
       event_sequence,duration_millis,end_reason,link_lost_millis,crash_id,
       crash_source,crash_kind,reporter_kind,signature,uptime_millis,
       loot_action,item_uid,template_id,loot_source_content_id,item_version
FROM (
    SELECT 0::smallint AS family,onboarding.event_id,onboarding.account_id,
           onboarding.character_id,onboarding.session_id,session.build_id,
           session.content_bundle_version,session.platform,session.region_id,
           session.environment,session.cohort_tags,
           floor(extract(epoch FROM onboarding.occurred_at)*1000)::bigint AS occurred_at_millis,
           floor(extract(epoch FROM onboarding.created_at)*1000000)::bigint AS commit_order,
           onboarding.event_kind,onboarding.source_id,onboarding.class_id,
           onboarding.source_content_id,NULL::bigint AS event_sequence,
           NULL::bigint AS duration_millis,NULL::smallint AS end_reason,
           NULL::bigint AS link_lost_millis,NULL::bytea AS crash_id,
           NULL::smallint AS crash_source,NULL::smallint AS crash_kind,
           NULL::smallint AS reporter_kind,NULL::bytea AS signature,
           NULL::bigint AS uptime_millis,NULL::smallint AS loot_action,
           NULL::bytea AS item_uid,NULL::text AS template_id,
           NULL::text AS loot_source_content_id,NULL::bigint AS item_version
    FROM onboarding_outbox_events_v1 AS onboarding
    JOIN core_telemetry_sessions_v1 AS session
      ON session.namespace_id=onboarding.namespace_id
     AND session.account_id=onboarding.account_id
     AND session.session_id=onboarding.session_id
    WHERE onboarding.namespace_id=$1 AND onboarding.published_at IS NULL
    UNION ALL
    SELECT 1::smallint,session_event.event_id,session_event.account_id,NULL::bytea,
           session_event.session_id,session.build_id,session.content_bundle_version,
           session.platform,session.region_id,session.environment,session.cohort_tags,
           floor(extract(epoch FROM session_event.occurred_at)*1000)::bigint,
           floor(extract(epoch FROM session_event.created_at)*1000000)::bigint,
           session_event.event_kind,session_event.source_id,NULL::text,NULL::text,
           session_event.event_sequence,session_event.duration_millis,
           session_event.end_reason,session_event.link_lost_millis,NULL::bytea,
           NULL::smallint,NULL::smallint,NULL::smallint,NULL::bytea,NULL::bigint,
           NULL::smallint,NULL::bytea,NULL::text,NULL::text,NULL::bigint
    FROM session_outbox_events_v1 AS session_event
    JOIN core_telemetry_sessions_v1 AS session
      ON session.namespace_id=session_event.namespace_id
     AND session.account_id=session_event.account_id
     AND session.session_id=session_event.session_id
    WHERE session_event.namespace_id=$1 AND session_event.published_at IS NULL
    UNION ALL
    SELECT 2::smallint,crash.event_id,crash.account_id,crash.character_id,
           crash.session_id,session.build_id,session.content_bundle_version,
           session.platform,session.region_id,session.environment,session.cohort_tags,
           floor(extract(epoch FROM crash.occurred_at)*1000)::bigint,
           floor(extract(epoch FROM crash.created_at)*1000000)::bigint,
           NULL::smallint,crash.crash_id,NULL::text,NULL::text,NULL::bigint,
           NULL::bigint,NULL::smallint,NULL::bigint,crash.crash_id,
           crash.crash_source,crash.crash_kind,crash.reporter_kind,crash.signature,
           crash.uptime_millis,NULL::smallint,NULL::bytea,NULL::text,NULL::text,
           NULL::bigint
    FROM crash_outbox_events_v1 AS crash
    JOIN core_telemetry_sessions_v1 AS session
      ON session.namespace_id=crash.namespace_id
     AND session.account_id=crash.account_id
     AND session.session_id=crash.session_id
    WHERE crash.namespace_id=$1 AND crash.published_at IS NULL
    UNION ALL
    SELECT 3::smallint,loot.event_id,loot.account_id,loot.character_id,
           loot.session_id,session.build_id,session.content_bundle_version,
           session.platform,session.region_id,session.environment,session.cohort_tags,
           floor(extract(epoch FROM loot.occurred_at)*1000)::bigint,
           floor(extract(epoch FROM loot.created_at)*1000000)::bigint,
           NULL::smallint,loot.ledger_event_id,NULL::text,NULL::text,NULL::bigint,
           NULL::bigint,NULL::smallint,NULL::bigint,NULL::bytea,NULL::smallint,
           NULL::smallint,NULL::smallint,NULL::bytea,NULL::bigint,loot.loot_action,
           loot.item_uid,loot.template_id,loot.source_content_id,loot.item_version
    FROM item_ledger_telemetry_outbox_v1 AS loot
    JOIN core_telemetry_sessions_v1 AS session
      ON session.namespace_id=loot.namespace_id
     AND session.account_id=loot.account_id
     AND session.session_id=loot.session_id
    WHERE loot.namespace_id=$1 AND loot.published_at IS NULL
) AS committed
ORDER BY commit_order,event_id,family
LIMIT $2
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredM03TelemetryPlatformV1 {
    Windows,
    Linux,
    MacOs,
    Unknown,
}

impl StoredM03TelemetryPlatformV1 {
    const fn code(self) -> i16 {
        match self {
            Self::Windows => 0,
            Self::Linux => 1,
            Self::MacOs => 2,
            Self::Unknown => 3,
        }
    }

    fn decode(value: i16) -> Result<Self, M03TelemetrySourceError> {
        match value {
            0 => Ok(Self::Windows),
            1 => Ok(Self::Linux),
            2 => Ok(Self::MacOs),
            3 => Ok(Self::Unknown),
            _ => Err(M03TelemetrySourceError::CorruptStoredSource),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredM03TelemetryEnvironmentV1 {
    Local,
    Test,
    Staging,
    Production,
}

impl StoredM03TelemetryEnvironmentV1 {
    const fn code(self) -> i16 {
        match self {
            Self::Local => 0,
            Self::Test => 1,
            Self::Staging => 2,
            Self::Production => 3,
        }
    }

    fn decode(value: i16) -> Result<Self, M03TelemetrySourceError> {
        match value {
            0 => Ok(Self::Local),
            1 => Ok(Self::Test),
            2 => Ok(Self::Staging),
            3 => Ok(Self::Production),
            _ => Err(M03TelemetrySourceError::CorruptStoredSource),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M03TelemetrySessionStartV1 {
    pub session_id: [u8; 16],
    pub account_id: [u8; 16],
    pub build_id: String,
    pub content_bundle_version: String,
    pub platform: StoredM03TelemetryPlatformV1,
    pub region_id: String,
    pub environment: StoredM03TelemetryEnvironmentV1,
    pub cohort_tags: Vec<String>,
    pub started_at_utc_millis: u64,
}

impl M03TelemetrySessionStartV1 {
    pub fn validate(&self) -> Result<(), M03TelemetrySourceError> {
        validate_id(self.session_id)?;
        validate_id(self.account_id)?;
        validate_stable(&self.build_id)?;
        validate_stable(&self.content_bundle_version)?;
        validate_stable(&self.region_id)?;
        validate_tags(&self.cohort_tags)?;
        validate_time(self.started_at_utc_millis)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredM03TelemetrySessionV1 {
    pub session_id: [u8; 16],
    pub account_id: [u8; 16],
    pub build_id: String,
    pub content_bundle_version: String,
    pub platform: StoredM03TelemetryPlatformV1,
    pub region_id: String,
    pub environment: StoredM03TelemetryEnvironmentV1,
    pub cohort_tags: Vec<String>,
    pub started_at_utc_millis: u64,
    pub ended_at_utc_millis: Option<u64>,
    pub end_reason: Option<StoredM03SessionEndReasonV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredM03SessionEndReasonV1 {
    CleanExit,
    LinkLost,
    TransportClosed,
    ClientCrash,
    ServerShutdown,
}

impl StoredM03SessionEndReasonV1 {
    const fn code(self) -> i16 {
        match self {
            Self::CleanExit => 0,
            Self::LinkLost => 1,
            Self::TransportClosed => 2,
            Self::ClientCrash => 3,
            Self::ServerShutdown => 4,
        }
    }

    fn decode(value: i16) -> Result<Self, M03TelemetrySourceError> {
        match value {
            0 => Ok(Self::CleanExit),
            1 => Ok(Self::LinkLost),
            2 => Ok(Self::TransportClosed),
            3 => Ok(Self::ClientCrash),
            4 => Ok(Self::ServerShutdown),
            _ => Err(M03TelemetrySourceError::CorruptStoredSource),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum M03SessionObservationV1 {
    Disconnected,
    Reconnected,
    Ended(StoredM03SessionEndReasonV1),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M03SessionObservationCommandV1 {
    pub session_id: [u8; 16],
    pub account_id: [u8; 16],
    pub observation_id: [u8; 16],
    pub occurred_at_utc_millis: u64,
    pub observation: M03SessionObservationV1,
}

impl M03SessionObservationCommandV1 {
    pub fn validate(&self) -> Result<(), M03TelemetrySourceError> {
        validate_id(self.session_id)?;
        validate_id(self.account_id)?;
        validate_id(self.observation_id)?;
        validate_time(self.occurred_at_utc_millis)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredM03CrashSourceV1 {
    Client,
    Server,
}

impl StoredM03CrashSourceV1 {
    const fn code(self) -> i16 {
        match self {
            Self::Client => 0,
            Self::Server => 1,
        }
    }

    fn decode(value: i16) -> Result<Self, M03TelemetrySourceError> {
        match value {
            0 => Ok(Self::Client),
            1 => Ok(Self::Server),
            _ => Err(M03TelemetrySourceError::CorruptStoredSource),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredM03CrashKindV1 {
    Panic,
    AccessViolation,
    OutOfMemory,
    Watchdog,
    Unknown,
}

impl StoredM03CrashKindV1 {
    const fn code(self) -> i16 {
        match self {
            Self::Panic => 0,
            Self::AccessViolation => 1,
            Self::OutOfMemory => 2,
            Self::Watchdog => 3,
            Self::Unknown => 4,
        }
    }

    fn decode(value: i16) -> Result<Self, M03TelemetrySourceError> {
        match value {
            0 => Ok(Self::Panic),
            1 => Ok(Self::AccessViolation),
            2 => Ok(Self::OutOfMemory),
            3 => Ok(Self::Watchdog),
            4 => Ok(Self::Unknown),
            _ => Err(M03TelemetrySourceError::CorruptStoredSource),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredM03CrashReporterV1 {
    ServerObserver,
    AuthenticatedClient,
    ApprovedCollector,
}

impl StoredM03CrashReporterV1 {
    const fn code(self) -> i16 {
        match self {
            Self::ServerObserver => 0,
            Self::AuthenticatedClient => 1,
            Self::ApprovedCollector => 2,
        }
    }

    fn decode(value: i16) -> Result<Self, M03TelemetrySourceError> {
        match value {
            0 => Ok(Self::ServerObserver),
            1 => Ok(Self::AuthenticatedClient),
            2 => Ok(Self::ApprovedCollector),
            _ => Err(M03TelemetrySourceError::CorruptStoredSource),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M03CrashObservationCommandV1 {
    pub crash_id: [u8; 16],
    pub account_id: [u8; 16],
    pub character_id: Option<[u8; 16]>,
    pub session_id: [u8; 16],
    pub source: StoredM03CrashSourceV1,
    pub kind: StoredM03CrashKindV1,
    pub reporter: StoredM03CrashReporterV1,
    pub signature: [u8; 32],
    pub uptime_millis: u64,
    pub occurred_at_utc_millis: u64,
}

impl M03CrashObservationCommandV1 {
    pub fn validate(&self) -> Result<(), M03TelemetrySourceError> {
        validate_id(self.crash_id)?;
        validate_id(self.account_id)?;
        validate_id(self.session_id)?;
        if let Some(character_id) = self.character_id {
            validate_id(character_id)?;
        }
        if self.signature.iter().all(|byte| *byte == 0)
            || matches!(
                (self.source, self.reporter),
                (
                    StoredM03CrashSourceV1::Client,
                    StoredM03CrashReporterV1::ServerObserver
                ) | (
                    StoredM03CrashSourceV1::Server,
                    StoredM03CrashReporterV1::AuthenticatedClient
                )
            )
        {
            return Err(M03TelemetrySourceError::InvalidCommand);
        }
        validate_time(self.occurred_at_utc_millis)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredM03TelemetryContextV1 {
    pub account_id: [u8; 16],
    pub character_id: Option<[u8; 16]>,
    pub session_id: [u8; 16],
    pub build_id: String,
    pub content_bundle_version: String,
    pub platform: StoredM03TelemetryPlatformV1,
    pub region_id: String,
    pub environment: StoredM03TelemetryEnvironmentV1,
    pub cohort_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredM03OnboardingEventV1 {
    AccountCreated,
    CharacterCreated {
        class_id: String,
    },
    CharacterEnteredCombat {
        class_id: String,
        source_content_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredM03SessionEventV1 {
    Started,
    Ended {
        duration_millis: u64,
        reason: StoredM03SessionEndReasonV1,
    },
    Disconnected,
    Reconnected {
        link_lost_millis: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredM03CrashEventV1 {
    pub crash_id: [u8; 16],
    pub source: StoredM03CrashSourceV1,
    pub kind: StoredM03CrashKindV1,
    pub reporter: StoredM03CrashReporterV1,
    pub signature: [u8; 32],
    pub uptime_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredM03LootActionV1 {
    Created,
    PickedUp,
    Equipped,
    Extracted,
    Destroyed,
}

impl StoredM03LootActionV1 {
    fn decode(value: i16) -> Result<Self, M03TelemetrySourceError> {
        match value {
            0 => Ok(Self::Created),
            1 => Ok(Self::PickedUp),
            2 => Ok(Self::Equipped),
            3 => Ok(Self::Extracted),
            4 => Ok(Self::Destroyed),
            _ => Err(M03TelemetrySourceError::CorruptStoredSource),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredM03LootEventV1 {
    pub action: StoredM03LootActionV1,
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub source_content_id: String,
    pub item_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredM03TelemetryEventV1 {
    Onboarding(StoredM03OnboardingEventV1),
    Session(StoredM03SessionEventV1),
    Crash(StoredM03CrashEventV1),
    Loot(StoredM03LootEventV1),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredM03TelemetrySourceV1 {
    pub event_id: [u8; 16],
    pub source_id: [u8; 16],
    pub commit_sequence: u64,
    pub occurred_at_utc_millis: u64,
    pub context: StoredM03TelemetryContextV1,
    pub event: StoredM03TelemetryEventV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum M03TelemetrySourceFamilyV1 {
    Onboarding,
    Session,
    Crash,
    Loot,
}

impl M03TelemetrySourceFamilyV1 {
    fn decode(value: i16) -> Result<Self, M03TelemetrySourceError> {
        match value {
            0 => Ok(Self::Onboarding),
            1 => Ok(Self::Session),
            2 => Ok(Self::Crash),
            3 => Ok(Self::Loot),
            _ => Err(M03TelemetrySourceError::CorruptStoredSource),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct M03TelemetryPublicationV1 {
    pub family: M03TelemetrySourceFamilyV1,
    pub event_id: [u8; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionObservationPlanV1 {
    event_kind: i16,
    duration_millis: Option<u64>,
    end_reason: Option<i16>,
    link_lost_millis: Option<u64>,
}

impl PostgresPersistence {
    pub async fn begin_m03_telemetry_session_v1(
        &self,
        command: &M03TelemetrySessionStartV1,
    ) -> Result<StoredM03TelemetrySourceV1, M03TelemetrySourceError> {
        command.validate()?;
        retry_transaction(|| self.begin_m03_telemetry_session_once_v1(command)).await
    }

    async fn begin_m03_telemetry_session_once_v1(
        &self,
        command: &M03TelemetrySessionStartV1,
    ) -> Result<StoredM03TelemetrySourceV1, M03TelemetrySourceError> {
        let mut transaction = self.begin_transaction().await?;
        if let Some(stored) =
            load_session_by_id(transaction.connection(), command.session_id).await?
        {
            if !session_start_matches(&stored, command) {
                return Err(M03TelemetrySourceError::IdempotencyConflict);
            }
            let source =
                load_session_source_by_sequence(transaction.connection(), command.session_id, 1)
                    .await?
                    .ok_or(M03TelemetrySourceError::CorruptStoredSource)?;
            transaction.rollback().await?;
            return Ok(source);
        }
        if load_open_session_by_account(transaction.connection(), command.account_id)
            .await?
            .is_some()
        {
            return Err(M03TelemetrySourceError::OpenSessionConflict);
        }
        sqlx::query(
            "INSERT INTO core_telemetry_sessions_v1
             (namespace_id,session_id,account_id,build_id,content_bundle_version,
              platform,region_id,environment,cohort_tags,started_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,
                     to_timestamp($10::double precision/1000.0))",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(command.session_id.as_slice())
        .bind(command.account_id.as_slice())
        .bind(&command.build_id)
        .bind(&command.content_bundle_version)
        .bind(command.platform.code())
        .bind(&command.region_id)
        .bind(command.environment.code())
        .bind(&command.cohort_tags)
        .bind(to_i64(command.started_at_utc_millis)?)
        .execute(transaction.connection())
        .await?;
        sqlx::query(
            "INSERT INTO session_outbox_events_v1
             (namespace_id,event_id,source_id,account_id,session_id,event_sequence,event_kind,
              occurred_at)
             VALUES ($1,derive_m03_telemetry_event_id_v1(
                         'gravebound.telemetry.session-started.v1',$2),
                     $2,$3,$2,1,0,to_timestamp($4::double precision/1000.0))",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(command.session_id.as_slice())
        .bind(command.account_id.as_slice())
        .bind(to_i64(command.started_at_utc_millis)?)
        .execute(transaction.connection())
        .await?;
        let source =
            load_session_source_by_sequence(transaction.connection(), command.session_id, 1)
                .await?
                .ok_or(M03TelemetrySourceError::CorruptStoredSource)?;
        transaction.commit().await?;
        Ok(source)
    }

    pub async fn load_open_m03_telemetry_session_v1(
        &self,
        account_id: [u8; 16],
    ) -> Result<Option<StoredM03TelemetrySessionV1>, M03TelemetrySourceError> {
        validate_id(account_id)?;
        let mut transaction = self.begin_read_transaction().await?;
        let result = load_open_session_by_account(transaction.connection(), account_id).await?;
        transaction.rollback().await?;
        Ok(result)
    }

    /// Loads the exact durable head for one logical session. Runtime recovery uses this instead
    /// of guessing whether an open root was connected or disconnected before process loss.
    pub async fn load_m03_telemetry_session_head_v1(
        &self,
        account_id: [u8; 16],
        session_id: [u8; 16],
    ) -> Result<StoredM03TelemetrySourceV1, M03TelemetrySourceError> {
        validate_id(account_id)?;
        validate_id(session_id)?;
        let mut transaction = self.begin_read_transaction().await?;
        let session = load_session_by_id(transaction.connection(), session_id)
            .await?
            .ok_or(M03TelemetrySourceError::SessionNotFound)?;
        if session.account_id != account_id {
            return Err(M03TelemetrySourceError::SessionAuthorityMismatch);
        }
        let source = load_latest_session_source(transaction.connection(), session_id)
            .await?
            .ok_or(M03TelemetrySourceError::CorruptStoredSource)?;
        transaction.rollback().await?;
        Ok(source)
    }

    pub async fn record_m03_session_observation_v1(
        &self,
        command: &M03SessionObservationCommandV1,
    ) -> Result<StoredM03TelemetrySourceV1, M03TelemetrySourceError> {
        command.validate()?;
        retry_transaction(|| self.record_m03_session_observation_once_v1(command)).await
    }

    async fn record_m03_session_observation_once_v1(
        &self,
        command: &M03SessionObservationCommandV1,
    ) -> Result<StoredM03TelemetrySourceV1, M03TelemetrySourceError> {
        let mut transaction = self.begin_transaction().await?;
        let session = lock_session(transaction.connection(), command.session_id)
            .await?
            .ok_or(M03TelemetrySourceError::SessionNotFound)?;
        if session.account_id != command.account_id {
            return Err(M03TelemetrySourceError::SessionAuthorityMismatch);
        }
        if let Some(existing) = load_session_source_by_source_id(
            transaction.connection(),
            command.session_id,
            command.observation_id,
        )
        .await?
        {
            if !session_observation_matches(&existing.event, command.observation) {
                return Err(M03TelemetrySourceError::IdempotencyConflict);
            }
            transaction.rollback().await?;
            return Ok(existing);
        }
        if session.ended_at_utc_millis.is_some() {
            return Err(M03TelemetrySourceError::SessionEnded);
        }
        let last = load_latest_session_source(transaction.connection(), command.session_id)
            .await?
            .ok_or(M03TelemetrySourceError::CorruptStoredSource)?;
        let plan = plan_session_observation(command, &session, &last)?;
        let next_sequence = last_session_sequence(transaction.connection(), command.session_id)
            .await?
            .checked_add(1)
            .ok_or(M03TelemetrySourceError::Capacity)?;
        sqlx::query(
            "INSERT INTO session_outbox_events_v1
             (namespace_id,event_id,source_id,account_id,session_id,event_sequence,event_kind,
              duration_millis,end_reason,link_lost_millis,occurred_at)
             VALUES ($1,derive_m03_telemetry_event_id_v1(
                         'gravebound.telemetry.session-observation.v1',$2),
                     $2,$3,$4,$5,$6,$7,$8,$9,
                     to_timestamp($10::double precision/1000.0))",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(command.observation_id.as_slice())
        .bind(command.account_id.as_slice())
        .bind(command.session_id.as_slice())
        .bind(to_i64(next_sequence)?)
        .bind(plan.event_kind)
        .bind(plan.duration_millis.map(to_i64).transpose()?)
        .bind(plan.end_reason)
        .bind(plan.link_lost_millis.map(to_i64).transpose()?)
        .bind(to_i64(command.occurred_at_utc_millis)?)
        .execute(transaction.connection())
        .await?;
        if let M03SessionObservationV1::Ended(reason) = command.observation {
            let changed = sqlx::query(
                "UPDATE core_telemetry_sessions_v1
                 SET ended_at=to_timestamp($1::double precision/1000.0),end_reason=$2
                 WHERE namespace_id=$3 AND session_id=$4 AND ended_at IS NULL",
            )
            .bind(to_i64(command.occurred_at_utc_millis)?)
            .bind(reason.code())
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(command.session_id.as_slice())
            .execute(transaction.connection())
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(M03TelemetrySourceError::InvalidTransition);
            }
        }
        let source = load_session_source_by_source_id(
            transaction.connection(),
            command.session_id,
            command.observation_id,
        )
        .await?
        .ok_or(M03TelemetrySourceError::CorruptStoredSource)?;
        transaction.commit().await?;
        Ok(source)
    }

    pub async fn record_m03_crash_observation_v1(
        &self,
        command: &M03CrashObservationCommandV1,
    ) -> Result<StoredM03TelemetrySourceV1, M03TelemetrySourceError> {
        command.validate()?;
        retry_transaction(|| self.record_m03_crash_observation_once_v1(command)).await
    }

    async fn record_m03_crash_observation_once_v1(
        &self,
        command: &M03CrashObservationCommandV1,
    ) -> Result<StoredM03TelemetrySourceV1, M03TelemetrySourceError> {
        let mut transaction = self.begin_transaction().await?;
        let session = lock_session(transaction.connection(), command.session_id)
            .await?
            .ok_or(M03TelemetrySourceError::SessionNotFound)?;
        if session.account_id != command.account_id {
            return Err(M03TelemetrySourceError::SessionAuthorityMismatch);
        }
        if command.occurred_at_utc_millis < session.started_at_utc_millis
            || session
                .ended_at_utc_millis
                .is_some_and(|ended| command.occurred_at_utc_millis > ended)
        {
            return Err(M03TelemetrySourceError::InvalidTransition);
        }
        if let Some(existing) =
            load_crash_source(transaction.connection(), command.crash_id).await?
        {
            if !crash_observation_matches(&existing, command) {
                return Err(M03TelemetrySourceError::IdempotencyConflict);
            }
            transaction.rollback().await?;
            return Ok(existing);
        }
        sqlx::query(
            "INSERT INTO crash_outbox_events_v1
             (namespace_id,event_id,crash_id,account_id,character_id,session_id,
              crash_source,crash_kind,reporter_kind,signature,uptime_millis,occurred_at)
             VALUES ($1,derive_m03_telemetry_event_id_v1(
                         'gravebound.telemetry.crash-observation.v1',$2),
                     $2,$3,$4,$5,$6,$7,$8,$9,$10,
                     to_timestamp($11::double precision/1000.0))",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(command.crash_id.as_slice())
        .bind(command.account_id.as_slice())
        .bind(command.character_id.map(|value| value.to_vec()))
        .bind(command.session_id.as_slice())
        .bind(command.source.code())
        .bind(command.kind.code())
        .bind(command.reporter.code())
        .bind(command.signature.as_slice())
        .bind(to_i64(command.uptime_millis)?)
        .bind(to_i64(command.occurred_at_utc_millis)?)
        .execute(transaction.connection())
        .await?;
        let source = load_crash_source(transaction.connection(), command.crash_id)
            .await?
            .ok_or(M03TelemetrySourceError::CorruptStoredSource)?;
        transaction.commit().await?;
        Ok(source)
    }

    pub async fn poll_m03_telemetry_sources_v1(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredM03TelemetrySourceV1>, M03TelemetrySourceError> {
        if limit == 0 || limit > MAX_M03_TELEMETRY_SOURCE_POLL_V1 {
            return Err(M03TelemetrySourceError::InvalidLimit);
        }
        let rows = sqlx::query(POLL_DOMAIN_SOURCES_SQL)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(i64::try_from(limit).map_err(|_| M03TelemetrySourceError::InvalidLimit)?)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(decode_source_row).collect()
    }

    pub async fn acknowledge_m03_telemetry_sources_v1(
        &self,
        accepted: &[M03TelemetryPublicationV1],
    ) -> Result<Vec<M03TelemetryPublicationV1>, M03TelemetrySourceError> {
        if accepted.len() > MAX_M03_TELEMETRY_SOURCE_POLL_V1 {
            return Err(M03TelemetrySourceError::InvalidLimit);
        }
        let mut canonical = accepted.to_vec();
        canonical.sort_unstable_by_key(|source| (source.event_id, source.family));
        if canonical
            .windows(2)
            .any(|pair| pair[0].event_id == pair[1].event_id)
        {
            return Err(M03TelemetrySourceError::InvalidAcknowledgement);
        }
        let mut transaction = self.begin_transaction().await?;
        for source in &canonical {
            validate_id(source.event_id)?;
            let changed = match source.family {
                M03TelemetrySourceFamilyV1::Onboarding => sqlx::query(
                    "UPDATE onboarding_outbox_events_v1
                     SET published_at=transaction_timestamp()
                     WHERE namespace_id=$1 AND event_id=$2 AND published_at IS NULL",
                )
                .bind(WIPEABLE_CORE_NAMESPACE)
                .bind(source.event_id.as_slice())
                .execute(&mut *transaction.connection())
                .await?
                .rows_affected(),
                M03TelemetrySourceFamilyV1::Session => sqlx::query(
                    "UPDATE session_outbox_events_v1
                     SET published_at=transaction_timestamp()
                     WHERE namespace_id=$1 AND event_id=$2 AND published_at IS NULL",
                )
                .bind(WIPEABLE_CORE_NAMESPACE)
                .bind(source.event_id.as_slice())
                .execute(&mut *transaction.connection())
                .await?
                .rows_affected(),
                M03TelemetrySourceFamilyV1::Crash => sqlx::query(
                    "UPDATE crash_outbox_events_v1
                     SET published_at=transaction_timestamp()
                     WHERE namespace_id=$1 AND event_id=$2 AND published_at IS NULL",
                )
                .bind(WIPEABLE_CORE_NAMESPACE)
                .bind(source.event_id.as_slice())
                .execute(&mut *transaction.connection())
                .await?
                .rows_affected(),
                M03TelemetrySourceFamilyV1::Loot => sqlx::query(
                    "UPDATE item_ledger_telemetry_outbox_v1
                     SET published_at=transaction_timestamp()
                     WHERE namespace_id=$1 AND event_id=$2 AND published_at IS NULL",
                )
                .bind(WIPEABLE_CORE_NAMESPACE)
                .bind(source.event_id.as_slice())
                .execute(&mut *transaction.connection())
                .await?
                .rows_affected(),
            };
            if changed != 1 {
                return Err(M03TelemetrySourceError::PublicationConflict);
            }
        }
        transaction.commit().await?;
        Ok(canonical)
    }
}

fn plan_session_observation(
    command: &M03SessionObservationCommandV1,
    session: &StoredM03TelemetrySessionV1,
    last: &StoredM03TelemetrySourceV1,
) -> Result<SessionObservationPlanV1, M03TelemetrySourceError> {
    if command.occurred_at_utc_millis < last.occurred_at_utc_millis {
        return Err(M03TelemetrySourceError::InvalidTransition);
    }
    let StoredM03TelemetryEventV1::Session(last_event) = &last.event else {
        return Err(M03TelemetrySourceError::CorruptStoredSource);
    };
    match command.observation {
        M03SessionObservationV1::Disconnected => {
            if matches!(last_event, StoredM03SessionEventV1::Disconnected) {
                return Err(M03TelemetrySourceError::InvalidTransition);
            }
            Ok(SessionObservationPlanV1 {
                event_kind: 2,
                duration_millis: None,
                end_reason: None,
                link_lost_millis: None,
            })
        }
        M03SessionObservationV1::Reconnected => {
            if !matches!(last_event, StoredM03SessionEventV1::Disconnected) {
                return Err(M03TelemetrySourceError::InvalidTransition);
            }
            Ok(SessionObservationPlanV1 {
                event_kind: 3,
                duration_millis: None,
                end_reason: None,
                link_lost_millis: Some(
                    command.occurred_at_utc_millis - last.occurred_at_utc_millis,
                ),
            })
        }
        M03SessionObservationV1::Ended(reason) => Ok(SessionObservationPlanV1 {
            event_kind: 1,
            duration_millis: Some(command.occurred_at_utc_millis - session.started_at_utc_millis),
            end_reason: Some(reason.code()),
            link_lost_millis: None,
        }),
    }
}

async fn retry_transaction<'a, F, Fut, T>(mut operation: F) -> Result<T, M03TelemetrySourceError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, M03TelemetrySourceError>> + 'a,
{
    for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
        match operation().await {
            Err(error) if attempt < MAX_TRANSACTION_ATTEMPTS && is_retryable(&error) => {}
            result => return result,
        }
    }
    unreachable!("bounded telemetry transaction loop always returns")
}

fn is_retryable(error: &M03TelemetrySourceError) -> bool {
    matches!(
        error,
        M03TelemetrySourceError::Database(sqlx::Error::Database(database))
            | M03TelemetrySourceError::Persistence(crate::PersistenceError::Database(
                sqlx::Error::Database(database)
            ))
            if matches!(database.code().as_deref(), Some("40001" | "40P01"))
    )
}

async fn load_open_session_by_account(
    connection: &mut PgConnection,
    account_id: [u8; 16],
) -> Result<Option<StoredM03TelemetrySessionV1>, M03TelemetrySourceError> {
    let row = sqlx::query(
        "SELECT session_id,account_id,build_id,content_bundle_version,platform,region_id,
                environment,cohort_tags,
                floor(extract(epoch FROM started_at)*1000)::bigint AS started_at_millis,
                NULL::bigint AS ended_at_millis,end_reason
         FROM core_telemetry_sessions_v1
         WHERE namespace_id=$1 AND account_id=$2 AND ended_at IS NULL",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(decode_session_row).transpose()
}

async fn load_session_by_id(
    connection: &mut PgConnection,
    session_id: [u8; 16],
) -> Result<Option<StoredM03TelemetrySessionV1>, M03TelemetrySourceError> {
    let row = sqlx::query(
        "SELECT session_id,account_id,build_id,content_bundle_version,platform,region_id,
                environment,cohort_tags,
                floor(extract(epoch FROM started_at)*1000)::bigint AS started_at_millis,
                CASE WHEN ended_at IS NULL THEN NULL
                     ELSE floor(extract(epoch FROM ended_at)*1000)::bigint END AS ended_at_millis,
                end_reason
         FROM core_telemetry_sessions_v1 WHERE namespace_id=$1 AND session_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(session_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(decode_session_row).transpose()
}

async fn lock_session(
    connection: &mut PgConnection,
    session_id: [u8; 16],
) -> Result<Option<StoredM03TelemetrySessionV1>, M03TelemetrySourceError> {
    let row = sqlx::query(
        "SELECT session_id,account_id,build_id,content_bundle_version,platform,region_id,
                environment,cohort_tags,
                floor(extract(epoch FROM started_at)*1000)::bigint AS started_at_millis,
                CASE WHEN ended_at IS NULL THEN NULL
                     ELSE floor(extract(epoch FROM ended_at)*1000)::bigint END AS ended_at_millis,
                end_reason
         FROM core_telemetry_sessions_v1 WHERE namespace_id=$1 AND session_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(session_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(decode_session_row).transpose()
}

fn decode_session_row(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredM03TelemetrySessionV1, M03TelemetrySourceError> {
    let end_reason = row
        .try_get::<Option<i16>, _>("end_reason")?
        .map(StoredM03SessionEndReasonV1::decode)
        .transpose()?;
    Ok(StoredM03TelemetrySessionV1 {
        session_id: fixed(row.try_get("session_id")?)?,
        account_id: fixed(row.try_get("account_id")?)?,
        build_id: row.try_get("build_id")?,
        content_bundle_version: row.try_get("content_bundle_version")?,
        platform: StoredM03TelemetryPlatformV1::decode(row.try_get("platform")?)?,
        region_id: row.try_get("region_id")?,
        environment: StoredM03TelemetryEnvironmentV1::decode(row.try_get("environment")?)?,
        cohort_tags: row.try_get("cohort_tags")?,
        started_at_utc_millis: nonnegative(row.try_get("started_at_millis")?)?,
        ended_at_utc_millis: optional_nonnegative(row.try_get("ended_at_millis")?)?,
        end_reason,
    })
}

async fn load_session_source_by_sequence(
    connection: &mut PgConnection,
    session_id: [u8; 16],
    sequence: u64,
) -> Result<Option<StoredM03TelemetrySourceV1>, M03TelemetrySourceError> {
    load_single_source(connection, session_id, Some(sequence), None).await
}

async fn load_session_source_by_source_id(
    connection: &mut PgConnection,
    session_id: [u8; 16],
    source_id: [u8; 16],
) -> Result<Option<StoredM03TelemetrySourceV1>, M03TelemetrySourceError> {
    load_single_source(connection, session_id, None, Some(source_id)).await
}

async fn load_latest_session_source(
    connection: &mut PgConnection,
    session_id: [u8; 16],
) -> Result<Option<StoredM03TelemetrySourceV1>, M03TelemetrySourceError> {
    let row = sqlx::query(
        r"SELECT 1::smallint AS family,session_event.event_id,session_event.account_id,
                 NULL::bytea AS character_id,session_event.session_id,session.build_id,
                 session.content_bundle_version,session.platform,session.region_id,
                 session.environment,session.cohort_tags,
                 floor(extract(epoch FROM session_event.occurred_at)*1000)::bigint
                     AS occurred_at_millis,
                 floor(extract(epoch FROM session_event.created_at)*1000000)::bigint
                     AS commit_order,
                 session_event.event_kind,session_event.source_id,NULL::text AS class_id,
                 NULL::text AS source_content_id,session_event.event_sequence,
                 session_event.duration_millis,session_event.end_reason,
                 session_event.link_lost_millis,NULL::bytea AS crash_id,
                 NULL::smallint AS crash_source,NULL::smallint AS crash_kind,
                 NULL::smallint AS reporter_kind,NULL::bytea AS signature,
                 NULL::bigint AS uptime_millis
          FROM session_outbox_events_v1 AS session_event
          JOIN core_telemetry_sessions_v1 AS session
            ON session.namespace_id=session_event.namespace_id
           AND session.account_id=session_event.account_id
           AND session.session_id=session_event.session_id
          WHERE session_event.namespace_id=$1 AND session_event.session_id=$2
          ORDER BY session_event.event_sequence DESC LIMIT 1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(session_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(decode_source_row).transpose()
}

async fn last_session_sequence(
    connection: &mut PgConnection,
    session_id: [u8; 16],
) -> Result<u64, M03TelemetrySourceError> {
    let value: Option<i64> = sqlx::query_scalar(
        "SELECT max(event_sequence) FROM session_outbox_events_v1
         WHERE namespace_id=$1 AND session_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(session_id.as_slice())
    .fetch_one(connection)
    .await?;
    positive(value.ok_or(M03TelemetrySourceError::CorruptStoredSource)?)
}

async fn load_single_source(
    connection: &mut PgConnection,
    session_id: [u8; 16],
    sequence: Option<u64>,
    source_id: Option<[u8; 16]>,
) -> Result<Option<StoredM03TelemetrySourceV1>, M03TelemetrySourceError> {
    let row = sqlx::query(
        r"SELECT 1::smallint AS family,session_event.event_id,session_event.account_id,
                 NULL::bytea AS character_id,session_event.session_id,session.build_id,
                 session.content_bundle_version,session.platform,session.region_id,
                 session.environment,session.cohort_tags,
                 floor(extract(epoch FROM session_event.occurred_at)*1000)::bigint
                     AS occurred_at_millis,
                 floor(extract(epoch FROM session_event.created_at)*1000000)::bigint
                     AS commit_order,
                 session_event.event_kind,session_event.source_id,NULL::text AS class_id,
                 NULL::text AS source_content_id,session_event.event_sequence,
                 session_event.duration_millis,session_event.end_reason,
                 session_event.link_lost_millis,NULL::bytea AS crash_id,
                 NULL::smallint AS crash_source,NULL::smallint AS crash_kind,
                 NULL::smallint AS reporter_kind,NULL::bytea AS signature,
                 NULL::bigint AS uptime_millis
          FROM session_outbox_events_v1 AS session_event
          JOIN core_telemetry_sessions_v1 AS session
            ON session.namespace_id=session_event.namespace_id
           AND session.account_id=session_event.account_id
           AND session.session_id=session_event.session_id
          WHERE session_event.namespace_id=$1 AND session_event.session_id=$2
            AND (($3::bigint IS NOT NULL AND session_event.event_sequence=$3)
                 OR ($4::bytea IS NOT NULL AND session_event.source_id=$4))",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(session_id.as_slice())
    .bind(sequence.map(to_i64).transpose()?)
    .bind(source_id.map(|value| value.to_vec()))
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(decode_source_row).transpose()
}

async fn load_crash_source(
    connection: &mut PgConnection,
    crash_id: [u8; 16],
) -> Result<Option<StoredM03TelemetrySourceV1>, M03TelemetrySourceError> {
    let query = r"SELECT 2::smallint AS family,crash.event_id,crash.account_id,
                         crash.character_id,crash.session_id,session.build_id,
                         session.content_bundle_version,session.platform,session.region_id,
                         session.environment,session.cohort_tags,
                         floor(extract(epoch FROM crash.occurred_at)*1000)::bigint
                             AS occurred_at_millis,
                         floor(extract(epoch FROM crash.created_at)*1000000)::bigint
                             AS commit_order,
                         NULL::smallint AS event_kind,crash.crash_id AS source_id,
                         NULL::text AS class_id,NULL::text AS source_content_id,
                         NULL::bigint AS event_sequence,NULL::bigint AS duration_millis,
                         NULL::smallint AS end_reason,NULL::bigint AS link_lost_millis,
                         crash.crash_id,crash.crash_source,crash.crash_kind,
                         crash.reporter_kind,crash.signature,crash.uptime_millis
                  FROM crash_outbox_events_v1 AS crash
                  JOIN core_telemetry_sessions_v1 AS session
                    ON session.namespace_id=crash.namespace_id
                   AND session.account_id=crash.account_id
                   AND session.session_id=crash.session_id
                  WHERE crash.namespace_id=$1 AND crash.crash_id=$2";
    let row = sqlx::query(query)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(crash_id.as_slice())
        .fetch_optional(connection)
        .await?;
    row.as_ref().map(decode_source_row).transpose()
}

fn decode_source_row(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredM03TelemetrySourceV1, M03TelemetrySourceError> {
    let family = M03TelemetrySourceFamilyV1::decode(row.try_get("family")?)?;
    let context = decode_source_context(row)?;
    let event = decode_source_event(row, family, &context)?;
    Ok(StoredM03TelemetrySourceV1 {
        event_id: fixed(row.try_get("event_id")?)?,
        source_id: fixed(row.try_get("source_id")?)?,
        commit_sequence: positive(row.try_get("commit_order")?)?,
        occurred_at_utc_millis: nonnegative(row.try_get("occurred_at_millis")?)?,
        context,
        event,
    })
}

fn decode_source_context(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredM03TelemetryContextV1, M03TelemetrySourceError> {
    let context = StoredM03TelemetryContextV1 {
        account_id: fixed(row.try_get("account_id")?)?,
        character_id: row
            .try_get::<Option<Vec<u8>>, _>("character_id")?
            .map(fixed)
            .transpose()?,
        session_id: fixed(row.try_get("session_id")?)?,
        build_id: row.try_get("build_id")?,
        content_bundle_version: row.try_get("content_bundle_version")?,
        platform: StoredM03TelemetryPlatformV1::decode(row.try_get("platform")?)?,
        region_id: row.try_get("region_id")?,
        environment: StoredM03TelemetryEnvironmentV1::decode(row.try_get("environment")?)?,
        cohort_tags: row.try_get("cohort_tags")?,
    };
    validate_tags(&context.cohort_tags)?;
    Ok(context)
}

fn decode_source_event(
    row: &sqlx::postgres::PgRow,
    family: M03TelemetrySourceFamilyV1,
    context: &StoredM03TelemetryContextV1,
) -> Result<StoredM03TelemetryEventV1, M03TelemetrySourceError> {
    match family {
        M03TelemetrySourceFamilyV1::Onboarding => {
            decode_onboarding_event(row, context).map(StoredM03TelemetryEventV1::Onboarding)
        }
        M03TelemetrySourceFamilyV1::Session => {
            decode_session_event(row).map(StoredM03TelemetryEventV1::Session)
        }
        M03TelemetrySourceFamilyV1::Crash => {
            decode_crash_event(row).map(StoredM03TelemetryEventV1::Crash)
        }
        M03TelemetrySourceFamilyV1::Loot => {
            decode_loot_event(row).map(StoredM03TelemetryEventV1::Loot)
        }
    }
}

fn decode_onboarding_event(
    row: &sqlx::postgres::PgRow,
    context: &StoredM03TelemetryContextV1,
) -> Result<StoredM03OnboardingEventV1, M03TelemetrySourceError> {
    let kind: i16 = row.try_get("event_kind")?;
    let class_id: Option<String> = row.try_get("class_id")?;
    let source_content_id: Option<String> = row.try_get("source_content_id")?;
    match kind {
        0 if context.character_id.is_none()
            && class_id.is_none()
            && source_content_id.is_none() =>
        {
            Ok(StoredM03OnboardingEventV1::AccountCreated)
        }
        1 => Ok(StoredM03OnboardingEventV1::CharacterCreated {
            class_id: class_id.ok_or(M03TelemetrySourceError::CorruptStoredSource)?,
        }),
        2 => Ok(StoredM03OnboardingEventV1::CharacterEnteredCombat {
            class_id: class_id.ok_or(M03TelemetrySourceError::CorruptStoredSource)?,
            source_content_id: source_content_id
                .ok_or(M03TelemetrySourceError::CorruptStoredSource)?,
        }),
        _ => Err(M03TelemetrySourceError::CorruptStoredSource),
    }
}

fn decode_session_event(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredM03SessionEventV1, M03TelemetrySourceError> {
    match row.try_get::<i16, _>("event_kind")? {
        0 => Ok(StoredM03SessionEventV1::Started),
        1 => Ok(StoredM03SessionEventV1::Ended {
            duration_millis: nonnegative(required_column(row, "duration_millis")?)?,
            reason: StoredM03SessionEndReasonV1::decode(required_column(row, "end_reason")?)?,
        }),
        2 => Ok(StoredM03SessionEventV1::Disconnected),
        3 => Ok(StoredM03SessionEventV1::Reconnected {
            link_lost_millis: nonnegative(required_column(row, "link_lost_millis")?)?,
        }),
        _ => Err(M03TelemetrySourceError::CorruptStoredSource),
    }
}

fn decode_crash_event(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredM03CrashEventV1, M03TelemetrySourceError> {
    Ok(StoredM03CrashEventV1 {
        crash_id: fixed(required_column(row, "crash_id")?)?,
        source: StoredM03CrashSourceV1::decode(required_column(row, "crash_source")?)?,
        kind: StoredM03CrashKindV1::decode(required_column(row, "crash_kind")?)?,
        reporter: StoredM03CrashReporterV1::decode(required_column(row, "reporter_kind")?)?,
        signature: fixed(required_column(row, "signature")?)?,
        uptime_millis: nonnegative(required_column(row, "uptime_millis")?)?,
    })
}

fn decode_loot_event(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredM03LootEventV1, M03TelemetrySourceError> {
    let template_id: String = required_column(row, "template_id")?;
    let source_content_id: String = required_column(row, "loot_source_content_id")?;
    validate_stable(&template_id)?;
    validate_stable(&source_content_id)?;
    Ok(StoredM03LootEventV1 {
        action: StoredM03LootActionV1::decode(required_column(row, "loot_action")?)?,
        item_uid: fixed(required_column(row, "item_uid")?)?,
        template_id,
        source_content_id,
        item_version: positive(required_column(row, "item_version")?)?,
    })
}

fn required_column<T>(row: &sqlx::postgres::PgRow, name: &str) -> Result<T, M03TelemetrySourceError>
where
    for<'row> T: sqlx::Decode<'row, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get::<Option<T>, _>(name)?
        .ok_or(M03TelemetrySourceError::CorruptStoredSource)
}

fn session_start_matches(
    stored: &StoredM03TelemetrySessionV1,
    command: &M03TelemetrySessionStartV1,
) -> bool {
    stored.session_id == command.session_id
        && stored.account_id == command.account_id
        && stored.build_id == command.build_id
        && stored.content_bundle_version == command.content_bundle_version
        && stored.platform == command.platform
        && stored.region_id == command.region_id
        && stored.environment == command.environment
        && stored.cohort_tags == command.cohort_tags
        && stored.started_at_utc_millis == command.started_at_utc_millis
}

fn session_observation_matches(
    stored: &StoredM03TelemetryEventV1,
    attempted: M03SessionObservationV1,
) -> bool {
    match (stored, attempted) {
        (
            StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Disconnected),
            M03SessionObservationV1::Disconnected,
        )
        | (
            StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Reconnected { .. }),
            M03SessionObservationV1::Reconnected,
        ) => true,
        (
            StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Ended {
                reason: stored,
                ..
            }),
            M03SessionObservationV1::Ended(attempted),
        ) => *stored == attempted,
        _ => false,
    }
}

fn crash_observation_matches(
    stored: &StoredM03TelemetrySourceV1,
    command: &M03CrashObservationCommandV1,
) -> bool {
    stored.context.account_id == command.account_id
        && stored.context.character_id == command.character_id
        && stored.context.session_id == command.session_id
        && stored.occurred_at_utc_millis == command.occurred_at_utc_millis
        && matches!(
            &stored.event,
            StoredM03TelemetryEventV1::Crash(event)
                if event.crash_id == command.crash_id
                    && event.source == command.source
                    && event.kind == command.kind
                    && event.reporter == command.reporter
                    && event.signature == command.signature
                    && event.uptime_millis == command.uptime_millis
        )
}

fn validate_id(value: [u8; 16]) -> Result<(), M03TelemetrySourceError> {
    if value.iter().all(|byte| *byte == 0) {
        return Err(M03TelemetrySourceError::InvalidCommand);
    }
    Ok(())
}

fn validate_stable(value: &str) -> Result<(), M03TelemetrySourceError> {
    StableTelemetryId::new(value).map_err(|_| M03TelemetrySourceError::InvalidCommand)?;
    Ok(())
}

fn validate_tags(values: &[String]) -> Result<(), M03TelemetrySourceError> {
    if values.len() > 16
        || values.windows(2).any(|pair| pair[0] >= pair[1])
        || values.iter().any(|value| validate_stable(value).is_err())
    {
        return Err(M03TelemetrySourceError::InvalidCommand);
    }
    Ok(())
}

fn validate_time(value: u64) -> Result<(), M03TelemetrySourceError> {
    if value == 0 || i64::try_from(value).is_err() {
        return Err(M03TelemetrySourceError::InvalidCommand);
    }
    Ok(())
}

fn to_i64(value: u64) -> Result<i64, M03TelemetrySourceError> {
    i64::try_from(value).map_err(|_| M03TelemetrySourceError::Capacity)
}

fn fixed<const N: usize>(value: Vec<u8>) -> Result<[u8; N], M03TelemetrySourceError> {
    value
        .try_into()
        .map_err(|_| M03TelemetrySourceError::CorruptStoredSource)
}

fn positive(value: i64) -> Result<u64, M03TelemetrySourceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value != 0)
        .ok_or(M03TelemetrySourceError::CorruptStoredSource)
}

fn nonnegative(value: i64) -> Result<u64, M03TelemetrySourceError> {
    u64::try_from(value).map_err(|_| M03TelemetrySourceError::CorruptStoredSource)
}

fn optional_nonnegative(value: Option<i64>) -> Result<Option<u64>, M03TelemetrySourceError> {
    value.map(nonnegative).transpose()
}

#[derive(Debug, Error)]
pub enum M03TelemetrySourceError {
    #[error("M03 telemetry source command is invalid")]
    InvalidCommand,
    #[error("M03 telemetry source limit is outside its bounded contract")]
    InvalidLimit,
    #[error("M03 telemetry source identity was replayed with changed material")]
    IdempotencyConflict,
    #[error("another durable logical session is already open for this account")]
    OpenSessionConflict,
    #[error("durable logical telemetry session was not found")]
    SessionNotFound,
    #[error("telemetry session belongs to another account")]
    SessionAuthorityMismatch,
    #[error("durable telemetry session has already ended")]
    SessionEnded,
    #[error("telemetry session transition is not canonical")]
    InvalidTransition,
    #[error("telemetry source or sequence capacity was exceeded")]
    Capacity,
    #[error("stored telemetry-domain source is corrupt")]
    CorruptStoredSource,
    #[error("telemetry publication acknowledgement is not canonical")]
    InvalidAcknowledgement,
    #[error("telemetry publication marker conflicted")]
    PublicationConflict,
    #[error(transparent)]
    Persistence(#[from] crate::PersistenceError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> M03TelemetrySessionStartV1 {
        M03TelemetrySessionStartV1 {
            session_id: [1; 16],
            account_id: [2; 16],
            build_id: "m03-core-dev-identity-1".into(),
            content_bundle_version: "core-dev".into(),
            platform: StoredM03TelemetryPlatformV1::Windows,
            region_id: "local".into(),
            environment: StoredM03TelemetryEnvironmentV1::Test,
            cohort_tags: vec!["cohort.private".into(), "staff".into()],
            started_at_utc_millis: 1_000,
        }
    }

    #[test]
    fn session_context_is_bounded_canonical_and_secret_resistant() {
        assert!(start().validate().is_ok());
        let mut invalid = start();
        invalid.cohort_tags.reverse();
        assert!(invalid.validate().is_err());
        invalid = start();
        invalid.build_id = "token-secret".into();
        assert!(invalid.validate().is_err());
        invalid = start();
        invalid.session_id = [0; 16];
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn crash_contract_excludes_zero_signatures_and_impossible_reporters() {
        let valid = M03CrashObservationCommandV1 {
            crash_id: [3; 16],
            account_id: [2; 16],
            character_id: None,
            session_id: [1; 16],
            source: StoredM03CrashSourceV1::Client,
            kind: StoredM03CrashKindV1::Panic,
            reporter: StoredM03CrashReporterV1::AuthenticatedClient,
            signature: [4; 32],
            uptime_millis: 500,
            occurred_at_utc_millis: 1_500,
        };
        assert!(valid.validate().is_ok());
        let mut invalid = valid.clone();
        invalid.signature = [0; 32];
        assert!(invalid.validate().is_err());
        invalid = valid;
        invalid.reporter = StoredM03CrashReporterV1::ServerObserver;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn poll_sql_reads_only_committed_source_tables_and_origin_context() {
        for required in [
            "onboarding_outbox_events_v1",
            "session_outbox_events_v1",
            "crash_outbox_events_v1",
            "item_ledger_telemetry_outbox_v1",
            "core_telemetry_sessions_v1",
            "published_at IS NULL",
            "ORDER BY commit_order,event_id,family",
            "LIMIT $2",
        ] {
            assert!(POLL_DOMAIN_SOURCES_SQL.contains(required));
        }
        for forbidden in [
            "item_instances",
            "FROM item_ledger_events",
            "reward_requests",
            "character_world_locations",
            "auth_ticket",
        ] {
            assert!(!POLL_DOMAIN_SOURCES_SQL.contains(forbidden));
        }
    }
}
