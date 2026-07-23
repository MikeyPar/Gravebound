use serde::Serialize;
use thiserror::Error;

use crate::{PseudonymousAccountId, StableTelemetryId, TelemetryId};

/// Version 2 makes correction authority explicitly optional instead of fabricating a zero count.
pub const TELEMETRY_EVENT_SCHEMA_VERSION: u16 = 2;
const MAX_COHORT_TAGS: usize = 16;
const MAX_ACTIVE_BARGAINS: usize = 16;
const MAX_STATUSES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryPlatformV1 {
    Windows,
    Linux,
    MacOs,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryEnvironmentV1 {
    Local,
    Test,
    Staging,
    Production,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryContextV1 {
    pub pseudonymous_account_id: PseudonymousAccountId,
    pub character_id: Option<TelemetryId>,
    pub session_id: TelemetryId,
    pub build_id: StableTelemetryId,
    pub content_bundle_version: StableTelemetryId,
    pub platform: TelemetryPlatformV1,
    pub region_id: StableTelemetryId,
    pub environment: TelemetryEnvironmentV1,
    pub cohort_tags: Vec<StableTelemetryId>,
}

impl TelemetryContextV1 {
    fn validate(&self) -> Result<(), TelemetryEventError> {
        if self.cohort_tags.len() > MAX_COHORT_TAGS {
            return Err(TelemetryEventError::Capacity);
        }
        if !strictly_sorted(&self.cohort_tags) {
            return Err(TelemetryEventError::NonCanonicalOrder);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OnboardingEventV1 {
    AccountCreated,
    TutorialStepCompleted { step_id: StableTelemetryId },
    CharacterCreated { class_id: StableTelemetryId },
    CharacterEnteredCombat { class_id: StableTelemetryId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndReasonV1 {
    CleanExit,
    LinkLost,
    TransportClosed,
    ClientCrash,
    ServerShutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEventV1 {
    Started,
    Ended {
        duration_millis: u64,
        reason: SessionEndReasonV1,
    },
    Disconnected,
    Reconnected {
        link_lost_millis: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LootActionV1 {
    Created,
    PickedUp,
    Equipped,
    Extracted,
    Destroyed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LootEventV1 {
    pub action: LootActionV1,
    pub item_id: TelemetryId,
    pub template_id: StableTelemetryId,
    pub source_content_id: StableTelemetryId,
    pub item_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractionEventV1 {
    pub terminal_id: TelemetryId,
    pub extraction_request_id: TelemetryId,
    pub placed_item_count: u16,
    pub credited_material_stack_count: u8,
    pub resolution_hold_stack_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallTriggerV1 {
    Explicit,
    LinkLost,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecallEventV1 {
    pub terminal_id: TelemetryId,
    pub trigger: RecallTriggerV1,
    pub destroyed_pending_item_count: u16,
    pub destroyed_material_stack_count: u8,
    pub preserved_equipped_item_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageTypeV1 {
    Physical,
    Veil,
    True,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallStateV1 {
    Idle,
    Channeling,
    Cancelled,
    LostRace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathCauseV1 {
    DirectHit,
    DamageOverTime,
    Environment,
    Disconnect,
    ServerFault,
    AdministrativeRestore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NetworkHealthV1 {
    pub ping_millis: u16,
    pub jitter_millis: u16,
    pub loss_basis_points: u16,
    /// None means the negotiated client runtime had no reconciliation-counter authority.
    pub correction_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeathEventV1 {
    pub death_id: TelemetryId,
    pub class_id: StableTelemetryId,
    pub level: u16,
    pub oath_id: Option<StableTelemetryId>,
    pub active_bargain_ids: Vec<StableTelemetryId>,
    pub lifetime_millis: u64,
    /// Present only when the committing source persisted session timing with the death.
    pub session_duration_millis: Option<u64>,
    pub killer_content_id: StableTelemetryId,
    pub killer_pattern_id: Option<StableTelemetryId>,
    pub damage_type: DamageTypeV1,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub pre_hit_health: u32,
    pub status_ids: Vec<StableTelemetryId>,
    pub dungeon_id: Option<StableTelemetryId>,
    pub room_id: Option<StableTelemetryId>,
    pub boss_phase_id: Option<StableTelemetryId>,
    /// Optional until the corresponding values are captured by the atomic death source.
    pub party_size: Option<u8>,
    pub contribution_basis_points: Option<u16>,
    pub item_power_band: Option<u16>,
    pub network_health: Option<NetworkHealthV1>,
    pub recall_state: RecallStateV1,
    pub cause: DeathCauseV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuccessorEventV1 {
    Created {
        source_death_id: TelemetryId,
        elapsed_from_summary_millis: u64,
    },
    EnteredCombat {
        source_death_id: TelemetryId,
        elapsed_from_summary_millis: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashSourceV1 {
    Client,
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashKindV1 {
    Panic,
    AccessViolation,
    OutOfMemory,
    Watchdog,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CrashEventV1 {
    pub crash_id: TelemetryId,
    pub source: CrashSourceV1,
    pub kind: CrashKindV1,
    /// Non-reversible collector-produced signature. Raw stack traces and messages are forbidden.
    pub signature: [u8; 32],
    pub uptime_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "category", content = "payload", rename_all = "snake_case")]
pub enum TelemetryEventV1 {
    Onboarding(OnboardingEventV1),
    Session(SessionEventV1),
    Loot(LootEventV1),
    Extraction(ExtractionEventV1),
    Recall(RecallEventV1),
    Death(Box<DeathEventV1>),
    Successor(SuccessorEventV1),
    Crash(CrashEventV1),
}

impl TelemetryEventV1 {
    #[must_use]
    pub const fn event_name(&self) -> &'static str {
        match self {
            Self::Onboarding(OnboardingEventV1::AccountCreated) => "account_created",
            Self::Onboarding(OnboardingEventV1::TutorialStepCompleted { .. }) => {
                "tutorial_step_completed"
            }
            Self::Onboarding(OnboardingEventV1::CharacterCreated { .. }) => "character_created",
            Self::Onboarding(OnboardingEventV1::CharacterEnteredCombat { .. }) => {
                "character_entered_combat"
            }
            Self::Session(SessionEventV1::Started) => "session_started",
            Self::Session(SessionEventV1::Ended { .. }) => "session_ended",
            Self::Session(SessionEventV1::Disconnected) => "disconnect",
            Self::Session(SessionEventV1::Reconnected { .. }) => "reconnect",
            Self::Loot(event) => match event.action {
                LootActionV1::Created => "item_created",
                LootActionV1::PickedUp => "item_picked_up",
                LootActionV1::Equipped => "item_equipped",
                LootActionV1::Extracted => "item_extracted",
                LootActionV1::Destroyed => "item_destroyed",
            },
            Self::Extraction(_) => "dungeon_extracted",
            Self::Recall(_) => "dungeon_recalled",
            Self::Death(_) => "character_died",
            Self::Successor(SuccessorEventV1::Created { .. }) => "successor_created",
            Self::Successor(SuccessorEventV1::EnteredCombat { .. }) => "successor_entered_combat",
            Self::Crash(CrashEventV1 {
                source: CrashSourceV1::Client,
                ..
            }) => "client_crash",
            Self::Crash(CrashEventV1 {
                source: CrashSourceV1::Server,
                ..
            }) => "server_crash",
        }
    }

    fn validate(&self, has_character: bool) -> Result<(), TelemetryEventError> {
        let requires_character = matches!(
            self,
            Self::Loot(_)
                | Self::Extraction(_)
                | Self::Recall(_)
                | Self::Death(_)
                | Self::Successor(_)
                | Self::Onboarding(
                    OnboardingEventV1::CharacterCreated { .. }
                        | OnboardingEventV1::CharacterEnteredCombat { .. }
                )
        );
        if requires_character && !has_character {
            return Err(TelemetryEventError::CharacterRequired);
        }
        match self {
            Self::Loot(event) if event.item_version == 0 => Err(TelemetryEventError::ZeroVersion),
            Self::Death(event) => validate_death(event),
            Self::Crash(event) if event.signature.iter().all(|byte| *byte == 0) => {
                Err(TelemetryEventError::ZeroSignature)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedTelemetryEnvelopeV1 {
    event_id: TelemetryId,
    occurred_at_utc_millis: u64,
    context: TelemetryContextV1,
    event: TelemetryEventV1,
}

impl VersionedTelemetryEnvelopeV1 {
    pub fn new(
        event_id: TelemetryId,
        occurred_at_utc_millis: u64,
        context: TelemetryContextV1,
        event: TelemetryEventV1,
    ) -> Result<Self, TelemetryEventError> {
        if occurred_at_utc_millis == 0 {
            return Err(TelemetryEventError::ZeroTimestamp);
        }
        context.validate()?;
        event.validate(context.character_id.is_some())?;
        Ok(Self {
            event_id,
            occurred_at_utc_millis,
            context,
            event,
        })
    }

    #[must_use]
    pub const fn event_id(&self) -> TelemetryId {
        self.event_id
    }

    #[must_use]
    pub const fn occurred_at_utc_millis(&self) -> u64 {
        self.occurred_at_utc_millis
    }

    #[must_use]
    pub const fn event_name(&self) -> &'static str {
        self.event.event_name()
    }

    pub(crate) fn to_redacted_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&ExportEnvelopeV1 {
            event_id: self.event_id,
            event_name: self.event.event_name(),
            event_schema_version: TELEMETRY_EVENT_SCHEMA_VERSION,
            occurred_at_utc_millis: self.occurred_at_utc_millis,
            pseudonymous_account_id: self.context.pseudonymous_account_id,
            character_id: self.context.character_id,
            session_id: self.context.session_id,
            build_id: &self.context.build_id,
            content_bundle_version: &self.context.content_bundle_version,
            platform: self.context.platform,
            region_id: &self.context.region_id,
            environment: self.context.environment,
            cohort_tags: &self.context.cohort_tags,
            event: &self.event,
        })
    }
}

#[derive(Serialize)]
struct ExportEnvelopeV1<'a> {
    event_id: TelemetryId,
    event_name: &'static str,
    event_schema_version: u16,
    #[serde(rename = "occurred_at_utc")]
    occurred_at_utc_millis: u64,
    pseudonymous_account_id: PseudonymousAccountId,
    character_id: Option<TelemetryId>,
    session_id: TelemetryId,
    build_id: &'a StableTelemetryId,
    content_bundle_version: &'a StableTelemetryId,
    platform: TelemetryPlatformV1,
    region_id: &'a StableTelemetryId,
    environment: TelemetryEnvironmentV1,
    cohort_tags: &'a [StableTelemetryId],
    event: &'a TelemetryEventV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TelemetryEventError {
    #[error("telemetry timestamp must be nonzero")]
    ZeroTimestamp,
    #[error("telemetry event exceeds a bounded collection capacity")]
    Capacity,
    #[error("telemetry collection is not strictly sorted and unique")]
    NonCanonicalOrder,
    #[error("telemetry event requires a character correlation")]
    CharacterRequired,
    #[error("telemetry version must be nonzero")]
    ZeroVersion,
    #[error("crash signature must be nonzero")]
    ZeroSignature,
    #[error("death telemetry is invalid")]
    InvalidDeath,
}

fn validate_death(event: &DeathEventV1) -> Result<(), TelemetryEventError> {
    if event.level == 0
        || event.party_size.is_some_and(|size| size == 0 || size > 8)
        || event
            .contribution_basis_points
            .is_some_and(|value| value > 10_000)
        || event
            .network_health
            .is_some_and(|health| health.loss_basis_points > 10_000)
        || event.active_bargain_ids.len() > MAX_ACTIVE_BARGAINS
        || event.status_ids.len() > MAX_STATUSES
        || !strictly_sorted(&event.active_bargain_ids)
        || !strictly_sorted(&event.status_ids)
        || event.cause == DeathCauseV1::ServerFault
    {
        return Err(TelemetryEventError::InvalidDeath);
    }
    Ok(())
}

fn strictly_sorted(values: &[StableTelemetryId]) -> bool {
    !values
        .windows(2)
        .any(|pair| pair[0].as_str() >= pair[1].as_str())
}
