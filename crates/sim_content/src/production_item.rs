//! Semantic compiler for the unpromoted production-style item/reward contracts.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use content_schema::{
    EquipmentSlot, ProductionArmorFamily, ProductionEquipmentBehavior, ProductionEquipmentFamily,
    ProductionItemDevelopmentTarget, ProductionItemRarity, ProductionItemRecords,
    ProductionItemStagePolicyRecord, ProductionItemTemplatePayload, ProductionItemTemplateRecord,
    ProductionMaterialPoolRecord, ProductionRarityProfileRecord, ProductionRewardRoll,
    ProductionRewardTableRecord, SCHEMA_VERSION,
};

const BASIS_POINTS: u32 = 10_000;
const CORE_AFFIX_MANIFEST_ID: &str = "manifest.affixes.core";

#[derive(Debug, Clone)]
pub struct CompiledProductionItemCatalog {
    revision_label: String,
    items: BTreeMap<String, ProductionItemTemplateRecord>,
    rarity_profiles: BTreeMap<String, ProductionRarityProfileRecord>,
    reward_tables: BTreeMap<String, ProductionRewardTableRecord>,
    material_pools: BTreeMap<String, ProductionMaterialPoolRecord>,
    stage_policies: BTreeMap<String, ProductionItemStagePolicyRecord>,
}

impl CompiledProductionItemCatalog {
    #[must_use]
    pub fn revision_label(&self) -> &str {
        &self.revision_label
    }

    #[must_use]
    pub const fn items(&self) -> &BTreeMap<String, ProductionItemTemplateRecord> {
        &self.items
    }

    #[must_use]
    pub const fn rarity_profiles(&self) -> &BTreeMap<String, ProductionRarityProfileRecord> {
        &self.rarity_profiles
    }

    #[must_use]
    pub const fn reward_tables(&self) -> &BTreeMap<String, ProductionRewardTableRecord> {
        &self.reward_tables
    }

    #[must_use]
    pub const fn material_pools(&self) -> &BTreeMap<String, ProductionMaterialPoolRecord> {
        &self.material_pools
    }

    #[must_use]
    pub const fn stage_policies(&self) -> &BTreeMap<String, ProductionItemStagePolicyRecord> {
        &self.stage_policies
    }
}

pub fn compile_production_item_catalog(
    target: &ProductionItemDevelopmentTarget,
    records: &ProductionItemRecords,
) -> Result<CompiledProductionItemCatalog> {
    validate_target(target)?;
    let items = keyed(&records.items, |record| record.header.id.as_str())?;
    let rarity_profiles = keyed(&records.rarity_profiles, |record| record.header.id.as_str())?;
    let reward_tables = keyed(&records.reward_tables, |record| record.header.id.as_str())?;
    let material_pools = keyed(&records.material_pools, |record| record.header.id.as_str())?;
    let stage_policies = keyed(&records.stage_policies, |record| record.header.id.as_str())?;
    require_exact_ids(&target.required_item_ids, items.keys(), "item")?;
    require_exact_ids(
        &target.required_rarity_profile_ids,
        rarity_profiles.keys(),
        "rarity profile",
    )?;
    require_exact_ids(
        &target.required_reward_table_ids,
        reward_tables.keys(),
        "reward table",
    )?;
    require_exact_ids(
        &target.required_material_pool_ids,
        material_pools.keys(),
        "material pool",
    )?;
    require_exact_ids(
        &target.required_stage_policy_ids,
        stage_policies.keys(),
        "stage policy",
    )?;
    for item in items.values() {
        validate_item(item)?;
    }
    for profile in rarity_profiles.values() {
        validate_rarity_profile(profile)?;
    }
    for pool in material_pools.values() {
        validate_material_pool(pool, &items)?;
    }
    for reward in reward_tables.values() {
        validate_reward(reward, &rarity_profiles, &material_pools)?;
    }
    for policy in stage_policies.values() {
        validate_core_policy(policy, &rarity_profiles)?;
    }
    Ok(CompiledProductionItemCatalog {
        revision_label: format!("core-dev.blake3.{}", target.expected_manifest_blake3),
        items,
        rarity_profiles,
        reward_tables,
        material_pools,
        stage_policies,
    })
}

