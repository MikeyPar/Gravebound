//! Canonical, bounded DTOs for one authoritative durable permadeath commit.
//!
//! This module validates a fully planned server-authored transaction. It deliberately does not
//! select the lethal cause, determine Echo eligibility, or decide what must be destroyed.

use std::{cmp::Ordering, collections::BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{PersistenceError, WIPEABLE_CORE_NAMESPACE};

pub const DURABLE_DEATH_SCHEMA_VERSION: u16 = 3;
pub const DURABLE_DEATH_SUMMARY_REVISION: u16 = 1;
pub const DURABLE_DEATH_CONTRACT: &str = "permadeath-v1";
pub const DURABLE_DEATH_TRACE_WINDOW_TICKS: u64 = 300;
pub const MAX_DURABLE_DEATH_TRACE_ENTRIES: usize = 4_096;
pub const MAX_DURABLE_DEATH_STATUSES_PER_ENTRY: usize = 32;
pub const MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES: usize = 4_096;
pub const MAX_DURABLE_DEATH_PLAN_PAYLOAD_BYTES: usize = 1_048_576;
pub const MAX_DURABLE_DEATH_RESULT_PAYLOAD_BYTES: usize = 65_536;

/// Exact transitive `GB-M03-06D` presentation revision compiled from
/// `content/core_dev/death_view.*`. These values are independent of the world-flow revision kept
/// on the durable event and retained live trace.
pub const CORE_DEATH_VIEW_RECORDS_BLAKE3: &str =
    "349730a1246857978d1412510ee23af46624ec80dbb3333be42aad2e47f1f8e0";
pub const CORE_DEATH_VIEW_ASSETS_BLAKE3: &str =
    "0160f06954c88aba61392f72af66031d6f7ff4a592beb24f7ebe9f1981cc7a68";
pub const CORE_DEATH_VIEW_LOCALIZATION_BLAKE3: &str =
    "c10bcc96887aac7db8c855f19d991e6185f46d1df39f7a37d3a31cb4b9ca1b92";

const PLAN_HASH_CONTEXT: &str = "gravebound.durable-death.plan.v1";
const REQUEST_HASH_CONTEXT: &str = "gravebound.durable-death.request.v1";
const TRACE_HASH_CONTEXT: &str = "gravebound.durable-death.trace.v1";
const DESTRUCTION_HASH_CONTEXT: &str = "gravebound.durable-death.destruction.v1";
const SUMMARY_HASH_CONTEXT: &str = "gravebound.durable-death.summary.v1";
const MEMORIAL_HASH_CONTEXT: &str = "gravebound.durable-death.memorial.v1";
const ECHO_HASH_CONTEXT: &str = "gravebound.durable-death.echo.v1";
const RESULT_HASH_CONTEXT: &str = "gravebound.durable-death.result.v1";
const BARGAIN_CLEANUP_ID_CONTEXT: &str = "gravebound.death.bargain-cleanup-id.v1";
const PRESERVED_PROJECTIONS: [(DurableSummaryProjectionKindV1, &str); 5] = [
    (
        DurableSummaryProjectionKindV1::PreservedAccountRecords,
        "projection.preserved.account_records",
    ),
    (
        DurableSummaryProjectionKindV1::PreservedCurrency,
        "projection.preserved.currency",
    ),
    (
        DurableSummaryProjectionKindV1::PreservedVault,
        "projection.preserved.vault",
    ),
    (
        DurableSummaryProjectionKindV1::PreservedCosmetics,
        "projection.preserved.cosmetics",
    ),
    (
        DurableSummaryProjectionKindV1::PreservedRecipes,
        "projection.preserved.recipes",
    ),
];
const CREATED_PROJECTIONS: [(DurableSummaryProjectionKindV1, &str); 2] = [
    (
        DurableSummaryProjectionKindV1::CreatedMemorial,
        "projection.created.memorial",
    ),
    (
        DurableSummaryProjectionKindV1::CreatedEcho,
        "projection.created.echo",
    ),
];

/// Promoted server content used to validate a planned death independently from player/request
/// material. Item entries are sorted by UTF-8 template ID and include only currently enabled
/// templates. Signature tags are presentation-only Echo authority for equipped Weapon/Relic
/// items; other slots must resolve without a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableDeathContentAuthorityV1 {
    pub content_revision: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
    pub enabled_items: Vec<DurableDeathItemContentAuthorityV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableDeathItemContentAuthorityV1 {
    pub template_id: String,
    pub echo_signature_tag: Option<String>,
}

/// Immutable localization/asset authority for rendering one stored death or Memorial snapshot.
/// It is sealed into the death request separately from the danger world's content authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableDeathPresentationAuthorityV1 {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

impl DurableDeathPresentationAuthorityV1 {
    #[must_use]
    pub fn core() -> Self {
        Self {
            records_blake3: CORE_DEATH_VIEW_RECORDS_BLAKE3.to_owned(),
            assets_blake3: CORE_DEATH_VIEW_ASSETS_BLAKE3.to_owned(),
            localization_blake3: CORE_DEATH_VIEW_LOCALIZATION_BLAKE3.to_owned(),
        }
    }

    pub fn validate(&self) -> Result<(), PersistenceError> {
        if !valid_lower_blake3(&self.records_blake3)
            || !valid_lower_blake3(&self.assets_blake3)
            || !valid_lower_blake3(&self.localization_blake3)
        {
            return Err(PersistenceError::DurableDeathContentMismatch);
        }
        Ok(())
    }
}

impl DurableDeathContentAuthorityV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if !valid_content_revision(&self.content_revision)
            || !valid_lower_blake3(&self.records_blake3)
            || !valid_lower_blake3(&self.assets_blake3)
            || !valid_lower_blake3(&self.localization_blake3)
            || self.enabled_items.len() > MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES
            || self.enabled_items.iter().any(|item| {
                !valid_stable_id(&item.template_id)
                    || !valid_optional_id(item.echo_signature_tag.as_deref())
            })
            || self
                .enabled_items
                .windows(2)
                .any(|pair| pair[0].template_id.as_bytes() >= pair[1].template_id.as_bytes())
        {
            return Err(PersistenceError::DurableDeathContentMismatch);
        }
        Ok(())
    }

    pub fn item(&self, template_id: &str) -> Option<&DurableDeathItemContentAuthorityV1> {
        self.enabled_items
            .binary_search_by(|item| item.template_id.as_bytes().cmp(template_id.as_bytes()))
            .ok()
            .map(|index| &self.enabled_items[index])
    }

    pub fn matches_event(&self, event: &DurableDeathEventV1) -> bool {
        self.content_revision == event.content_revision
            && self.records_blake3 == event.records_blake3
            && self.assets_blake3 == event.assets_blake3
            && self.localization_blake3 == event.localization_blake3
    }
}

