//! Independently hashed, non-promotable Core Oath/Bargain content target.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};
use content_schema::{
    BargainBehavior, BargainRecord, CoreWorldFlowCopyFile, OathBargainDevelopmentTarget,
    OathBargainRecords, OathBehavior, OathRecord, ProductionItemAssetKind,
    ProductionItemAssetManifest, ReleaseStage, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sim_core::{
    GraveArbalistOath, GraveMarkDefinition, ResolvedArbalistOathStats, SlipstepDefinition,
    StillnessDefinition, WeaponDefinition, duration_ms_to_ticks_nearest,
    resolve_arbalist_oath_stats,
};

use crate::{
    CompiledProductionItemCatalog, ContentPackage, compile_core_crossbow,
    core_crossbow_attack_interval_micros, first_playable_grave_mark, first_playable_slipstep,
    first_playable_stillness,
};

const INITIAL_WARNING_KEY: &str = "ui.oath.initial_warning";
const INITIAL_WARNING_VALUE: &str = "This oath persists for this character’s life. Changing it later costs 40 Ash and requires confirmation in Lantern Halls.";
const EXPECTED_COPY: [(&str, &str); 11] = [
    (
        "bargain.bell_debt.description",
        "Every fifth primary attack repeats after 300 ms for 50% damage; Primary attack rate -15%",
    ),
    ("bargain.bell_debt.name", "Bell Debt"),
    (
        "bargain.cinder_hunger.description",
        "+18% direct outgoing damage; -12% max health",
    ),
    ("bargain.cinder_hunger.name", "Cinder Hunger"),
    (
        "bargain.lantern_ash.description",
        "Potion healing +40%; Only one consumable belt slot is active",
    ),
    ("bargain.lantern_ash.name", "Lantern Ash"),
    (
        "oath.arbalist.long_vigil.description",
        "Focused activates after 350 ms rather than 600 ms; Grave Mark range increases by 2 tiles and mark bonus becomes 20%; Max health is reduced by 10%.",
    ),
    ("oath.arbalist.long_vigil.name", "Long Vigil"),
    (
        "oath.arbalist.nailkeeper.description",
        "Grave Mark creates a 1.25 tile nail trap at the first enemy or wall impact; Trap arms after 400 ms, lasts 5 seconds, deals 0.9W, and Frostbinds for 1.5 seconds; Maximum two active traps per Arbalist; oldest is removed when a third is created; Primary attack rate is reduced by 8%.",
    ),
    ("oath.arbalist.nailkeeper.name", "Nailkeeper"),
    (INITIAL_WARNING_KEY, INITIAL_WARNING_VALUE),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OathBargainSourceHashes {
    pub manifest_blake3: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

#[derive(Debug, Clone)]
pub struct CompiledOathBargainCatalog {
    target_name: String,
    revision_label: String,
    hashes: OathBargainSourceHashes,
    oaths: BTreeMap<String, OathRecord>,
    bargains: BTreeMap<String, BargainRecord>,
    localization: BTreeMap<String, String>,
}

impl CompiledOathBargainCatalog {
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub fn revision_label(&self) -> &str {
        &self.revision_label
    }

    #[must_use]
    pub const fn hashes(&self) -> &OathBargainSourceHashes {
        &self.hashes
    }

    #[must_use]
    pub const fn oaths(&self) -> &BTreeMap<String, OathRecord> {
        &self.oaths
    }

    #[must_use]
    pub const fn bargains(&self) -> &BTreeMap<String, BargainRecord> {
        &self.bargains
    }

    #[must_use]
    pub fn localized(&self, key: &str) -> Option<&str> {
        self.localization.get(key).map(String::as_str)
    }
}

/// Resolves one compiled Core Oath against authored base class/item values.
pub fn resolve_core_arbalist_oath_stats(
    catalog: &CompiledOathBargainCatalog,
    oath_id: &str,
    base_focused_activation_ticks: u32,
    base_grave_mark_range_milli_tiles: u32,
    base_marked_primary_bonus_basis_points: u32,
    base_primary_interval_micros: u32,
    ordinary_attack_rate_basis_points: u32,
) -> Result<ResolvedArbalistOathStats> {
    let record = catalog
        .oaths
        .get(oath_id)
        .with_context(|| format!("Core Oath `{oath_id}` is unavailable"))?;
    if !record.header.enabled {
        bail!("Core Oath `{oath_id}` is disabled");
    }
    let oath =
        GraveArbalistOath::from_content_id(oath_id).map_err(|error| anyhow::anyhow!(error))?;
    resolve_arbalist_oath_stats(
        oath,
        base_focused_activation_ticks,
        base_grave_mark_range_milli_tiles,
        base_marked_primary_bonus_basis_points,
        base_primary_interval_micros,
        ordinary_attack_rate_basis_points,
    )
    .map_err(|error| anyhow::anyhow!(error))
}

#[derive(Debug, Clone)]
pub struct CoreOathedCombatDefinitions {
    pub oath: GraveArbalistOath,
    pub weapon: WeaponDefinition,
    pub grave_mark: GraveMarkDefinition,
    pub slipstep: SlipstepDefinition,
    pub stillness: StillnessDefinition,
    pub maximum_health_multiplier_basis_points: u32,
}

pub fn compile_core_oathed_combat_definitions(
    class_package: &ContentPackage,
    item_catalog: &CompiledProductionItemCatalog,
    oath_catalog: &CompiledOathBargainCatalog,
    oath_id: &str,
    weapon_id: &str,
    item_level: u8,
    ordinary_attack_rate_basis_points: u32,
) -> Result<CoreOathedCombatDefinitions> {
    let base_mark = first_playable_grave_mark(class_package)?;
    let base_stillness = first_playable_stillness(class_package)?;
    let base_interval = core_crossbow_attack_interval_micros(item_catalog, weapon_id)?;
    let stats = resolve_core_arbalist_oath_stats(
        oath_catalog,
        oath_id,
        base_stillness.activation_ticks(),
        base_mark.range_milli_tiles(),
        base_mark.marked_primary_bonus_basis_points(),
        base_interval,
        ordinary_attack_rate_basis_points,
    )?;
    let oath =
        GraveArbalistOath::from_content_id(oath_id).map_err(|error| anyhow::anyhow!(error))?;
    Ok(CoreOathedCombatDefinitions {
        oath,
        weapon: compile_core_crossbow(
            item_catalog,
            weapon_id,
            item_level,
            stats.primary_interval_micros,
        )?,
        grave_mark: base_mark
            .with_range_and_marked_primary_bonus(
                stats.grave_mark_range_milli_tiles,
                stats.marked_primary_bonus_basis_points,
            )
            .context("resolved Oath Grave Mark is invalid")?,
        slipstep: first_playable_slipstep(class_package)?,
        stillness: base_stillness
            .with_activation_ticks(stats.focused_activation_ticks)
            .context("resolved Oath Stillness is invalid")?,
        maximum_health_multiplier_basis_points: stats.maximum_health_multiplier_basis_points,
    })
}

#[derive(Serialize)]
struct ManifestDigest<'a> {
    schema_version: u32,
    records_blake3: &'a str,
    assets_blake3: &'a str,
    localization_blake3: &'a str,
}

pub fn load_core_development_oaths_bargains(root: &Path) -> Result<CompiledOathBargainCatalog> {
    let core = root.join("core_dev");
    let target_bytes = read(&core.join("oaths_bargains.json"))?;
    let records_bytes = read(&core.join("oaths_bargains.records.json"))?;
    let assets_bytes = read(&core.join("oaths_bargains.assets.json"))?;
    let localization_bytes = read(&core.join("oaths_bargains.en-US.json"))?;
    let target: OathBargainDevelopmentTarget = parse(&target_bytes, "oaths_bargains.json")?;
    let records: OathBargainRecords = parse(&records_bytes, "oaths_bargains.records.json")?;
    let assets: ProductionItemAssetManifest = parse(&assets_bytes, "oaths_bargains.assets.json")?;
    let localization: CoreWorldFlowCopyFile =
        parse(&localization_bytes, "oaths_bargains.en-US.json")?;
    let hashes = source_hashes(&records_bytes, &assets_bytes, &localization_bytes)?;
    validate_hashes(&target, &hashes)?;
    compile(root, target, records, &assets, &localization, hashes)
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("failed to read {}", path.display()))
}

fn parse<T: for<'de> Deserialize<'de>>(bytes: &[u8], name: &str) -> Result<T> {
    serde_json::from_slice(bytes).with_context(|| format!("invalid Core source {name}"))
}

fn source_hashes(records: &[u8], assets: &[u8], copy: &[u8]) -> Result<OathBargainSourceHashes> {
    let records_blake3 = blake3::hash(records).to_hex().to_string();
    let assets_blake3 = blake3::hash(assets).to_hex().to_string();
    let localization_blake3 = blake3::hash(copy).to_hex().to_string();
    let digest = ManifestDigest {
        schema_version: SCHEMA_VERSION,
        records_blake3: &records_blake3,
        assets_blake3: &assets_blake3,
        localization_blake3: &localization_blake3,
    };
    let manifest = serde_json::to_vec(&digest).context("failed to encode Oath/Bargain manifest")?;
    Ok(OathBargainSourceHashes {
        manifest_blake3: blake3::hash(&manifest).to_hex().to_string(),
        records_blake3,
        assets_blake3,
        localization_blake3,
    })
}

fn validate_hashes(
    target: &OathBargainDevelopmentTarget,
    actual: &OathBargainSourceHashes,
) -> Result<()> {
    let expected = OathBargainSourceHashes {
        manifest_blake3: target.expected_manifest_blake3.clone(),
        records_blake3: target.expected_records_blake3.clone(),
        assets_blake3: target.expected_assets_blake3.clone(),
        localization_blake3: target.expected_localization_blake3.clone(),
    };
    if expected != *actual {
        bail!("Core Oath/Bargain hash mismatch: expected {expected:?}; actual {actual:?}");
    }
    Ok(())
}

fn compile(
    root: &Path,
    target: OathBargainDevelopmentTarget,
    records: OathBargainRecords,
    assets: &ProductionItemAssetManifest,
    localization: &CoreWorldFlowCopyFile,
    hashes: OathBargainSourceHashes,
) -> Result<CompiledOathBargainCatalog> {
    if target.schema_version != SCHEMA_VERSION || target.target_name != "core-dev-oaths-bargains" {
        bail!("Core Oath/Bargain target metadata is invalid");
    }
    let oaths = keyed(records.oaths, |record| record.header.id.as_str())?;
    let bargains = keyed(records.bargains, |record| record.header.id.as_str())?;
    require_ids(
        &target.required_oath_ids,
        oaths.keys(),
        ["oath.arbalist.long_vigil", "oath.arbalist.nailkeeper"],
    )?;
    require_ids(
        &target.required_bargain_ids,
        bargains.keys(),
        [
            "bargain.bell_debt",
            "bargain.cinder_hunger",
            "bargain.lantern_ash",
        ],
    )?;
    validate_oaths(&oaths)?;
    validate_bargains(&bargains)?;
    validate_assets(root, &oaths, &bargains, assets)?;
    validate_copy(&oaths, &bargains, localization)?;
    validate_core_combinations(&oaths, &bargains)?;
    Ok(CompiledOathBargainCatalog {
        target_name: target.target_name,
        revision_label: format!("core-dev.blake3.{}", hashes.manifest_blake3),
        hashes,
        oaths,
        bargains,
        localization: localization
            .entries
            .iter()
            .map(|entry| (entry.key.as_str().to_owned(), entry.value.clone()))
            .collect(),
    })
}

fn keyed<T>(records: Vec<T>, id: impl Fn(&T) -> &str) -> Result<BTreeMap<String, T>> {
    let mut result = BTreeMap::new();
    for record in records {
        let key = id(&record).to_owned();
        if result.insert(key.clone(), record).is_some() {
            bail!("duplicate Core choice record `{key}`");
        }
    }
    Ok(result)
}

fn require_ids<'a, const N: usize>(
    required: &[content_schema::ContentId],
    actual: impl Iterator<Item = &'a String>,
    expected: [&str; N],
) -> Result<()> {
    let required = required
        .iter()
        .map(content_schema::ContentId::as_str)
        .collect::<Vec<_>>();
    let actual = actual.map(String::as_str).collect::<Vec<_>>();
    if required != expected || actual != expected {
        bail!("Core choice allowlist does not match its exact stage set");
    }
    Ok(())
}