fn validate_target(target: &ProductionItemDevelopmentTarget) -> Result<()> {
    if target.schema_version != SCHEMA_VERSION || target.target_name.trim().is_empty() {
        bail!("production item target metadata is invalid");
    }
    for hash in [
        &target.expected_manifest_blake3,
        &target.expected_records_blake3,
        &target.expected_assets_blake3,
        &target.expected_localization_blake3,
    ] {
        if !valid_blake3(hash) {
            bail!("production item target contains an invalid BLAKE3 digest");
        }
    }
    Ok(())
}

fn keyed<T: Clone>(records: &[T], id: impl Fn(&T) -> &str) -> Result<BTreeMap<String, T>> {
    let mut map = BTreeMap::new();
    for record in records {
        let key = id(record).to_owned();
        if map.insert(key.clone(), record.clone()).is_some() {
            bail!("duplicate production item record `{key}`");
        }
    }
    Ok(map)
}

fn require_exact_ids<'a>(
    required: &[content_schema::ContentId],
    actual: impl Iterator<Item = &'a String>,
    kind: &str,
) -> Result<()> {
    if !strictly_sorted_unique(required.iter().map(content_schema::ContentId::as_str)) {
        bail!("required {kind} IDs must be sorted and unique");
    }
    let required = required
        .iter()
        .map(content_schema::ContentId::as_str)
        .collect::<BTreeSet<_>>();
    let actual = actual.map(String::as_str).collect::<BTreeSet<_>>();
    if required != actual {
        bail!("required {kind} IDs do not exactly match records");
    }
    Ok(())
}

fn validate_item(record: &ProductionItemTemplateRecord) -> Result<()> {
    validate_header(&record.header)?;
    match &record.payload {
        ProductionItemTemplatePayload::Equipment {
            slot,
            class_id,
            family,
            minimum_item_level,
            maximum_item_level,
            core_maximum_item_level_override,
            capability_tags,
            affix_exclusion_ids,
            behavior,
        } => {
            if *minimum_item_level == 0
                || minimum_item_level > maximum_item_level
                || *maximum_item_level > 20
                || !strictly_sorted_unique(capability_tags.iter().map(String::as_str))
                || capability_tags.iter().any(|tag| !valid_tag(tag))
                || !strictly_sorted_unique(
                    affix_exclusion_ids
                        .iter()
                        .map(content_schema::ContentId::as_str),
                )
            {
                bail!("item `{}` has invalid levels or tags", record.header.id);
            }
            let class_slot = matches!(slot, EquipmentSlot::Weapon | EquipmentSlot::Relic);
            if class_slot != class_id.is_some() {
                bail!("item `{}` has invalid class-slot pairing", record.header.id);
            }
            if let Some(override_level) = core_maximum_item_level_override
                && (!matches!(slot, EquipmentSlot::Armor | EquipmentSlot::Charm)
                    || override_level < maximum_item_level
                    || *override_level > 10)
            {
                bail!(
                    "item `{}` has invalid Core level override",
                    record.header.id
                );
            }
            validate_family_slot(*family, *slot)?;
            validate_behavior(*family, behavior)?;
        }
        ProductionItemTemplatePayload::Consumable {
            stack_cap,
            behavior,
        } => {
            if *stack_cap == 0 {
                bail!("consumable `{}` has zero stack cap", record.header.id);
            }
            match behavior {
                content_schema::ProductionConsumableBehavior::RedTonic {
                    restore_maximum_health_basis_points,
                    restore_duration_millis,
                    shared_cooldown_millis,
                    consumed_on_use,
                    ..
                } if *restore_maximum_health_basis_points > 0
                    && *restore_duration_millis > 0
                    && *shared_cooldown_millis > 0
                    && *consumed_on_use => {}
                content_schema::ProductionConsumableBehavior::PurifyingSalt {
                    removes_bleed,
                    removes_hex,
                    shared_cooldown_millis,
                    consumed_on_use,
                    ..
                } if *removes_bleed
                    && *removes_hex
                    && *shared_cooldown_millis > 0
                    && *consumed_on_use => {}
                _ => bail!("consumable `{}` behavior is invalid", record.header.id),
            }
        }
        ProductionItemTemplatePayload::Material {
            wallet_cap,
            pouch_stack_cap,
            reward_tags,
        } => {
            if *wallet_cap == 0
                || *pouch_stack_cap == 0
                || !strictly_sorted_unique(reward_tags.iter().map(String::as_str))
                || reward_tags.iter().any(|tag| !valid_tag(tag))
            {
                bail!("material `{}` is invalid", record.header.id);
            }
        }
    }
    Ok(())
}

