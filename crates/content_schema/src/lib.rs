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

/// Benchmark boss definition. Timelines remain authored data and compile into integer ticks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BossRecord {
    #[serde(flatten)]
    pub header: CommonHeader,
    pub numeric_payload: BossPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BossPayload {
    pub health: u32,
    pub armor: u32,
    pub hurtbox_radius_milli_tiles: u32,
    pub position: Point,
    pub movement_mode: BossMovementMode,
    pub summons_enabled: bool,
    pub status_effect_ids: Vec<ContentId>,
    pub target_solo_duration_min_ms: u32,
    pub target_solo_duration_max_ms: u32,
    pub soft_enrage_ms: u32,
    pub introduction_ms: u32,
    pub phase_break_ms: u32,
    pub phase_break_received_damage_multiplier_basis_points: u32,
    pub soft_enrage_downtime_multiplier_basis_points: u32,
    pub phase_two_health_threshold_basis_points: u32,
    pub phase_three_health_threshold_basis_points: u32,
    pub low_health_restart_basis_points: u32,
    pub phase_three_low_health_loop_ms: u32,
    pub fan_offsets_degrees: Vec<i16>,
    pub ring_index_count: u32,
    pub ring_omitted_count: u32,
    pub ring_gap_advance: u32,
    pub phase_three_second_gap_advance: u32,
    pub ring_preview_ms: u32,
    pub cross_axis_sets_degrees: Vec<[u16; 2]>,
    pub fan_pattern_id: ContentId,
    pub ring_pattern_id: ContentId,
    pub cross_pattern_id: ContentId,
    pub phase_timelines: Vec<BossPhaseTimeline>,
    pub reward_table_id: ContentId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BossMovementMode {
    Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BossPhaseTimeline {
    pub phase: BossPhase,
    pub loop_ms: u32,
    pub cues: Vec<BossTimelineCueRecord>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BossPhase {
    Phase1,
    Phase2,
    Phase3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BossTimelineCueRecord {
    pub kind: BossCueKind,
    pub starts_at_ms: u32,
    pub resolves_at_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BossCueKind {
    Fan,
    Ring,
    RingPreviewA,
    RingPreviewB,
    Cross,
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
    pub allowed_boss_ids: Vec<ContentId>,
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

/// Identifies a compiler input that is intentionally outside the release/promotion pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreDevelopmentTargetKind {
    /// A narrow internal-only subset used while the complete Core manifest is still authored.
    UnpromotedIdentitySubset,
}

/// Strict input contract for the unpromoted Core identity compiler.
///
/// This is deliberately not a [`ReleaseManifest`]: it has no bundle ID, release stage,
/// promotion metadata, or output package path. `SPEC-CONFLICT-004` reserves `core.1.0.0`
/// until every Core record and promotion gate is complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreDevelopmentTarget {
    pub schema_version: u32,
    pub target_kind: CoreDevelopmentTargetKind,
    pub source_content_version: String,
    pub required_class_ids: Vec<ContentId>,
    pub required_ability_ids: Vec<ContentId>,
    pub presentation_asset_ids: Vec<ContentId>,
}

/// Identifies a world-flow compiler input that cannot be promoted or loaded as a release bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreWorldFlowTargetKind {
    UnpromotedWorldFlowSubset,
}

/// Ordered allowlists for the independently reviewed `GB-M03-03A` development target.
///
/// This descriptor intentionally has no content version, release stage, package ID, or
/// promotion metadata. Formal Core packaging remains owned by `CONT-VALID-003`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreWorldFlowDevelopmentTarget {
    pub schema_version: u32,
    pub target_kind: CoreWorldFlowTargetKind,
    pub target_name: String,
    pub required_hub_ids: Vec<ContentId>,
    pub required_world_ids: Vec<ContentId>,
    pub required_object_ids: Vec<ContentId>,
    pub required_asset_ids: Vec<ContentId>,
    pub required_localization_keys: Vec<ContentId>,
    pub expected_records_blake3: String,
    pub expected_assets_blake3: String,
    pub expected_localization_blake3: String,
}

/// Metadata shared by unpromoted Core world-flow records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreDevelopmentHeader {
    pub id: ContentId,
    pub schema_version: u32,
    pub enabled: bool,
    pub earliest_release_stage: ReleaseStage,
    pub localization_name_key: ContentId,
    pub localization_description_key: ContentId,
    pub asset_ids: Vec<ContentId>,
    pub tags: Vec<String>,
    pub source_document_feature_id: String,
}

/// Exact fixed-point point in milli-tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MilliTilePoint {
    pub x: i32,
    pub y: i32,
}

