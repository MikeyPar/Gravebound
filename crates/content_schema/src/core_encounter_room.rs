//! Strict authoring contracts for the unpromoted `GB-M03-03D` encounter and room slice.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ContentId, CoreDevelopmentHeader, CoreLocalizedCopyEntry, DamageBand, DamageType,
    MilliTileCircle, MilliTilePoint, MilliTileRectangle,
};

/// Identifies a compiler input that cannot be loaded or promoted as a release bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreEncounterRoomTargetKind {
    UnpromotedEncounterRoomSubset,
}

/// Ordered allowlists and immutable source hashes for the `GB-M03-03D` content boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreEncounterRoomDevelopmentTarget {
    pub schema_version: u32,
    pub target_kind: CoreEncounterRoomTargetKind,
    pub target_name: String,
    pub required_normal_enemy_ids: Vec<ContentId>,
    pub required_miniboss_ids: Vec<ContentId>,
    pub required_pattern_ids: Vec<ContentId>,
    pub required_room_template_ids: Vec<ContentId>,
    pub required_pack_ids: Vec<ContentId>,
    pub required_layout_ids: Vec<ContentId>,
    pub required_asset_ids: Vec<ContentId>,
    pub required_localization_keys: Vec<ContentId>,
    pub expected_records_blake3: String,
    pub expected_assets_blake3: String,
    pub expected_localization_blake3: String,
}

/// Whether `03D` reuses an immutable FP definition or authors a new Core definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreEncounterSourceKind {
    ImmutableFirstPlayable,
    AuthoredCore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreEncounterRank {
    Normal,
    Miniboss,
}