fn validate_family_slot(family: ProductionEquipmentFamily, slot: EquipmentSlot) -> Result<()> {
    let valid = match slot {
        EquipmentSlot::Weapon => matches!(
            family,
            ProductionEquipmentFamily::Sword
                | ProductionEquipmentFamily::Crossbow
                | ProductionEquipmentFamily::HexFocus
        ),
        EquipmentSlot::Relic => matches!(
            family,
            ProductionEquipmentFamily::VanguardRelic
                | ProductionEquipmentFamily::ArbalistRelic
                | ProductionEquipmentFamily::WitchRelic
        ),
        EquipmentSlot::Armor => matches!(
            family,
            ProductionEquipmentFamily::Ashplate
                | ProductionEquipmentFamily::Gravehide
                | ProductionEquipmentFamily::Saltglass
                | ProductionEquipmentFamily::Pilgrim
                | ProductionEquipmentFamily::Rootweave
                | ProductionEquipmentFamily::Bellguard
        ),
        EquipmentSlot::Charm => family == ProductionEquipmentFamily::Charm,
    };
    if !valid {
        bail!("equipment family is illegal for its slot");
    }
    Ok(())
}

fn validate_behavior(
    family: ProductionEquipmentFamily,
    behavior: &ProductionEquipmentBehavior,
) -> Result<()> {
    match (family, behavior) {
        (
            ProductionEquipmentFamily::Crossbow,
            ProductionEquipmentBehavior::Crossbow {
                template_damage_scalar_basis_points,
                attack_interval_micros,
                range_milli_tiles,
                projectile_speed_milli_tiles_per_second,
                projectile_radius_milli_tiles,
                bolt_angles_milli_degrees,
                maximum_hits_per_target_per_release,
                second_target_damage_basis_points,
                ..
            },
        ) if *template_damage_scalar_basis_points > 0
            && *attack_interval_micros > 0
            && *range_milli_tiles > 0
            && *projectile_speed_milli_tiles_per_second > 0
            && *projectile_radius_milli_tiles > 0
            && !bolt_angles_milli_degrees.is_empty()
            && strictly_sorted_unique(bolt_angles_milli_degrees.iter().copied())
            && *maximum_hits_per_target_per_release > 0
            && second_target_damage_basis_points.is_none_or(|value| value > 0) =>
        {
            Ok(())
        }
        (
            ProductionEquipmentFamily::ArbalistRelic,
            ProductionEquipmentBehavior::ArbalistRelic { .. },
        )
        | (
            ProductionEquipmentFamily::Ashplate,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Ashplate,
            },
        )
        | (
            ProductionEquipmentFamily::Gravehide,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Gravehide,
            },
        )
        | (
            ProductionEquipmentFamily::Saltglass,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Saltglass,
            },
        )
        | (
            ProductionEquipmentFamily::Pilgrim,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Pilgrim,
            },
        )
        | (
            ProductionEquipmentFamily::Rootweave,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Rootweave,
            },
        )
        | (
            ProductionEquipmentFamily::Bellguard,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Bellguard,
            },
        )
        | (ProductionEquipmentFamily::Charm, ProductionEquipmentBehavior::Charm { .. }) => Ok(()),
        _ => bail!("equipment behavior does not match its family"),
    }
}