fn validate_header(
    header: &content_schema::CoreDevelopmentHeader,
    expected_tags: &[&str],
    source: &str,
) -> Result<()> {
    let expected_icon = format!("icon.{}", header.id);
    if header.schema_version != SCHEMA_VERSION
        || !header.enabled
        || header.earliest_release_stage != ReleaseStage::Core
        || header.localization_name_key.as_str() != format!("{}.name", header.id)
        || header.localization_description_key.as_str() != format!("{}.description", header.id)
        || header.asset_ids.len() != 1
        || header.asset_ids[0].as_str() != expected_icon
        || header.tags.iter().map(String::as_str).collect::<Vec<_>>() != expected_tags
        || header.source_document_feature_id != source
    {
        bail!("Core choice header `{}` is not exact", header.id);
    }
    Ok(())
}

fn validate_oaths(oaths: &BTreeMap<String, OathRecord>) -> Result<()> {
    let long = &oaths["oath.arbalist.long_vigil"];
    validate_header(
        &long.header,
        &[
            "ability.mark",
            "class.grave_arbalist",
            "max_health_mod",
            "oath",
            "passive.stillness",
        ],
        "CLS-020",
    )?;
    if long.class_id.as_str() != "class.grave_arbalist"
        || long.unlock_level != 10
        || long.resolution_step != 8
        || long.behavior
            != (OathBehavior::LongVigil {
                focused_activation_millis: 350,
                grave_mark_range_bonus_milli_tiles: 2_000,
                grave_mark_primary_bonus_basis_points: 2_000,
                maximum_health_multiplier_basis_points: 9_000,
            })
    {
        bail!("Long Vigil payload is not exact");
    }
    let nail = &oaths["oath.arbalist.nailkeeper"];
    validate_header(
        &nail.header,
        &[
            "ability.mark",
            "class.grave_arbalist",
            "oath",
            "outgoing.status",
            "primary_cadence",
            "trap",
        ],
        "CLS-020",
    )?;
    if nail.class_id.as_str() != "class.grave_arbalist"
        || nail.unlock_level != 10
        || nail.resolution_step != 8
        || nail.behavior
            != (OathBehavior::Nailkeeper {
                trap_radius_milli_tiles: 1_250,
                arm_delay_millis: 400,
                lifetime_millis: 5_000,
                direct_damage_coefficient_basis_points: 9_000,
                frostbind_duration_millis: 1_500,
                maximum_live_traps: 2,
                primary_interval_multiplier_basis_points: 10_800,
                create_on_enemy_impact: true,
                create_on_solid_impact: true,
                enemy_impact_applies_grave_mark_first: true,
                solid_impact_applies_grave_mark: false,
                requires_exit_after_arming_for_existing_occupants: true,
                consumes_on_first_legal_enemy_entry: true,
                snapshots_weapon_power_at_creation: true,
                overflow_order: content_schema::OathTrapOverflowOrder::CreatedTickThenEntityId,
            })
    {
        bail!("Nailkeeper payload is not exact");
    }
    validate_runtime_oath_constants(long, nail)?;
    Ok(())
}

