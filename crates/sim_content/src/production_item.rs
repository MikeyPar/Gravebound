//! Semantic compiler for the unpromoted production-style item/reward contracts.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use content_schema::{
    CoreWorldFlowCopyFile, EquipmentSlot, ProductionArmorFamily, ProductionEquipmentBehavior,
    ProductionEquipmentFamily, ProductionItemAssetKind, ProductionItemAssetManifest,
    ProductionItemDevelopmentTarget, ProductionItemRarity, ProductionItemRecords,
    ProductionItemStagePolicyRecord, ProductionItemTemplatePayload, ProductionItemTemplateRecord,
    ProductionMaterialPoolRecord, ProductionRarityProfileRecord, ProductionRewardRoll,
    ProductionRewardTableRecord, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const BASIS_POINTS: u32 = 10_000;
const CORE_AFFIX_MANIFEST_ID: &str = "manifest.affixes.core";

#[derive(Debug, Clone)]
pub struct CompiledProductionItemCatalog {
    target_name: String,
    hashes: ProductionItemSourceHashes,
    revision_label: String,
    items: BTreeMap<String, ProductionItemTemplateRecord>,
    rarity_profiles: BTreeMap<String, ProductionRarityProfileRecord>,
    reward_tables: BTreeMap<String, ProductionRewardTableRecord>,
    material_pools: BTreeMap<String, ProductionMaterialPoolRecord>,
    stage_policies: BTreeMap<String, ProductionItemStagePolicyRecord>,
}