/// Exact fixed-point axis-aligned rectangle in milli-tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MilliTileRectangle {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Exact fixed-point circle in milli-tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MilliTileCircle {
    pub center: MilliTilePoint,
    pub radius: u32,
}

/// One authored polyline. Consecutive points form axis-aligned road segments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreRoadPolyline {
    pub width_milli_tiles: u32,
    pub points: Vec<MilliTilePoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreMapOrigin {
    Northwest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreProhibitedCreation {
    Hostile,
    Damage,
    Projectile,
    Pickup,
    Drop,
}

/// Strict Lantern Halls geometry record for the unpromoted Core target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreHubRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub width_tiles: u32,
    pub height_tiles: u32,
    pub origin: CoreMapOrigin,
    pub solid_shell_tiles: u32,
    pub player_radius_milli_tiles: u32,
    pub minimum_aisle_width_milli_tiles: u32,
    pub safe_noncombat: bool,
    pub default_spawn: MilliTilePoint,
    pub character_select_return: MilliTilePoint,
    pub solid_rectangles: Vec<MilliTileRectangle>,
    pub prohibited_creation: Vec<CoreProhibitedCreation>,
    pub object_ids: Vec<ContentId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreWorldTerrain {
    ClearMud,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreDisabledWorldSystem {
    MacroScheduler,
    RealmCycle,
    Siege,
    Retirement,
}

/// Strict M03 private micro-realm geometry record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreWorldRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub width_tiles: u32,
    pub height_tiles: u32,
    pub origin: CoreMapOrigin,
    pub solid_shell_tiles: u32,
    pub base_terrain: CoreWorldTerrain,
    pub capacity: u32,
    pub disabled_systems: Vec<CoreDisabledWorldSystem>,
    pub realm_gate: MilliTileRectangle,
    pub player_spawn: MilliTilePoint,
    pub lantern_fork_safe_area: MilliTileCircle,
    pub bell_portal_area: MilliTileCircle,
    pub roads: Vec<CoreRoadPolyline>,
    pub candidate_spawn_anchors: Vec<MilliTilePoint>,
    pub intentionally_excluded_anchor: MilliTilePoint,
    pub enabled_spawn_anchor_count: u32,
    pub object_ids: Vec<ContentId>,
}

/// Semantic spatial representation for one hub/world child. The closed variants prevent illegal
/// combinations such as a station without a clear radius or a spawn anchor with interaction area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "object_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreWorldObjectGeometry {
    PointInteractable {
        point: MilliTilePoint,
        clear_radius_milli_tiles: u32,
    },
    RectangleLandmark {
        rectangle: MilliTileRectangle,
    },
    CircleLandmark {
        circle: MilliTileCircle,
    },
    CirclePortal {
        circle: MilliTileCircle,
    },
    RectanglePortal {
        rectangle: MilliTileRectangle,
    },
    SpawnAnchor {
        point: MilliTilePoint,
    },
}

/// Typed child record with explicit parent ownership and player-route integration gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreWorldObjectRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub parent_id: ContentId,
    pub geometry: CoreWorldObjectGeometry,
    pub authored_core_enabled: bool,
    pub integration_gate: Option<String>,
}

/// Complete strict source set for `GB-M03-03A`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreWorldFlowRecords {
    pub schema_version: u32,
    pub hubs: Vec<CoreHubRecord>,
    pub worlds: Vec<CoreWorldRecord>,
    pub objects: Vec<CoreWorldObjectRecord>,
}

/// Resolution kind for a symbolic graybox asset reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreGrayboxAssetKind {
    GeneratedCollisionTilemap,
    GrayboxMarker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreGrayboxAsset {
    pub asset_id: ContentId,
    pub source_record_id: ContentId,
    pub kind: CoreGrayboxAssetKind,
}

/// Asset-reference closure for the world-flow development target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreGrayboxAssetManifest {
    pub schema_version: u32,
    pub assets: Vec<CoreGrayboxAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreLocalizedCopyEntry {
    pub key: ContentId,
    pub value: String,
}

/// Exact localized labels resolved by the world-flow development target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreWorldFlowCopyFile {
    pub schema_version: u32,
    pub locale: String,
    pub entries: Vec<CoreLocalizedCopyEntry>,
}