fn validate_runtime_oath_constants(long: &OathRecord, nail: &OathRecord) -> Result<()> {
    let OathBehavior::LongVigil {
        focused_activation_millis,
        grave_mark_range_bonus_milli_tiles,
        grave_mark_primary_bonus_basis_points,
        maximum_health_multiplier_basis_points,
    } = long.behavior
    else {
        bail!("Long Vigil runtime binding has the wrong behavior kind");
    };
    let OathBehavior::Nailkeeper {
        trap_radius_milli_tiles,
        arm_delay_millis,
        lifetime_millis,
        direct_damage_coefficient_basis_points,
        frostbind_duration_millis,
        maximum_live_traps,
        primary_interval_multiplier_basis_points,
        ..
    } = nail.behavior
    else {
        bail!("Nailkeeper runtime binding has the wrong behavior kind");
    };
    if duration_ms_to_ticks_nearest(u64::from(focused_activation_millis))
        != u64::from(sim_core::LONG_VIGIL_FOCUSED_ACTIVATION_TICKS)
        || grave_mark_range_bonus_milli_tiles
            != sim_core::LONG_VIGIL_GRAVE_MARK_RANGE_BONUS_MILLI_TILES
        || u32::from(grave_mark_primary_bonus_basis_points)
            != sim_core::LONG_VIGIL_MARKED_PRIMARY_BONUS_BASIS_POINTS
        || u32::from(maximum_health_multiplier_basis_points)
            != sim_core::LONG_VIGIL_MAX_HEALTH_MULTIPLIER_BASIS_POINTS
        || trap_radius_milli_tiles != sim_core::NAILKEEPER_TRAP_RADIUS_MILLI_TILES
        || duration_ms_to_ticks_nearest(u64::from(arm_delay_millis))
            != u64::from(sim_core::NAILKEEPER_ARM_TICKS)
        || duration_ms_to_ticks_nearest(u64::from(lifetime_millis))
            != u64::from(sim_core::NAILKEEPER_LIFETIME_TICKS)
        || u32::from(direct_damage_coefficient_basis_points)
            != sim_core::NAILKEEPER_DAMAGE_BASIS_POINTS
        || duration_ms_to_ticks_nearest(u64::from(frostbind_duration_millis))
            != u64::from(sim_core::NAILKEEPER_FROSTBIND_TICKS)
        || usize::from(maximum_live_traps) != sim_core::NAILKEEPER_MAXIMUM_ACTIVE_TRAPS
        || u32::from(primary_interval_multiplier_basis_points)
            != sim_core::NAILKEEPER_PRIMARY_INTERVAL_MULTIPLIER_BASIS_POINTS
    {
        bail!("Core Oath content and simulation constants diverged");
    }
    Ok(())
}