impl CompiledProductionItemCatalog {
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub const fn hashes(&self) -> &ProductionItemSourceHashes {
        &self.hashes
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionItemSourceHashes {
    pub manifest_blake3: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

#[derive(Serialize)]
struct ProductionItemManifestDigest<'a> {
    schema_version: u32,
    records_blake3: &'a str,
    assets_blake3: &'a str,
    localization_blake3: &'a str,
}

/// Loads and verifies the independently hashed, non-promotable Core item target.
pub fn load_core_development_items(root: &Path) -> Result<CompiledProductionItemCatalog> {
    let core = root.join("core_dev");
    let target_bytes = read_bytes(&core.join("items.json"))?;
    let records_bytes = read_bytes(&core.join("items.records.json"))?;
    let assets_bytes = read_bytes(&core.join("items.assets.json"))?;
    let localization_bytes = read_bytes(&core.join("items.en-US.json"))?;
    let target: ProductionItemDevelopmentTarget = parse_json(&target_bytes, "items.json")?;
    let records: ProductionItemRecords = parse_json(&records_bytes, "items.records.json")?;
    let assets: ProductionItemAssetManifest = parse_json(&assets_bytes, "items.assets.json")?;
    let localization: CoreWorldFlowCopyFile = parse_json(&localization_bytes, "items.en-US.json")?;
    let hashes = source_hashes(&records_bytes, &assets_bytes, &localization_bytes)?;
    validate_source_hashes(&target, &hashes)?;
    validate_item_assets(&records, &assets)?;
    validate_item_localization(&records, &localization)?;
    let mut compiled = compile_production_item_catalog(&target, &records)?;
    compiled.hashes = hashes;
    validate_exact_core_reward_closure(&compiled)?;
    Ok(compiled)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("failed to read {}", path.display()))
}

fn parse_json<T: for<'de> Deserialize<'de>>(bytes: &[u8], name: &str) -> Result<T> {
    serde_json::from_slice(bytes).with_context(|| format!("invalid Core item source {name}"))
}

fn source_hashes(
    records: &[u8],
    assets: &[u8],
    localization: &[u8],
) -> Result<ProductionItemSourceHashes> {
    let records_blake3 = blake3::hash(records).to_hex().to_string();
    let assets_blake3 = blake3::hash(assets).to_hex().to_string();
    let localization_blake3 = blake3::hash(localization).to_hex().to_string();
    let digest = ProductionItemManifestDigest {
        schema_version: SCHEMA_VERSION,
        records_blake3: &records_blake3,
        assets_blake3: &assets_blake3,
        localization_blake3: &localization_blake3,
    };
    let manifest_bytes = serde_json::to_vec(&digest).context("failed to encode item manifest")?;
    Ok(ProductionItemSourceHashes {
        manifest_blake3: blake3::hash(&manifest_bytes).to_hex().to_string(),
        records_blake3,
        assets_blake3,
        localization_blake3,
    })
}

fn validate_source_hashes(
    target: &ProductionItemDevelopmentTarget,
    actual: &ProductionItemSourceHashes,
) -> Result<()> {
    let expected = ProductionItemSourceHashes {
        manifest_blake3: target.expected_manifest_blake3.clone(),
        records_blake3: target.expected_records_blake3.clone(),
        assets_blake3: target.expected_assets_blake3.clone(),
        localization_blake3: target.expected_localization_blake3.clone(),
    };
    if expected != *actual {
        bail!("Core item source hash mismatch: expected {expected:?}; actual {actual:?}");
    }
    Ok(())
}

fn validate_item_assets(
    records: &ProductionItemRecords,
    manifest: &ProductionItemAssetManifest,
) -> Result<()> {
    if manifest.schema_version != SCHEMA_VERSION
        || !strictly_sorted_unique(manifest.assets.iter().map(|asset| asset.asset_id.as_str()))
        || manifest.assets.len() != records.items.len()
    {
        bail!("Core item asset manifest metadata is invalid");
    }
    for (item, asset) in records.items.iter().zip(&manifest.assets) {
        let expected = format!("icon.{}", item.header.id);
        if asset.kind != ProductionItemAssetKind::ItemIcon
            || asset.source_record_id != item.header.id
            || asset.asset_id.as_str() != expected
            || item.header.asset_ids.as_slice() != [asset.asset_id.clone()]
        {
            bail!("Core item icon closure failed for `{}`", item.header.id);
        }
    }
    Ok(())
}

fn validate_item_localization(
    records: &ProductionItemRecords,
    copy: &CoreWorldFlowCopyFile,
) -> Result<()> {
    let required = records
        .items
        .iter()
        .flat_map(|item| {
            [
                item.header.localization_description_key.as_str(),
                item.header.localization_name_key.as_str(),
            ]
        })
        .collect::<Vec<_>>();
    if copy.schema_version != SCHEMA_VERSION
        || copy.locale != "en-US"
        || copy.entries.len() != required.len()
        || !strictly_sorted_unique(copy.entries.iter().map(|entry| entry.key.as_str()))
        || copy.entries.iter().zip(required).any(|(entry, key)| {
            entry.key.as_str() != key
                || entry.value.trim().is_empty()
                || entry
                    .value
                    .chars()
                    .any(|character| character.is_control() && character != '\n')
        })
    {
        bail!("Core item localization closure is invalid");
    }
    Ok(())
}

fn validate_exact_core_reward_closure(catalog: &CompiledProductionItemCatalog) -> Result<()> {
    let equipment_count = catalog
        .items
        .values()
        .filter(|item| {
            matches!(
                item.payload,
                ProductionItemTemplatePayload::Equipment { .. }
            )
        })
        .count();
    let consumable_count = catalog.items.len() - equipment_count;
    if catalog.items.len() != 18 || equipment_count != 17 || consumable_count != 1 {
        bail!("Core item catalog must contain exactly 17 equipment templates and one consumable");
    }
    validate_shared_red_tonic(catalog)?;
    require_exact_core_reward_payloads(catalog)?;
    let mut reachable = BTreeSet::new();
    for table in catalog.reward_tables.values() {
        for roll in &table.ordered_rolls {
            let (minimum, maximum, pools): (u8, u8, &[PlannedEquipmentPool]) = match roll {
                ProductionRewardRoll::Equipment {
                    minimum_item_level,
                    maximum_item_level,
                    ..
                } => (
                    *minimum_item_level,
                    *maximum_item_level,
                    &[
                        PlannedEquipmentPool::CurrentClass(EquipmentSlot::Weapon),
                        PlannedEquipmentPool::CurrentClass(EquipmentSlot::Relic),
                        PlannedEquipmentPool::Universal(EquipmentSlot::Armor),
                        PlannedEquipmentPool::Universal(EquipmentSlot::Charm),
                    ],
                ),
                ProductionRewardRoll::UniversalItem {
                    minimum_item_level,
                    maximum_item_level,
                    ..
                } => (
                    *minimum_item_level,
                    *maximum_item_level,
                    &[
                        PlannedEquipmentPool::Universal(EquipmentSlot::Armor),
                        PlannedEquipmentPool::Universal(EquipmentSlot::Charm),
                    ],
                ),
                ProductionRewardRoll::Material { .. } => continue,
            };
            for level in minimum..=maximum {
                for pool in pools {
                    let candidates = catalog
                        .items
                        .iter()
                        .filter(|(_, item)| {
                            legal_equipment_candidate(item, *pool, level, "class.grave_arbalist")
                        })
                        .map(|(id, _)| id.as_str())
                        .collect::<Vec<_>>();
                    if candidates.is_empty() {
                        bail!("Core reward pool has no candidate at item level {level}");
                    }
                    reachable.extend(candidates);
                }
            }
        }
    }
    let equipment_ids = catalog
        .items
        .iter()
        .filter(|(_, item)| {
            matches!(
                item.payload,
                ProductionItemTemplatePayload::Equipment { .. }
            )
        })
        .map(|(id, _)| id.as_str())
        .collect::<BTreeSet<_>>();
    if reachable != equipment_ids {
        bail!("not every Core equipment template is reachable from an authored reward source");
    }
    Ok(())
}

fn validate_shared_red_tonic(catalog: &CompiledProductionItemCatalog) -> Result<()> {
    let tonic = catalog
        .items
        .get("consumable.red_tonic")
        .context("Core catalog is missing the shared Red Tonic")?;
    if tonic.header.earliest_release_stage != content_schema::ReleaseStage::Fp
        || tonic.header.tags != ["belt", "consumable", "item"]
        || !matches!(
            tonic.payload,
            ProductionItemTemplatePayload::Consumable {
                stack_cap: 6,
                behavior: content_schema::ProductionConsumableBehavior::RedTonic {
                    restore_maximum_health_basis_points: 3_000,
                    restore_duration_millis: 400,
                    shared_cooldown_millis: 2_000,
                    damage_interrupts_restore: false,
                    consumed_on_use: true,
                },
            }
        )
    {
        bail!("Core Red Tonic differs from its stable First Playable lineage");
    }
    Ok(())
}

fn require_exact_core_reward_payloads(catalog: &CompiledProductionItemCatalog) -> Result<()> {
    let expected = [
        (
            "reward.boss_caldus",
            serde_json::json!([
                {"roll_kind":"equipment","presence_basis_points":10000,"count":2,"minimum_item_level":8,"maximum_item_level":10,"rarity_profile_id":"rarity.core_fixed"},
                {"roll_kind":"material","presence_basis_points":10000,"count":1,"material_pool_id":"material_pool.core.red_tonic_2"}
            ]),
        ),
        (
            "reward.elite_outer",
            serde_json::json!([
                {"roll_kind":"equipment","presence_basis_points":10000,"count":1,"minimum_item_level":2,"maximum_item_level":8,"rarity_profile_id":"rarity.core_fixed"},
                {"roll_kind":"material","presence_basis_points":2500,"count":1,"material_pool_id":"material_pool.core.red_tonic_1"}
            ]),
        ),
        (
            "reward.miniboss_t1",
            serde_json::json!([
                {"roll_kind":"equipment","presence_basis_points":10000,"count":1,"minimum_item_level":5,"maximum_item_level":10,"rarity_profile_id":"rarity.core_fixed"},
                {"roll_kind":"equipment","presence_basis_points":3500,"count":1,"minimum_item_level":5,"maximum_item_level":10,"rarity_profile_id":"rarity.core_fixed"}
            ]),
        ),
        (
            "reward.normal_outer",
            serde_json::json!([
                {"roll_kind":"universal_item","presence_basis_points":800,"count":1,"minimum_item_level":1,"maximum_item_level":6,"rarity_profile_id":"rarity.core_fixed"},
                {"roll_kind":"material","presence_basis_points":1200,"count":1,"material_pool_id":"material_pool.core.red_tonic_1"}
            ]),
        ),
    ];
    for (id, expected_rolls) in expected {
        let actual = catalog
            .reward_tables
            .get(id)
            .with_context(|| format!("missing exact Core reward table `{id}`"))?;
        if serde_json::to_value(&actual.ordered_rolls)? != expected_rolls {
            bail!("Core reward table `{id}` differs from the exact stage override");
        }
    }
    Ok(())
}

pub trait ProductionRewardDrawSource {
    fn draw_below(&mut self, upper_exclusive: u32) -> Result<u32, ProductionRewardPlanningError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionRewardPlanRequest<'a> {
    pub reward_table_id: &'a str,
    pub stage_policy_id: &'a str,
    pub current_class_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionRewardPlan {
    pub content_revision: String,
    pub reward_table_id: String,
    pub entries: Vec<ProductionRewardPlanEntry>,
}

impl ProductionRewardPlan {
    pub fn canonical_hash(&self) -> Result<[u8; 32], ProductionRewardPlanningError> {
        let encoded =
            serde_json::to_vec(self).map_err(|_| ProductionRewardPlanningError::EncodingFailed)?;
        Ok(*blake3::hash(&encoded).as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "entry_kind", rename_all = "snake_case")]
pub enum ProductionRewardPlanEntry {
    Equipment {
        roll_index: u16,
        template_id: String,
        item_level: u8,
        rarity: ProductionItemRarity,
    },
    Material {
        roll_index: u16,
        item_id: String,
        quantity: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlannedEquipmentPool {
    CurrentClass(EquipmentSlot),
    Universal(EquipmentSlot),
}

impl CompiledProductionItemCatalog {
    /// Resolves only immutable reward facts. UID allocation, affixes, placement, and ledgers are
    /// later transactional concerns and cannot be reached through this function.
    pub fn plan_reward(
        &self,
        request: &ProductionRewardPlanRequest<'_>,
        draws: &mut impl ProductionRewardDrawSource,
    ) -> Result<ProductionRewardPlan, ProductionRewardPlanningError> {
        let table = self
            .reward_tables
            .get(request.reward_table_id)
            .ok_or(ProductionRewardPlanningError::UnknownRewardTable)?;
        let policy = self
            .stage_policies
            .get(request.stage_policy_id)
            .ok_or(ProductionRewardPlanningError::UnknownStagePolicy)?;
        let fixed_rarity = policy
            .fixed_rarity_profile_id
            .as_ref()
            .and_then(|id| self.rarity_profiles.get(id.as_str()))
            .ok_or(ProductionRewardPlanningError::InvalidCompiledCatalog)?;
        let mut entries = Vec::new();
        let mut roll_index = 0_u16;
        for roll in &table.ordered_rolls {
            let (presence, count) = roll_presence_count(roll);
            let present = draws.draw_below(10_000)? < u32::from(presence);
            for _ in 0..count {
                let index = roll_index;
                roll_index = roll_index
                    .checked_add(1)
                    .ok_or(ProductionRewardPlanningError::RollIndexExhausted)?;
                if !present {
                    continue;
                }
                match roll {
                    ProductionRewardRoll::Equipment {
                        minimum_item_level,
                        maximum_item_level,
                        rarity_profile_id,
                        ..
                    } => {
                        let pool = choose_equipment_pool(policy, draws)?;
                        entries.push(self.plan_equipment(
                            index,
                            pool,
                            rarity_profile_id.as_str(),
                            *minimum_item_level,
                            *maximum_item_level,
                            fixed_rarity,
                            policy.maximum_item_level,
                            request.current_class_id,
                            draws,
                        )?);
                    }
                    ProductionRewardRoll::UniversalItem {
                        minimum_item_level,
                        maximum_item_level,
                        rarity_profile_id,
                        ..
                    } => {
                        let slot = if draws.draw_below(10_000)?
                            < u32::from(policy.armor_within_universal_basis_points)
                        {
                            EquipmentSlot::Armor
                        } else {
                            EquipmentSlot::Charm
                        };
                        entries.push(self.plan_equipment(
                            index,
                            PlannedEquipmentPool::Universal(slot),
                            rarity_profile_id.as_str(),
                            *minimum_item_level,
                            *maximum_item_level,
                            fixed_rarity,
                            policy.maximum_item_level,
                            request.current_class_id,
                            draws,
                        )?);
                    }
                    ProductionRewardRoll::Material {
                        material_pool_id, ..
                    } => {
                        entries.push(self.plan_material(
                            index,
                            material_pool_id.as_str(),
                            draws,
                        )?);
                    }
                }
            }
        }
        Ok(ProductionRewardPlan {
            content_revision: self.revision_label.clone(),
            reward_table_id: request.reward_table_id.to_owned(),
            entries,
        })
    }

    #[allow(clippy::too_many_arguments)] // Every argument is one authored reward dimension.
    fn plan_equipment(
        &self,
        roll_index: u16,
        pool: PlannedEquipmentPool,
        source_rarity_profile_id: &str,
        source_minimum_item_level: u8,
        source_maximum_item_level: u8,
        fixed_rarity: &ProductionRarityProfileRecord,
        stage_maximum_item_level: u8,
        current_class_id: &str,
        draws: &mut impl ProductionRewardDrawSource,
    ) -> Result<ProductionRewardPlanEntry, ProductionRewardPlanningError> {
        let _source_profile = self
            .rarity_profiles
            .get(source_rarity_profile_id)
            .ok_or(ProductionRewardPlanningError::InvalidCompiledCatalog)?;
        let maximum = source_maximum_item_level.min(stage_maximum_item_level);
        if source_minimum_item_level > maximum {
            return Err(ProductionRewardPlanningError::NoLegalItemLevel);
        }
        let level_width = u32::from(maximum - source_minimum_item_level) + 1;
        let item_level = source_minimum_item_level
            + u8::try_from(draws.draw_below(level_width)?)
                .map_err(|_| ProductionRewardPlanningError::DrawOutOfRange)?;
        let rarity = draw_rarity(fixed_rarity, draws)?;
        let candidates = self
            .items
            .iter()
            .filter_map(|(id, item)| {
                legal_equipment_candidate(item, pool, item_level, current_class_id)
                    .then_some(id.as_str())
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err(ProductionRewardPlanningError::NoLegalTemplate);
        }
        let selected = usize::try_from(
            draws.draw_below(
                u32::try_from(candidates.len())
                    .map_err(|_| ProductionRewardPlanningError::DrawOutOfRange)?,
            )?,
        )
        .map_err(|_| ProductionRewardPlanningError::DrawOutOfRange)?;
        Ok(ProductionRewardPlanEntry::Equipment {
            roll_index,
            template_id: candidates[selected].to_owned(),
            item_level,
            rarity,
        })
    }

    fn plan_material(
        &self,
        roll_index: u16,
        material_pool_id: &str,
        draws: &mut impl ProductionRewardDrawSource,
    ) -> Result<ProductionRewardPlanEntry, ProductionRewardPlanningError> {
        let pool = self
            .material_pools
            .get(material_pool_id)
            .ok_or(ProductionRewardPlanningError::InvalidCompiledCatalog)?;
        let draw = draws.draw_below(100)?;
        let mut cumulative = 0_u32;
        let selected = pool
            .ordered_outcomes
            .iter()
            .find(|outcome| {
                cumulative += u32::from(outcome.weight);
                draw < cumulative
            })
            .ok_or(ProductionRewardPlanningError::InvalidCompiledCatalog)?;
        Ok(ProductionRewardPlanEntry::Material {
            roll_index,
            item_id: selected.item_id.as_str().to_owned(),
            quantity: selected.quantity,
        })
    }
}

fn roll_presence_count(roll: &ProductionRewardRoll) -> (u16, u8) {
    match roll {
        ProductionRewardRoll::Equipment {
            presence_basis_points,
            count,
            ..
        }
        | ProductionRewardRoll::UniversalItem {
            presence_basis_points,
            count,
            ..
        }
        | ProductionRewardRoll::Material {
            presence_basis_points,
            count,
            ..
        } => (*presence_basis_points, *count),
    }
}

fn choose_equipment_pool(
    policy: &ProductionItemStagePolicyRecord,
    draws: &mut impl ProductionRewardDrawSource,
) -> Result<PlannedEquipmentPool, ProductionRewardPlanningError> {
    let usability = draws.draw_below(10_000)?;
    let current_end = u32::from(policy.current_class_weapon_relic_basis_points);
    let other_end = current_end + u32::from(policy.other_class_weapon_relic_basis_points);
    if usability < current_end {
        let slot = if draws.draw_below(10_000)? < u32::from(policy.weapon_within_class_basis_points)
        {
            EquipmentSlot::Weapon
        } else {
            EquipmentSlot::Relic
        };
        Ok(PlannedEquipmentPool::CurrentClass(slot))
    } else if usability < other_end {
        Err(ProductionRewardPlanningError::OtherClassUnavailable)
    } else {
        let slot =
            if draws.draw_below(10_000)? < u32::from(policy.armor_within_universal_basis_points) {
                EquipmentSlot::Armor
            } else {
                EquipmentSlot::Charm
            };
        Ok(PlannedEquipmentPool::Universal(slot))
    }
}

fn draw_rarity(
    profile: &ProductionRarityProfileRecord,
    draws: &mut impl ProductionRewardDrawSource,
) -> Result<ProductionItemRarity, ProductionRewardPlanningError> {
    let draw = draws.draw_below(10_000)?;
    let mut cumulative = 0_u32;
    profile
        .ordered_weights
        .iter()
        .find_map(|weight| {
            cumulative += u32::from(weight.weight_basis_points);
            (draw < cumulative).then_some(weight.rarity)
        })
        .ok_or(ProductionRewardPlanningError::InvalidCompiledCatalog)
}

fn legal_equipment_candidate(
    record: &ProductionItemTemplateRecord,
    pool: PlannedEquipmentPool,
    item_level: u8,
    current_class_id: &str,
) -> bool {
    let ProductionItemTemplatePayload::Equipment {
        slot,
        class_id,
        minimum_item_level,
        maximum_item_level,
        core_maximum_item_level_override,
        ..
    } = &record.payload
    else {
        return false;
    };
    let maximum = core_maximum_item_level_override.unwrap_or(*maximum_item_level);
    if !record.header.enabled
        || item_level < *minimum_item_level
        || item_level > maximum
        || *slot
            != match pool {
                PlannedEquipmentPool::CurrentClass(slot)
                | PlannedEquipmentPool::Universal(slot) => slot,
            }
    {
        return false;
    }
    match pool {
        PlannedEquipmentPool::CurrentClass(_) => class_id
            .as_ref()
            .is_some_and(|class| class.as_str() == current_class_id),
        PlannedEquipmentPool::Universal(_) => class_id.is_none(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProductionRewardPlanningError {
    #[error("reward table is not compiled")]
    UnknownRewardTable,
    #[error("item stage policy is not compiled")]
    UnknownStagePolicy,
    #[error("compiled item/reward catalog is internally inconsistent")]
    InvalidCompiledCatalog,
    #[error("bounded reward draw source returned an invalid value")]
    DrawOutOfRange,
    #[error("bounded reward draw source is exhausted")]
    DrawSourceExhausted,
    #[error("Core has no other-class equipment pool")]
    OtherClassUnavailable,
    #[error("reward profile has no legal Core item level")]
    NoLegalItemLevel,
    #[error("reward roll has no legal template")]
    NoLegalTemplate,
    #[error("reward roll index exhausted")]
    RollIndexExhausted,
    #[error("reward plan encoding failed")]
    EncodingFailed,
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
        target_name: target.target_name.clone(),
        hashes: ProductionItemSourceHashes {
            manifest_blake3: target.expected_manifest_blake3.clone(),
            records_blake3: target.expected_records_blake3.clone(),
            assets_blake3: target.expected_assets_blake3.clone(),
            localization_blake3: target.expected_localization_blake3.clone(),
        },
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
    validate_armor_behavior(behavior)?;
    if let ProductionEquipmentBehavior::Charm { effect } = behavior {
        validate_charm_behavior(effect)?;
    }
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
                ..
            },
        )
        | (
            ProductionEquipmentFamily::Gravehide,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Gravehide,
                ..
            },
        )
        | (
            ProductionEquipmentFamily::Saltglass,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Saltglass,
                ..
            },
        )
        | (
            ProductionEquipmentFamily::Pilgrim,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Pilgrim,
                ..
            },
        )
        | (
            ProductionEquipmentFamily::Rootweave,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Rootweave,
                ..
            },
        )
        | (
            ProductionEquipmentFamily::Bellguard,
            ProductionEquipmentBehavior::Armor {
                family: ProductionArmorFamily::Bellguard,
                ..
            },
        )
        | (ProductionEquipmentFamily::Charm, ProductionEquipmentBehavior::Charm { .. }) => Ok(()),
        _ => bail!("equipment behavior does not match its family"),
    }
}

fn validate_armor_behavior(behavior: &ProductionEquipmentBehavior) -> Result<()> {
    let ProductionEquipmentBehavior::Armor {
        affected_negative_statuses,
        excluded_negative_statuses,
        negative_status_duration_reduction_basis_points,
        direct_hit_barrier,
        ..
    } = behavior
    else {
        return Ok(());
    };
    if !strictly_sorted_unique(affected_negative_statuses.iter().copied())
        || !strictly_sorted_unique(excluded_negative_statuses.iter().copied())
        || affected_negative_statuses
            .iter()
            .any(|status| excluded_negative_statuses.contains(status))
        || (*negative_status_duration_reduction_basis_points == 0
            && !affected_negative_statuses.is_empty())
        || (*negative_status_duration_reduction_basis_points > 0
            && affected_negative_statuses.is_empty())
    {
        bail!("armor status behavior is invalid");
    }
    if let Some(barrier) = direct_hit_barrier
        && (barrier.triggering_damage_bands.is_empty()
            || !strictly_sorted_unique(barrier.triggering_damage_bands.iter().map(String::as_str))
            || barrier.raw_base_health_hundredths == 0
            || barrier.raw_health_per_level_hundredths == 0
            || barrier.duration_millis == 0
            || barrier.internal_cooldown_millis == 0
            || !barrier.cannot_retrigger_while_active)
    {
        bail!("armor direct-hit barrier is invalid");
    }
    Ok(())
}

fn validate_charm_behavior(effect: &content_schema::ProductionCharmEffect) -> Result<()> {
    match effect {
        content_schema::ProductionCharmEffect::RestedPrimaryDamage {
            idle_millis,
            bonus_basis_points,
            consumed_on_release_regardless_of_hit,
        } if *idle_millis > 0
            && *bonus_basis_points > 0
            && *consumed_on_release_regardless_of_hit =>
        {
            Ok(())
        }
        content_schema::ProductionCharmEffect::PotionHealing { bonus_basis_points }
            if *bonus_basis_points > 0 =>
        {
            Ok(())
        }
        content_schema::ProductionCharmEffect::NamedNegativeStatusDuration {
            reduction_basis_points,
            affected_statuses,
            excluded_statuses,
        } if *reduction_basis_points > 0
            && !affected_statuses.is_empty()
            && strictly_sorted_unique(affected_statuses.iter().copied())
            && strictly_sorted_unique(excluded_statuses.iter().copied())
            && affected_statuses
                .iter()
                .all(|status| !excluded_statuses.contains(status)) =>
        {
            Ok(())
        }
        _ => bail!("charm behavior is invalid"),
    }
}

fn validate_rarity_profile(record: &ProductionRarityProfileRecord) -> Result<()> {
    validate_infrastructure_header(&record.header)?;
    if record.ordered_weights.is_empty() {
        bail!("rarity profile `{}` has no weights", record.header.id);
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
    validate_infrastructure_header(&record.header)?;
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
    validate_infrastructure_header(&record.header)?;
    if record.ordered_rolls.is_empty() {
        bail!("reward table `{}` is empty", record.header.id);
    }
    for roll in &record.ordered_rolls {
        let (presence, count) = match roll {
            ProductionRewardRoll::Equipment {
                presence_basis_points,
                count,
                minimum_item_level,
                maximum_item_level,
                rarity_profile_id,
            }
            | ProductionRewardRoll::UniversalItem {
                presence_basis_points,
                count,
                minimum_item_level,
                maximum_item_level,
                rarity_profile_id,
            } => {
                if *minimum_item_level == 0
                    || minimum_item_level > maximum_item_level
                    || *maximum_item_level > 20
                    || !rarity_profiles.contains_key(rarity_profile_id.as_str())
                {
                    bail!(
                        "reward table `{}` has invalid item levels or rarity profile",
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
    validate_infrastructure_header(&record.header)?;
    if record.current_class_weapon_relic_basis_points != 8_500
        || record.other_class_weapon_relic_basis_points != 0
        || record.universal_armor_charm_basis_points != 1_500
        || record.weapon_within_class_basis_points != 5_000
        || record.armor_within_universal_basis_points != 5_000
        || record.maximum_item_level != 10
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

fn validate_infrastructure_header(
    header: &content_schema::ProductionInfrastructureHeader,
) -> Result<()> {
    if header.schema_version != SCHEMA_VERSION
        || !header.enabled
        || header.source_document_feature_id.trim().is_empty()
        || !strictly_sorted_unique(header.tags.iter().map(String::as_str))
    {
        bail!(
            "production infrastructure header `{}` is invalid",
            header.id
        );
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
    use std::path::PathBuf;

    use content_schema::{
        ContentId, CoreDevelopmentHeader, ProductionConsumableBehavior,
        ProductionInfrastructureHeader, ProductionItemStagePolicyRecord, ProductionItemTargetKind,
        ProductionMaterialPoolOutcome, ProductionRarityWeight, ReleaseStage,
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

    fn infrastructure_header(value: &str) -> ProductionInfrastructureHeader {
        ProductionInfrastructureHeader {
            id: id(value),
            schema_version: 1,
            enabled: true,
            earliest_release_stage: ReleaseStage::Core,
            tags: vec!["compiler_infrastructure".to_owned()],
            source_document_feature_id: "CONT-REWARD-004".to_owned(),
        }
    }

    fn ashplate_behavior() -> ProductionEquipmentBehavior {
        ProductionEquipmentBehavior::Armor {
            family: ProductionArmorFamily::Ashplate,
            raw_health_base_hundredths: 600,
            raw_health_per_level_hundredths: 100,
            raw_armor_base_hundredths: 200,
            raw_armor_per_level_hundredths: 30,
            raw_resistance_base_basis_points: 0,
            raw_resistance_per_level_basis_points: 0,
            fixed_movement_basis_points: -300,
            fixed_healing_received_basis_points: 0,
            negative_status_duration_reduction_basis_points: 0,
            affected_negative_statuses: vec![],
            excluded_negative_statuses: vec![],
            direct_hit_barrier: None,
        }
    }

    fn content_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn checked_in_core_item_target_has_exact_manifest_and_reward_closure() {
        let compiled = load_core_development_items(&content_root()).unwrap();
        assert_eq!(compiled.target_name(), "core-dev-items");
        assert_eq!(compiled.items().len(), 18);
        assert_eq!(compiled.reward_tables().len(), 4);
        assert_eq!(compiled.material_pools().len(), 2);
        assert_eq!(compiled.rarity_profiles().len(), 1);
        assert_eq!(compiled.stage_policies().len(), 1);
        assert_eq!(
            compiled.hashes().manifest_blake3,
            "3f1cb2c0e0638ea41b787b28fee4d108351fd6489da126359636a1f6c564519e"
        );
        assert_eq!(
            compiled.revision_label(),
            "core-dev.blake3.3f1cb2c0e0638ea41b787b28fee4d108351fd6489da126359636a1f6c564519e"
        );
        assert_eq!(
            compiled
                .items()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "consumable.red_tonic",
                "item.armor.ashplate.t1",
                "item.armor.bellguard.t1",
                "item.armor.gravehide.t1",
                "item.armor.pilgrim.t1",
                "item.armor.rootweave.t1",
                "item.armor.saltglass.t1",
                "item.charm.bell_locket.t1",
                "item.charm.ember_tooth.t1",
                "item.charm.salt_knot.t1",
                "item.relic.arbalist.barbed_ledger",
                "item.relic.arbalist.cracked_mark_lens",
                "item.relic.arbalist.long_lens",
                "item.relic.arbalist.slip_clasp",
                "item.weapon.crossbow.grave_repeater",
                "item.weapon.crossbow.mourners_fan",
                "item.weapon.crossbow.pilgrim_longbolt",
                "item.weapon.crossbow.pine_crossbow",
            ]
        );
    }

    #[test]
    fn core_source_closure_rejects_asset_copy_and_reward_drift() {
        let core = content_root().join("core_dev");
        let records: ProductionItemRecords =
            serde_json::from_slice(&fs::read(core.join("items.records.json")).unwrap()).unwrap();
        let mut assets: ProductionItemAssetManifest =
            serde_json::from_slice(&fs::read(core.join("items.assets.json")).unwrap()).unwrap();
        let copy: CoreWorldFlowCopyFile =
            serde_json::from_slice(&fs::read(core.join("items.en-US.json")).unwrap()).unwrap();
        validate_item_assets(&records, &assets).unwrap();
        validate_item_localization(&records, &copy).unwrap();

        assets.assets.swap(0, 1);
        assert!(validate_item_assets(&records, &assets).is_err());

        let target: ProductionItemDevelopmentTarget =
            serde_json::from_slice(&fs::read(core.join("items.json")).unwrap()).unwrap();
        let mut drifted = records;
        let ProductionRewardRoll::Equipment {
            maximum_item_level, ..
        } = &mut drifted.reward_tables[0].ordered_rolls[0]
        else {
            panic!("Caldus equipment roll");
        };
        *maximum_item_level = 9;
        let compiled = compile_production_item_catalog(&target, &drifted).unwrap();
        assert!(validate_exact_core_reward_closure(&compiled).is_err());
    }

    #[test]
    fn all_six_authored_armor_families_resolve_exactly_at_core_level_ten() {
        let compiled = load_core_development_items(&content_root()).unwrap();
        let expected = [
            ("item.armor.ashplate.t1", 16, 5, 0, None),
            ("item.armor.bellguard.t1", 18, 3, 0, Some(15)),
            ("item.armor.gravehide.t1", 32, 2, 0, None),
            ("item.armor.pilgrim.t1", 14, 1, 0, None),
            ("item.armor.rootweave.t1", 24, 2, 0, None),
            ("item.armor.saltglass.t1", 14, 1, 700, None),
        ];
        for (id, health, armor, resistance, barrier) in expected {
            let item = &compiled.items()[id];
            let ProductionItemTemplatePayload::Equipment {
                behavior:
                    ProductionEquipmentBehavior::Armor {
                        raw_health_base_hundredths,
                        raw_health_per_level_hundredths,
                        raw_armor_base_hundredths,
                        raw_armor_per_level_hundredths,
                        raw_resistance_base_basis_points,
                        raw_resistance_per_level_basis_points,
                        direct_hit_barrier,
                        ..
                    },
                ..
            } = &item.payload
            else {
                panic!("exact Core armor payload");
            };
            let resolved = sim_core::resolve_armor_base(sim_core::ArmorBaseRequest {
                item_level: 10,
                rarity: sim_core::EquipmentRarity::Forged,
                raw_health_base_hundredths: *raw_health_base_hundredths,
                raw_health_per_level_hundredths: *raw_health_per_level_hundredths,
                raw_armor_base_hundredths: *raw_armor_base_hundredths,
                raw_armor_per_level_hundredths: *raw_armor_per_level_hundredths,
                raw_resistance_base_basis_points: *raw_resistance_base_basis_points,
                raw_resistance_per_level_basis_points: *raw_resistance_per_level_basis_points,
                barrier_raw_base_health_hundredths: direct_hit_barrier
                    .as_ref()
                    .map(|value| value.raw_base_health_hundredths),
                barrier_raw_health_per_level_hundredths: direct_hit_barrier
                    .as_ref()
                    .map(|value| value.raw_health_per_level_hundredths),
            })
            .unwrap();
            assert_eq!(
                (
                    resolved.maximum_health,
                    resolved.armor,
                    resolved.resistance_basis_points,
                    resolved.direct_hit_barrier_health,
                ),
                (health, armor, resistance, barrier),
                "{id}"
            );
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
                header: header("item.armor.ashplate.t1"),
                payload: ProductionItemTemplatePayload::Equipment {
                    slot: EquipmentSlot::Armor,
                    class_id: None,
                    family: ProductionEquipmentFamily::Ashplate,
                    minimum_item_level: 1,
                    maximum_item_level: 6,
                    core_maximum_item_level_override: Some(10),
                    capability_tags: vec!["slot.armor".to_owned()],
                    affix_exclusion_ids: vec![],
                    behavior: ashplate_behavior(),
                },
            },
            ProductionItemTemplateRecord {
                header: header("item.charm.ember_tooth.t1"),
                payload: ProductionItemTemplatePayload::Equipment {
                    slot: EquipmentSlot::Charm,
                    class_id: None,
                    family: ProductionEquipmentFamily::Charm,
                    minimum_item_level: 1,
                    maximum_item_level: 6,
                    core_maximum_item_level_override: Some(10),
                    capability_tags: vec!["slot.charm".to_owned()],
                    affix_exclusion_ids: vec![],
                    behavior: ProductionEquipmentBehavior::Charm {
                        effect: content_schema::ProductionCharmEffect::RestedPrimaryDamage {
                            idle_millis: 2_000,
                            bonus_basis_points: 1_500,
                            consumed_on_release_regardless_of_hit: true,
                        },
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
        let rarity_profiles = vec![
            ProductionRarityProfileRecord {
                header: infrastructure_header("rarity.core_fixed"),
                ordered_weights: vec![ProductionRarityWeight {
                    rarity: ProductionItemRarity::Forged,
                    weight_basis_points: 10_000,
                }],
            },
            ProductionRarityProfileRecord {
                header: infrastructure_header("rarity.normal_outer"),
                ordered_weights: vec![
                    ProductionRarityWeight {
                        rarity: ProductionItemRarity::Forged,
                        weight_basis_points: 7_000,
                    },
                    ProductionRarityWeight {
                        rarity: ProductionItemRarity::Oathed,
                        weight_basis_points: 2_600,
                    },
                    ProductionRarityWeight {
                        rarity: ProductionItemRarity::Relic,
                        weight_basis_points: 400,
                    },
                ],
            },
        ];
        let material_pools = vec![ProductionMaterialPoolRecord {
            header: infrastructure_header("material.core_tonic"),
            ordered_outcomes: vec![ProductionMaterialPoolOutcome {
                item_id: id("consumable.red_tonic"),
                quantity: 1,
                weight: 100,
            }],
        }];
        let reward_tables = vec![
            ProductionRewardTableRecord {
                header: infrastructure_header("reward.elite_outer"),
                ordered_rolls: vec![ProductionRewardRoll::Equipment {
                    presence_basis_points: 10_000,
                    count: 1,
                    minimum_item_level: 1,
                    maximum_item_level: 6,
                    rarity_profile_id: id("rarity.normal_outer"),
                }],
            },
            ProductionRewardTableRecord {
                header: infrastructure_header("reward.normal_outer"),
                ordered_rolls: vec![
                    ProductionRewardRoll::UniversalItem {
                        presence_basis_points: 800,
                        count: 1,
                        minimum_item_level: 1,
                        maximum_item_level: 6,
                        rarity_profile_id: id("rarity.normal_outer"),
                    },
                    ProductionRewardRoll::Material {
                        presence_basis_points: 1_200,
                        count: 1,
                        material_pool_id: id("material.core_tonic"),
                    },
                ],
            },
        ];
        let stage_policies = vec![ProductionItemStagePolicyRecord {
            header: infrastructure_header("policy.items.core"),
            current_class_weapon_relic_basis_points: 8_500,
            other_class_weapon_relic_basis_points: 0,
            universal_armor_charm_basis_points: 1_500,
            weapon_within_class_basis_points: 5_000,
            armor_within_universal_basis_points: 5_000,
            maximum_item_level: 10,
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
                id("item.armor.ashplate.t1"),
                id("item.charm.ember_tooth.t1"),
                id("item.weapon.crossbow.pine_crossbow"),
            ],
            required_rarity_profile_ids: vec![id("rarity.core_fixed"), id("rarity.normal_outer")],
            required_reward_table_ids: vec![id("reward.elite_outer"), id("reward.normal_outer")],
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
        assert_eq!(compiled.items().len(), 4);
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
            behavior: ashplate_behavior(),
        };
        assert!(compile_production_item_catalog(&target, &records).is_err());
    }

    #[derive(Debug)]
    struct DrawTape(std::collections::VecDeque<u32>);

    impl DrawTape {
        fn new(values: impl IntoIterator<Item = u32>) -> Self {
            Self(values.into_iter().collect())
        }

        fn exhausted(&self) -> bool {
            self.0.is_empty()
        }
    }

    impl ProductionRewardDrawSource for DrawTape {
        fn draw_below(
            &mut self,
            upper_exclusive: u32,
        ) -> Result<u32, ProductionRewardPlanningError> {
            let value = self
                .0
                .pop_front()
                .ok_or(ProductionRewardPlanningError::DrawSourceExhausted)?;
            if upper_exclusive == 0 || value >= upper_exclusive {
                return Err(ProductionRewardPlanningError::DrawOutOfRange);
            }
            Ok(value)
        }
    }

    #[test]
    fn injected_draw_order_resolves_current_class_and_universal_pools_exactly() {
        let (target, records) = fixture();
        let compiled = compile_production_item_catalog(&target, &records).unwrap();
        let request = ProductionRewardPlanRequest {
            reward_table_id: "reward.elite_outer",
            stage_policy_id: "policy.items.core",
            current_class_id: "class.grave_arbalist",
        };
        // presence, usability=current, slot=weapon, level=6, rarity=Forged, template.
        let mut tape = DrawTape::new([0, 0, 0, 5, 0, 0]);
        let plan = compiled.plan_reward(&request, &mut tape).unwrap();
        assert!(tape.exhausted());
        assert_eq!(
            plan.entries,
            vec![ProductionRewardPlanEntry::Equipment {
                roll_index: 0,
                template_id: "item.weapon.crossbow.pine_crossbow".to_owned(),
                item_level: 6,
                rarity: ProductionItemRarity::Forged,
            }]
        );

        let request = ProductionRewardPlanRequest {
            reward_table_id: "reward.normal_outer",
            ..request
        };
        // universal present, armor slot, level=6, rarity=Forged, template, material absent.
        let mut tape = DrawTape::new([0, 0, 5, 0, 0, 9_999]);
        let plan = compiled.plan_reward(&request, &mut tape).unwrap();
        assert!(tape.exhausted());
        assert_eq!(
            plan.entries,
            vec![ProductionRewardPlanEntry::Equipment {
                roll_index: 0,
                template_id: "item.armor.ashplate.t1".to_owned(),
                item_level: 6,
                rarity: ProductionItemRarity::Forged,
            }]
        );
    }

    #[test]
    fn material_category_plan_and_revision_bound_hash_are_deterministic() {
        let (target, records) = fixture();
        let compiled = compile_production_item_catalog(&target, &records).unwrap();
        let request = ProductionRewardPlanRequest {
            reward_table_id: "reward.normal_outer",
            stage_policy_id: "policy.items.core",
            current_class_id: "class.grave_arbalist",
        };
        // universal absent, material present, singleton material outcome.
        let mut first_tape = DrawTape::new([9_999, 0, 0]);
        let first = compiled.plan_reward(&request, &mut first_tape).unwrap();
        let mut second_tape = DrawTape::new([9_999, 0, 0]);
        let second = compiled.plan_reward(&request, &mut second_tape).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.canonical_hash(), second.canonical_hash());
        assert_eq!(
            first.entries,
            vec![ProductionRewardPlanEntry::Material {
                roll_index: 1,
                item_id: "consumable.red_tonic".to_owned(),
                quantity: 1,
            }]
        );

        let (mut changed_target, records) = fixture();
        changed_target.expected_manifest_blake3 = "e".repeat(64);
        let changed = compile_production_item_catalog(&changed_target, &records).unwrap();
        let mut changed_tape = DrawTape::new([9_999, 0, 0]);
        let changed = changed.plan_reward(&request, &mut changed_tape).unwrap();
        assert_ne!(first.canonical_hash(), changed.canonical_hash());
    }

    #[test]
    fn core_equipment_reallocates_at_85_percent_and_caps_source_level_at_ten() {
        let (target, mut records) = fixture();
        let ProductionRewardRoll::Equipment {
            maximum_item_level, ..
        } = &mut records.reward_tables[0].ordered_rolls[0]
        else {
            panic!("fixture equipment roll");
        };
        *maximum_item_level = 20;
        let compiled = compile_production_item_catalog(&target, &records).unwrap();
        let request = ProductionRewardPlanRequest {
            reward_table_id: "reward.elite_outer",
            stage_policy_id: "policy.items.core",
            current_class_id: "class.grave_arbalist",
        };
        // Boundary 8500 is universal, then Armor; the stage-capped width is exactly 10.
        let mut tape = DrawTape::new([0, 8_500, 0, 9, 0, 0]);
        let plan = compiled.plan_reward(&request, &mut tape).unwrap();
        assert!(tape.exhausted());
        assert_eq!(
            plan.entries,
            vec![ProductionRewardPlanEntry::Equipment {
                roll_index: 0,
                template_id: "item.armor.ashplate.t1".to_owned(),
                item_level: 10,
                rarity: ProductionItemRarity::Forged,
            }]
        );
    }

    #[test]
    fn exhausted_or_out_of_range_draw_tapes_fail_closed() {
        let (target, records) = fixture();
        let compiled = compile_production_item_catalog(&target, &records).unwrap();
        let request = ProductionRewardPlanRequest {
            reward_table_id: "reward.elite_outer",
            stage_policy_id: "policy.items.core",
            current_class_id: "class.grave_arbalist",
        };
        assert_eq!(
            compiled.plan_reward(&request, &mut DrawTape::new([])),
            Err(ProductionRewardPlanningError::DrawSourceExhausted)
        );
        assert_eq!(
            compiled.plan_reward(&request, &mut DrawTape::new([10_000])),
            Err(ProductionRewardPlanningError::DrawOutOfRange)
        );
    }
}