/// Required labels for every safe Core identity screen state in `GB-M03-01`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreIdentityPhaseCopy {
    pub boot: String,
    pub patch_check: String,
    pub authenticating: String,
    pub roster_loading: String,
    pub roster_empty: String,
    pub roster_ready: String,
    pub character_creation: String,
    pub creating: String,
    pub selecting: String,
    pub selected: String,
    pub disconnected: String,
    pub disabled: String,
    pub error: String,
}

/// Strict `en-US` copy source for the unpromoted Core identity UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreIdentityCopyFile {
    pub schema_version: u32,
    pub locale: String,
    pub window_title: String,
    pub brand_header: String,
    pub wipe_warning: String,
    pub phases: CoreIdentityPhaseCopy,
    pub status_template: String,
    pub loading_roster: String,
    pub populated_slot_template: String,
    pub empty_slot_template: String,
    pub selected_badge: String,
    pub class_detail_template: String,
    pub create_action: String,
    pub select_slot_action_template: String,
    pub retry_action: String,
    pub footer_template: String,
    pub closed_feature_literal: String,
    pub not_equipped_literal: String,
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
    use std::{fs, path::Path};

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

    #[test]
    fn core_development_target_cannot_describe_a_release_or_promotion() {
        let base = serde_json::json!({
            "schema_version": 1,
            "target_kind": "unpromoted_identity_subset",
            "source_content_version": "fp.1.0.0",
            "required_class_ids": ["class.grave_arbalist"],
            "required_ability_ids": [],
            "presentation_asset_ids": ["sprite.class.grave_arbalist"]
        });
        for (field, value) in [
            ("bundle_id", serde_json::json!("core.1.0.0")),
            ("release_stage", serde_json::json!("core")),
            ("promotion", serde_json::json!({"approved": true})),
            ("output_package", serde_json::json!("core.1.0.0.zip")),
        ] {
            let mut changed = base.clone();
            changed
                .as_object_mut()
                .expect("test target is an object")
                .insert(field.to_owned(), value);
            let error = serde_json::from_value::<CoreDevelopmentTarget>(changed)
                .expect_err("release metadata must not cross the development boundary");
            assert!(
                error
                    .to_string()
                    .contains(&format!("unknown field `{field}`")),
                "{error}"
            );
        }
    }

    #[test]
    fn checked_in_core_development_schema_matches_the_rust_contract() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/core_development_target.schema.json");
        let checked_in: serde_json::Value =
            serde_json::from_slice(&fs::read(path).expect("checked-in Core development schema"))
                .expect("valid JSON Schema");
        let generated = serde_json::to_value(schemars::schema_for!(CoreDevelopmentTarget))
            .expect("serializable generated schema");
        assert_eq!(checked_in, generated);
    }

    #[test]
    fn checked_in_core_world_flow_schemas_match_the_rust_contracts() {
        let schema_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let cases = [
            (
                "core_world_flow_target.schema.json",
                serde_json::to_value(schemars::schema_for!(CoreWorldFlowDevelopmentTarget))
                    .expect("serializable target schema"),
            ),
            (
                "core_world_flow_records.schema.json",
                serde_json::to_value(schemars::schema_for!(CoreWorldFlowRecords))
                    .expect("serializable records schema"),
            ),
            (
                "core_graybox_assets.schema.json",
                serde_json::to_value(schemars::schema_for!(CoreGrayboxAssetManifest))
                    .expect("serializable asset schema"),
            ),
            (
                "core_world_flow_copy.schema.json",
                serde_json::to_value(schemars::schema_for!(CoreWorldFlowCopyFile))
                    .expect("serializable copy schema"),
            ),
        ];
        for (name, generated) in cases {
            let text = fs::read_to_string(schema_root.join(name)).expect("checked-in schema");
            let checked_in: serde_json::Value =
                serde_json::from_str(&text).expect("valid checked-in schema");
            assert_eq!(checked_in, generated, "schema drift in {name}");
        }
    }

    #[test]
    fn checked_in_core_identity_copy_schema_matches_the_rust_contract() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/core_identity_copy.schema.json");
        let checked_in: serde_json::Value =
            serde_json::from_slice(&fs::read(path).expect("checked-in Core identity copy schema"))
                .expect("valid JSON Schema");
        let generated = serde_json::to_value(schemars::schema_for!(CoreIdentityCopyFile))
            .expect("serializable generated schema");
        assert_eq!(checked_in, generated);
    }
}
