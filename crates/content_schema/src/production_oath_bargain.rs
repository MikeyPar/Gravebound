//! Strict production Oath, Bargain, and temporary Core shrine contracts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ContentId, CoreDevelopmentHeader};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OathBargainTargetKind {
    UnpromotedOathBargainSubset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OathBargainDevelopmentTarget {
    pub schema_version: u32,
    pub target_kind: OathBargainTargetKind,
    pub target_name: String,
    pub required_oath_ids: Vec<ContentId>,
    pub required_bargain_ids: Vec<ContentId>,
    pub expected_manifest_blake3: String,
    pub expected_records_blake3: String,
    pub expected_assets_blake3: String,
    pub expected_localization_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OathBargainRecords {
    pub oaths: Vec<OathRecord>,
    pub bargains: Vec<BargainRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OathRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub class_id: ContentId,
    pub unlock_level: u8,
    pub resolution_step: u8,
    pub behavior: OathBehavior,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "oath_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OathBehavior {
    LongVigil {
        focused_activation_millis: u32,
        grave_mark_range_bonus_milli_tiles: u32,
        grave_mark_primary_bonus_basis_points: u16,
        maximum_health_multiplier_basis_points: u16,
    },
    Nailkeeper {
        trap_radius_milli_tiles: u32,
        arm_delay_millis: u32,
        lifetime_millis: u32,
        direct_damage_coefficient_basis_points: u16,
        frostbind_duration_millis: u32,
        maximum_live_traps: u8,
        primary_attack_rate_multiplier_basis_points: u16,
        create_on_enemy_impact: bool,
        create_on_solid_impact: bool,
        enemy_impact_applies_grave_mark_first: bool,
        solid_impact_applies_grave_mark: bool,
        requires_exit_after_arming_for_existing_occupants: bool,
        consumes_on_first_legal_enemy_entry: bool,
        snapshots_weapon_power_at_creation: bool,
        overflow_order: OathTrapOverflowOrder,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OathTrapOverflowOrder {
    CreatedTickThenEntityId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BargainRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub maximum_active_per_character: u8,
    pub resolution_step: u8,
    pub behavior: BargainBehavior,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "bargain_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BargainBehavior {
    CinderHunger {
        outgoing_direct_damage_multiplier_basis_points: u16,
        maximum_health_multiplier_basis_points: u16,
    },
    BellDebt {
        accepted_primary_emissions_per_repeat: u8,
        repeat_delay_millis: u32,
        repeat_damage_multiplier_basis_points: u16,
        primary_attack_rate_multiplier_basis_points: u16,
        counts_legal_misses: bool,
        generated_repeats_advance_counter: bool,
        snapshots_aim_and_resolved_behavior: bool,
        uses_live_origin_at_repeat: bool,
        repeat_is_recursive: bool,
        repeat_spends_cooldown_or_resource: bool,
        counter_persists_reconnect_and_room_change: bool,
        counter_resets_on_acquisition_purge_death_retirement_or_safe_transfer: bool,
        cancel_pending_repeat_when_dead_transferred_or_primary_illegal: bool,
    },
    LanternAsh {
        potion_healing_multiplier_basis_points: u16,
        active_belt_slot_count: u8,
        active_belt_index: u8,
        inactive_slot_remains_stored_visible_locked: bool,
    },
}