fn validate_rarity_profile(record: &ProductionRarityProfileRecord) -> Result<()> {
    validate_header(&record.header)?;
    if record.minimum_item_level == 0
        || record.minimum_item_level > record.maximum_item_level
        || record.maximum_item_level > 20
        || record.ordered_weights.is_empty()
    {
        bail!("rarity profile `{}` has invalid levels", record.header.id);
    }
    let mut rarities = BTreeSet::new();
    let mut total = 0_u32;
    for weight in &record.ordered_weights {
        if weight.rarity == ProductionItemRarity::Worn
            || weight.weight_basis_points == 0
            || !rarities.insert(weight.rarity)
        {
            bail!("rarity profile `{}` has invalid weights", record.header.id);
        }
        total += u32::from(weight.weight_basis_points);
    }
    if total != BASIS_POINTS {
        bail!("rarity profile `{}` does not total 10000", record.header.id);
    }
    Ok(())
}

fn validate_material_pool(
    record: &ProductionMaterialPoolRecord,
    items: &BTreeMap<String, ProductionItemTemplateRecord>,
) -> Result<()> {
    validate_header(&record.header)?;
    if record.ordered_outcomes.is_empty() {
        bail!("material pool `{}` is empty", record.header.id);
    }
    let mut total = 0_u32;
    for outcome in &record.ordered_outcomes {
        let item = items
            .get(outcome.item_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("material pool outcome is missing"))?;
        if matches!(
            item.payload,
            ProductionItemTemplatePayload::Equipment { .. }
        ) || outcome.quantity == 0
            || outcome.weight == 0
        {
            bail!(
                "material pool `{}` leaks an invalid outcome",
                record.header.id
            );
        }
        total += u32::from(outcome.weight);
    }
    if total != 100 {
        bail!(
            "material pool `{}` weights do not total 100",
            record.header.id
        );
    }
    Ok(())
}

fn validate_reward(
    record: &ProductionRewardTableRecord,
    rarity_profiles: &BTreeMap<String, ProductionRarityProfileRecord>,
    material_pools: &BTreeMap<String, ProductionMaterialPoolRecord>,
) -> Result<()> {
    validate_header(&record.header)?;
    if record.ordered_rolls.is_empty() {
        bail!("reward table `{}` is empty", record.header.id);
    }
    for roll in &record.ordered_rolls {
        let (presence, count) = match roll {
            ProductionRewardRoll::Equipment {
                presence_basis_points,
                count,
                rarity_profile_id,
            }
            | ProductionRewardRoll::UniversalItem {
                presence_basis_points,
                count,
                rarity_profile_id,
            } => {
                if !rarity_profiles.contains_key(rarity_profile_id.as_str()) {
                    bail!(
                        "reward table `{}` has a missing rarity profile",
                        record.header.id
                    );
                }
                (*presence_basis_points, *count)
            }
            ProductionRewardRoll::Material {
                presence_basis_points,
                count,
                material_pool_id,
            } => {
                if !material_pools.contains_key(material_pool_id.as_str()) {
                    bail!(
                        "reward table `{}` has a missing material pool",
                        record.header.id
                    );
                }
                (*presence_basis_points, *count)
            }
        };
        if presence == 0 || u32::from(presence) > BASIS_POINTS || count == 0 {
            bail!("reward table `{}` has an invalid roll", record.header.id);
        }
    }
    Ok(())
}

fn validate_core_policy(
    record: &ProductionItemStagePolicyRecord,
    rarity_profiles: &BTreeMap<String, ProductionRarityProfileRecord>,
) -> Result<()> {
    validate_header(&record.header)?;
    if record.current_class_weapon_relic_basis_points != 8_500
        || record.other_class_weapon_relic_basis_points != 0
        || record.universal_armor_charm_basis_points != 1_500
        || record.weapon_within_class_basis_points != 5_000
        || record.armor_within_universal_basis_points != 5_000
        || record.affix_manifest_id.as_str() != CORE_AFFIX_MANIFEST_ID
        || record.enabled_family_fragment_checks
        || record.enabled_cosmetic_checks
    {
        bail!("Core item stage policy `{}` is not exact", record.header.id);
    }
    let profile = record
        .fixed_rarity_profile_id
        .as_ref()
        .and_then(|id| rarity_profiles.get(id.as_str()))
        .ok_or_else(|| anyhow::anyhow!("Core fixed rarity profile is missing"))?;
    if profile.ordered_weights.len() != 1
        || profile.ordered_weights[0].rarity != ProductionItemRarity::Forged
        || profile.ordered_weights[0].weight_basis_points != 10_000
    {
        bail!("Core rarity profile must be exactly Forged 10000");
    }
    Ok(())
}

