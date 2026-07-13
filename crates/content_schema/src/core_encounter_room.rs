//! Strict authoring contracts for the unpromoted `GB-M03-03D` encounter and room slice.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ContentId, CoreDevelopmentHeader, CoreLocalizedCopyEntry, MilliTileCircle, MilliTilePoint,
    MilliTileRectangle,
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