/// Exact roster membership. Behavior definitions are compiled in the subsequent `03D` layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreEncounterRosterMember {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub rank: CoreEncounterRank,
    pub source_kind: CoreEncounterSourceKind,
    pub required_pattern_ids: Vec<ContentId>,
    pub reward_profile_id: ContentId,
    pub xp_profile_id: ContentId,
    pub authored_core_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreEnemyRole {
    Fodder,
    Pressure,
    Disruptor,
    Anchor,
    Elite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreEnemyStateStage {
    SpawnTelegraph,
    Acquire,
    MoveOrPosition,
    Telegraph,
    Attack,
    Recover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreTargetSelection {
    NearestLivingDamageableInAggroTieLowestEntityId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreTelegraphLock {
    AimAndPositionAtTelegraphStart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "movement_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreEnemyLocomotion {
    RushRetreat {
        approach_speed_milli_tiles_per_second: u32,
        trigger_distance_milli_tiles: u32,
        charge_distance_milli_tiles: u32,
        charge_duration_milliseconds: u32,
        retreat_speed_milli_tiles_per_second: u32,
        retreat_duration_milliseconds: u32,
    },
    MaintainDistance {
        movement_speed_milli_tiles_per_second: u32,
        preferred_distance_milli_tiles: u32,
    },
    OrbitAnchor {
        movement_speed_milli_tiles_per_second: u32,
        orbit_radius_milli_tiles: u32,
    },
    PursueStopChargeHome {
        movement_speed_milli_tiles_per_second: u32,
        stop_distance_milli_tiles: u32,
    },
    Stationary,
}

/// Exact authored behavior for the five Core actors not reused from immutable FP definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreAuthoredEnemyBehaviorRecord {
    pub owner_id: ContentId,
    pub role: CoreEnemyRole,
    pub state_sequence: Vec<CoreEnemyStateStage>,
    pub target_selection: CoreTargetSelection,
    pub telegraph_lock: CoreTelegraphLock,
    pub maximum_health: u32,
    pub armor: u16,
    pub collision_radius_milli_tiles: u32,
    pub hurtbox_radius_milli_tiles: u32,
    pub aggro_radius_milli_tiles: u32,
    pub leash_radius_milli_tiles: u32,
    pub target_reacquire_milliseconds: u32,
    pub no_target_reset_milliseconds: u32,
    pub spawn_warning_milliseconds: u32,
    pub spawn_invulnerability_milliseconds: u32,
    pub introduction_milliseconds: u32,
    pub contact_damage: u32,
    pub drop_reward_on_reset: bool,
    pub locomotion: CoreEnemyLocomotion,
    pub pattern_ids: Vec<ContentId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CorePatternCounterplay {
    Strafe,
    FollowGap,
    LeaveTelegraph,
    MoveWithRotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CorePatternMemoryFamily {
    ChargeOrContact,
    FanProjectile,
    RotatingProjectile,
    RadialProjectile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CorePatternDisposition {
    OneContactHitPerCast,
    ConsumeOnPlayerOrSolid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreAttackGroupRule {
    DistinctProjectileHitGroups,
    OneContactHitPerCast,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "warning_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CorePatternWarning {
    Standalone {
        first_milliseconds: u32,
        repeated_milliseconds: u32,
    },
    ParentOnly,
    RecoveryPreview {
        duration_milliseconds: u32,
        major_audio: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreRadialGapRelation {
    TargetOpposite,
    TargetFacing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "geometry_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreAuthoredPatternGeometry {
    Charge {
        distance_milli_tiles: u32,
        duration_milliseconds: u32,
    },
    AlternatingFan {
        first_offsets_milli_degrees: Vec<i32>,
        second_offsets_milli_degrees: Vec<i32>,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
    RotatingArms {
        arm_count: u16,
        clockwise_milli_degrees_per_second: u32,
        emission_interval_milliseconds: u32,
        active_duration_milliseconds: u32,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
    ChargeLane {
        width_milli_tiles: u32,
        length_milli_tiles: u32,
        charge_duration_milliseconds: u32,
    },
    RadialGap {
        index_count: u16,
        omitted_adjacent_count: u16,
        relation: CoreRadialGapRelation,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
    ProjectileFan {
        shot_count: u16,
        total_arc_milli_degrees: u32,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
}

/// Fully normalized authored hostile pattern. Reused FP patterns remain in the immutable package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreAuthoredPatternRecord {
    pub id: ContentId,
    pub owner_id: ContentId,
    pub telegraph_id: ContentId,
    pub audio_cue_id: ContentId,
    pub major_audio_cue_id: Option<ContentId>,
    pub damage_type: DamageType,
    pub damage_band: DamageBand,
    pub raw_damage: u32,
    pub threat_cost: u16,
    pub warning: CorePatternWarning,
    pub cycle_milliseconds: u32,
    pub quiet_milliseconds: u32,
    pub geometry: CoreAuthoredPatternGeometry,
    pub counterplay: CorePatternCounterplay,
    pub memory_family: CorePatternMemoryFamily,
    pub disposition: CorePatternDisposition,
    pub attack_group_rule: CoreAttackGroupRule,
    pub acceleration_milli_tiles_per_second_squared: u32,
    pub pierces_players: bool,
    pub statuses: Vec<ContentId>,
    pub cancel_on_phase_change: bool,
    pub maximum_active_instances: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreRoomDoorSide {
    North,
    East,
    South,
    West,
}

/// One door centered on a room edge. Offset is measured from the edge's northwest endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreRoomDoor {
    pub id: String,
    pub side: CoreRoomDoorSide,
    pub offset_milli_tiles: u32,
    pub width_milli_tiles: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreRoomVolumeKind {
    Solid,
    DeepWater,
    WalkableBoundary,
    PatternLane,
    ObjectiveArea,
}

/// Closed geometry variants prevent an author from supplying incompatible spatial fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "shape", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreRoomVolumeGeometry {
    Rectangle {
        rectangle: MilliTileRectangle,
    },
    Circle {
        circle: MilliTileCircle,
    },
    Polyline {
        width_milli_tiles: u32,
        points: Vec<MilliTilePoint>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreRoomVolume {
    pub id: String,
    pub kind: CoreRoomVolumeKind,
    pub geometry: CoreRoomVolumeGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreRoomAnchorKind {
    SafeEntry,
    Exit,
    Fodder,
    Pressure,
    Disruptor,
    AnchorEnemy,
    Miniboss,
    Stage,
    Add,
    Shrine,
    Stabilization,
    Chest,
    Boss,
    ChargeEndpoint,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreRoomAnchor {
    pub id: String,
    pub kind: CoreRoomAnchorKind,
    pub point: MilliTilePoint,
    pub bound_content_id: Option<ContentId>,
}

/// One exact northwest-origin Bell Sepulcher template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreRoomTemplateRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub width_milli_tiles: u32,
    pub height_milli_tiles: u32,
    pub doors: Vec<CoreRoomDoor>,
    pub volumes: Vec<CoreRoomVolume>,
    pub anchors: Vec<CoreRoomAnchor>,
    pub safe_noncombat: bool,
    pub authored_core_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreEncounterPackMember {
    pub enemy_id: ContentId,
    pub count: u16,
    pub threat_each: u16,
}

/// Exact simultaneous pack or fixed-room encounter composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreEncounterPackRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub members: Vec<CoreEncounterPackMember>,
    pub base_budget: u16,
    pub warning_milliseconds: u16,
    pub simultaneous_spawn: bool,
    pub authored_core_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreFixedLayoutNode {
    pub node_id: String,
    pub grid_x: i32,
    pub grid_y: i32,
    pub rotation_degrees: u16,
    pub room_template_id: ContentId,
    pub encounter: Option<CoreFixedRoomEncounter>,
    pub counts_toward_six_room_total: bool,
}

/// Inline authored fallback composition. The specification does not assign these room encounters
/// stable pack IDs, so the schema deliberately avoids inventing any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreFixedRoomEncounter {
    pub members: Vec<CoreEncounterPackMember>,
    pub base_budget: u16,
    pub warning_milliseconds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreFixedLayoutEdge {
    pub from_node_id: String,
    pub to_node_id: String,
    pub from_door_id: String,
    pub to_door_id: String,
    pub corridor_width_milli_tiles: u32,
    pub corridor_length_tiles: u16,
}

/// Fixed M03 main chain plus authored-but-disabled branch metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreFixedLayoutRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub main_chain_node_ids: Vec<String>,
    pub nodes: Vec<CoreFixedLayoutNode>,
    pub edges: Vec<CoreFixedLayoutEdge>,
    pub disabled_branch_node_ids: Vec<String>,
    pub branches_enabled: bool,
    pub seeded_selection_enabled: bool,
    pub authored_core_enabled: bool,
}

/// Complete strict authoring set for the first `GB-M03-03D` compiler layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreEncounterRoomRecords {
    pub schema_version: u32,
    pub roster: Vec<CoreEncounterRosterMember>,
    pub authored_behaviors: Vec<CoreAuthoredEnemyBehaviorRecord>,
    pub authored_patterns: Vec<CoreAuthoredPatternRecord>,
    pub rooms: Vec<CoreRoomTemplateRecord>,
    pub packs: Vec<CoreEncounterPackRecord>,
    pub layouts: Vec<CoreFixedLayoutRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreEncounterRoomAssetKind {
    EnemySilhouette,
    EnemyPortrait,
    MinibossSilhouette,
    MinibossPortrait,
    RoomTilemap,
    Telegraph,
    WarningAudio,
    MajorWarningAudio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreEncounterRoomAsset {
    pub asset_id: ContentId,
    pub source_record_id: ContentId,
    pub kind: CoreEncounterRoomAssetKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreEncounterRoomAssetManifest {
    pub schema_version: u32,
    pub assets: Vec<CoreEncounterRoomAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreEncounterRoomCopyFile {
    pub schema_version: u32,
    pub locale: String,
    pub entries: Vec<CoreLocalizedCopyEntry>,
}