fn validate_bargains(bargains: &BTreeMap<String, BargainRecord>) -> Result<()> {
    validate_bargain(
        &bargains["bargain.bell_debt"],
        &[
            "bargain",
            "primary.repeat",
            "primary_cadence",
            "voluntary_risk",
        ],
        &BargainBehavior::BellDebt {
            accepted_primary_emissions_per_repeat: 5,
            repeat_delay_millis: 300,
            repeat_damage_multiplier_basis_points: 5_000,
            primary_attack_rate_multiplier_basis_points: 8_500,
            counts_legal_misses: true,
            generated_repeats_advance_counter: false,
            snapshots_aim_and_resolved_behavior: true,
            uses_live_origin_at_repeat: true,
            repeat_is_recursive: false,
            repeat_spends_cooldown_or_resource: false,
            counter_persists_reconnect_and_room_change: true,
            counter_resets_on_acquisition_purge_death_retirement_or_safe_transfer: true,
            cancel_pending_repeat_when_dead_transferred_or_primary_illegal: true,
        },
    )?;
    validate_bargain(
        &bargains["bargain.cinder_hunger"],
        &[
            "bargain",
            "direct_output",
            "max_health_mod",
            "voluntary_risk",
        ],
        &BargainBehavior::CinderHunger {
            outgoing_direct_damage_multiplier_basis_points: 11_800,
            maximum_health_multiplier_basis_points: 8_800,
        },
    )?;
    validate_bargain(
        &bargains["bargain.lantern_ash"],
        &[
            "bargain",
            "belt_constraint",
            "potion_output",
            "voluntary_risk",
        ],
        &BargainBehavior::LanternAsh {
            potion_healing_multiplier_basis_points: 14_000,
            active_belt_slot_count: 1,
            active_belt_index: 0,
            inactive_slot_remains_stored_visible_locked: true,
        },
    )
}