fn validate_header(header: &content_schema::CoreDevelopmentHeader) -> Result<()> {
    if header.schema_version != SCHEMA_VERSION
        || header.localization_name_key == header.localization_description_key
        || !strictly_sorted_unique(
            header
                .asset_ids
                .iter()
                .map(content_schema::ContentId::as_str),
        )
        || !strictly_sorted_unique(header.tags.iter().map(String::as_str))
    {
        bail!("production item header `{}` is invalid", header.id);
    }
    Ok(())
}

fn strictly_sorted_unique<T: Ord>(values: impl IntoIterator<Item = T>) -> bool {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|previous| previous >= &value) {
            return false;
        }
        previous = Some(value);
    }
    true
}

fn valid_tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_')
        })
}

fn valid_blake3(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use content_schema::{
        ContentId, CoreDevelopmentHeader, ProductionConsumableBehavior,
        ProductionItemStagePolicyRecord, ProductionItemTargetKind, ProductionMaterialPoolOutcome,
        ProductionRarityWeight, ReleaseStage,
    };

    use super::*;

    fn id(value: &str) -> ContentId {
        ContentId::parse(value).unwrap()
    }

    fn header(value: &str) -> CoreDevelopmentHeader {
        CoreDevelopmentHeader {
            id: id(value),
            schema_version: 1,
            enabled: true,
            earliest_release_stage: ReleaseStage::Core,
            localization_name_key: id(&format!("{value}.name")),
            localization_description_key: id(&format!("{value}.description")),
            asset_ids: vec![id(&format!("icon.{value}"))],
            tags: vec!["item".to_owned()],
            source_document_feature_id: "CONT-CATALOG-002".to_owned(),
        }
    }

    #[allow(clippy::too_many_lines)] // The complete cross-record fixture is intentionally visible.
    fn fixture() -> (ProductionItemDevelopmentTarget, ProductionItemRecords) {
        let items = vec![
            ProductionItemTemplateRecord {
                header: header("consumable.red_tonic"),
                payload: ProductionItemTemplatePayload::Consumable {
                    stack_cap: 6,
                    behavior: ProductionConsumableBehavior::RedTonic {
                        restore_maximum_health_basis_points: 3_000,
                        restore_duration_millis: 400,
                        shared_cooldown_millis: 2_000,
                        damage_interrupts_restore: false,
                        consumed_on_use: true,
                    },
                },
            },
            ProductionItemTemplateRecord {
                header: header("item.weapon.crossbow.pine_crossbow"),
                payload: ProductionItemTemplatePayload::Equipment {
                    slot: EquipmentSlot::Weapon,
                    class_id: Some(id("class.grave_arbalist")),
                    family: ProductionEquipmentFamily::Crossbow,
                    minimum_item_level: 1,
                    maximum_item_level: 20,
                    core_maximum_item_level_override: None,
                    capability_tags: vec!["family.crossbow".to_owned(), "modifiable.W".to_owned()],
                    affix_exclusion_ids: vec![],
                    behavior: ProductionEquipmentBehavior::Crossbow {
                        template_damage_scalar_basis_points: 10_000,
                        attack_interval_micros: 454_545,
                        range_milli_tiles: 9_500,
                        projectile_speed_milli_tiles_per_second: 14_000,
                        projectile_radius_milli_tiles: 100,
                        bolt_angles_milli_degrees: vec![0],
                        maximum_hits_per_target_per_release: 1,
                        pierce_count: 0,
                        second_target_damage_basis_points: None,
                    },
                },
            },
        ];
        let rarity_profiles = vec![ProductionRarityProfileRecord {
            header: header("rarity.core_fixed"),
            minimum_item_level: 1,
            maximum_item_level: 10,
            ordered_weights: vec![ProductionRarityWeight {
                rarity: ProductionItemRarity::Forged,
                weight_basis_points: 10_000,
            }],
        }];
        let material_pools = vec![ProductionMaterialPoolRecord {
            header: header("material.core_tonic"),
            ordered_outcomes: vec![ProductionMaterialPoolOutcome {
                item_id: id("consumable.red_tonic"),
                quantity: 1,
                weight: 100,
            }],
        }];
        let reward_tables = vec![ProductionRewardTableRecord {
            header: header("reward.normal_outer"),
            ordered_rolls: vec![
                ProductionRewardRoll::UniversalItem {
                    presence_basis_points: 800,
                    count: 1,
                    rarity_profile_id: id("rarity.core_fixed"),
                },
                ProductionRewardRoll::Material {
                    presence_basis_points: 1_200,
                    count: 1,
                    material_pool_id: id("material.core_tonic"),
                },
            ],
        }];
        let stage_policies = vec![ProductionItemStagePolicyRecord {
            header: header("policy.items.core"),
            current_class_weapon_relic_basis_points: 8_500,
            other_class_weapon_relic_basis_points: 0,
            universal_armor_charm_basis_points: 1_500,
            weapon_within_class_basis_points: 5_000,
            armor_within_universal_basis_points: 5_000,
            fixed_rarity_profile_id: Some(id("rarity.core_fixed")),
            affix_manifest_id: id("manifest.affixes.core"),
            enabled_family_fragment_checks: false,
            enabled_cosmetic_checks: false,
        }];
        let records = ProductionItemRecords {
            items,
            rarity_profiles,
            reward_tables,
            material_pools,
            stage_policies,
        };
        let target = ProductionItemDevelopmentTarget {
            schema_version: 1,
            target_kind: ProductionItemTargetKind::UnpromotedItemRewardSubset,
            target_name: "core-items-dev".to_owned(),
            required_item_ids: vec![
                id("consumable.red_tonic"),
                id("item.weapon.crossbow.pine_crossbow"),
            ],
            required_rarity_profile_ids: vec![id("rarity.core_fixed")],
            required_reward_table_ids: vec![id("reward.normal_outer")],
            required_material_pool_ids: vec![id("material.core_tonic")],
            required_stage_policy_ids: vec![id("policy.items.core")],
            expected_manifest_blake3: "a".repeat(64),
            expected_records_blake3: "b".repeat(64),
            expected_assets_blake3: "c".repeat(64),
            expected_localization_blake3: "d".repeat(64),
        };
        (target, records)
    }

    #[test]
    fn exact_core_fixture_compiles_to_immutable_development_revision() {
        let (target, records) = fixture();
        let compiled = compile_production_item_catalog(&target, &records).unwrap();
        assert_eq!(
            compiled.revision_label(),
            format!("core-dev.blake3.{}", "a".repeat(64))
        );
        assert_eq!(compiled.items().len(), 2);
    }

    #[test]
    fn category_leak_bad_policy_and_unsorted_target_fail_closed() {
        let (target, mut records) = fixture();
        records.material_pools[0].ordered_outcomes[0].item_id =
            id("item.weapon.crossbow.pine_crossbow");
        assert!(compile_production_item_catalog(&target, &records).is_err());

        let (target, mut records) = fixture();
        records.stage_policies[0].current_class_weapon_relic_basis_points = 7_500;
        assert!(compile_production_item_catalog(&target, &records).is_err());

        let (mut target, records) = fixture();
        target.required_item_ids.reverse();
        assert!(compile_production_item_catalog(&target, &records).is_err());
    }

    #[test]
    fn template_rarity_is_instance_only_and_fixed_profile_is_exact() {
        let (target, mut records) = fixture();
        records.rarity_profiles[0].ordered_weights[0].rarity = ProductionItemRarity::Oathed;
        assert!(compile_production_item_catalog(&target, &records).is_err());

        let (target, mut records) = fixture();
        records.items[1].payload = ProductionItemTemplatePayload::Equipment {
            slot: EquipmentSlot::Armor,
            class_id: Some(id("class.grave_arbalist")),
            family: ProductionEquipmentFamily::Ashplate,
            minimum_item_level: 1,
            maximum_item_level: 6,
            core_maximum_item_level_override: Some(10),
            capability_tags: vec!["slot.armor".to_owned()],
            affix_exclusion_ids: vec![],
            behavior: ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Ashplate,
            },
        };
        assert!(compile_production_item_catalog(&target, &records).is_err());
    }
}
