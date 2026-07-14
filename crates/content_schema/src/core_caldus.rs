//! Strict authoring contracts for the unpromoted `GB-M03-03E` Sir Caldus slice.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ContentId, CoreDevelopmentHeader, CoreLocalizedCopyEntry, DamageType, MilliTilePoint};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreCaldusTargetKind {
    UnpromotedCaldusSubset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusDevelopmentTarget {
    pub schema_version: u32,
    pub target_kind: CoreCaldusTargetKind,
    pub target_name: String,
    pub required_boss_ids: Vec<ContentId>,
    pub required_pattern_ids: Vec<ContentId>,
    pub required_exit_ids: Vec<ContentId>,
    pub required_asset_ids: Vec<ContentId>,
    pub required_localization_keys: Vec<ContentId>,
    pub expected_records_blake3: String,
    pub expected_assets_blake3: String,
    pub expected_localization_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusParticipantLockRecord {
    pub load_timeout_milliseconds: u32,
    pub ready_countdown_milliseconds: u32,
    pub introduction_milliseconds: u32,
    pub empty_reset_milliseconds: u32,
    pub minimum_locked_participants: u8,
    pub maximum_locked_participants: u8,
    pub runtime_capacity: u8,
    pub safe_entrance_radius_milli_tiles: u32,
    pub late_entry_allowed: bool,
    pub death_or_recall_rescales: bool,
    pub recall_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusPhaseOneRecord {
    pub loop_milliseconds: u32,
    pub shield_starts_milliseconds: Vec<u32>,
    pub bell_ring_start_milliseconds: u32,
    pub ring_gap_initial_index: u8,
    pub ring_gap_advance: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusPhaseTwoRecord {
    pub loop_milliseconds: u32,
    pub charge_starts_milliseconds: Vec<u32>,
    pub shield_starts_milliseconds: Vec<u32>,
    pub charge_direction_lock_milliseconds: u32,
    pub charge_movement_start_milliseconds: u32,
    pub charge_end_milliseconds: u32,
    pub center_return_speed_milli_tiles_per_second: u32,
    pub center_stop_radius_milli_tiles: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusPhaseThreeRecord {
    pub loop_milliseconds: u32,
    pub low_health_loop_milliseconds: u32,
    pub preview_windows_milliseconds: Vec<[u32; 2]>,
    pub ring_emissions_milliseconds: Vec<u32>,
    pub shield_start_milliseconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusBossRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub arena_id: ContentId,
    pub reward_profile_id: ContentId,
    pub xp_profile_id: ContentId,
    pub exit_id: ContentId,
    pub base_health: u32,
    pub additional_participant_health_basis_points: u16,
    pub armor: u16,
    pub recommended_level: u8,
    pub recommended_item_level: u8,
    pub target_solo_duration_minimum_milliseconds: u32,
    pub target_solo_duration_maximum_milliseconds: u32,
    pub collision_radius_milli_tiles: u32,
    pub hurtbox_radius_milli_tiles: u32,
    pub contact_damage: u32,
    pub resistance_basis_points: u16,
    pub spawn: MilliTilePoint,
    pub stage: MilliTilePoint,
    pub group_anchors: Vec<MilliTilePoint>,
    pub arena_center: MilliTilePoint,
    pub charge_endpoints: Vec<MilliTilePoint>,
    pub participant_lock: CoreCaldusParticipantLockRecord,
    pub phase_two_threshold_percent: u8,
    pub phase_three_threshold_percent: u8,
    pub low_health_threshold_percent: u8,
    pub phase_break_milliseconds: u32,
    pub break_incoming_damage_basis_points: u16,
    pub soft_enrage_milliseconds: u32,
    pub soft_enrage_downtime_basis_points: u16,
    pub phase_one: CoreCaldusPhaseOneRecord,
    pub phase_two: CoreCaldusPhaseTwoRecord,
    pub phase_three: CoreCaldusPhaseThreeRecord,
    pub pattern_ids: Vec<ContentId>,
    pub stationary_phase_numbers: Vec<u8>,
    pub phase_two_movement_is_charge_and_center_return_only: bool,
    pub authored_core_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "pattern_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreCaldusPatternPayload {
    ShieldArc {
        warning_milliseconds: u32,
        projectile_count: u8,
        total_arc_milli_degrees: u32,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
        group_target_thresholds: Vec<[u8; 2]>,
        group_stagger_milliseconds: u32,
    },
    BellRing {
        warning_milliseconds: u32,
        index_count: u8,
        omitted_adjacent_count: u8,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
    ChargeLane {
        warning_milliseconds: u32,
        width_milli_tiles: u32,
        travel_milli_tiles: u32,
        travel_milliseconds: u32,
        maximum_hits_per_player_per_cast: u8,
    },
    ChargeStopRing {
        parent_pattern_id: ContentId,
        index_count: u8,
        omitted_adjacent_count: u8,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreCaldusDamageBand {
    Major,
    Severe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreCaldusCounterplay {
    Strafe,
    FollowGap,
    LeaveTelegraph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreCaldusMemoryFamily {
    FanProjectile,
    RadialProjectile,
    ChargeOrContact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreCaldusDisposition {
    ConsumeOnPlayerOrSolid,
    OneContactHitPerCast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreCaldusAttackGroupRule {
    DistinctProjectileHitGroups,
    OneContactHitPerCast,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusPatternRecord {
    pub id: ContentId,
    pub owner_id: ContentId,
    pub telegraph_id: ContentId,
    pub audio_cue_id: ContentId,
    pub major_audio_cue_id: ContentId,
    pub damage_type: DamageType,
    pub damage_band: CoreCaldusDamageBand,
    pub raw_damage: u32,
    pub threat_cost: u16,
    pub counterplay: CoreCaldusCounterplay,
    pub memory_family: CoreCaldusMemoryFamily,
    pub disposition: CoreCaldusDisposition,
    pub attack_group_rule: CoreCaldusAttackGroupRule,
    pub acceleration_milli_tiles_per_second_squared: u32,
    pub pierces_players: bool,
    pub statuses: Vec<ContentId>,
    pub cancel_on_phase_change: bool,
    pub fevered_repeat_eligible: bool,
    pub maximum_active_instances: u16,
    pub payload: CoreCaldusPatternPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusExitRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub arena_id: ContentId,
    pub boss_id: ContentId,
    pub required_reward_profile_id: ContentId,
    pub point: MilliTilePoint,
    pub destination_content_id: ContentId,
    pub arrival: CoreCaldusSafeArrival,
    pub requires_committed_extraction_receipt: bool,
    pub authored_core_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreCaldusSafeArrival {
    HallDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusRoomBindingRecord {
    pub layout_id: ContentId,
    pub node_id: String,
    pub arena_id: ContentId,
    pub boss_id: ContentId,
    pub reward_profile_id: ContentId,
    pub exit_id: ContentId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusRecords {
    pub schema_version: u32,
    pub bosses: Vec<CoreCaldusBossRecord>,
    pub patterns: Vec<CoreCaldusPatternRecord>,
    pub exits: Vec<CoreCaldusExitRecord>,
    pub room_bindings: Vec<CoreCaldusRoomBindingRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreCaldusAssetKind {
    BossSilhouette,
    BossPortrait,
    Telegraph,
    WarningAudio,
    MajorWarningAudio,
    ExitMarker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusAsset {
    pub asset_id: ContentId,
    pub source_record_id: ContentId,
    pub kind: CoreCaldusAssetKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusAssetManifest {
    pub schema_version: u32,
    pub assets: Vec<CoreCaldusAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCaldusCopyFile {
    pub schema_version: u32,
    pub locale: String,
    pub entries: Vec<CoreLocalizedCopyEntry>,
}