fn validate_bargain(
    bargain: &BargainRecord,
    tags: &[&str],
    behavior: &BargainBehavior,
) -> Result<()> {
    validate_header(&bargain.header, tags, "BRG-003")?;
    if bargain.maximum_active_per_character != 3
        || bargain.resolution_step != 8
        || &bargain.behavior != behavior
    {
        bail!("Bargain payload `{}` is not exact", bargain.header.id);
    }
    Ok(())
}

fn validate_assets(
    root: &Path,
    oaths: &BTreeMap<String, OathRecord>,
    bargains: &BTreeMap<String, BargainRecord>,
    manifest: &ProductionItemAssetManifest,
) -> Result<()> {
    if manifest.schema_version != SCHEMA_VERSION || manifest.assets.len() != 5 {
        bail!("Core choice asset manifest count is invalid");
    }
    let expected_asset_ids = [
        "icon.bargain.bell_debt",
        "icon.bargain.cinder_hunger",
        "icon.bargain.lantern_ash",
        "icon.oath.arbalist.long_vigil",
        "icon.oath.arbalist.nailkeeper",
    ];
    if manifest
        .assets
        .iter()
        .map(|asset| asset.asset_id.as_str())
        .collect::<Vec<_>>()
        != expected_asset_ids
    {
        bail!("Core choice asset allowlist is not exact");
    }
    let mut source_text = None;
    for asset in &manifest.assets {
        let record = oaths
            .get(asset.source_record_id.as_str())
            .map(|value| &value.header)
            .or_else(|| {
                bargains
                    .get(asset.source_record_id.as_str())
                    .map(|value| &value.header)
            })
            .context("choice asset references an unknown record")?;
        let expected_id = format!("icon.{}", record.id);
        let symbol = record
            .id
            .as_str()
            .rsplit('.')
            .next()
            .expect("validated ID")
            .replace('_', "-");
        let source_path = root.join("..").join(&asset.source_path);
        let bytes = fs::read(&source_path)
            .with_context(|| format!("missing choice icon source {}", source_path.display()))?;
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let text = String::from_utf8(bytes).context("choice icon source is not UTF-8 SVG")?;
        source_text.get_or_insert(text.clone());
        if asset.kind != ProductionItemAssetKind::ChoiceIcon
            || asset.asset_id.as_str() != expected_id
            || record.asset_ids.as_slice() != [asset.asset_id.clone()]
            || asset.source_path != "assets/core/oaths_bargains/core_choice_icons.svg"
            || asset.source_blake3 != hash
            || asset.runtime_bundle.as_str() != "bundle.core.oaths_bargains"
            || asset.content_dependencies.as_slice() != [record.id.clone()]
            || asset.anchor.x_pixels != 32
            || asset.anchor.y_pixels != 32
            || asset.dimensions.width_pixels != 64
            || asset.dimensions.height_pixels != 64
            || asset.animation_fps != 0
            || asset.collision_metadata_reference.is_some()
            || asset.readability_tags.len() != 3
            || asset.audio_priority.is_some()
            || asset.memory_budget_bytes != 16_384
            || asset.platform_variants.get("svg_symbol") != Some(&symbol)
            || asset.license_source_record.trim().is_empty()
            || !text.contains(&format!("id=\"{symbol}\""))
        {
            bail!("choice icon `{}` is not exact", asset.asset_id);
        }
    }
    let text = source_text.context("choice icon source is absent")?;
    if text.matches("<symbol ").count() != 5 || text.matches("<use ").count() != 5 {
        bail!("choice icon contact sheet is incomplete");
    }
    Ok(())
}

