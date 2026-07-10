//! Versioned, strict serialized contracts shared by Gravebound tools and runtime loaders.

use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Initial schema version named by `CONT-001`.
pub const SCHEMA_VERSION: u32 = 1;
/// Exact First Playable content bundle from `CONT-FP-001`.
pub const FIRST_PLAYABLE_CONTENT_VERSION: &str = "fp.1.0.0";

/// Stable dot-separated content identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct ContentId(String);

impl ContentId {
    /// Parses and validates a stable identifier.
    pub fn parse(value: impl Into<String>) -> Result<Self, IdError> {
        let value = value.into();
        validate_identifier(&value, false)?;
        Ok(Self(value))
    }

    /// Returns the canonical identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for ContentId {
    type Err = IdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for ContentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Stable roadmap feature identifier, for example `GB-M00-07`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct FeatureId(String);

impl FeatureId {
    /// Parses a feature ID used for acceptance traceability.
    pub fn parse(value: impl Into<String>) -> Result<Self, IdError> {
        let value = value.into();
        validate_identifier(&value, true)?;
        Ok(Self(value))
    }

    /// Returns the canonical feature ID text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FeatureId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for FeatureId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Identifier parse failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid {kind} identifier `{value}`: {reason}")]
pub struct IdError {
    kind: &'static str,
    value: String,
    reason: &'static str,
}

fn validate_identifier(value: &str, feature: bool) -> Result<(), IdError> {
    let valid = if feature {
        let parts: Vec<_> = value.split('-').collect();
        let milestone = parts.get(1).copied().unwrap_or_default();
        let task = parts.get(2).copied().unwrap_or_default();
        let task_bytes = task.as_bytes();
        parts.len() == 3
            && parts[0] == "GB"
            && milestone.len() == 3
            && milestone.starts_with('M')
            && milestone[1..]
                .chars()
                .all(|character| character.is_ascii_digit())
            && (task == "GATE"
                || ((task_bytes.len() == 2 || task_bytes.len() == 3)
                    && task_bytes[..2].iter().all(u8::is_ascii_digit)
                    && task_bytes[2..].iter().all(u8::is_ascii_uppercase)))
    } else {
        value.len() <= 128
            && value.split('.').count() >= 2
            && value.split('.').all(|segment| {
                !segment.is_empty()
                    && segment.chars().all(|character| {
                        character.is_ascii_lowercase()
                            || character.is_ascii_digit()
                            || character == '_'
                    })
            })
    };
    if valid {
        Ok(())
    } else {
        Err(IdError {
            kind: if feature { "feature" } else { "content" },
            value: value.to_owned(),
            reason: if feature {
                "expected GB-M<digits>-<uppercase task>"
            } else {
                "expected 2+ lowercase dot-separated snake_case segments, maximum 128 bytes"
            },
        })
    }
}

/// Earliest release stage that may enable a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseStage {
    Fp,
    Core,
    Slice,
    Alpha,
    Playtest,
    Ea,
}

/// Required metadata present on every runtime definition under `CONT-001`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommonHeader {
    pub id: ContentId,
    pub schema_version: u32,
    pub content_version: String,
    pub enabled: bool,
    pub release_stage: ReleaseStage,
    pub localization_name_key: ContentId,
    pub localization_description_key: ContentId,
    pub asset_ids: Vec<ContentId>,
    pub tags: Vec<String>,
    pub source_document_feature_id: String,
}

/// Class definition and its stable gameplay references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassRecord {
    #[serde(flatten)]
    pub header: CommonHeader,
    pub numeric_payload: ClassPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassPayload {
    pub starting_max_health: u32,
    pub health_per_level: u32,
    pub starting_armor: u32,
    pub armor_growth_levels: Vec<u32>,
    pub movement_speed_milli_tiles_per_second: u32,
    pub weapon_family: String,
    pub primary_ability_id: ContentId,
    pub active_ability_ids: Vec<ContentId>,
    pub passive_ability_id: ContentId,
}

/// Ability definition with a fully materialized type-specific payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AbilityRecord {
    #[serde(flatten)]
    pub header: CommonHeader,
    pub numeric_payload: AbilityPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "ability_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AbilityPayload {
    Primary {
        range_milli_tiles: u32,
        attacks_per_second_basis_points: u32,
        projectile_radius_milli_tiles: u32,
        stops_on_first_enemy: bool,
    },
    GraveMark {
        cooldown_ms: u32,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        weapon_damage_multiplier_basis_points: u32,
        duration_ms: u32,
        marked_primary_bonus_basis_points: u32,
        maximum_marked_targets: u32,
    },
    Slipstep {
        cooldown_ms: u32,
        travel_milli_tiles: u32,
        travel_ms: u32,
        direct_damage_reduction_basis_points: u32,
        empowered_window_ms: u32,
        projectile_speed_bonus_basis_points: u32,
        pierce_bonus: u32,
        exhaustion_ms: u32,
    },
    Stillness {
        activation_ms: u32,
        movement_threshold_basis_points: u32,
        projectile_speed_bonus_basis_points: u32,
        primary_damage_bonus_basis_points: u32,
        break_on_damage: bool,
        break_on_slipstep: bool,
    },
}

