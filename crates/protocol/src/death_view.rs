//! Authenticated, read-only durable-death views for protocol 1.14.
//!
//! The contract follows `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-001`, `DTH-020`,
//! `TECH-020`-`022`), `Gravebound_Content_Production_Spec_v1.md` (`CONT-HUB-002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-06`). It deliberately has no lethal
//! resolution command and accepts no client-authored account, character, cause, trace,
//! destruction, Echo, or aggregate-version material. The authenticated server session supplies
//! account ownership when dispatching these queries.

use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

use crate::{ManifestHash, NetworkChannel, WireText};

pub const DEATH_VIEW_SCHEMA_VERSION: u16 = 2;
pub const DEATH_VIEW_ID_BYTES: usize = 16;
pub const DEATH_VIEW_DIGEST_BYTES: usize = 32;
pub const DEATH_VIEW_ID_MAX_BYTES: usize = 96;
pub const DEATH_VIEW_CHARACTER_NAME_MAX_BYTES: usize = 24;
pub const DEATH_VIEW_MAX_BARGAINS: usize = 3;
pub const DEATH_VIEW_MAX_STATUSES_PER_TRACE_ENTRY: usize = 32;
pub const DEATH_VIEW_MAX_SUMMARY_DAMAGE_ENTRIES: usize = 5;
pub const DEATH_VIEW_MAX_LOST_PROJECTIONS: u16 = 4_096;
pub const DEATH_VIEW_MAX_LOST_PROJECTIONS_PER_PAGE: u16 = 32;
pub const DEATH_VIEW_MAX_MEMORIALS_PER_PAGE: u8 = 32;
pub const DEATH_VIEW_MAX_TRACE_ENTRIES_PER_PAGE: u8 = 8;
pub const DEATH_VIEW_MAX_TRACE_ENTRIES: u16 = 4_096;
pub const DEATH_VIEW_TRACE_WINDOW_TICKS: u64 = 300;

pub const DEATH_SUMMARY_REVISION: u16 = 1;
const PRESERVED_CONTENT_IDS: [&str; 5] = [
    "projection.preserved.account_records",
    "projection.preserved.currency",
    "projection.preserved.vault",
    "projection.preserved.cosmetics",
    "projection.preserved.recipes",
];
const CREATED_CONTENT_IDS: [&str; 2] = ["projection.created.memorial", "projection.created.echo"];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeathCharacterName(String);