fn validate_copy(
    oaths: &BTreeMap<String, OathRecord>,
    bargains: &BTreeMap<String, BargainRecord>,
    copy: &CoreWorldFlowCopyFile,
) -> Result<()> {
    let mut required = oaths
        .values()
        .flat_map(|record| {
            [
                record.header.localization_description_key.as_str(),
                record.header.localization_name_key.as_str(),
            ]
        })
        .chain(bargains.values().flat_map(|record| {
            [
                record.header.localization_description_key.as_str(),
                record.header.localization_name_key.as_str(),
            ]
        }))
        .chain([INITIAL_WARNING_KEY])
        .collect::<Vec<_>>();
    required.sort_unstable();
    let actual_copy = copy
        .entries
        .iter()
        .map(|entry| (entry.key.as_str(), entry.value.as_str()))
        .collect::<Vec<_>>();
    if copy.schema_version != SCHEMA_VERSION
        || copy.locale != "en-US"
        || copy
            .entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>()
            != required
        || actual_copy != EXPECTED_COPY
        || copy
            .entries
            .iter()
            .any(|entry| entry.value.trim().is_empty())
        || copy
            .entries
            .iter()
            .find(|entry| entry.key.as_str() == INITIAL_WARNING_KEY)
            .is_none_or(|entry| entry.value != INITIAL_WARNING_VALUE)
    {
        bail!("Core Oath/Bargain localization closure is invalid");
    }
    Ok(())
}

