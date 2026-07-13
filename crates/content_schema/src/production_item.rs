//! Strict production-style item and reward authoring contracts.
//!
//! These types are intentionally parallel to the immutable First Playable prototype schemas.
//! Rarity and rolled affixes belong to item instances, never shared templates.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ContentId, CoreDevelopmentHeader, EquipmentSlot};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProductionItemTargetKind {
    UnpromotedItemRewardSubset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductionItemDevelopmentTarget {
    pub schema_version: u32,
    pub target_kind: ProductionItemTargetKind,
    pub target_name: String,
    pub required_item_ids: Vec<ContentId>,
    pub required_rarity_profile_ids: Vec<ContentId>,
    pub required_reward_table_ids: Vec<ContentId>,
    pub required_material_pool_ids: Vec<ContentId>,
    pub required_stage_policy_ids: Vec<ContentId>,
    /// Exact lowercase BLAKE3 digest of the complete item/reward manifest bytes.
    pub expected_manifest_blake3: String,
    pub expected_records_blake3: String,
    pub expected_assets_blake3: String,
    pub expected_localization_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductionItemRecords {
    pub items: Vec<ProductionItemTemplateRecord>,
    pub rarity_profiles: Vec<ProductionRarityProfileRecord>,
    pub reward_tables: Vec<ProductionRewardTableRecord>,
    pub material_pools: Vec<ProductionMaterialPoolRecord>,
    pub stage_policies: Vec<ProductionItemStagePolicyRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductionItemTemplateRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub payload: ProductionItemTemplatePayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "item_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductionItemTemplatePayload {
    Equipment {
        slot: EquipmentSlot,
        class_id: Option<ContentId>,
        family: ProductionEquipmentFamily,
        minimum_item_level: u8,
        maximum_item_level: u8,
        core_maximum_item_level_override: Option<u8>,
        capability_tags: Vec<String>,
        affix_exclusion_ids: Vec<ContentId>,
        behavior: ProductionEquipmentBehavior,
    },
    Consumable {
        stack_cap: u8,
        behavior: ProductionConsumableBehavior,
    },
    Material {
        wallet_cap: u16,
        pouch_stack_cap: u8,
        reward_tags: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProductionEquipmentFamily {
    Sword,
    Crossbow,
    HexFocus,
    VanguardRelic,
    ArbalistRelic,
    WitchRelic,
    Ashplate,
    Gravehide,
    Saltglass,
    Pilgrim,
    Rootweave,
    Bellguard,
    Charm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "behavior_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductionEquipmentBehavior {
    Crossbow {
        template_damage_scalar_basis_points: u16,
        attack_interval_micros: u32,
        range_milli_tiles: u32,
        projectile_speed_milli_tiles_per_second: u32,
        projectile_radius_milli_tiles: u16,
        bolt_angles_milli_degrees: Vec<i32>,
        maximum_hits_per_target_per_release: u8,
        pierce_count: u8,
        second_target_damage_basis_points: Option<u16>,
    },
    ArbalistRelic {
        mark: Option<ArbalistMarkReplacement>,
        slipstep: Option<ArbalistSlipstepReplacement>,
        stillness: Option<ArbalistStillnessReplacement>,
    },
    Armor {
        family: ProductionArmorFamily,
    },
    Charm {
        effect: ProductionCharmEffect,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArbalistMarkReplacement {
    pub range_milli_tiles: Option<u32>,
    pub projectile_speed_milli_tiles_per_second: Option<u32>,
    pub direct_damage_coefficient_basis_points: Option<u16>,
    pub duration_millis: Option<u32>,
    pub primary_bonus_basis_points: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArbalistSlipstepReplacement {
    pub distance_milli_tiles: Option<u32>,
    pub duration_millis: Option<u32>,
    pub damage_reduction_basis_points: Option<u16>,
    pub cooldown_millis: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArbalistStillnessReplacement {
    pub activation_millis: Option<u32>,
    pub projectile_speed_bonus_basis_points: Option<u16>,
    pub primary_damage_bonus_basis_points: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProductionArmorFamily {
    Ashplate,
    Gravehide,
    Saltglass,
    Pilgrim,
    Rootweave,
    Bellguard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "effect_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductionCharmEffect {
    RestedPrimaryDamage {
        idle_millis: u32,
        bonus_basis_points: u16,
        consumed_on_release: bool,
    },
    PotionHealing {
        bonus_basis_points: u16,
    },
    NamedNegativeStatusDuration {
        reduction_basis_points: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "effect_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductionConsumableBehavior {
    RedTonic {
        restore_maximum_health_basis_points: u16,
        restore_duration_millis: u32,
        shared_cooldown_millis: u32,
        damage_interrupts_restore: bool,
        consumed_on_use: bool,
    },
    PurifyingSalt {
        removes_bleed: bool,
        removes_hex: bool,
        restore_maximum_health_basis_points: u16,
        shared_cooldown_millis: u32,
        consumed_on_use: bool,
    },
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProductionItemRarity {
    Worn,
    Forged,
    Oathed,
    Relic,
    Sainted,
    BlackUnique,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductionRarityWeight {
    pub rarity: ProductionItemRarity,
    pub weight_basis_points: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductionRarityProfileRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub minimum_item_level: u8,
    pub maximum_item_level: u8,
    pub ordered_weights: Vec<ProductionRarityWeight>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductionRewardTableRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub ordered_rolls: Vec<ProductionRewardRoll>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "roll_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductionRewardRoll {
    Equipment {
        presence_basis_points: u16,
        count: u8,
        rarity_profile_id: ContentId,
    },
    UniversalItem {
        presence_basis_points: u16,
        count: u8,
        rarity_profile_id: ContentId,
    },
    Material {
        presence_basis_points: u16,
        count: u8,
        material_pool_id: ContentId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductionMaterialPoolOutcome {
    pub item_id: ContentId,
    pub quantity: u16,
    pub weight: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductionMaterialPoolRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub ordered_outcomes: Vec<ProductionMaterialPoolOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductionItemStagePolicyRecord {
    #[serde(flatten)]
    pub header: CoreDevelopmentHeader,
    pub current_class_weapon_relic_basis_points: u16,
    pub other_class_weapon_relic_basis_points: u16,
    pub universal_armor_charm_basis_points: u16,
    pub weapon_within_class_basis_points: u16,
    pub armor_within_universal_basis_points: u16,
    pub maximum_item_level: u8,
    pub fixed_rarity_profile_id: Option<ContentId>,
    pub affix_manifest_id: ContentId,
    pub enabled_family_fragment_checks: bool,
    pub enabled_cosmetic_checks: bool,
}