impl DeathCharacterName {
    pub fn new(value: impl Into<String>) -> Result<Self, DeathViewValidationError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > DEATH_VIEW_CHARACTER_NAME_MAX_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(DeathViewValidationError::InvalidCharacterName);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for DeathCharacterName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DeathCharacterName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathViewContentRevisionV1 {
    pub records_blake3: ManifestHash,
    pub assets_blake3: ManifestHash,
    pub localization_blake3: ManifestHash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathMemorialCursorV1 {
    pub death_at_unix_ms: u64,
    pub death_id: [u8; DEATH_VIEW_ID_BYTES],
}

impl DeathMemorialCursorV1 {
    fn validate(self) -> Result<(), DeathViewValidationError> {
        if self.death_at_unix_ms == 0 || all_zero(&self.death_id) {
            return Err(DeathViewValidationError::InvalidCursor);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathViewRequestV1 {
    LatestCommitted,
    Summary {
        death_id: [u8; DEATH_VIEW_ID_BYTES],
        lost_start_ordinal: u16,
        lost_limit: u16,
    },
    MemorialPage {
        after: Option<DeathMemorialCursorV1>,
        limit: u8,
    },
    TracePage {
        death_id: [u8; DEATH_VIEW_ID_BYTES],
        start_ordinal: u16,
        limit: u8,
    },
}

impl DeathViewRequestV1 {
    fn validate(&self) -> Result<(), DeathViewValidationError> {
        match self {
            Self::LatestCommitted => Ok(()),
            Self::Summary {
                death_id,
                lost_start_ordinal,
                lost_limit,
            } => {
                require_death_id(death_id)?;
                if *lost_start_ordinal > DEATH_VIEW_MAX_LOST_PROJECTIONS
                    || !(1..=DEATH_VIEW_MAX_LOST_PROJECTIONS_PER_PAGE).contains(lost_limit)
                {
                    return Err(DeathViewValidationError::InvalidPageLimit);
                }
                Ok(())
            }
            Self::MemorialPage { after, limit } => {
                if !(1..=DEATH_VIEW_MAX_MEMORIALS_PER_PAGE).contains(limit) {
                    return Err(DeathViewValidationError::InvalidPageLimit);
                }
                after.map_or(Ok(()), DeathMemorialCursorV1::validate)
            }
            Self::TracePage {
                death_id,
                start_ordinal,
                limit,
            } => {
                require_death_id(death_id)?;
                if *start_ordinal >= DEATH_VIEW_MAX_TRACE_ENTRIES
                    || !(1..=DEATH_VIEW_MAX_TRACE_ENTRIES_PER_PAGE).contains(limit)
                {
                    return Err(DeathViewValidationError::InvalidPageLimit);
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathViewFrameV1 {
    pub schema_version: u16,
    pub sequence: u32,
    pub content_revision: DeathViewContentRevisionV1,
    pub request: DeathViewRequestV1,
}

impl DeathViewFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Control
    }

    pub fn validate(&self) -> Result<(), DeathViewValidationError> {
        if self.schema_version != DEATH_VIEW_SCHEMA_VERSION {
            return Err(DeathViewValidationError::UnsupportedSchemaVersion);
        }
        if self.sequence == 0 {
            return Err(DeathViewValidationError::ZeroSequence);
        }
        self.request.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathCauseV1 {
    DirectHit,
    DamageOverTime,
    Environment,
    Disconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathDamageTypeV1 {
    Physical,
    Veil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathNetworkStateV1 {
    Connected,
    Degraded,
    LinkLost,
    Reattached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathRecallStateV1 {
    Inactive,
    Channeling,
    CompletionPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathEchoOutcomeV1 {
    NotEligible,
    Dormant,
    Available,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestCommittedDeathV1 {
    pub death_id: [u8; DEATH_VIEW_ID_BYTES],
    pub character_id: [u8; DEATH_VIEW_ID_BYTES],
    pub death_at_unix_ms: u64,
    pub death_tick: u64,
    pub cause: DeathCauseV1,
    pub killer_content_id: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub killer_pattern_id: Option<WireText<DEATH_VIEW_ID_MAX_BYTES>>,
    pub network_state: DeathNetworkStateV1,
    pub recall_state: DeathRecallStateV1,
    pub trace_entry_count: u16,
    pub trace_digest: [u8; DEATH_VIEW_DIGEST_BYTES],
    pub destruction_entry_count: u16,
    pub destruction_digest: [u8; DEATH_VIEW_DIGEST_BYTES],
    pub summary_snapshot_digest: [u8; DEATH_VIEW_DIGEST_BYTES],
    pub content_revision: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub presentation_revision: DeathViewContentRevisionV1,
}

impl LatestCommittedDeathV1 {
    fn validate(&self) -> Result<(), DeathViewValidationError> {
        require_death_id(&self.death_id)?;
        if all_zero(&self.character_id)
            || self.death_at_unix_ms == 0
            || self.death_tick == 0
            || !valid_stable_id(&self.killer_content_id)
            || self
                .killer_pattern_id
                .as_ref()
                .is_some_and(|value| !valid_stable_id(value))
            || self.trace_entry_count == 0
            || self.trace_entry_count > DEATH_VIEW_MAX_TRACE_ENTRIES
            || self.destruction_entry_count > DEATH_VIEW_MAX_LOST_PROJECTIONS
            || zero_digest(&self.trace_digest)
            || zero_digest(&self.destruction_digest)
            || zero_digest(&self.summary_snapshot_digest)
            || !valid_content_revision(&self.content_revision)
        {
            return Err(DeathViewValidationError::InvalidDeathProjection);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathTraceStatusV1 {
    pub ordinal: u8,
    pub status_id: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub remaining_ticks: u32,
    pub stack_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathTraceEntryV1 {
    pub ordinal: u16,
    pub event_tick: u64,
    pub event_ordinal: u32,
    pub source_content_id: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub source_entity_id: Option<[u8; DEATH_VIEW_ID_BYTES]>,
    pub pattern_id: Option<WireText<DEATH_VIEW_ID_MAX_BYTES>>,
    pub attack_id: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub damage_type: DeathDamageTypeV1,
    pub pre_health: u32,
    pub post_health: u32,
    pub source_x_milli_tiles: i32,
    pub source_y_milli_tiles: i32,
    pub network_state: DeathNetworkStateV1,
    pub recall_state: DeathRecallStateV1,
    pub lethal: bool,
    pub statuses: Vec<DeathTraceStatusV1>,
}

impl DeathTraceEntryV1 {
    fn validate(&self, death_tick: u64) -> Result<(), DeathViewValidationError> {
        if self.event_tick == 0
            || self.event_tick > death_tick
            || death_tick.saturating_sub(self.event_tick) > DEATH_VIEW_TRACE_WINDOW_TICKS
            || !valid_stable_id(&self.source_content_id)
            || self.source_entity_id == Some([0; DEATH_VIEW_ID_BYTES])
            || self
                .pattern_id
                .as_ref()
                .is_some_and(|value| !valid_stable_id(value))
            || !valid_stable_id(&self.attack_id)
            || self.pre_health == 0
            || self.post_health != self.pre_health.saturating_sub(self.final_damage)
            || self.lethal != (self.post_health == 0)
            || self.statuses.len() > DEATH_VIEW_MAX_STATUSES_PER_TRACE_ENTRY
        {
            return Err(DeathViewValidationError::InvalidTrace);
        }

        let mut status_ids = BTreeSet::new();
        for (index, status) in self.statuses.iter().enumerate() {
            if status.ordinal != u8::try_from(index).unwrap_or(u8::MAX)
                || !valid_stable_id(&status.status_id)
                || status.remaining_ticks > 108_000
                || !(1..=255).contains(&status.stack_count)
                || !status_ids.insert(status.status_id.as_str())
            {
                return Err(DeathViewValidationError::InvalidTrace);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathSummaryProjectionKindV1 {
    LostItem,
    LostRunMaterial,
    PreservedAccountRecords,
    PreservedCurrency,
    PreservedVault,
    PreservedCosmetics,
    PreservedRecipes,
    CreatedMemorial,
    CreatedEcho,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathSummaryProjectionEntryV1 {
    pub ordinal: u16,
    pub kind: DeathSummaryProjectionKindV1,
    pub content_id: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub quantity: u32,
    pub item_uid: Option<[u8; DEATH_VIEW_ID_BYTES]>,
}

impl DeathSummaryProjectionEntryV1 {
    fn validate_loss(&self, expected_ordinal: u16) -> bool {
        self.ordinal == expected_ordinal
            && valid_stable_id(&self.content_id)
            && match self.kind {
                DeathSummaryProjectionKindV1::LostItem => {
                    self.quantity == 1 && self.item_uid.is_some_and(|uid| !all_zero(&uid))
                }
                DeathSummaryProjectionKindV1::LostRunMaterial => {
                    self.quantity > 0 && self.item_uid.is_none()
                }
                _ => false,
            }
    }

    fn matches_fixed(
        &self,
        expected_ordinal: usize,
        expected_kind: DeathSummaryProjectionKindV1,
        expected_content_id: &str,
    ) -> bool {
        self.ordinal == u16::try_from(expected_ordinal).unwrap_or(u16::MAX)
            && self.kind == expected_kind
            && self.content_id.as_str() == expected_content_id
            && self.quantity == 1
            && self.item_uid.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathSummaryViewV1 {
    pub death_id: [u8; DEATH_VIEW_ID_BYTES],
    pub summary_revision: u16,
    pub hero_label_key: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub character_name_snapshot: DeathCharacterName,
    pub class_id: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub level: u8,
    pub oath_id: Option<WireText<DEATH_VIEW_ID_MAX_BYTES>>,
    pub bargains: Vec<WireText<DEATH_VIEW_ID_MAX_BYTES>>,
    pub lifetime_ms: u64,
    pub final_deed_id: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub lethal_trace_ordinal: u16,
    pub last_five_damage: Vec<DeathTraceEntryV1>,
    pub lost_total_count: u16,
    pub lost_start_ordinal: u16,
    pub lost: Vec<DeathSummaryProjectionEntryV1>,
    pub next_lost_ordinal: Option<u16>,
    pub preserved: Vec<DeathSummaryProjectionEntryV1>,
    pub created: Vec<DeathSummaryProjectionEntryV1>,
    pub echo_outcome: DeathEchoOutcomeV1,
    pub death_tick: u64,
    pub content_revision: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub snapshot_digest: [u8; DEATH_VIEW_DIGEST_BYTES],
    pub presentation_revision: DeathViewContentRevisionV1,
}

impl DeathSummaryViewV1 {
    fn validate(&self, requested_limit: u16) -> Result<(), DeathViewValidationError> {
        require_death_id(&self.death_id)?;
        if self.summary_revision != DEATH_SUMMARY_REVISION
            || !valid_stable_id(&self.hero_label_key)
            || !valid_stable_id(&self.class_id)
            || !(1..=10).contains(&self.level)
            || self
                .oath_id
                .as_ref()
                .is_some_and(|value| !valid_stable_id(value))
            || !valid_unique_ids(&self.bargains, DEATH_VIEW_MAX_BARGAINS)
            || !valid_stable_id(&self.final_deed_id)
            || self.last_five_damage.is_empty()
            || self.last_five_damage.len() > DEATH_VIEW_MAX_SUMMARY_DAMAGE_ENTRIES
            || self.last_five_damage.len()
                != usize::from(self.lethal_trace_ordinal.saturating_add(1)).min(5)
            || self.last_five_damage[0].ordinal
                != self
                    .lethal_trace_ordinal
                    .saturating_add(1)
                    .saturating_sub(u16::try_from(self.last_five_damage.len()).unwrap_or(u16::MAX))
            || self.lost_total_count > DEATH_VIEW_MAX_LOST_PROJECTIONS
            || self.death_tick == 0
            || !valid_content_revision(&self.content_revision)
            || zero_digest(&self.snapshot_digest)
        {
            return Err(DeathViewValidationError::InvalidSummary);
        }

        validate_trace_slice(
            &self.last_five_damage,
            self.last_five_damage[0].ordinal,
            self.death_tick,
            self.lethal_trace_ordinal.saturating_add(1),
        )?;
        if self
            .last_five_damage
            .last()
            .is_none_or(|entry| entry.ordinal != self.lethal_trace_ordinal || !entry.lethal)
        {
            return Err(DeathViewValidationError::InvalidSummary);
        }

        let expected_count = usize::from(
            requested_limit.min(
                self.lost_total_count
                    .saturating_sub(self.lost_start_ordinal),
            ),
        );
        if self.lost_start_ordinal > self.lost_total_count
            || self.lost.len() != expected_count
            || self.lost.iter().enumerate().any(|(index, entry)| {
                !entry.validate_loss(
                    self.lost_start_ordinal
                        .saturating_add(u16::try_from(index).unwrap_or(u16::MAX)),
                )
            })
        {
            return Err(DeathViewValidationError::InvalidSummary);
        }
        let mut item_uids = BTreeSet::new();
        let mut material_ids = BTreeSet::new();
        for entry in &self.lost {
            match entry.kind {
                DeathSummaryProjectionKindV1::LostItem => {
                    if entry.item_uid.is_none_or(|uid| !item_uids.insert(uid)) {
                        return Err(DeathViewValidationError::InvalidSummary);
                    }
                }
                DeathSummaryProjectionKindV1::LostRunMaterial => {
                    if !material_ids.insert(entry.content_id.as_str()) {
                        return Err(DeathViewValidationError::InvalidSummary);
                    }
                }
                _ => return Err(DeathViewValidationError::InvalidSummary),
            }
        }
        let expected_next = self
            .lost_start_ordinal
            .checked_add(u16::try_from(self.lost.len()).unwrap_or(u16::MAX))
            .filter(|next| *next < self.lost_total_count);
        if self.next_lost_ordinal != expected_next
            || !fixed_projection_set(&self.preserved, &PRESERVED_CONTENT_IDS, true)
            || !fixed_projection_set(&self.created, &CREATED_CONTENT_IDS, false)
        {
            return Err(DeathViewValidationError::InvalidSummary);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathMemorialEntryV1 {
    pub cursor: DeathMemorialCursorV1,
    pub summary_revision: u16,
    pub summary_snapshot_digest: [u8; DEATH_VIEW_DIGEST_BYTES],
    pub presentation_key: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub presentation_digest: [u8; DEATH_VIEW_DIGEST_BYTES],
    pub character_name_snapshot: DeathCharacterName,
    pub class_id: WireText<DEATH_VIEW_ID_MAX_BYTES>,
    pub level: u8,
    pub echo_outcome: DeathEchoOutcomeV1,
    pub presentation_revision: DeathViewContentRevisionV1,
}

impl DeathMemorialEntryV1 {
    fn validate(&self) -> Result<(), DeathViewValidationError> {
        self.cursor.validate()?;
        if self.summary_revision != DEATH_SUMMARY_REVISION
            || zero_digest(&self.summary_snapshot_digest)
            || !valid_stable_id(&self.presentation_key)
            || zero_digest(&self.presentation_digest)
            || !valid_stable_id(&self.class_id)
            || !(1..=10).contains(&self.level)
        {
            return Err(DeathViewValidationError::InvalidMemorialPage);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathTracePageV1 {
    pub death_id: [u8; DEATH_VIEW_ID_BYTES],
    pub death_tick: u64,
    pub total_entry_count: u16,
    pub trace_digest: [u8; DEATH_VIEW_DIGEST_BYTES],
    pub start_ordinal: u16,
    pub entries: Vec<DeathTraceEntryV1>,
    pub next_ordinal: Option<u16>,
    pub presentation_revision: DeathViewContentRevisionV1,
}

impl DeathTracePageV1 {
    fn validate(&self, requested_limit: u8) -> Result<(), DeathViewValidationError> {
        require_death_id(&self.death_id)?;
        if self.death_tick == 0
            || self.total_entry_count == 0
            || self.total_entry_count > DEATH_VIEW_MAX_TRACE_ENTRIES
            || zero_digest(&self.trace_digest)
            || self.start_ordinal >= self.total_entry_count
        {
            return Err(DeathViewValidationError::InvalidTrace);
        }
        let expected_count = usize::from(
            u16::from(requested_limit)
                .min(self.total_entry_count.saturating_sub(self.start_ordinal)),
        );
        if self.entries.len() != expected_count {
            return Err(DeathViewValidationError::InvalidTrace);
        }
        validate_trace_slice(
            &self.entries,
            self.start_ordinal,
            self.death_tick,
            self.total_entry_count,
        )?;
        let expected_next = self
            .start_ordinal
            .checked_add(u16::try_from(self.entries.len()).unwrap_or(u16::MAX))
            .filter(|next| *next < self.total_entry_count);
        if self.next_ordinal != expected_next {
            return Err(DeathViewValidationError::InvalidTrace);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathViewResultCodeV1 {
    Unauthenticated,
    FeatureDisabled,
    DeathNotFound,
    DeathNotOwned,
    PageOutOfRange,
    ContentMismatch,
    CorruptStoredRecord,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathViewResultV1 {
    Latest {
        schema_version: u16,
        request_sequence: u32,
        death: Option<LatestCommittedDeathV1>,
    },
    Summary {
        schema_version: u16,
        request_sequence: u32,
        requested_lost_limit: u16,
        summary: DeathSummaryViewV1,
    },
    MemorialPage {
        schema_version: u16,
        request_sequence: u32,
        requested_limit: u8,
        entries: Vec<DeathMemorialEntryV1>,
        next_cursor: Option<DeathMemorialCursorV1>,
    },
    TracePage {
        schema_version: u16,
        request_sequence: u32,
        requested_limit: u8,
        page: DeathTracePageV1,
    },
    Error {
        schema_version: u16,
        request_sequence: u32,
        code: DeathViewResultCodeV1,
    },
}

impl DeathViewResultV1 {
    pub fn validate(&self) -> Result<(), DeathViewValidationError> {
        let (schema_version, request_sequence) = match self {
            Self::Latest {
                schema_version,
                request_sequence,
                ..
            }
            | Self::Summary {
                schema_version,
                request_sequence,
                ..
            }
            | Self::MemorialPage {
                schema_version,
                request_sequence,
                ..
            }
            | Self::TracePage {
                schema_version,
                request_sequence,
                ..
            }
            | Self::Error {
                schema_version,
                request_sequence,
                ..
            } => (*schema_version, *request_sequence),
        };
        if schema_version != DEATH_VIEW_SCHEMA_VERSION {
            return Err(DeathViewValidationError::UnsupportedSchemaVersion);
        }
        if request_sequence == 0 {
            return Err(DeathViewValidationError::ZeroSequence);
        }

        match self {
            Self::Latest { death, .. } => death
                .as_ref()
                .map_or(Ok(()), LatestCommittedDeathV1::validate),
            Self::Summary {
                requested_lost_limit,
                summary,
                ..
            } => {
                if !(1..=DEATH_VIEW_MAX_LOST_PROJECTIONS_PER_PAGE).contains(requested_lost_limit) {
                    return Err(DeathViewValidationError::InvalidPageLimit);
                }
                summary.validate(*requested_lost_limit)
            }
            Self::MemorialPage {
                requested_limit,
                entries,
                next_cursor,
                ..
            } => validate_memorial_page(*requested_limit, entries, *next_cursor),
            Self::TracePage {
                requested_limit,
                page,
                ..
            } => {
                if !(1..=DEATH_VIEW_MAX_TRACE_ENTRIES_PER_PAGE).contains(requested_limit) {
                    return Err(DeathViewValidationError::InvalidPageLimit);
                }
                page.validate(*requested_limit)
            }
            Self::Error { .. } => Ok(()),
        }
    }
}

fn validate_memorial_page(
    requested_limit: u8,
    entries: &[DeathMemorialEntryV1],
    next_cursor: Option<DeathMemorialCursorV1>,
) -> Result<(), DeathViewValidationError> {
    if !(1..=DEATH_VIEW_MAX_MEMORIALS_PER_PAGE).contains(&requested_limit)
        || entries.len() > usize::from(requested_limit)
    {
        return Err(DeathViewValidationError::InvalidPageLimit);
    }
    for entry in entries {
        entry.validate()?;
    }
    if entries
        .windows(2)
        .any(|pair| !memorial_precedes(pair[0].cursor, pair[1].cursor))
    {
        return Err(DeathViewValidationError::InvalidMemorialPage);
    }
    if let Some(cursor) = next_cursor {
        cursor.validate()?;
        if entries.len() != usize::from(requested_limit)
            || entries.last().is_none_or(|last| cursor != last.cursor)
        {
            return Err(DeathViewValidationError::InvalidCursor);
        }
    }
    Ok(())
}

fn validate_trace_slice(
    entries: &[DeathTraceEntryV1],
    start_ordinal: u16,
    death_tick: u64,
    total_entry_count: u16,
) -> Result<(), DeathViewValidationError> {
    let mut previous_order = None;
    for (index, entry) in entries.iter().enumerate() {
        let expected_ordinal = start_ordinal
            .checked_add(u16::try_from(index).unwrap_or(u16::MAX))
            .ok_or(DeathViewValidationError::InvalidTrace)?;
        if entry.ordinal != expected_ordinal {
            return Err(DeathViewValidationError::InvalidTrace);
        }
        entry.validate(death_tick)?;
        let order = (entry.event_tick, entry.event_ordinal);
        if previous_order.is_some_and(|previous| previous >= order)
            || (entry.lethal && entry.ordinal.saturating_add(1) != total_entry_count)
        {
            return Err(DeathViewValidationError::InvalidTrace);
        }
        previous_order = Some(order);
    }
    if entries
        .last()
        .is_some_and(|entry| entry.ordinal.saturating_add(1) == total_entry_count && !entry.lethal)
    {
        return Err(DeathViewValidationError::InvalidTrace);
    }
    Ok(())
}

fn fixed_projection_set<const N: usize>(
    entries: &[DeathSummaryProjectionEntryV1],
    ids: &[&str; N],
    preserved: bool,
) -> bool {
    if entries.len() != N {
        return false;
    }
    entries.iter().enumerate().all(|(index, entry)| {
        let kind = if preserved {
            match index {
                0 => DeathSummaryProjectionKindV1::PreservedAccountRecords,
                1 => DeathSummaryProjectionKindV1::PreservedCurrency,
                2 => DeathSummaryProjectionKindV1::PreservedVault,
                3 => DeathSummaryProjectionKindV1::PreservedCosmetics,
                _ => DeathSummaryProjectionKindV1::PreservedRecipes,
            }
        } else if index == 0 {
            DeathSummaryProjectionKindV1::CreatedMemorial
        } else {
            DeathSummaryProjectionKindV1::CreatedEcho
        };
        entry.matches_fixed(index, kind, ids[index])
    })
}

fn memorial_precedes(left: DeathMemorialCursorV1, right: DeathMemorialCursorV1) -> bool {
    left.death_at_unix_ms > right.death_at_unix_ms
        || (left.death_at_unix_ms == right.death_at_unix_ms && left.death_id < right.death_id)
}

fn require_death_id(death_id: &[u8; DEATH_VIEW_ID_BYTES]) -> Result<(), DeathViewValidationError> {
    if is_uuid_v7(death_id) {
        Ok(())
    } else {
        Err(DeathViewValidationError::InvalidDeathId)
    }
}

const fn is_uuid_v7(value: &[u8; DEATH_VIEW_ID_BYTES]) -> bool {
    !all_zero(value) && value[6] >> 4 == 7 && value[8] & 0b1100_0000 == 0b1000_0000
}

fn valid_unique_ids(values: &[WireText<DEATH_VIEW_ID_MAX_BYTES>], max: usize) -> bool {
    let mut unique = BTreeSet::new();
    values.len() <= max
        && values
            .iter()
            .all(|value| valid_stable_id(value) && unique.insert(value.as_str()))
}

fn valid_content_revision(value: &WireText<DEATH_VIEW_ID_MAX_BYTES>) -> bool {
    const PREFIX: &str = "core-dev.blake3.";
    value.as_str().len() == PREFIX.len() + 64
        && value.as_str().starts_with(PREFIX)
        && value.as_str()[PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_stable_id(value: &WireText<DEATH_VIEW_ID_MAX_BYTES>) -> bool {
    value.as_str().len() >= 3
        && value.as_str().split('.').all(|segment| {
            !segment.is_empty()
                && segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-')
                })
        })
}

const fn all_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    let mut index = 0;
    while index < N {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

const fn zero_digest(digest: &[u8; DEATH_VIEW_DIGEST_BYTES]) -> bool {
    all_zero(digest)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DeathViewValidationError {
    #[error("death-view schema version is unsupported")]
    UnsupportedSchemaVersion,
    #[error("death-view sequence must be nonzero")]
    ZeroSequence,
    #[error("death ID must be a nonzero UUIDv7")]
    InvalidDeathId,
    #[error("death-view page limit is outside its bounded range")]
    InvalidPageLimit,
    #[error("death-view cursor is invalid")]
    InvalidCursor,
    #[error("death-view character name is invalid")]
    InvalidCharacterName,
    #[error("latest committed death projection is invalid")]
    InvalidDeathProjection,
    #[error("death summary projection is invalid")]
    InvalidSummary,
    #[error("memorial page is invalid")]
    InvalidMemorialPage,
    #[error("combat trace page is invalid")]
    InvalidTrace,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_revision() -> DeathViewContentRevisionV1 {
        DeathViewContentRevisionV1 {
            records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
        }
    }

    fn trace_entry(ordinal: u16, lethal: bool) -> DeathTraceEntryV1 {
        DeathTraceEntryV1 {
            ordinal,
            event_tick: 300 + u64::from(ordinal),
            event_ordinal: u32::from(ordinal),
            source_content_id: WireText::new("enemy.bell_warden").unwrap(),
            source_entity_id: Some([7; 16]),
            pattern_id: Some(WireText::new("pattern.bell_ring").unwrap()),
            attack_id: WireText::new("attack.bell_ring").unwrap(),
            raw_damage: 12,
            final_damage: if lethal { 12 } else { 4 },
            damage_type: DeathDamageTypeV1::Physical,
            pre_health: 12,
            post_health: if lethal { 0 } else { 8 },
            source_x_milli_tiles: 1_000,
            source_y_milli_tiles: 2_000,
            network_state: DeathNetworkStateV1::Connected,
            recall_state: DeathRecallStateV1::Inactive,
            lethal,
            statuses: Vec::new(),
        }
    }

    fn uuid_v7(seed: u8) -> [u8; DEATH_VIEW_ID_BYTES] {
        let mut value = [seed; DEATH_VIEW_ID_BYTES];
        value[6] = 0x70 | (seed & 0x0f);
        value[8] = 0x80 | (seed & 0x3f);
        value
    }

    fn fixed_projection(
        ordinal: u16,
        kind: DeathSummaryProjectionKindV1,
        content_id: &str,
    ) -> DeathSummaryProjectionEntryV1 {
        DeathSummaryProjectionEntryV1 {
            ordinal,
            kind,
            content_id: WireText::new(content_id).unwrap(),
            quantity: 1,
            item_uid: None,
        }
    }

    fn summary() -> DeathSummaryViewV1 {
        DeathSummaryViewV1 {
            death_id: uuid_v7(1),
            summary_revision: 1,
            hero_label_key: WireText::new("hero.grave_arbalist").unwrap(),
            character_name_snapshot: DeathCharacterName::new("Mara Ash").unwrap(),
            class_id: WireText::new("class.grave_arbalist").unwrap(),
            level: 10,
            oath_id: Some(WireText::new("oath.long_vigil").unwrap()),
            bargains: vec![WireText::new("bargain.bell_debt").unwrap()],
            lifetime_ms: 600_000,
            final_deed_id: WireText::new("deed.bell_sepulcher").unwrap(),
            lethal_trace_ordinal: 1,
            last_five_damage: vec![trace_entry(0, false), trace_entry(1, true)],
            lost_total_count: 1,
            lost_start_ordinal: 0,
            lost: vec![DeathSummaryProjectionEntryV1 {
                ordinal: 0,
                kind: DeathSummaryProjectionKindV1::LostItem,
                content_id: WireText::new("item.weapon.gravebow").unwrap(),
                quantity: 1,
                item_uid: Some([8; 16]),
            }],
            next_lost_ordinal: None,
            preserved: PRESERVED_CONTENT_IDS
                .iter()
                .enumerate()
                .map(|(index, id)| {
                    let kind = match index {
                        0 => DeathSummaryProjectionKindV1::PreservedAccountRecords,
                        1 => DeathSummaryProjectionKindV1::PreservedCurrency,
                        2 => DeathSummaryProjectionKindV1::PreservedVault,
                        3 => DeathSummaryProjectionKindV1::PreservedCosmetics,
                        _ => DeathSummaryProjectionKindV1::PreservedRecipes,
                    };
                    fixed_projection(u16::try_from(index).unwrap(), kind, id)
                })
                .collect(),
            created: vec![
                fixed_projection(
                    0,
                    DeathSummaryProjectionKindV1::CreatedMemorial,
                    CREATED_CONTENT_IDS[0],
                ),
                fixed_projection(
                    1,
                    DeathSummaryProjectionKindV1::CreatedEcho,
                    CREATED_CONTENT_IDS[1],
                ),
            ],
            echo_outcome: DeathEchoOutcomeV1::Available,
            death_tick: 301,
            content_revision: WireText::new(format!("core-dev.blake3.{}", "d".repeat(64))).unwrap(),
            presentation_revision: content_revision(),
            snapshot_digest: [9; 32],
        }
    }

    #[test]
    fn schema_v2_successful_response_layouts_have_pinned_wire_bytes() {
        let latest = LatestCommittedDeathV1 {
            death_id: uuid_v7(1),
            character_id: [2; 16],
            death_at_unix_ms: 1,
            death_tick: 301,
            cause: DeathCauseV1::DirectHit,
            killer_content_id: WireText::new("enemy.bell_warden").unwrap(),
            killer_pattern_id: Some(WireText::new("pattern.bell_ring").unwrap()),
            network_state: DeathNetworkStateV1::Connected,
            recall_state: DeathRecallStateV1::Inactive,
            trace_entry_count: 2,
            trace_digest: [2; 32],
            destruction_entry_count: 1,
            destruction_digest: [3; 32],
            summary_snapshot_digest: [4; 32],
            content_revision: WireText::new(format!("core-dev.blake3.{}", "d".repeat(64))).unwrap(),
            presentation_revision: content_revision(),
        };
        let memorial = DeathMemorialEntryV1 {
            cursor: DeathMemorialCursorV1 {
                death_at_unix_ms: 1,
                death_id: uuid_v7(1),
            },
            summary_revision: 1,
            summary_snapshot_digest: [4; 32],
            presentation_key: WireText::new("memorial.default").unwrap(),
            presentation_digest: [5; 32],
            character_name_snapshot: DeathCharacterName::new("Mara").unwrap(),
            class_id: WireText::new("class.grave_arbalist").unwrap(),
            level: 10,
            echo_outcome: DeathEchoOutcomeV1::Dormant,
            presentation_revision: content_revision(),
        };
        let trace = DeathTracePageV1 {
            death_id: uuid_v7(1),
            death_tick: 301,
            total_entry_count: 2,
            trace_digest: [2; 32],
            start_ordinal: 0,
            entries: vec![trace_entry(0, false), trace_entry(1, true)],
            next_ordinal: None,
            presentation_revision: content_revision(),
        };
        let results = [
            DeathViewResultV1::Latest {
                schema_version: DEATH_VIEW_SCHEMA_VERSION,
                request_sequence: 1,
                death: Some(latest),
            },
            DeathViewResultV1::Summary {
                schema_version: DEATH_VIEW_SCHEMA_VERSION,
                request_sequence: 1,
                requested_lost_limit: 1,
                summary: summary(),
            },
            DeathViewResultV1::MemorialPage {
                schema_version: DEATH_VIEW_SCHEMA_VERSION,
                request_sequence: 1,
                requested_limit: 1,
                entries: vec![memorial],
                next_cursor: None,
            },
            DeathViewResultV1::TracePage {
                schema_version: DEATH_VIEW_SCHEMA_VERSION,
                request_sequence: 1,
                requested_limit: 2,
                page: trace,
            },
        ];
        let hashes = results.map(|result| {
            assert_eq!(result.validate(), Ok(()));
            let message = crate::WireMessage::ReliableEvent(crate::ReliableEventFrame {
                sequence: 1,
                server_tick: 301,
                event: crate::ReliableEvent::DeathViewResult(Box::new(result)),
            });
            let frame = crate::encode_frame(&message).unwrap();
            assert_eq!(crate::decode_frame(&frame), Ok(message));
            blake3::hash(&frame).to_hex().to_string()
        });
        assert_eq!(
            hashes,
            [
                "a75ca0fd3b87b078d42019616fd1bfbe5c9c570774f932b63ccb4b4d314caeee".to_owned(),
                "c61301e9c5b47b146b3f6e06596072ad2e8d5eabf3da2dd670535e40c352fdba".to_owned(),
                "88872c328f6b17dc702e5ecd65d32b1fbf9dcdadd216196563aa3dd894b0c428".to_owned(),
                "2f7f8e142d6e8f1c5ca90a6a096bdf42de32adc49318c137f8dd50307fc095a8".to_owned(),
            ]
        );
    }

    #[test]
    fn requests_are_read_only_versioned_and_strictly_bounded() {
        let mut frame = DeathViewFrameV1 {
            schema_version: DEATH_VIEW_SCHEMA_VERSION,
            sequence: 1,
            content_revision: content_revision(),
            request: DeathViewRequestV1::Summary {
                death_id: uuid_v7(1),
                lost_start_ordinal: 0,
                lost_limit: DEATH_VIEW_MAX_LOST_PROJECTIONS_PER_PAGE,
            },
        };
        assert_eq!(frame.channel(), NetworkChannel::Control);
        assert_eq!(frame.validate(), Ok(()));
        frame.schema_version += 1;
        assert_eq!(
            frame.validate(),
            Err(DeathViewValidationError::UnsupportedSchemaVersion)
        );
        frame.schema_version = DEATH_VIEW_SCHEMA_VERSION;
        frame.request = DeathViewRequestV1::TracePage {
            death_id: [0; 16],
            start_ordinal: 0,
            limit: 1,
        };
        assert_eq!(
            frame.validate(),
            Err(DeathViewValidationError::InvalidDeathId)
        );
        frame.request = DeathViewRequestV1::MemorialPage {
            after: None,
            limit: DEATH_VIEW_MAX_MEMORIALS_PER_PAGE + 1,
        };
        assert_eq!(
            frame.validate(),
            Err(DeathViewValidationError::InvalidPageLimit)
        );
    }

    #[test]
    fn stored_summary_snapshot_and_pagination_validate_exactly() {
        let result = DeathViewResultV1::Summary {
            schema_version: DEATH_VIEW_SCHEMA_VERSION,
            request_sequence: 1,
            requested_lost_limit: 1,
            summary: summary(),
        };
        assert_eq!(result.validate(), Ok(()));

        let mut invalid = summary();
        invalid.lost[0].ordinal = 1;
        assert_eq!(
            DeathViewResultV1::Summary {
                schema_version: DEATH_VIEW_SCHEMA_VERSION,
                request_sequence: 1,
                requested_lost_limit: 1,
                summary: invalid,
            }
            .validate(),
            Err(DeathViewValidationError::InvalidSummary)
        );
    }

    #[test]
    fn trace_pages_require_contiguous_order_and_one_terminal_lethal_entry() {
        let page = DeathTracePageV1 {
            death_id: uuid_v7(1),
            presentation_revision: content_revision(),
            death_tick: 301,
            total_entry_count: 2,
            trace_digest: [2; 32],
            start_ordinal: 0,
            entries: vec![trace_entry(0, false), trace_entry(1, true)],
            next_ordinal: None,
        };
        assert_eq!(page.validate(2), Ok(()));
        let mut invalid = page;
        invalid.entries[0].ordinal = 1;
        assert_eq!(
            invalid.validate(2),
            Err(DeathViewValidationError::InvalidTrace)
        );
    }

    #[test]
    fn maximum_trace_page_and_status_bounds_fit_one_reliable_frame() {
        let mut entries = (0..DEATH_VIEW_MAX_TRACE_ENTRIES_PER_PAGE)
            .map(|ordinal| {
                trace_entry(
                    u16::from(ordinal),
                    ordinal + 1 == DEATH_VIEW_MAX_TRACE_ENTRIES_PER_PAGE,
                )
            })
            .collect::<Vec<_>>();
        for entry in &mut entries {
            entry.statuses = (0..DEATH_VIEW_MAX_STATUSES_PER_TRACE_ENTRY)
                .map(|ordinal| DeathTraceStatusV1 {
                    ordinal: u8::try_from(ordinal).unwrap(),
                    status_id: WireText::new(format!("status.trace_{ordinal:02}")).unwrap(),
                    remaining_ticks: 108_000,
                    stack_count: 255,
                })
                .collect();
        }
        let result = DeathViewResultV1::TracePage {
            schema_version: DEATH_VIEW_SCHEMA_VERSION,
            request_sequence: 1,
            requested_limit: DEATH_VIEW_MAX_TRACE_ENTRIES_PER_PAGE,
            page: DeathTracePageV1 {
                death_id: uuid_v7(1),
                presentation_revision: content_revision(),
                death_tick: 307,
                total_entry_count: u16::from(DEATH_VIEW_MAX_TRACE_ENTRIES_PER_PAGE),
                trace_digest: [2; 32],
                start_ordinal: 0,
                entries,
                next_ordinal: None,
            },
        };
        assert_eq!(result.validate(), Ok(()));
        let message = crate::WireMessage::ReliableEvent(crate::ReliableEventFrame {
            sequence: 1,
            server_tick: 307,
            event: crate::ReliableEvent::DeathViewResult(Box::new(result)),
        });
        let frame = crate::encode_frame(&message).unwrap();
        assert!(frame.len() <= crate::RELIABLE_FRAME_LIMIT);

        let crate::WireMessage::ReliableEvent(frame) = message else {
            unreachable!();
        };
        let crate::ReliableEvent::DeathViewResult(mut result) = frame.event else {
            unreachable!();
        };
        let DeathViewResultV1::TracePage { page, .. } = result.as_mut() else {
            unreachable!();
        };
        page.entries[0].statuses.push(DeathTraceStatusV1 {
            ordinal: u8::try_from(DEATH_VIEW_MAX_STATUSES_PER_TRACE_ENTRY).unwrap(),
            status_id: WireText::new("status.one_too_many").unwrap(),
            remaining_ticks: 1,
            stack_count: 1,
        });
        assert_eq!(
            result.validate(),
            Err(DeathViewValidationError::InvalidTrace)
        );
    }

    #[test]
    fn memorial_pages_are_newest_first_with_exact_continuation_cursor() {
        let entry = |time, id| DeathMemorialEntryV1 {
            cursor: DeathMemorialCursorV1 {
                death_at_unix_ms: time,
                death_id: uuid_v7(id),
            },
            summary_revision: 1,
            summary_snapshot_digest: [3; 32],
            presentation_key: WireText::new("memorial.default").unwrap(),
            presentation_digest: [4; 32],
            character_name_snapshot: DeathCharacterName::new("Mara").unwrap(),
            class_id: WireText::new("class.grave_arbalist").unwrap(),
            level: 10,
            echo_outcome: DeathEchoOutcomeV1::Dormant,
            presentation_revision: content_revision(),
        };
        let entries = vec![entry(20, 2), entry(10, 1)];
        assert_eq!(
            validate_memorial_page(2, &entries, Some(entries[1].cursor)),
            Ok(())
        );
        assert_eq!(
            validate_memorial_page(2, &[entries[1].clone(), entries[0].clone()], None),
            Err(DeathViewValidationError::InvalidMemorialPage)
        );
        let tied = vec![entry(20, 1), entry(20, 2)];
        assert_eq!(validate_memorial_page(2, &tied, None), Ok(()));
    }

    #[test]
    fn display_names_are_utf8_byte_bounded_and_control_free() {
        assert_eq!(
            DeathCharacterName::new("Mara Ash").unwrap().as_str(),
            "Mara Ash"
        );
        assert_eq!(
            DeathCharacterName::new("é".repeat(13)),
            Err(DeathViewValidationError::InvalidCharacterName)
        );
        assert_eq!(
            DeathCharacterName::new("Mara\nAsh"),
            Err(DeathViewValidationError::InvalidCharacterName)
        );
    }
}