fn validate_core_combinations(
    oaths: &BTreeMap<String, OathRecord>,
    bargains: &BTreeMap<String, BargainRecord>,
) -> Result<()> {
    if oaths.len() != 2 || bargains.len() != 3 {
        bail!("Core Oath/Bargain combination set is invalid");
    }
    // Long Vigil (0.90) and Cinder Hunger (0.88) are the only Core health multipliers.
    let combined_health_basis_points = (9_000_u32 * 8_800 + 5_000) / 10_000;
    if combined_health_basis_points != 7_920 || combined_health_basis_points < 7_000 {
        bail!("Core Oath/Bargain combination violates the maximum-health floor");
    }
    // Cinder Hunger's +18% remains inside the global +50% outgoing modifier cap.
    if 11_800_u16 > 15_000 {
        bail!("Core Bargain outgoing damage exceeds the global cap");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn content_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn checked_in_core_oaths_bargains_are_exact_and_unpromoted() {
        let compiled = load_core_development_oaths_bargains(&content_root()).unwrap();
        assert_eq!(compiled.target_name(), "core-dev-oaths-bargains");
        assert_eq!(compiled.oaths().len(), 2);
        assert_eq!(compiled.bargains().len(), 3);
        assert_eq!(
            compiled.localized("oath.arbalist.long_vigil.name"),
            Some("Long Vigil")
        );
        assert_eq!(
            compiled.localized(INITIAL_WARNING_KEY),
            Some(INITIAL_WARNING_VALUE)
        );
        assert!(compiled.revision_label().starts_with("core-dev.blake3."));
        assert!(!compiled.revision_label().contains("core.1.0.0"));
        let vigil = resolve_core_arbalist_oath_stats(
            &compiled,
            "oath.arbalist.long_vigil",
            18,
            11_000,
            1_500,
            454_545,
            10_000,
        )
        .unwrap();
        assert_eq!(vigil.focused_activation_ticks, 11);
        assert_eq!(vigil.grave_mark_range_milli_tiles, 13_000);
        assert_eq!(vigil.marked_primary_bonus_basis_points, 2_000);
        let nailkeeper = resolve_core_arbalist_oath_stats(
            &compiled,
            "oath.arbalist.nailkeeper",
            18,
            11_000,
            1_500,
            454_545,
            10_000,
        )
        .unwrap();
        assert_eq!(nailkeeper.primary_interval_micros, 490_909);

        let (class_package, _) = crate::load_and_validate(&content_root()).unwrap();
        let items = crate::load_core_development_items(&content_root()).unwrap();
        let vigil_definitions = compile_core_oathed_combat_definitions(
            &class_package,
            &items,
            &compiled,
            "oath.arbalist.long_vigil",
            "item.weapon.crossbow.pine_crossbow",
            1,
            10_000,
        )
        .unwrap();
        assert_eq!(vigil_definitions.stillness.activation_ticks(), 11);
        assert_eq!(vigil_definitions.grave_mark.range_milli_tiles(), 13_000);
        assert_eq!(
            vigil_definitions
                .grave_mark
                .marked_primary_bonus_basis_points(),
            2_000
        );
        assert_eq!(vigil_definitions.weapon.attack_interval_ticks(), 14);
        assert_eq!(
            vigil_definitions.maximum_health_multiplier_basis_points,
            9_000
        );
        let nail_definitions = compile_core_oathed_combat_definitions(
            &class_package,
            &items,
            &compiled,
            "oath.arbalist.nailkeeper",
            "item.weapon.crossbow.pine_crossbow",
            1,
            10_000,
        )
        .unwrap();
        assert_eq!(nail_definitions.weapon.attack_interval_ticks(), 15);
        assert_eq!(nail_definitions.oath, GraveArbalistOath::Nailkeeper);
    }

    #[test]
    fn exact_semantics_reject_numeric_tag_asset_and_copy_drift() {
        let core = content_root().join("core_dev");
        let mut records: OathBargainRecords =
            serde_json::from_slice(&fs::read(core.join("oaths_bargains.records.json")).unwrap())
                .unwrap();
        let mut oaths = keyed(records.oaths.clone(), |record| record.header.id.as_str()).unwrap();
        let bargains = keyed(records.bargains.clone(), |record| record.header.id.as_str()).unwrap();
        let OathBehavior::LongVigil {
            focused_activation_millis,
            ..
        } = &mut oaths.get_mut("oath.arbalist.long_vigil").unwrap().behavior
        else {
            panic!("Long Vigil payload");
        };
        *focused_activation_millis = 351;
        assert!(validate_oaths(&oaths).is_err());

        records.bargains[0].header.tags.swap(1, 2);
        let drifted = keyed(records.bargains, |record| record.header.id.as_str()).unwrap();
        assert!(validate_bargains(&drifted).is_err());

        let mut assets: ProductionItemAssetManifest =
            serde_json::from_slice(&fs::read(core.join("oaths_bargains.assets.json")).unwrap())
                .unwrap();
        assets.assets.swap(0, 1);
        let exact_oaths = keyed(records.oaths, |record| record.header.id.as_str()).unwrap();
        assert!(validate_assets(&content_root(), &exact_oaths, &bargains, &assets).is_err());

        let mut copy: CoreWorldFlowCopyFile =
            serde_json::from_slice(&fs::read(core.join("oaths_bargains.en-US.json")).unwrap())
                .unwrap();
        copy.entries.last_mut().unwrap().value.push('!');
        assert!(validate_copy(&exact_oaths, &bargains, &copy).is_err());
    }

    #[test]
    fn every_core_oath_bargain_crossbow_combination_respects_caps_and_cadence() {
        let (class_package, _) = crate::load_and_validate(&content_root()).unwrap();
        let items = crate::load_core_development_items(&content_root()).unwrap();
        let choices = load_core_development_oaths_bargains(&content_root()).unwrap();
        let weapons = [
            ("item.weapon.crossbow.grave_repeater", 4),
            ("item.weapon.crossbow.mourners_fan", 10),
            ("item.weapon.crossbow.pilgrim_longbolt", 7),
            ("item.weapon.crossbow.pine_crossbow", 1),
        ];
        let bargains = [
            sim_core::CoreBargainModifier::CinderHunger,
            sim_core::CoreBargainModifier::BellDebt,
            sim_core::CoreBargainModifier::LanternAsh,
        ];
        for (oath_id, oath) in [
            (
                "oath.arbalist.long_vigil",
                sim_core::GraveArbalistOath::LongVigil,
            ),
            (
                "oath.arbalist.nailkeeper",
                sim_core::GraveArbalistOath::Nailkeeper,
            ),
        ] {
            for mask in 0_u8..8 {
                let active = bargains
                    .iter()
                    .enumerate()
                    .filter_map(|(index, bargain)| (mask & (1 << index) != 0).then_some(*bargain))
                    .collect::<Vec<_>>();
                let modifiers = sim_core::resolve_core_choice_modifiers(oath, &active).unwrap();
                assert!(modifiers.maximum_health_multiplier_basis_points >= 7_000);
                assert!(
                    sim_core::resolve_oath_maximum_health(
                        120,
                        modifiers.maximum_health_multiplier_basis_points,
                    )
                    .unwrap()
                        >= 84
                );
                for (weapon_id, minimum_level) in weapons {
                    let definitions = compile_core_oathed_combat_definitions(
                        &class_package,
                        &items,
                        &choices,
                        oath_id,
                        weapon_id,
                        minimum_level,
                        modifiers.ordinary_attack_rate_basis_points,
                    )
                    .unwrap();
                    assert!(definitions.weapon.attack_interval_ticks() > 0);
                }
            }
        }
    }
}