/// Derives the canonical life-cleanup outbox identity bound into the death request and receipt.
pub fn derive_durable_death_bargain_cleanup_event_id(
    death_id: [u8; 16],
    mutation_id: [u8; 16],
) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new_derive_key(BARGAIN_CLEANUP_ID_CONTEXT);
    for part in [death_id.as_slice(), mutation_id.as_slice()] {
        hasher.update(&(part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    let mut value = [0_u8; 16];
    value.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    if value == [0; 16] {
        value[15] = 1;
    }
    value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathVersionAdvanceV1 {
    pub pre: u64,
    pub post: u64,
}

impl DeathVersionAdvanceV1 {
    fn valid(self) -> bool {
        self.pre > 0 && self.pre.checked_add(1) == Some(self.post)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathAggregateVersionsV1 {
    pub account: DeathVersionAdvanceV1,
    pub character: DeathVersionAdvanceV1,
    pub progression: DeathVersionAdvanceV1,
    pub inventory: DeathVersionAdvanceV1,
    pub oath_bargain: DeathVersionAdvanceV1,
    pub life_metrics: DeathVersionAdvanceV1,
}

impl DeathAggregateVersionsV1 {
    fn valid(&self) -> bool {
        self.account.valid()
            && self.character.valid()
            && self.progression.valid()
            && self.inventory.valid()
            && self.oath_bargain.valid()
            && self.life_metrics.valid()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableDeathCauseV1 {
    DirectHit,
    DamageOverTime,
    Environment,
    Disconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableDamageTypeV1 {
    Physical,
    Veil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableNetworkStateV1 {
    Connected,
    Degraded,
    LinkLost,
    Reattached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableRecallStateV1 {
    Inactive,
    Channeling,
    CompletionPending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableDeathEventV1 {
    pub schema_version: u16,
    pub namespace_id: String,
    pub death_id: [u8; 16],
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub former_roster_ordinal: u8,
    pub mutation_id: [u8; 16],
    pub bargain_cleanup_event_id: [u8; 16],
    pub canonical_request_hash: [u8; 32],
    pub content_revision: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
    pub presentation: DurableDeathPresentationAuthorityV1,
    pub instance_id: [u8; 16],
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub region_id: String,
    pub room_id: String,
    pub death_tick: u64,
    pub committed_at_unix_ms: u64,
    pub cause: DurableDeathCauseV1,
    pub killer_content_id: String,
    pub killer_pattern_id: Option<String>,
    pub killer_attack_id: String,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub damage_type: DurableDamageTypeV1,
    pub pre_hit_health: u32,
    pub source_x_milli_tiles: i32,
    pub source_y_milli_tiles: i32,
    pub network_state: DurableNetworkStateV1,
    pub recall_state: DurableRecallStateV1,
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub versions: DeathAggregateVersionsV1,
    pub trace_entry_count: u16,
    pub trace_digest: [u8; 32],
    pub destruction_entry_count: u16,
    pub destruction_digest: [u8; 32],
}

impl DurableDeathEventV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        let identities = [
            self.account_id,
            self.character_id,
            self.mutation_id,
            self.bargain_cleanup_event_id,
            self.instance_id,
            self.lineage_id,
            self.restore_point_id,
        ];
        if self.schema_version != DURABLE_DEATH_SCHEMA_VERSION
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || !is_uuid_v7(self.death_id)
            || identities.contains(&[0; 16])
            || self.bargain_cleanup_event_id
                != derive_durable_death_bargain_cleanup_event_id(self.death_id, self.mutation_id)
            || !(1..=2).contains(&self.former_roster_ordinal)
            || is_zero_hash(self.canonical_request_hash)
            || !valid_content_revision(&self.content_revision)
            || !valid_lower_blake3(&self.records_blake3)
            || !valid_lower_blake3(&self.assets_blake3)
            || !valid_lower_blake3(&self.localization_blake3)
            || self.presentation.validate().is_err()
            || !valid_stable_id(&self.region_id)
            || !valid_stable_id(&self.room_id)
            || self.death_tick == 0
            || self.committed_at_unix_ms == 0
            || !valid_stable_id(&self.killer_content_id)
            || self
                .killer_pattern_id
                .as_deref()
                .is_some_and(|value| !valid_stable_id(value))
            || !valid_stable_id(&self.killer_attack_id)
            || self.final_damage == 0
            || self.pre_hit_health == 0
            || self.final_damage < self.pre_hit_health
            || !self.versions.valid()
            || self.trace_entry_count == 0
            || usize::from(self.trace_entry_count) > MAX_DURABLE_DEATH_TRACE_ENTRIES
            || usize::from(self.destruction_entry_count) > MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES
            || is_zero_hash(self.trace_digest)
            || is_zero_hash(self.destruction_digest)
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableTraceStatusV1 {
    pub ordinal: u8,
    pub status_id: String,
    pub remaining_ticks: u32,
    pub stack_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableCombatTraceEntryV1 {
    pub ordinal: u16,
    pub event_tick: u64,
    pub event_ordinal: u32,
    pub source_content_id: String,
    pub source_entity_id: Option<[u8; 16]>,
    pub pattern_id: Option<String>,
    pub attack_id: String,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub damage_type: DurableDamageTypeV1,
    pub pre_health: u32,
    pub post_health: u32,
    pub source_x_milli_tiles: i32,
    pub source_y_milli_tiles: i32,
    pub network_state: DurableNetworkStateV1,
    pub recall_state: DurableRecallStateV1,
    pub lethal: bool,
    pub statuses: Vec<DurableTraceStatusV1>,
}

impl DurableCombatTraceEntryV1 {
    fn validate(&self, expected_ordinal: usize, death_tick: u64) -> bool {
        self.ordinal == u16::try_from(expected_ordinal).unwrap_or(u16::MAX)
            && self.event_tick > 0
            && self.event_tick <= death_tick
            && death_tick.saturating_sub(self.event_tick) <= DURABLE_DEATH_TRACE_WINDOW_TICKS
            && valid_stable_id(&self.source_content_id)
            && self.source_entity_id != Some([0; 16])
            && self.pattern_id.as_deref().is_none_or(valid_stable_id)
            && valid_stable_id(&self.attack_id)
            && self.pre_health > 0
            && self.post_health == self.pre_health.saturating_sub(self.final_damage)
            && self.lethal == (self.post_health == 0)
            && self.statuses.len() <= MAX_DURABLE_DEATH_STATUSES_PER_ENTRY
            && contiguous_unique_statuses(&self.statuses)
    }

    fn matches_lethal_event(&self, event: &DurableDeathEventV1) -> bool {
        self.event_tick == event.death_tick
            && self.source_content_id == event.killer_content_id
            && self.pattern_id == event.killer_pattern_id
            && self.attack_id == event.killer_attack_id
            && self.raw_damage == event.raw_damage
            && self.final_damage == event.final_damage
            && self.damage_type == event.damage_type
            && self.pre_health == event.pre_hit_health
            && self.source_x_milli_tiles == event.source_x_milli_tiles
            && self.source_y_milli_tiles == event.source_y_milli_tiles
            && self.network_state == event.network_state
            && self.recall_state == event.recall_state
            && self.lethal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DurableEquipmentSlotV1 {
    Weapon,
    Relic,
    Armor,
    Charm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableDestructionLocationV1 {
    Equipment {
        slot: DurableEquipmentSlotV1,
    },
    Belt {
        index: u8,
    },
    RunBackpack {
        index: u8,
    },
    PersonalGround {
        instance_id: [u8; 16],
        pickup_id: [u8; 16],
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableDestructionEntryV1 {
    Item {
        ordinal: u16,
        content_id: String,
        item_uid: [u8; 16],
        location: DurableDestructionLocationV1,
        pre_item_version: u64,
        post_item_version: u64,
        ledger_event_id: [u8; 16],
    },
    RunMaterial {
        ordinal: u16,
        material_id: String,
        destroyed_quantity: u32,
        pre_material_quantity: u32,
        pre_material_version: u64,
        post_material_version: u64,
    },
}

impl DurableDestructionEntryV1 {
    pub const fn ordinal(&self) -> u16 {
        match self {
            Self::Item { ordinal, .. } | Self::RunMaterial { ordinal, .. } => *ordinal,
        }
    }

    fn valid(&self) -> bool {
        match self {
            Self::Item {
                content_id,
                item_uid,
                location,
                pre_item_version,
                post_item_version,
                ledger_event_id,
                ..
            } => {
                valid_stable_id(content_id)
                    && *item_uid != [0; 16]
                    && *ledger_event_id != [0; 16]
                    && *pre_item_version > 0
                    && pre_item_version.checked_add(1) == Some(*post_item_version)
                    && match location {
                        DurableDestructionLocationV1::Equipment { .. } => true,
                        DurableDestructionLocationV1::Belt { index } => *index <= 1,
                        DurableDestructionLocationV1::RunBackpack { index } => *index <= 7,
                        DurableDestructionLocationV1::PersonalGround {
                            instance_id,
                            pickup_id,
                        } => *instance_id != [0; 16] && *pickup_id != [0; 16],
                    }
            }
            Self::RunMaterial {
                material_id,
                destroyed_quantity,
                pre_material_quantity,
                pre_material_version,
                post_material_version,
                ..
            } => {
                valid_stable_id(material_id)
                    && *destroyed_quantity > 0
                    && destroyed_quantity == pre_material_quantity
                    && *pre_material_version > 0
                    && pre_material_version.checked_add(1) == Some(*post_material_version)
            }
        }
    }

    fn canonical_cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (
                Self::Item {
                    item_uid: left_uid,
                    location: left,
                    ..
                },
                Self::Item {
                    item_uid: right_uid,
                    location: right,
                    ..
                },
            ) => location_cmp(left, *left_uid, right, *right_uid),
            (Self::Item { .. }, Self::RunMaterial { .. }) => Ordering::Less,
            (Self::RunMaterial { .. }, Self::Item { .. }) => Ordering::Greater,
            (
                Self::RunMaterial {
                    material_id: left, ..
                },
                Self::RunMaterial {
                    material_id: right, ..
                },
            ) => left.as_bytes().cmp(right.as_bytes()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableEchoOutcomeV1 {
    NotEligible,
    Dormant,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableSummaryProjectionKindV1 {
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
pub struct DurableSummaryProjectionEntryV1 {
    pub ordinal: u16,
    pub kind: DurableSummaryProjectionKindV1,
    pub content_id: String,
    pub quantity: u32,
    pub item_uid: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableSummaryProjectionsV1 {
    pub lost: Vec<DurableSummaryProjectionEntryV1>,
    pub preserved: Vec<DurableSummaryProjectionEntryV1>,
    pub created: Vec<DurableSummaryProjectionEntryV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableOrderedContentIdV1 {
    pub ordinal: u16,
    pub content_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableSummaryDamageReferenceV1 {
    pub ordinal: u8,
    pub trace_ordinal: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableDeathSummaryV1 {
    pub schema_version: u16,
    pub namespace_id: String,
    pub death_id: [u8; 16],
    pub summary_revision: u16,
    pub hero_label_key: String,
    pub character_name_snapshot: String,
    pub class_id: String,
    pub level: u8,
    pub oath_id: Option<String>,
    pub bargains: Vec<DurableOrderedContentIdV1>,
    pub lifetime_ms: u64,
    pub final_deed_id: String,
    pub lethal_trace_ordinal: u16,
    pub last_five_damage: Vec<DurableSummaryDamageReferenceV1>,
    pub projections: DurableSummaryProjectionsV1,
    pub echo_outcome: DurableEchoOutcomeV1,
    pub content_revision: String,
    pub snapshot_digest: [u8; 32],
}

impl DurableDeathSummaryV1 {
    pub fn expected_snapshot_digest(&self) -> Result<[u8; 32], PersistenceError> {
        #[derive(Serialize)]
        struct Material<'a> {
            schema_version: u16,
            namespace_id: &'a str,
            death_id: [u8; 16],
            summary_revision: u16,
            hero_label_key: &'a str,
            character_name_snapshot: &'a str,
            class_id: &'a str,
            level: u8,
            oath_id: Option<&'a str>,
            bargains: &'a [DurableOrderedContentIdV1],
            lifetime_ms: u64,
            final_deed_id: &'a str,
            lethal_trace_ordinal: u16,
            last_five_damage: &'a [DurableSummaryDamageReferenceV1],
            projections: &'a DurableSummaryProjectionsV1,
            echo_outcome: DurableEchoOutcomeV1,
            content_revision: &'a str,
        }

        canonical_digest(
            SUMMARY_HASH_CONTEXT,
            &Material {
                schema_version: self.schema_version,
                namespace_id: &self.namespace_id,
                death_id: self.death_id,
                summary_revision: self.summary_revision,
                hero_label_key: &self.hero_label_key,
                character_name_snapshot: &self.character_name_snapshot,
                class_id: &self.class_id,
                level: self.level,
                oath_id: self.oath_id.as_deref(),
                bargains: &self.bargains,
                lifetime_ms: self.lifetime_ms,
                final_deed_id: &self.final_deed_id,
                lethal_trace_ordinal: self.lethal_trace_ordinal,
                last_five_damage: &self.last_five_damage,
                projections: &self.projections,
                echo_outcome: self.echo_outcome,
                content_revision: &self.content_revision,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableMemorialRecordV1 {
    pub schema_version: u16,
    pub namespace_id: String,
    pub death_id: [u8; 16],
    pub account_id: [u8; 16],
    pub death_at_unix_ms: u64,
    pub summary_revision: u16,
    pub summary_snapshot_digest: [u8; 32],
    pub presentation_key: String,
    pub presentation_digest: [u8; 32],
}

impl DurableMemorialRecordV1 {
    pub fn expected_presentation_digest(&self) -> Result<[u8; 32], PersistenceError> {
        #[derive(Serialize)]
        struct Material<'a> {
            schema_version: u16,
            namespace_id: &'a str,
            death_id: [u8; 16],
            account_id: [u8; 16],
            death_at_unix_ms: u64,
            summary_revision: u16,
            summary_snapshot_digest: [u8; 32],
            presentation_key: &'a str,
        }

        canonical_digest(
            MEMORIAL_HASH_CONTEXT,
            &Material {
                schema_version: self.schema_version,
                namespace_id: &self.namespace_id,
                death_id: self.death_id,
                account_id: self.account_id,
                death_at_unix_ms: self.death_at_unix_ms,
                summary_revision: self.summary_revision,
                summary_snapshot_digest: self.summary_snapshot_digest,
                presentation_key: &self.presentation_key,
            },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableEchoStateV1 {
    Dormant,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableEchoTransitionReasonV1 {
    EligibleDeath,
    OldestDormantPromotion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableEchoRecordV1 {
    pub schema_version: u16,
    pub namespace_id: String,
    pub echo_id: [u8; 16],
    pub death_id: [u8; 16],
    pub account_id: [u8; 16],
    pub character_name_snapshot: String,
    pub class_id: String,
    pub oath_id: Option<String>,
    pub level: u8,
    pub appearance_snapshot_id: String,
    pub appearance_theme_id: String,
    pub weapon_signature_tag: Option<String>,
    pub relic_signature_tag: Option<String>,
    pub bargains: Vec<DurableOrderedContentIdV1>,
    pub deed_tags: Vec<DurableOrderedContentIdV1>,
    pub killer_content_id: String,
    pub killer_pattern_id: Option<String>,
    pub death_region_id: String,
    pub power_band: u8,
    pub created_at_unix_ms: u64,
    pub state: DurableEchoStateV1,
    pub content_revision: String,
    pub snapshot_digest: [u8; 32],
}

impl DurableEchoRecordV1 {
    pub fn expected_snapshot_digest(&self) -> Result<[u8; 32], PersistenceError> {
        #[derive(Serialize)]
        struct Material<'a> {
            schema_version: u16,
            namespace_id: &'a str,
            echo_id: [u8; 16],
            death_id: [u8; 16],
            account_id: [u8; 16],
            character_name_snapshot: &'a str,
            class_id: &'a str,
            oath_id: Option<&'a str>,
            level: u8,
            appearance_snapshot_id: &'a str,
            appearance_theme_id: &'a str,
            weapon_signature_tag: Option<&'a str>,
            relic_signature_tag: Option<&'a str>,
            bargains: &'a [DurableOrderedContentIdV1],
            deed_tags: &'a [DurableOrderedContentIdV1],
            killer_content_id: &'a str,
            killer_pattern_id: Option<&'a str>,
            death_region_id: &'a str,
            power_band: u8,
            created_at_unix_ms: u64,
            content_revision: &'a str,
        }

        canonical_digest(
            ECHO_HASH_CONTEXT,
            &Material {
                schema_version: self.schema_version,
                namespace_id: &self.namespace_id,
                echo_id: self.echo_id,
                death_id: self.death_id,
                account_id: self.account_id,
                character_name_snapshot: &self.character_name_snapshot,
                class_id: &self.class_id,
                oath_id: self.oath_id.as_deref(),
                level: self.level,
                appearance_snapshot_id: &self.appearance_snapshot_id,
                appearance_theme_id: &self.appearance_theme_id,
                weapon_signature_tag: self.weapon_signature_tag.as_deref(),
                relic_signature_tag: self.relic_signature_tag.as_deref(),
                bargains: &self.bargains,
                deed_tags: &self.deed_tags,
                killer_content_id: &self.killer_content_id,
                killer_pattern_id: self.killer_pattern_id.as_deref(),
                death_region_id: &self.death_region_id,
                power_band: self.power_band,
                created_at_unix_ms: self.created_at_unix_ms,
                content_revision: &self.content_revision,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableEchoTransitionV1 {
    pub echo_id: [u8; 16],
    pub echo_death_id: [u8; 16],
    pub ordinal: u16,
    pub previous_state: Option<DurableEchoStateV1>,
    pub next_state: DurableEchoStateV1,
    pub reason: DurableEchoTransitionReasonV1,
    pub source_death_id: Option<[u8; 16]>,
    pub trigger_death_id: [u8; 16],
    pub committed_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableEchoEnvelopeV1 {
    pub created: DurableEchoRecordV1,
    pub creation_transition: DurableEchoTransitionV1,
    pub preexisting_available_echo_id: Option<[u8; 16]>,
    pub promotion: Option<DurableEchoTransitionV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoritativeDeathPlanV1 {
    pub schema_version: u16,
    pub event: DurableDeathEventV1,
    pub trace: Vec<DurableCombatTraceEntryV1>,
    pub destruction: Vec<DurableDestructionEntryV1>,
    pub summary: DurableDeathSummaryV1,
    pub memorial: DurableMemorialRecordV1,
    pub echo: Option<DurableEchoEnvelopeV1>,
}

impl AuthoritativeDeathPlanV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        self.event.validate()?;
        if self.schema_version != DURABLE_DEATH_SCHEMA_VERSION
            || self.trace.len() != usize::from(self.event.trace_entry_count)
            || self.destruction.len() != usize::from(self.event.destruction_entry_count)
            || self.trace.len() > MAX_DURABLE_DEATH_TRACE_ENTRIES
            || self.destruction.len() > MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES
        {
            return Err(corrupt());
        }
        validate_trace(&self.event, &self.trace)?;
        validate_destruction(&self.destruction)?;
        if self.event.trace_digest != canonical_digest(TRACE_HASH_CONTEXT, &self.trace)?
            || self.event.destruction_digest
                != canonical_digest(DESTRUCTION_HASH_CONTEXT, &self.destruction)?
        {
            return Err(corrupt());
        }
        validate_summary(self)?;
        validate_memorial(self)?;
        validate_echo(self)?;
        let payload = postcard::to_stdvec(self).map_err(|_| corrupt())?;
        if payload.is_empty() || payload.len() > MAX_DURABLE_DEATH_PLAN_PAYLOAD_BYTES {
            return Err(corrupt());
        }
        Ok(())
    }

    pub fn canonical_plan_hash(&self) -> Result<[u8; 32], PersistenceError> {
        self.validate()?;
        // PostgreSQL authors commit time inside the serializable transaction. Normalize that
        // timestamp and the two digests derived from it so intent identity remains stable when the
        // repository binds the exact transaction timestamp. The event's request back-reference is
        // likewise excluded to avoid a hash cycle.
        let mut material = self.clone();
        material.event.canonical_request_hash = [0; 32];
        material.clear_commit_authority();
        canonical_digest(PLAN_HASH_CONTEXT, &material)
    }

    fn clear_commit_authority(&mut self) {
        self.event.committed_at_unix_ms = 0;
        self.memorial.death_at_unix_ms = 0;
        self.memorial.presentation_digest = [0; 32];
        if let Some(echo) = &mut self.echo {
            echo.created.created_at_unix_ms = 0;
            echo.created.snapshot_digest = [0; 32];
            echo.creation_transition.committed_at_unix_ms = 0;
            if let Some(promotion) = &mut echo.promotion {
                promotion.committed_at_unix_ms = 0;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableDeathCommitRequestV1 {
    pub schema_version: u16,
    pub contract: String,
    pub mutation_id: [u8; 16],
    pub issued_at_unix_ms: u64,
    pub canonical_plan_hash: [u8; 32],
    pub canonical_request_hash: [u8; 32],
    pub plan: AuthoritativeDeathPlanV1,
}

impl DurableDeathCommitRequestV1 {
    pub fn seal(
        mut plan: AuthoritativeDeathPlanV1,
        issued_at_unix_ms: u64,
    ) -> Result<Self, PersistenceError> {
        // A temporary nonzero back-reference lets the otherwise complete event validate while the
        // enclosing request hash is derived. The final value is replaced and revalidated below.
        plan.event.canonical_request_hash = [1; 32];
        let canonical_plan_hash = plan.canonical_plan_hash()?;
        let mut request = Self {
            schema_version: DURABLE_DEATH_SCHEMA_VERSION,
            contract: DURABLE_DEATH_CONTRACT.into(),
            mutation_id: plan.event.mutation_id,
            issued_at_unix_ms,
            canonical_plan_hash,
            canonical_request_hash: [0; 32],
            plan,
        };
        request.canonical_request_hash = request.expected_request_hash()?;
        request.plan.event.canonical_request_hash = request.canonical_request_hash;
        request.validate()?;
        Ok(request)
    }

    pub fn expected_request_hash(&self) -> Result<[u8; 32], PersistenceError> {
        #[derive(Serialize)]
        struct Material<'a> {
            schema_version: u16,
            contract: &'a str,
            namespace_id: &'a str,
            account_id: [u8; 16],
            character_id: [u8; 16],
            death_id: [u8; 16],
            mutation_id: [u8; 16],
            issued_at_unix_ms: u64,
            canonical_plan_hash: [u8; 32],
        }

        canonical_digest(
            REQUEST_HASH_CONTEXT,
            &Material {
                schema_version: self.schema_version,
                contract: &self.contract,
                namespace_id: &self.plan.event.namespace_id,
                account_id: self.plan.event.account_id,
                character_id: self.plan.event.character_id,
                death_id: self.plan.event.death_id,
                mutation_id: self.mutation_id,
                issued_at_unix_ms: self.issued_at_unix_ms,
                canonical_plan_hash: self.canonical_plan_hash,
            },
        )
    }

    /// Rebinds every database-authored timestamp to one `PostgreSQL` transaction instant without
    /// changing canonical request/plan identity. Timestamp-derived snapshot digests are rebuilt.
    pub fn bind_commit_time(&mut self, committed_at_unix_ms: u64) -> Result<(), PersistenceError> {
        if committed_at_unix_ms == 0 || committed_at_unix_ms < self.issued_at_unix_ms {
            return Err(corrupt());
        }
        let original_plan_hash = self.canonical_plan_hash;
        let original_request_hash = self.canonical_request_hash;
        self.plan.event.committed_at_unix_ms = committed_at_unix_ms;
        self.plan.memorial.death_at_unix_ms = committed_at_unix_ms;
        if let Some(echo) = &mut self.plan.echo {
            echo.created.created_at_unix_ms = committed_at_unix_ms;
            echo.creation_transition.committed_at_unix_ms = committed_at_unix_ms;
            if let Some(promotion) = &mut echo.promotion {
                promotion.committed_at_unix_ms = committed_at_unix_ms;
            }
            echo.created.snapshot_digest = echo.created.expected_snapshot_digest()?;
        }
        self.plan.memorial.presentation_digest =
            self.plan.memorial.expected_presentation_digest()?;
        if self.plan.canonical_plan_hash()? != original_plan_hash
            || self.expected_request_hash()? != original_request_hash
        {
            return Err(corrupt());
        }
        self.validate()
    }

    pub fn validate(&self) -> Result<(), PersistenceError> {
        self.plan.validate()?;
        if self.schema_version != DURABLE_DEATH_SCHEMA_VERSION
            || self.contract != DURABLE_DEATH_CONTRACT
            || self.mutation_id == [0; 16]
            || self.mutation_id != self.plan.event.mutation_id
            || self.canonical_request_hash != self.plan.event.canonical_request_hash
            || self.issued_at_unix_ms == 0
            || self.issued_at_unix_ms > self.plan.event.committed_at_unix_ms
            || self.canonical_plan_hash != self.plan.canonical_plan_hash()?
            || self.canonical_request_hash != self.expected_request_hash()?
        {
            return Err(corrupt());
        }
        let payload = postcard::to_stdvec(self).map_err(|_| corrupt())?;
        if payload.is_empty() || payload.len() > MAX_DURABLE_DEATH_PLAN_PAYLOAD_BYTES {
            return Err(corrupt());
        }
        Ok(())
    }

    pub fn payload(&self) -> Result<Vec<u8>, PersistenceError> {
        self.validate()?;
        postcard::to_stdvec(self).map_err(|_| corrupt())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, PersistenceError> {
        decode_canonical(bytes, MAX_DURABLE_DEATH_PLAN_PAYLOAD_BYTES, Self::validate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurableDeathResultCodeV1 {
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCommittedDeathResultV1 {
    pub schema_version: u16,
    pub contract: String,
    pub namespace_id: String,
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub mutation_id: [u8; 16],
    pub death_id: [u8; 16],
    pub canonical_request_hash: [u8; 32],
    pub canonical_plan_hash: [u8; 32],
    pub result_code: DurableDeathResultCodeV1,
    pub issued_at_unix_ms: u64,
    pub committed_at_unix_ms: u64,
    pub versions: DeathAggregateVersionsV1,
    pub trace_digest: [u8; 32],
    pub destruction_digest: [u8; 32],
    pub summary_digest: [u8; 32],
    pub memorial_digest: [u8; 32],
    pub echo_outcome: DurableEchoOutcomeV1,
    pub created_echo_id: Option<[u8; 16]>,
    pub promoted_echo_id: Option<[u8; 16]>,
}

impl StoredCommittedDeathResultV1 {
    pub fn from_request(request: &DurableDeathCommitRequestV1) -> Result<Self, PersistenceError> {
        request.validate()?;
        let plan = &request.plan;
        let (created_echo_id, promoted_echo_id) = plan.echo.as_ref().map_or((None, None), |echo| {
            (
                Some(echo.created.echo_id),
                echo.promotion.as_ref().map(|value| value.echo_id),
            )
        });
        let result = Self {
            schema_version: DURABLE_DEATH_SCHEMA_VERSION,
            contract: DURABLE_DEATH_CONTRACT.into(),
            namespace_id: plan.event.namespace_id.clone(),
            account_id: plan.event.account_id,
            character_id: plan.event.character_id,
            mutation_id: request.mutation_id,
            death_id: plan.event.death_id,
            canonical_request_hash: request.canonical_request_hash,
            canonical_plan_hash: request.canonical_plan_hash,
            result_code: DurableDeathResultCodeV1::Committed,
            issued_at_unix_ms: request.issued_at_unix_ms,
            committed_at_unix_ms: plan.event.committed_at_unix_ms,
            versions: plan.event.versions.clone(),
            trace_digest: plan.event.trace_digest,
            destruction_digest: plan.event.destruction_digest,
            summary_digest: plan.summary.snapshot_digest,
            memorial_digest: plan.memorial.presentation_digest,
            echo_outcome: plan.summary.echo_outcome,
            created_echo_id,
            promoted_echo_id,
        };
        result.validate_against(request)?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.schema_version != DURABLE_DEATH_SCHEMA_VERSION
            || self.contract != DURABLE_DEATH_CONTRACT
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || [self.account_id, self.character_id, self.mutation_id].contains(&[0; 16])
            || !is_uuid_v7(self.death_id)
            || is_zero_hash(self.canonical_request_hash)
            || is_zero_hash(self.canonical_plan_hash)
            || self.issued_at_unix_ms == 0
            || self.committed_at_unix_ms < self.issued_at_unix_ms
            || !self.versions.valid()
            || [
                self.trace_digest,
                self.destruction_digest,
                self.summary_digest,
                self.memorial_digest,
            ]
            .into_iter()
            .any(is_zero_hash)
            || self.created_echo_id.is_some_and(|value| !is_uuid_v7(value))
            || self
                .promoted_echo_id
                .is_some_and(|value| !is_uuid_v7(value))
            || match self.echo_outcome {
                DurableEchoOutcomeV1::NotEligible => {
                    self.created_echo_id.is_some() || self.promoted_echo_id.is_some()
                }
                DurableEchoOutcomeV1::Dormant => {
                    self.created_echo_id.is_none() || self.created_echo_id == self.promoted_echo_id
                }
                DurableEchoOutcomeV1::Available => {
                    self.created_echo_id.is_none() || self.created_echo_id != self.promoted_echo_id
                }
            }
        {
            return Err(corrupt());
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        request: &DurableDeathCommitRequestV1,
    ) -> Result<(), PersistenceError> {
        self.validate()?;
        request.validate()?;
        let expected = Self::from_validated_request(request);
        if self != &expected {
            return Err(corrupt());
        }
        Ok(())
    }

    fn from_validated_request(request: &DurableDeathCommitRequestV1) -> Self {
        let plan = &request.plan;
        let (created_echo_id, promoted_echo_id) = plan.echo.as_ref().map_or((None, None), |echo| {
            (
                Some(echo.created.echo_id),
                echo.promotion.as_ref().map(|value| value.echo_id),
            )
        });
        Self {
            schema_version: DURABLE_DEATH_SCHEMA_VERSION,
            contract: DURABLE_DEATH_CONTRACT.into(),
            namespace_id: plan.event.namespace_id.clone(),
            account_id: plan.event.account_id,
            character_id: plan.event.character_id,
            mutation_id: request.mutation_id,
            death_id: plan.event.death_id,
            canonical_request_hash: request.canonical_request_hash,
            canonical_plan_hash: request.canonical_plan_hash,
            result_code: DurableDeathResultCodeV1::Committed,
            issued_at_unix_ms: request.issued_at_unix_ms,
            committed_at_unix_ms: plan.event.committed_at_unix_ms,
            versions: plan.event.versions.clone(),
            trace_digest: plan.event.trace_digest,
            destruction_digest: plan.event.destruction_digest,
            summary_digest: plan.summary.snapshot_digest,
            memorial_digest: plan.memorial.presentation_digest,
            echo_outcome: plan.summary.echo_outcome,
            created_echo_id,
            promoted_echo_id,
        }
    }

    pub fn payload(&self) -> Result<Vec<u8>, PersistenceError> {
        self.validate()?;
        let payload = postcard::to_stdvec(self).map_err(|_| corrupt())?;
        if payload.is_empty() || payload.len() > MAX_DURABLE_DEATH_RESULT_PAYLOAD_BYTES {
            return Err(corrupt());
        }
        Ok(payload)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, PersistenceError> {
        decode_canonical(
            bytes,
            MAX_DURABLE_DEATH_RESULT_PAYLOAD_BYTES,
            Self::validate,
        )
    }

    pub fn digest(&self) -> Result<[u8; 32], PersistenceError> {
        Ok(blake3::derive_key(RESULT_HASH_CONTEXT, &self.payload()?))
    }
}

fn validate_trace(
    event: &DurableDeathEventV1,
    trace: &[DurableCombatTraceEntryV1],
) -> Result<(), PersistenceError> {
    if trace.is_empty() || trace.len() > MAX_DURABLE_DEATH_TRACE_ENTRIES {
        return Err(corrupt());
    }
    let mut previous_order = None;
    for (index, entry) in trace.iter().enumerate() {
        let order = (entry.event_tick, entry.event_ordinal);
        if !entry.validate(index, event.death_tick)
            || previous_order.is_some_and(|previous| previous >= order)
            || (entry.lethal && index + 1 != trace.len())
        {
            return Err(corrupt());
        }
        previous_order = Some(order);
    }
    if trace
        .last()
        .is_none_or(|entry| !entry.matches_lethal_event(event))
    {
        return Err(corrupt());
    }
    Ok(())
}

fn validate_destruction(entries: &[DurableDestructionEntryV1]) -> Result<(), PersistenceError> {
    if entries.len() > MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES {
        return Err(corrupt());
    }
    let mut item_ids = BTreeSet::new();
    let mut ledger_ids = BTreeSet::new();
    let mut material_ids = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.ordinal() != u16::try_from(index).unwrap_or(u16::MAX)
            || !entry.valid()
            || index > 0 && entries[index - 1].canonical_cmp(entry) != Ordering::Less
        {
            return Err(corrupt());
        }
        match entry {
            DurableDestructionEntryV1::Item {
                item_uid,
                ledger_event_id,
                ..
            } => {
                if !item_ids.insert(*item_uid) || !ledger_ids.insert(*ledger_event_id) {
                    return Err(corrupt());
                }
            }
            DurableDestructionEntryV1::RunMaterial { material_id, .. } => {
                if !material_ids.insert(material_id.as_bytes()) {
                    return Err(corrupt());
                }
            }
        }
    }
    Ok(())
}

fn validate_summary(plan: &AuthoritativeDeathPlanV1) -> Result<(), PersistenceError> {
    let summary = &plan.summary;
    let expected_lifetime_ms = plan
        .event
        .lifetime_ticks
        .checked_mul(1_000)
        .map(|value| value / 30)
        .ok_or_else(corrupt)?;
    if summary.schema_version != DURABLE_DEATH_SCHEMA_VERSION
        || summary.namespace_id != plan.event.namespace_id
        || summary.death_id != plan.event.death_id
        || summary.summary_revision != DURABLE_DEATH_SUMMARY_REVISION
        || !valid_stable_id(&summary.hero_label_key)
        || !valid_utf8_bytes(&summary.character_name_snapshot, 1, 24)
        || !valid_stable_id(&summary.class_id)
        || !(1..=10).contains(&summary.level)
        || summary
            .oath_id
            .as_deref()
            .is_some_and(|value| !valid_stable_id(value))
        || !contiguous_unique_content(&summary.bargains, 3)
        || summary.lifetime_ms != expected_lifetime_ms
        || !valid_stable_id(&summary.final_deed_id)
        || summary.content_revision != plan.event.content_revision
        || summary.snapshot_digest != summary.expected_snapshot_digest()?
    {
        return Err(corrupt());
    }

    let last_trace_ordinal = plan
        .trace
        .last()
        .map(|entry| entry.ordinal)
        .ok_or_else(corrupt)?;
    let timeline_start = plan.trace.len().saturating_sub(5);
    if summary.lethal_trace_ordinal != last_trace_ordinal
        || summary.last_five_damage.len() != plan.trace.len().min(5)
        || summary
            .last_five_damage
            .iter()
            .enumerate()
            .any(|(index, reference)| {
                reference.ordinal != u8::try_from(index).unwrap_or(u8::MAX)
                    || reference.trace_ordinal != plan.trace[timeline_start + index].ordinal
            })
        || !valid_summary_projections(&summary.projections, &plan.destruction)
    {
        return Err(corrupt());
    }
    Ok(())
}

fn validate_memorial(plan: &AuthoritativeDeathPlanV1) -> Result<(), PersistenceError> {
    let memorial = &plan.memorial;
    if memorial.schema_version != DURABLE_DEATH_SCHEMA_VERSION
        || memorial.namespace_id != plan.event.namespace_id
        || memorial.death_id != plan.event.death_id
        || memorial.account_id != plan.event.account_id
        || memorial.death_at_unix_ms != plan.event.committed_at_unix_ms
        || memorial.summary_revision != plan.summary.summary_revision
        || memorial.summary_snapshot_digest != plan.summary.snapshot_digest
        || !valid_stable_id(&memorial.presentation_key)
        || memorial.presentation_digest != memorial.expected_presentation_digest()?
    {
        return Err(corrupt());
    }
    Ok(())
}

fn validate_echo(plan: &AuthoritativeDeathPlanV1) -> Result<(), PersistenceError> {
    let Some(envelope) = &plan.echo else {
        return if plan.summary.echo_outcome == DurableEchoOutcomeV1::NotEligible {
            Ok(())
        } else {
            Err(corrupt())
        };
    };
    let echo = &envelope.created;
    if echo.schema_version != DURABLE_DEATH_SCHEMA_VERSION
        || echo.namespace_id != plan.event.namespace_id
        || !is_uuid_v7(echo.echo_id)
        || echo.death_id != plan.event.death_id
        || echo.account_id != plan.event.account_id
        || echo.character_name_snapshot != plan.summary.character_name_snapshot
        || echo.class_id != plan.summary.class_id
        || echo.oath_id != plan.summary.oath_id
        || echo.level != 10
        || echo.class_id != "class.grave_arbalist"
        || echo.appearance_snapshot_id != "appearance.default.grave_arbalist"
        || echo.appearance_theme_id != "theme.echo.arbalist_ash"
        || !valid_optional_id(echo.weapon_signature_tag.as_deref())
        || !valid_optional_id(echo.relic_signature_tag.as_deref())
        || echo.bargains != plan.summary.bargains
        || !contiguous_unique_content(&echo.deed_tags, 32)
        || echo.killer_content_id != plan.event.killer_content_id
        || echo.killer_pattern_id != plan.event.killer_pattern_id
        || echo.death_region_id != plan.event.region_id
        || !(1..=5).contains(&echo.power_band)
        || echo.created_at_unix_ms != plan.event.committed_at_unix_ms
        || echo.content_revision != plan.event.content_revision
        || echo.snapshot_digest != echo.expected_snapshot_digest()?
        || !valid_creation_transition(&envelope.creation_transition, echo, plan)
        || !valid_projector_prestate(envelope, echo)
        || !valid_promotion(envelope.promotion.as_ref(), echo, plan)
    {
        return Err(corrupt());
    }
    let expected_outcome = if envelope
        .promotion
        .as_ref()
        .is_some_and(|promotion| promotion.echo_id == echo.echo_id)
    {
        DurableEchoOutcomeV1::Available
    } else {
        DurableEchoOutcomeV1::Dormant
    };
    let expected_state = if expected_outcome == DurableEchoOutcomeV1::Available {
        DurableEchoStateV1::Available
    } else {
        DurableEchoStateV1::Dormant
    };
    if echo.state != expected_state || plan.summary.echo_outcome != expected_outcome {
        return Err(corrupt());
    }
    Ok(())
}

fn valid_projector_prestate(
    envelope: &DurableEchoEnvelopeV1,
    created: &DurableEchoRecordV1,
) -> bool {
    match (
        envelope.preexisting_available_echo_id,
        envelope.promotion.as_ref(),
    ) {
        (Some(available_echo_id), None) => {
            is_uuid_v7(available_echo_id) && available_echo_id != created.echo_id
        }
        (None, Some(_)) => true,
        (Some(_), Some(_)) | (None, None) => false,
    }
}

fn valid_creation_transition(
    transition: &DurableEchoTransitionV1,
    echo: &DurableEchoRecordV1,
    plan: &AuthoritativeDeathPlanV1,
) -> bool {
    transition.echo_id == echo.echo_id
        && transition.echo_death_id == echo.death_id
        && transition.ordinal == 0
        && transition.previous_state.is_none()
        && transition.next_state == DurableEchoStateV1::Dormant
        && transition.reason == DurableEchoTransitionReasonV1::EligibleDeath
        && transition.source_death_id == Some(plan.event.death_id)
        && transition.trigger_death_id == plan.event.death_id
        && transition.committed_at_unix_ms == plan.event.committed_at_unix_ms
}

fn valid_promotion(
    promotion: Option<&DurableEchoTransitionV1>,
    created: &DurableEchoRecordV1,
    plan: &AuthoritativeDeathPlanV1,
) -> bool {
    promotion.is_none_or(|transition| {
        is_uuid_v7(transition.echo_id)
            && is_uuid_v7(transition.echo_death_id)
            && transition.ordinal > 0
            && transition.previous_state == Some(DurableEchoStateV1::Dormant)
            && transition.next_state == DurableEchoStateV1::Available
            && transition.reason == DurableEchoTransitionReasonV1::OldestDormantPromotion
            && transition.source_death_id.is_none()
            && transition.trigger_death_id == plan.event.death_id
            && transition.committed_at_unix_ms == plan.event.committed_at_unix_ms
            && (transition.echo_id != created.echo_id
                || (transition.echo_death_id == created.death_id && transition.ordinal == 1))
            && (transition.echo_id == created.echo_id
                || transition.echo_death_id != created.death_id)
    })
}

fn valid_summary_projections(
    projections: &DurableSummaryProjectionsV1,
    destruction: &[DurableDestructionEntryV1],
) -> bool {
    if projections.lost.len() != destruction.len()
        || projections
            .lost
            .iter()
            .enumerate()
            .any(|(index, projection)| {
                !projection_matches_loss(index, projection, &destruction[index])
            })
    {
        return false;
    }
    exact_fixed_projections(&projections.preserved, &PRESERVED_PROJECTIONS)
        && exact_fixed_projections(&projections.created, &CREATED_PROJECTIONS)
}

fn projection_matches_loss(
    index: usize,
    projection: &DurableSummaryProjectionEntryV1,
    destruction: &DurableDestructionEntryV1,
) -> bool {
    if projection.ordinal != u16::try_from(index).unwrap_or(u16::MAX) {
        return false;
    }
    match destruction {
        DurableDestructionEntryV1::Item {
            content_id,
            item_uid,
            ..
        } => {
            projection.kind == DurableSummaryProjectionKindV1::LostItem
                && projection.content_id == *content_id
                && projection.quantity == 1
                && projection.item_uid == Some(*item_uid)
        }
        DurableDestructionEntryV1::RunMaterial {
            material_id,
            destroyed_quantity,
            ..
        } => {
            projection.kind == DurableSummaryProjectionKindV1::LostRunMaterial
                && projection.content_id == *material_id
                && projection.quantity == *destroyed_quantity
                && projection.item_uid.is_none()
        }
    }
}

fn exact_fixed_projections(
    actual: &[DurableSummaryProjectionEntryV1],
    expected: &[(DurableSummaryProjectionKindV1, &str)],
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .enumerate()
            .all(|(index, (entry, (kind, content_id)))| {
                entry.ordinal == u16::try_from(index).unwrap_or(u16::MAX)
                    && entry.kind == *kind
                    && entry.content_id == *content_id
                    && entry.quantity == 1
                    && entry.item_uid.is_none()
            })
}

fn contiguous_unique_statuses(statuses: &[DurableTraceStatusV1]) -> bool {
    let mut ids = BTreeSet::new();
    statuses.iter().enumerate().all(|(index, status)| {
        status.ordinal == u8::try_from(index).unwrap_or(u8::MAX)
            && valid_stable_id(&status.status_id)
            && status.remaining_ticks <= 108_000
            && (1..=255).contains(&status.stack_count)
            && ids.insert(status.status_id.as_bytes())
    })
}

fn contiguous_unique_content(values: &[DurableOrderedContentIdV1], max: usize) -> bool {
    let mut ids = BTreeSet::new();
    values.len() <= max
        && values.iter().enumerate().all(|(index, value)| {
            value.ordinal == u16::try_from(index).unwrap_or(u16::MAX)
                && valid_stable_id(&value.content_id)
                && ids.insert(value.content_id.as_bytes())
        })
}

fn location_cmp(
    left: &DurableDestructionLocationV1,
    left_uid: [u8; 16],
    right: &DurableDestructionLocationV1,
    right_uid: [u8; 16],
) -> Ordering {
    match (left, right) {
        (
            DurableDestructionLocationV1::Equipment { slot: left },
            DurableDestructionLocationV1::Equipment { slot: right },
        ) => left.cmp(right),
        (DurableDestructionLocationV1::Equipment { .. }, _) => Ordering::Less,
        (_, DurableDestructionLocationV1::Equipment { .. }) => Ordering::Greater,
        (
            DurableDestructionLocationV1::Belt { index: left },
            DurableDestructionLocationV1::Belt { index: right },
        ) => (*left, left_uid).cmp(&(*right, right_uid)),
        (DurableDestructionLocationV1::Belt { .. }, _) => Ordering::Less,
        (_, DurableDestructionLocationV1::Belt { .. }) => Ordering::Greater,
        (
            DurableDestructionLocationV1::RunBackpack { index: left },
            DurableDestructionLocationV1::RunBackpack { index: right },
        ) => left.cmp(right),
        (DurableDestructionLocationV1::RunBackpack { .. }, _) => Ordering::Less,
        (_, DurableDestructionLocationV1::RunBackpack { .. }) => Ordering::Greater,
        (
            DurableDestructionLocationV1::PersonalGround {
                instance_id: left_instance,
                pickup_id: left_pickup,
            },
            DurableDestructionLocationV1::PersonalGround {
                instance_id: right_instance,
                pickup_id: right_pickup,
            },
        ) => (*left_instance, *left_pickup, left_uid).cmp(&(
            *right_instance,
            *right_pickup,
            right_uid,
        )),
    }
}

fn decode_canonical<T>(
    bytes: &[u8],
    max_bytes: usize,
    validate: impl FnOnce(&T) -> Result<(), PersistenceError>,
) -> Result<T, PersistenceError>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(corrupt());
    }
    let value = postcard::from_bytes(bytes).map_err(|_| corrupt())?;
    validate(&value)?;
    if postcard::to_stdvec(&value).map_err(|_| corrupt())? != bytes {
        return Err(corrupt());
    }
    Ok(value)
}

fn canonical_digest<T: Serialize>(context: &str, value: &T) -> Result<[u8; 32], PersistenceError> {
    let bytes = postcard::to_stdvec(value).map_err(|_| corrupt())?;
    Ok(blake3::derive_key(context, &bytes))
}

fn valid_content_revision(value: &str) -> bool {
    const PREFIX: &str = "core-dev.blake3.";
    value.len() == PREFIX.len() + 64
        && value.starts_with(PREFIX)
        && value[PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_lower_blake3(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_stable_id(value: &str) -> bool {
    valid_utf8_bytes(value, 3, 96)
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'
                        || byte == b'-'
                })
        })
}

fn valid_optional_id(value: Option<&str>) -> bool {
    value.is_none_or(valid_stable_id)
}

fn valid_utf8_bytes(value: &str, min: usize, max: usize) -> bool {
    // `str::len` is the encoded UTF-8 byte length, not the Unicode scalar count.
    (min..=max).contains(&value.len()) && !value.chars().any(char::is_control)
}

fn is_uuid_v7(value: [u8; 16]) -> bool {
    value != [0; 16] && value[6] >> 4 == 7 && value[8] & 0b1100_0000 == 0b1000_0000
}

fn is_zero_hash(value: [u8; 32]) -> bool {
    value == [0; 32]
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredDurableDeath
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn uuid_v7(seed: u8) -> [u8; 16] {
        let mut value = [seed; 16];
        value[6] = 0x70 | (seed & 0x0f);
        value[8] = 0x80 | (seed & 0x3f);
        value
    }

    fn versions() -> DeathAggregateVersionsV1 {
        let advance = DeathVersionAdvanceV1 { pre: 4, post: 5 };
        DeathAggregateVersionsV1 {
            account: advance,
            character: advance,
            progression: advance,
            inventory: advance,
            oath_bargain: advance,
            life_metrics: advance,
        }
    }

    fn fixed_projection(
        ordinal: u16,
        kind: DurableSummaryProjectionKindV1,
        content_id: &str,
    ) -> DurableSummaryProjectionEntryV1 {
        DurableSummaryProjectionEntryV1 {
            ordinal,
            kind,
            content_id: content_id.into(),
            quantity: 1,
            item_uid: None,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn valid_request() -> DurableDeathCommitRequestV1 {
        let death_id = uuid_v7(7);
        let account_id = [1; 16];
        let character_id = [2; 16];
        let mutation_id = [3; 16];
        let content_revision = format!("core-dev.blake3.{}", "a".repeat(64));
        let trace = vec![
            DurableCombatTraceEntryV1 {
                ordinal: 0,
                event_tick: 999,
                event_ordinal: 0,
                source_content_id: "enemy.warden".into(),
                source_entity_id: Some([8; 16]),
                pattern_id: Some("pattern.warden.arc".into()),
                attack_id: "attack.warden.arc".into(),
                raw_damage: 12,
                final_damage: 10,
                damage_type: DurableDamageTypeV1::Physical,
                pre_health: 20,
                post_health: 10,
                source_x_milli_tiles: 1_000,
                source_y_milli_tiles: -2_000,
                network_state: DurableNetworkStateV1::Connected,
                recall_state: DurableRecallStateV1::Inactive,
                lethal: false,
                statuses: vec![DurableTraceStatusV1 {
                    ordinal: 0,
                    status_id: "status.hex".into(),
                    remaining_ticks: 30,
                    stack_count: 1,
                }],
            },
            DurableCombatTraceEntryV1 {
                ordinal: 1,
                event_tick: 1_000,
                event_ordinal: 0,
                source_content_id: "enemy.warden".into(),
                source_entity_id: Some([8; 16]),
                pattern_id: Some("pattern.warden.arc".into()),
                attack_id: "attack.warden.arc".into(),
                raw_damage: 20,
                final_damage: 10,
                damage_type: DurableDamageTypeV1::Physical,
                pre_health: 10,
                post_health: 0,
                source_x_milli_tiles: 1_000,
                source_y_milli_tiles: -2_000,
                network_state: DurableNetworkStateV1::Connected,
                recall_state: DurableRecallStateV1::Inactive,
                lethal: true,
                statuses: vec![],
            },
        ];
        let destruction = vec![DurableDestructionEntryV1::Item {
            ordinal: 0,
            content_id: "item.warden_blade".into(),
            item_uid: [9; 16],
            location: DurableDestructionLocationV1::Equipment {
                slot: DurableEquipmentSlotV1::Weapon,
            },
            pre_item_version: 2,
            post_item_version: 3,
            ledger_event_id: [10; 16],
        }];
        let mut event = DurableDeathEventV1 {
            schema_version: DURABLE_DEATH_SCHEMA_VERSION,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            death_id,
            account_id,
            character_id,
            former_roster_ordinal: 1,
            mutation_id,
            bargain_cleanup_event_id: derive_durable_death_bargain_cleanup_event_id(
                death_id,
                mutation_id,
            ),
            canonical_request_hash: [1; 32],
            content_revision: content_revision.clone(),
            records_blake3: "b".repeat(64),
            assets_blake3: "c".repeat(64),
            localization_blake3: "d".repeat(64),
            presentation: DurableDeathPresentationAuthorityV1::core(),
            instance_id: [4; 16],
            lineage_id: [5; 16],
            restore_point_id: [6; 16],
            region_id: "region.core".into(),
            room_id: "room.boss".into(),
            death_tick: 1_000,
            committed_at_unix_ms: 2_000,
            cause: DurableDeathCauseV1::DirectHit,
            killer_content_id: "enemy.warden".into(),
            killer_pattern_id: Some("pattern.warden.arc".into()),
            killer_attack_id: "attack.warden.arc".into(),
            raw_damage: 20,
            final_damage: 10,
            damage_type: DurableDamageTypeV1::Physical,
            pre_hit_health: 10,
            source_x_milli_tiles: 1_000,
            source_y_milli_tiles: -2_000,
            network_state: DurableNetworkStateV1::Connected,
            recall_state: DurableRecallStateV1::Inactive,
            lifetime_ticks: 18_000,
            permadeath_combat_ticks: 18_000,
            versions: versions(),
            trace_entry_count: 2,
            trace_digest: [0; 32],
            destruction_entry_count: 1,
            destruction_digest: [0; 32],
        };
        event.trace_digest = canonical_digest(TRACE_HASH_CONTEXT, &trace).unwrap();
        event.destruction_digest =
            canonical_digest(DESTRUCTION_HASH_CONTEXT, &destruction).unwrap();
        let projections = DurableSummaryProjectionsV1 {
            lost: vec![DurableSummaryProjectionEntryV1 {
                ordinal: 0,
                kind: DurableSummaryProjectionKindV1::LostItem,
                content_id: "item.warden_blade".into(),
                quantity: 1,
                item_uid: Some([9; 16]),
            }],
            preserved: vec![
                fixed_projection(
                    0,
                    DurableSummaryProjectionKindV1::PreservedAccountRecords,
                    "projection.preserved.account_records",
                ),
                fixed_projection(
                    1,
                    DurableSummaryProjectionKindV1::PreservedCurrency,
                    "projection.preserved.currency",
                ),
                fixed_projection(
                    2,
                    DurableSummaryProjectionKindV1::PreservedVault,
                    "projection.preserved.vault",
                ),
                fixed_projection(
                    3,
                    DurableSummaryProjectionKindV1::PreservedCosmetics,
                    "projection.preserved.cosmetics",
                ),
                fixed_projection(
                    4,
                    DurableSummaryProjectionKindV1::PreservedRecipes,
                    "projection.preserved.recipes",
                ),
            ],
            created: vec![
                fixed_projection(
                    0,
                    DurableSummaryProjectionKindV1::CreatedMemorial,
                    "projection.created.memorial",
                ),
                fixed_projection(
                    1,
                    DurableSummaryProjectionKindV1::CreatedEcho,
                    "projection.created.echo",
                ),
            ],
        };
        let mut summary = DurableDeathSummaryV1 {
            schema_version: DURABLE_DEATH_SCHEMA_VERSION,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            death_id,
            summary_revision: 1,
            hero_label_key: "hero.core.label".into(),
            character_name_snapshot: "Mara".into(),
            class_id: "class.grave_arbalist".into(),
            level: 10,
            oath_id: Some("oath.black_bell".into()),
            bargains: vec![DurableOrderedContentIdV1 {
                ordinal: 0,
                content_id: "bargain.cinder_hunger".into(),
            }],
            lifetime_ms: 600_000,
            final_deed_id: "deed.core.sir_caldus_defeated".into(),
            lethal_trace_ordinal: 1,
            last_five_damage: vec![
                DurableSummaryDamageReferenceV1 {
                    ordinal: 0,
                    trace_ordinal: 0,
                },
                DurableSummaryDamageReferenceV1 {
                    ordinal: 1,
                    trace_ordinal: 1,
                },
            ],
            projections,
            echo_outcome: DurableEchoOutcomeV1::NotEligible,
            content_revision,
            snapshot_digest: [0; 32],
        };
        summary.snapshot_digest = summary.expected_snapshot_digest().unwrap();
        let mut memorial = DurableMemorialRecordV1 {
            schema_version: DURABLE_DEATH_SCHEMA_VERSION,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            death_id,
            account_id,
            death_at_unix_ms: 2_000,
            summary_revision: 1,
            summary_snapshot_digest: summary.snapshot_digest,
            presentation_key: "memorial.core.default".into(),
            presentation_digest: [0; 32],
        };
        memorial.presentation_digest = memorial.expected_presentation_digest().unwrap();
        let plan = AuthoritativeDeathPlanV1 {
            schema_version: DURABLE_DEATH_SCHEMA_VERSION,
            event,
            trace,
            destruction,
            summary,
            memorial,
            echo: None,
        };
        DurableDeathCommitRequestV1::seal(plan, 1_900).unwrap()
    }

    fn enable_new_echo_promotion(request: &mut DurableDeathCommitRequestV1) {
        request.plan.summary.echo_outcome = DurableEchoOutcomeV1::Available;
        request.plan.summary.snapshot_digest =
            request.plan.summary.expected_snapshot_digest().unwrap();
        request.plan.memorial.summary_snapshot_digest = request.plan.summary.snapshot_digest;
        request.plan.memorial.presentation_digest = request
            .plan
            .memorial
            .expected_presentation_digest()
            .unwrap();

        let echo_id = uuid_v7(11);
        let mut echo = DurableEchoRecordV1 {
            schema_version: DURABLE_DEATH_SCHEMA_VERSION,
            namespace_id: request.plan.event.namespace_id.clone(),
            echo_id,
            death_id: request.plan.event.death_id,
            account_id: request.plan.event.account_id,
            character_name_snapshot: request.plan.summary.character_name_snapshot.clone(),
            class_id: request.plan.summary.class_id.clone(),
            oath_id: request.plan.summary.oath_id.clone(),
            level: 10,
            appearance_snapshot_id: "appearance.default.grave_arbalist".into(),
            appearance_theme_id: "theme.echo.arbalist_ash".into(),
            weapon_signature_tag: Some("signature.weapon.bow".into()),
            relic_signature_tag: Some("signature.relic.bell".into()),
            bargains: request.plan.summary.bargains.clone(),
            deed_tags: vec![DurableOrderedContentIdV1 {
                ordinal: 0,
                content_id: "deed.core.sir_caldus_defeated".into(),
            }],
            killer_content_id: request.plan.event.killer_content_id.clone(),
            killer_pattern_id: request.plan.event.killer_pattern_id.clone(),
            death_region_id: request.plan.event.region_id.clone(),
            power_band: 1,
            created_at_unix_ms: request.plan.event.committed_at_unix_ms,
            state: DurableEchoStateV1::Available,
            content_revision: request.plan.event.content_revision.clone(),
            snapshot_digest: [0; 32],
        };
        echo.snapshot_digest = echo.expected_snapshot_digest().unwrap();
        let creation_transition = DurableEchoTransitionV1 {
            echo_id,
            echo_death_id: request.plan.event.death_id,
            ordinal: 0,
            previous_state: None,
            next_state: DurableEchoStateV1::Dormant,
            reason: DurableEchoTransitionReasonV1::EligibleDeath,
            source_death_id: Some(request.plan.event.death_id),
            trigger_death_id: request.plan.event.death_id,
            committed_at_unix_ms: request.plan.event.committed_at_unix_ms,
        };
        let promotion = DurableEchoTransitionV1 {
            echo_id,
            echo_death_id: request.plan.event.death_id,
            ordinal: 1,
            previous_state: Some(DurableEchoStateV1::Dormant),
            next_state: DurableEchoStateV1::Available,
            reason: DurableEchoTransitionReasonV1::OldestDormantPromotion,
            source_death_id: None,
            trigger_death_id: request.plan.event.death_id,
            committed_at_unix_ms: request.plan.event.committed_at_unix_ms,
        };
        request.plan.echo = Some(DurableEchoEnvelopeV1 {
            created: echo,
            creation_transition,
            preexisting_available_echo_id: None,
            promotion: Some(promotion),
        });
        request.canonical_plan_hash = request.plan.canonical_plan_hash().unwrap();
        request.canonical_request_hash = request.expected_request_hash().unwrap();
        request.plan.event.canonical_request_hash = request.canonical_request_hash;
    }

    #[test]
    fn canonical_request_and_committed_result_round_trip() {
        let request = valid_request();
        request.validate().unwrap();
        let mut independent_clocks = request.plan.event.clone();
        independent_clocks.lifetime_ticks = 17_000;
        independent_clocks.permadeath_combat_ticks = 18_000;
        independent_clocks.validate().unwrap();
        let payload = request.payload().unwrap();
        assert_eq!(
            DurableDeathCommitRequestV1::decode(&payload).unwrap(),
            request
        );

        let result = StoredCommittedDeathResultV1::from_request(&request).unwrap();
        let result_payload = result.payload().unwrap();
        assert_eq!(
            StoredCommittedDeathResultV1::decode(&result_payload).unwrap(),
            result
        );
        assert_ne!(result.digest().unwrap(), [0; 32]);
    }

    #[test]
    fn postgres_commit_time_rebinding_preserves_intent_identity() {
        let mut request = valid_request();
        enable_new_echo_promotion(&mut request);
        request.validate().unwrap();
        let plan_hash = request.canonical_plan_hash;
        let request_hash = request.canonical_request_hash;

        request.bind_commit_time(2_500).unwrap();

        assert_eq!(request.plan.event.committed_at_unix_ms, 2_500);
        assert_eq!(request.plan.memorial.death_at_unix_ms, 2_500);
        let echo = request.plan.echo.as_ref().unwrap();
        assert_eq!(echo.created.created_at_unix_ms, 2_500);
        assert_eq!(echo.creation_transition.committed_at_unix_ms, 2_500);
        assert_eq!(echo.promotion.as_ref().unwrap().committed_at_unix_ms, 2_500);
        assert_eq!(request.canonical_plan_hash, plan_hash);
        assert_eq!(request.canonical_request_hash, request_hash);
        StoredCommittedDeathResultV1::from_request(&request).unwrap();
    }

    #[test]
    fn eligible_echo_creation_and_self_promotion_are_one_bound_envelope() {
        let mut request = valid_request();
        enable_new_echo_promotion(&mut request);
        request.validate().unwrap();

        let echo = &request.plan.echo.as_ref().unwrap().created;
        let available_digest = echo.expected_snapshot_digest().unwrap();
        let mut later_dormant_projection = echo.clone();
        later_dormant_projection.state = DurableEchoStateV1::Dormant;
        assert_eq!(
            later_dormant_projection.expected_snapshot_digest().unwrap(),
            available_digest,
            "mutable CONT-ECHO-009 state is transition-tail authority, not snapshot material"
        );

        let result = StoredCommittedDeathResultV1::from_request(&request).unwrap();
        assert_eq!(result.echo_outcome, DurableEchoOutcomeV1::Available);
        assert_eq!(result.created_echo_id, result.promoted_echo_id);

        request
            .plan
            .echo
            .as_mut()
            .unwrap()
            .promotion
            .as_mut()
            .unwrap()
            .ordinal = 0;
        assert!(request.validate().is_err());
    }

    #[test]
    fn preexisting_available_echo_and_promotion_are_exactly_one_projector_outcome() {
        let mut request = valid_request();
        enable_new_echo_promotion(&mut request);
        let envelope = request.plan.echo.as_mut().unwrap();
        envelope.preexisting_available_echo_id = Some(uuid_v7(12));
        envelope.promotion = None;
        envelope.created.state = DurableEchoStateV1::Dormant;
        request.plan.summary.echo_outcome = DurableEchoOutcomeV1::Dormant;
        request.plan.summary.snapshot_digest =
            request.plan.summary.expected_snapshot_digest().unwrap();
        request.plan.memorial.summary_snapshot_digest = request.plan.summary.snapshot_digest;
        request.plan.memorial.presentation_digest = request
            .plan
            .memorial
            .expected_presentation_digest()
            .unwrap();
        request.canonical_plan_hash = request.plan.canonical_plan_hash().unwrap();
        request.canonical_request_hash = request.expected_request_hash().unwrap();
        request.plan.event.canonical_request_hash = request.canonical_request_hash;
        request.validate().unwrap();

        let mut illegal_xor = request.clone();
        illegal_xor.plan.echo.as_mut().unwrap().promotion = Some(DurableEchoTransitionV1 {
            echo_id: uuid_v7(13),
            echo_death_id: uuid_v7(14),
            ordinal: 1,
            previous_state: Some(DurableEchoStateV1::Dormant),
            next_state: DurableEchoStateV1::Available,
            reason: DurableEchoTransitionReasonV1::OldestDormantPromotion,
            source_death_id: None,
            trigger_death_id: illegal_xor.plan.event.death_id,
            committed_at_unix_ms: illegal_xor.plan.event.committed_at_unix_ms,
        });
        assert!(illegal_xor.plan.validate().is_err());

        let mut missing_outcome = request;
        missing_outcome
            .plan
            .echo
            .as_mut()
            .unwrap()
            .preexisting_available_echo_id = None;
        assert!(missing_outcome.plan.validate().is_err());
    }

    #[test]
    fn altered_plan_or_request_hash_fails_closed() {
        let mut changed_plan = valid_request();
        changed_plan.plan.event.room_id = "room.altered".into();
        assert!(changed_plan.validate().is_err());

        let mut changed_request = valid_request();
        changed_request.issued_at_unix_ms -= 1;
        assert!(changed_request.validate().is_err());

        let request = valid_request();
        let mut result = StoredCommittedDeathResultV1::from_request(&request).unwrap();
        result.canonical_request_hash[0] ^= 1;
        assert!(result.validate_against(&request).is_err());
    }

    #[test]
    fn uuid_v7_and_utf8_byte_boundaries_are_strict() {
        let mut wrong_uuid = valid_request();
        wrong_uuid.plan.event.death_id[6] = 0x40;
        assert!(wrong_uuid.plan.validate().is_err());

        let mut oversized_name = valid_request();
        oversized_name.plan.summary.character_name_snapshot = "é".repeat(13);
        oversized_name.plan.summary.snapshot_digest = oversized_name
            .plan
            .summary
            .expected_snapshot_digest()
            .unwrap();
        assert!(oversized_name.plan.validate().is_err());

        let mut wrong_world_hash = valid_request();
        wrong_world_hash.plan.event.records_blake3 = "A".repeat(64);
        assert!(wrong_world_hash.plan.validate().is_err());

        let mut wrong_presentation_hash = valid_request();
        wrong_presentation_hash
            .plan
            .event
            .presentation
            .records_blake3 = "A".repeat(64);
        assert!(wrong_presentation_hash.plan.validate().is_err());

        let mut invalid_roster_archive = valid_request();
        invalid_roster_archive.plan.event.former_roster_ordinal = 0;
        assert!(invalid_roster_archive.plan.validate().is_err());

        let mut malformed = valid_request().payload().unwrap();
        let name = malformed
            .windows(4)
            .position(|window| window == b"Mara")
            .expect("fixture contains the UTF-8 name");
        malformed[name] = 0xff;
        assert!(DurableDeathCommitRequestV1::decode(&malformed).is_err());
    }

    #[test]
    fn promoted_content_authority_is_exact_sorted_and_independent() {
        let request = valid_request();
        let event = &request.plan.event;
        let mut authority = DurableDeathContentAuthorityV1 {
            content_revision: event.content_revision.clone(),
            records_blake3: event.records_blake3.clone(),
            assets_blake3: event.assets_blake3.clone(),
            localization_blake3: event.localization_blake3.clone(),
            enabled_items: vec![
                DurableDeathItemContentAuthorityV1 {
                    template_id: "item.armor.core".into(),
                    echo_signature_tag: None,
                },
                DurableDeathItemContentAuthorityV1 {
                    template_id: "item.weapon.bow".into(),
                    echo_signature_tag: Some("signature.weapon.bow".into()),
                },
            ],
        };
        authority.validate().unwrap();
        assert!(authority.matches_event(event));
        assert_eq!(
            authority
                .item("item.weapon.bow")
                .and_then(|item| item.echo_signature_tag.as_deref()),
            Some("signature.weapon.bow")
        );

        authority.enabled_items.swap(0, 1);
        assert!(matches!(
            authority.validate(),
            Err(PersistenceError::DurableDeathContentMismatch)
        ));

        authority.enabled_items.swap(0, 1);
        authority.records_blake3 = "f".repeat(64);
        authority.validate().unwrap();
        assert!(!authority.matches_event(event));
    }

    #[test]
    fn ordinal_gaps_and_noncanonical_destruction_order_are_rejected() {
        let mut status_gap = valid_request();
        status_gap.plan.trace[0].statuses[0].ordinal = 1;
        status_gap.plan.event.trace_digest =
            canonical_digest(TRACE_HASH_CONTEXT, &status_gap.plan.trace).unwrap();
        assert!(status_gap.plan.validate().is_err());

        let mut destruction_order = valid_request();
        destruction_order.plan.destruction.insert(
            0,
            DurableDestructionEntryV1::RunMaterial {
                ordinal: 0,
                material_id: "material.ash".into(),
                destroyed_quantity: 2,
                pre_material_quantity: 2,
                pre_material_version: 1,
                post_material_version: 2,
            },
        );
        if let DurableDestructionEntryV1::Item { ordinal, .. } =
            &mut destruction_order.plan.destruction[1]
        {
            *ordinal = 1;
        }
        destruction_order.plan.event.destruction_entry_count = 2;
        destruction_order.plan.event.destruction_digest = canonical_digest(
            DESTRUCTION_HASH_CONTEXT,
            &destruction_order.plan.destruction,
        )
        .unwrap();
        assert!(destruction_order.plan.validate().is_err());
    }

    #[test]
    fn trace_window_versions_and_canonical_payload_boundaries_fail_closed() {
        let mut old_trace = valid_request();
        old_trace.plan.trace[0].event_tick = 699;
        old_trace.plan.event.trace_digest =
            canonical_digest(TRACE_HASH_CONTEXT, &old_trace.plan.trace).unwrap();
        assert!(old_trace.plan.validate().is_err());

        let mut bad_version = valid_request();
        bad_version.plan.event.versions.inventory.post = 6;
        assert!(bad_version.plan.validate().is_err());

        let mut trailing = valid_request().payload().unwrap();
        trailing.push(0);
        assert!(DurableDeathCommitRequestV1::decode(&trailing).is_err());
        assert!(DurableDeathCommitRequestV1::decode(&[]).is_err());
        assert!(
            DurableDeathCommitRequestV1::decode(&vec![0; MAX_DURABLE_DEATH_PLAN_PAYLOAD_BYTES + 1])
                .is_err()
        );
    }
}