/// Enemy definition. Detailed attack geometry is owned by referenced pattern records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnemyRecord {
    #[serde(flatten)]
    pub header: CommonHeader,
    pub numeric_payload: EnemyPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnemyPayload {
    pub role: EnemyRole,
    pub health: u32,
    pub armor: u32,
    pub hurtbox_radius_milli_tiles: u32,
    pub movement_speed_milli_tiles_per_second: u32,
    pub aggro_radius_milli_tiles: u32,
    pub leash_radius_milli_tiles: u32,
    pub spawn_telegraph_ms: u32,
    pub state_machine: Vec<StateStep>,
    pub pattern_ids: Vec<ContentId>,
    pub reward_table_id: ContentId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnemyRole {
    Fodder,
    Pressure,
    Anchor,
    Boss,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StateStep {
    pub state: String,
    pub duration_ms: Option<u32>,
}

/// Hostile attack contract used by the deterministic scheduler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternRecord {
    #[serde(flatten)]
    pub header: CommonHeader,
    pub numeric_payload: PatternPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternPayload {
    pub pattern_kind: PatternKind,
    pub cycle_ms: u32,
    pub first_telegraph_ms: u32,
    pub repeated_telegraph_ms: u32,
    pub projectile_count: u32,
    pub projectile_speed_milli_tiles_per_second: Option<u32>,
    pub projectile_radius_milli_tiles: Option<u32>,
    pub projectile_lifetime_ms: Option<u32>,
    pub lane_width_milli_tiles: Option<u32>,
    pub active_ms: Option<u32>,
    pub raw_damage: u32,
    pub damage_type: DamageType,
    pub damage_band: DamageBand,
    pub threat_cost: u32,
    pub echo_memory_family: String,
    pub counterplay: String,
    pub projectile_disposition: String,
    pub telegraph_id: ContentId,
    pub audio_cue_id: ContentId,
    pub maximum_active_instances: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    AimedFan,
    GapRing,
    CrossLanes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DamageType {
    Physical,
    Veil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DamageBand {
    Chip,
    Pressure,
    Major,
}

/// First Playable arena geometry and encounter references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArenaRecord {
    #[serde(flatten)]
    pub header: CommonHeader,
    pub numeric_payload: ArenaPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArenaPayload {
    pub width_tiles: u32,
    pub height_tiles: u32,
    pub shell_thickness_tiles: u32,
    pub player_spawn: Point,
    pub boss_spawn: Point,
    pub pillars: Vec<Rectangle>,
    pub anchors: Vec<NamedPoint>,
    pub allowed_enemy_ids: Vec<ContentId>,
    pub allowed_reward_table_ids: Vec<ContentId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rectangle {
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
    pub width_milli_tiles: u32,
    pub height_milli_tiles: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamedPoint {
    pub id: String,
    pub point: Point,
}

/// Prototype equipment or production consumable definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemRecord {
    #[serde(flatten)]
    pub header: CommonHeader,
    pub numeric_payload: ItemPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "item_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemPayload {
    Equipment {
        slot: EquipmentSlot,
        rarity: ItemRarity,
        effects: Vec<ItemEffect>,
    },
    Consumable {
        belt_stack_cap: u32,
        restore_max_health_basis_points: u32,
        restore_duration_ms: u32,
        shared_cooldown_ms: u32,
        damage_interrupts_restore: bool,
        consumed_on_use: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentSlot {
    Weapon,
    Relic,
    Armor,
    Charm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemRarity {
    Worn,
    Forged,
    Oathed,
    Relic,
}

/// Explicit fixed-point behavior modification. No display-text parsing occurs at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemEffect {
    pub stat: String,
    pub operation: EffectOperation,
    pub value: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectOperation {
    Set,
    Add,
    MultiplyBasisPoints,
}

/// Deterministic reward table made of ordered independent or guaranteed roll groups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DropTableRecord {
    #[serde(flatten)]
    pub header: CommonHeader,
    pub numeric_payload: DropTablePayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DropTablePayload {
    pub roll_groups: Vec<DropRollGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DropRollGroup {
    pub group_id: String,
    pub presence_basis_points: u32,
    pub selections: u32,
    pub without_replacement: bool,
    pub outcomes: Vec<WeightedOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WeightedOutcome {
    pub item_id: ContentId,
    pub weight: u32,
}

/// Checked-in release manifest defining the only records enabled together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub content_version: String,
    pub release_stage: ReleaseStage,
    pub required_content_ids: Vec<ContentId>,
}

/// Traceability registry. Every implementation task must have explicit acceptance criteria.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureRegistry {
    pub schema_version: u32,
    pub features: Vec<FeatureEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureEntry {
    pub feature_id: FeatureId,
    pub title: String,
    pub milestone: String,
    pub depends_on: Vec<FeatureId>,
    pub acceptance_criteria: Vec<String>,
    pub source_document_ids: Vec<String>,
}

/// Asset allowlist used to fail unresolved presentation references at build time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetManifest {
    pub schema_version: u32,
    pub asset_ids: Vec<ContentId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids_reject_case_spaces_and_empty_segments() {
        assert!(ContentId::parse("class.grave_arbalist").is_ok());
        assert!(ContentId::parse("Class.grave_arbalist").is_err());
        assert!(ContentId::parse("class..grave_arbalist").is_err());
        assert!(ContentId::parse("class grave_arbalist").is_err());
    }

    #[test]
    fn feature_ids_are_strict_and_stable() {
        assert!(FeatureId::parse("GB-M00-07").is_ok());
        assert!(FeatureId::parse("GB-M01-01A").is_ok());
        assert!(FeatureId::parse("gb-m00-07").is_err());
        assert!(FeatureId::parse("GB-M-01").is_err());
        assert!(FeatureId::parse("GB-M1-01").is_err());
        assert!(FeatureId::parse("GB-M01-001").is_err());
    }

    #[test]
    fn strict_schemas_reject_unknown_fields() {
        let text = r#"{
            "schema_version": 1,
            "content_version": "fp.1.0.0",
            "release_stage": "fp",
            "required_content_ids": [],
            "invented_default": true
        }"#;
        let error = serde_json::from_str::<ReleaseManifest>(text).expect_err("unknown field");
        assert!(error.to_string().contains("unknown field"));
    }
}
