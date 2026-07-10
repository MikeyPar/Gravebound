//! Immutable, strict content loading and semantic validation for simulation consumers.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use content_schema::{
    AbilityRecord, ArenaRecord, AssetManifest, ClassRecord, CommonHeader, ContentId,
    DropTableRecord, EnemyRecord, FIRST_PLAYABLE_CONTENT_VERSION, FeatureRegistry, ItemRecord,
    PatternRecord, ReleaseManifest, ReleaseStage, SCHEMA_VERSION,
};

/// Exact record counts for the M01 prototype bundle defined by `CONT-FP-001` through `CONT-FP-008`.
pub const FIRST_PLAYABLE_DOMAIN_COUNTS: [(&str, usize); 7] = [
    ("class", 1),
    ("ability", 4),
    ("enemy", 3),
    ("pattern", 3),
    ("arena", 1),
    ("item", 13),
    ("drop_table", 5),
];

/// Fully deserialized immutable First Playable package.
#[derive(Debug, Clone)]
pub struct ContentPackage {
    pub classes: Vec<ClassRecord>,
    pub abilities: Vec<AbilityRecord>,
    pub enemies: Vec<EnemyRecord>,
    pub patterns: Vec<PatternRecord>,
    pub arenas: Vec<ArenaRecord>,
    pub items: Vec<ItemRecord>,
    pub drop_tables: Vec<DropTableRecord>,
    pub release_manifest: ReleaseManifest,
    pub feature_registry: FeatureRegistry,
    pub asset_manifest: AssetManifest,
    pub localization: BTreeMap<String, String>,
}

/// Deterministic validation result printed by tools and CI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    pub content_version: String,
    pub record_count: usize,
    pub feature_count: usize,
    pub package_hash_blake3: String,
}

/// Reports the schema version this loader accepts.
#[must_use]
pub const fn supported_schema_version() -> u32 {
    SCHEMA_VERSION
}

/// Loads all known strict schemas from a content root and validates cross-record semantics.
pub fn load_and_validate(root: &Path) -> Result<(ContentPackage, ValidationReport)> {
    let package = ContentPackage {
        classes: read_json(&root.join("fp/classes.json"))?,
        abilities: read_json(&root.join("fp/abilities.json"))?,
        enemies: read_json(&root.join("fp/enemies.json"))?,
        patterns: read_json(&root.join("fp/patterns.json"))?,
        arenas: read_json(&root.join("fp/arenas.json"))?,
        items: read_json(&root.join("fp/items.json"))?,
        drop_tables: read_json(&root.join("fp/drop_tables.json"))?,
        release_manifest: read_json(&root.join("manifests/fp.1.0.0.json"))?,
        feature_registry: read_json(&root.join("features/registry.json"))?,
        asset_manifest: read_json(&root.join("manifests/assets.fp.json"))?,
        localization: read_json(&root.join("localization/en-US.json"))?,
    };
    validate_package(&package)?;
    let report = ValidationReport {
        content_version: package.release_manifest.content_version.clone(),
        record_count: all_headers(&package).len(),
        feature_count: package.feature_registry.features.len(),
        package_hash_blake3: hash_content_tree(root)?,
    };
    Ok((package, report))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read required content file {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("schema mismatch in {}", path.display()))
}

fn validate_package(package: &ContentPackage) -> Result<()> {
    validate_manifest(package)?;
    validate_headers(package)?;
    validate_features(&package.feature_registry)?;
    validate_references(package)?;
    validate_fp_combination(package)?;
    Ok(())
}

fn validate_manifest(package: &ContentPackage) -> Result<()> {
    let manifest = &package.release_manifest;
    if manifest.schema_version != SCHEMA_VERSION
        || manifest.content_version != FIRST_PLAYABLE_CONTENT_VERSION
        || manifest.release_stage != ReleaseStage::Fp
    {
        bail!("First Playable manifest must be schema 1, version fp.1.0.0, stage fp");
    }
    let required: BTreeSet<_> = manifest.required_content_ids.iter().cloned().collect();
    if required.len() != manifest.required_content_ids.len() {
        bail!("First Playable manifest contains duplicate content IDs");
    }
    let enabled: BTreeSet<_> = all_headers(package)
        .into_iter()
        .filter(|header| header.enabled)
        .map(|header| header.id.clone())
        .collect();
    if required != enabled {
        bail!("First Playable manifest IDs do not exactly match enabled content records");
    }
    Ok(())
}

fn validate_headers(package: &ContentPackage) -> Result<()> {
    let asset_ids: BTreeSet<_> = package.asset_manifest.asset_ids.iter().collect();
    if package.asset_manifest.schema_version != SCHEMA_VERSION {
        bail!("asset manifest schema version must be {SCHEMA_VERSION}");
    }
    if asset_ids.len() != package.asset_manifest.asset_ids.len() {
        bail!("asset manifest contains duplicate IDs");
    }

    let mut seen = BTreeSet::new();
    for header in all_headers(package) {
        if !seen.insert(header.id.clone()) {
            bail!("duplicate content ID {}", header.id);
        }
        if header.schema_version != SCHEMA_VERSION {
            bail!("{} has unsupported schema version", header.id);
        }
        if header.content_version != FIRST_PLAYABLE_CONTENT_VERSION
            || header.release_stage != ReleaseStage::Fp
            || !header.enabled
        {
            bail!("{} is not a legal enabled fp.1.0.0 record", header.id);
        }
        if !header.source_document_feature_id.starts_with("CONT-")
            && !header.source_document_feature_id.starts_with("CLS-")
        {
            bail!("{} has an invalid source document feature ID", header.id);
        }
        for key in [
            &header.localization_name_key,
            &header.localization_description_key,
        ] {
            if !package.localization.contains_key(key.as_str()) {
                bail!("{} references missing localization key {}", header.id, key);
            }
        }
        let expected_name = format!("{}.name", header.id);
        let expected_description = format!("{}.description", header.id);
        if header.localization_name_key.as_str() != expected_name
            || header.localization_description_key.as_str() != expected_description
        {
            bail!(
                "{} does not use the required derived localization keys",
                header.id
            );
        }
        if header.asset_ids.is_empty() {
            bail!("{} must reference at least one asset", header.id);
        }
        for asset_id in &header.asset_ids {
            if !asset_ids.contains(asset_id) {
                bail!("{} references missing asset {}", header.id, asset_id);
            }
        }
    }
    Ok(())
}

fn validate_features(registry: &FeatureRegistry) -> Result<()> {
    if registry.schema_version != SCHEMA_VERSION {
        bail!("feature registry schema version must be {SCHEMA_VERSION}");
    }
    let ids: BTreeSet<_> = registry
        .features
        .iter()
        .map(|feature| feature.feature_id.clone())
        .collect();
    if ids.len() != registry.features.len() {
        bail!("feature registry contains duplicate feature IDs");
    }
    for feature in &registry.features {
        if feature.title.trim().is_empty()
            || feature.milestone.trim().is_empty()
            || feature.acceptance_criteria.is_empty()
            || feature
                .acceptance_criteria
                .iter()
                .any(|criterion| criterion.trim().is_empty())
            || feature.source_document_ids.is_empty()
        {
            bail!(
                "{} has incomplete traceability or acceptance criteria",
                feature.feature_id
            );
        }
        for dependency in &feature.depends_on {
            if !ids.contains(dependency) {
                bail!(
                    "{} references unknown dependency {}",
                    feature.feature_id,
                    dependency
                );
            }
        }
    }
    for required in [
        "GB-M00-01",
        "GB-M00-02",
        "GB-M00-03",
        "GB-M00-04",
        "GB-M00-05",
        "GB-M00-06",
        "GB-M00-07",
        "GB-M00-08",
        "GB-M01-01A",
    ] {
        if !ids.iter().any(|id| id.as_str() == required) {
            bail!("feature registry is missing required task {required}");
        }
    }
    Ok(())
}

fn validate_references(package: &ContentPackage) -> Result<()> {
    let abilities: BTreeSet<_> = package
        .abilities
        .iter()
        .map(|record| record.header.id.clone())
        .collect();
    let patterns: BTreeSet<_> = package
        .patterns
        .iter()
        .map(|record| record.header.id.clone())
        .collect();
    let enemies: BTreeSet<_> = package
        .enemies
        .iter()
        .map(|record| record.header.id.clone())
        .collect();
    let items: BTreeSet<_> = package
        .items
        .iter()
        .map(|record| record.header.id.clone())
        .collect();
    let tables: BTreeSet<_> = package
        .drop_tables
        .iter()
        .map(|record| record.header.id.clone())
        .collect();

    for class in &package.classes {
        for ability_id in std::iter::once(&class.numeric_payload.primary_ability_id)
            .chain(class.numeric_payload.active_ability_ids.iter())
            .chain(std::iter::once(&class.numeric_payload.passive_ability_id))
        {
            require_ref(&class.header.id, ability_id, &abilities, "ability")?;
        }
    }
    for enemy in &package.enemies {
        if enemy.numeric_payload.pattern_ids.is_empty()
            || enemy.numeric_payload.state_machine.is_empty()
        {
            bail!(
                "{} must define a state machine and at least one pattern",
                enemy.header.id
            );
        }
        for pattern_id in &enemy.numeric_payload.pattern_ids {
            require_ref(&enemy.header.id, pattern_id, &patterns, "pattern")?;
        }
        require_ref(
            &enemy.header.id,
            &enemy.numeric_payload.reward_table_id,
            &tables,
            "drop table",
        )?;
    }
    let assets: BTreeSet<_> = package.asset_manifest.asset_ids.iter().cloned().collect();
    for pattern in &package.patterns {
        require_ref(
            &pattern.header.id,
            &pattern.numeric_payload.telegraph_id,
            &assets,
            "telegraph asset",
        )?;
        require_ref(
            &pattern.header.id,
            &pattern.numeric_payload.audio_cue_id,
            &assets,
            "audio cue asset",
        )?;
    }
    for arena in &package.arenas {
        for enemy_id in &arena.numeric_payload.allowed_enemy_ids {
            require_ref(&arena.header.id, enemy_id, &enemies, "enemy")?;
        }
        for table_id in &arena.numeric_payload.allowed_reward_table_ids {
            require_ref(&arena.header.id, table_id, &tables, "drop table")?;
        }
    }
    validate_drop_tables(package, &items)?;
    Ok(())
}

fn validate_drop_tables(package: &ContentPackage, items: &BTreeSet<ContentId>) -> Result<()> {
    for table in &package.drop_tables {
        if table.numeric_payload.roll_groups.is_empty() {
            bail!("{} must contain at least one roll group", table.header.id);
        }
        let mut group_ids = BTreeSet::new();
        for group in &table.numeric_payload.roll_groups {
            if !group_ids.insert(&group.group_id)
                || group.presence_basis_points > 10_000
                || group.selections == 0
                || group.outcomes.is_empty()
            {
                bail!("{} contains an invalid reward roll group", table.header.id);
            }
            let mut outcomes = BTreeSet::new();
            let mut total_weight = 0_u64;
            for outcome in &group.outcomes {
                require_ref(&table.header.id, &outcome.item_id, items, "item")?;
                if !outcomes.insert(&outcome.item_id) || outcome.weight == 0 {
                    bail!(
                        "{} contains a duplicate or zero-weight outcome",
                        table.header.id
                    );
                }
                total_weight += u64::from(outcome.weight);
            }
            if total_weight == 0
                || (group.without_replacement && group.selections as usize > outcomes.len())
            {
                bail!(
                    "{} contains an impossible reward selection",
                    table.header.id
                );
            }
            if outcomes.len() > 1 && total_weight != 100 {
                bail!("{} reward weights must total exactly 100", table.header.id);
            }
        }
    }
    Ok(())
}

fn require_ref<T: Ord + fmt::Display>(
    owner: &ContentId,
    target: &T,
    set: &BTreeSet<T>,
    domain: &str,
) -> Result<()> {
    if !set.contains(target) {
        bail!("{owner} references missing {domain} {target}");
    }
    Ok(())
}

fn validate_fp_combination(package: &ContentPackage) -> Result<()> {
    let actual = [
        ("class", package.classes.len()),
        ("ability", package.abilities.len()),
        ("enemy", package.enemies.len()),
        ("pattern", package.patterns.len()),
        ("arena", package.arenas.len()),
        ("item", package.items.len()),
        ("drop_table", package.drop_tables.len()),
    ];
    if actual != FIRST_PLAYABLE_DOMAIN_COUNTS {
        bail!(
            "illegal M01 content combination: expected {FIRST_PLAYABLE_DOMAIN_COUNTS:?}, got {actual:?}"
        );
    }
    require_exact_ids(
        &package.classes,
        &["class.grave_arbalist"],
        |record| &record.header.id,
        "class",
    )?;
    require_exact_ids(
        &package.abilities,
        &[
            "ability.arbalist.grave_mark",
            "ability.arbalist.primary_crossbow",
            "ability.arbalist.slipstep",
            "ability.arbalist.stillness",
        ],
        |record| &record.header.id,
        "ability",
    )?;
    require_exact_ids(
        &package.enemies,
        &[
            "enemy.bell_reed",
            "enemy.chain_sentry",
            "enemy.drowned_pilgrim",
        ],
        |record| &record.header.id,
        "enemy",
    )?;
    require_exact_ids(
        &package.arenas,
        &["arena.prototype.bell_laboratory_01"],
        |record| &record.header.id,
        "arena",
    )?;
    require_exact_ids(
        &package.patterns,
        &[
            "pattern.enemy.bell_reed.gap_ring",
            "pattern.enemy.chain_sentry.cross_lanes",
            "pattern.enemy.drowned_pilgrim.fan",
        ],
        |record| &record.header.id,
        "pattern",
    )?;
    require_exact_ids(
        &package.items,
        &[
            "consumable.red_tonic",
            "item.prototype.armor.parish_leather",
            "item.prototype.armor.reedcloth_wraps",
            "item.prototype.armor.saltglass_coat",
            "item.prototype.charm.still_eye",
            "item.prototype.charm.undertaker_knot",
            "item.prototype.relic.dented_scope",
            "item.prototype.relic.mark_lens",
            "item.prototype.relic.slip_clasp",
            "item.prototype.weapon.grave_repeater",
            "item.prototype.weapon.longbolt_crossbow",
            "item.prototype.weapon.pine_crossbow",
            "item.prototype.weapon.scatterbow",
        ],
        |record| &record.header.id,
        "item",
    )?;
    require_exact_ids(
        &package.drop_tables,
        &[
            "reward.prototype.boss",
            "reward.prototype.normal_enemy",
            "reward.prototype.wave_1",
            "reward.prototype.wave_2",
            "reward.prototype.wave_3",
        ],
        |record| &record.header.id,
        "drop table",
    )?;
    Ok(())
}

fn require_exact_ids<T, F>(records: &[T], expected: &[&str], id: F, domain: &str) -> Result<()>
where
    F: Fn(&T) -> &ContentId,
{
    let actual: BTreeSet<_> = records.iter().map(|record| id(record).as_str()).collect();
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    if actual != expected {
        bail!("illegal M01 {domain} IDs: expected {expected:?}, got {actual:?}");
    }
    Ok(())
}

fn all_headers(package: &ContentPackage) -> Vec<&CommonHeader> {
    package
        .classes
        .iter()
        .map(|record| &record.header)
        .chain(package.abilities.iter().map(|record| &record.header))
        .chain(package.enemies.iter().map(|record| &record.header))
        .chain(package.patterns.iter().map(|record| &record.header))
        .chain(package.arenas.iter().map(|record| &record.header))
        .chain(package.items.iter().map(|record| &record.header))
        .chain(package.drop_tables.iter().map(|record| &record.header))
        .collect()
}

fn hash_content_tree(root: &Path) -> Result<String> {
    let mut paths = Vec::new();
    collect_json_paths(root, root, &mut paths)?;
    paths.sort();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gravebound-content-package-v1\0");
    for relative in paths {
        let relative_text = relative.to_string_lossy().replace('\\', "/");
        let bytes = fs::read(root.join(&relative))?;
        hasher.update(&(relative_text.len() as u64).to_le_bytes());
        hasher.update(relative_text.as_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn collect_json_paths(root: &Path, current: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_paths(root, &path, output)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            output.push(path.strip_prefix(root)?.to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn valid_package() -> ContentPackage {
        load_and_validate(&content_root())
            .expect("checked-in content must validate")
            .0
    }

    #[test]
    fn checked_in_first_playable_package_is_valid() {
        let (_, report) = load_and_validate(&content_root()).expect("valid package");
        assert_eq!(report.content_version, FIRST_PLAYABLE_CONTENT_VERSION);
        assert_eq!(report.record_count, 30);
    }

    #[test]
    fn missing_cross_reference_fails() {
        let mut package = valid_package();
        package.classes[0].numeric_payload.primary_ability_id =
            ContentId::parse("ability.arbalist.missing").expect("valid test ID");
        let error = validate_references(&package).expect_err("missing ref must fail");
        assert!(error.to_string().contains("missing ability"));
    }

    #[test]
    fn illegal_m01_id_combination_fails() {
        let mut package = valid_package();
        package.items[0].header.id =
            ContentId::parse("item.prototype.invalid_substitute").expect("valid test ID");
        let error = validate_fp_combination(&package).expect_err("substitute must fail");
        assert!(error.to_string().contains("illegal M01 item IDs"));
    }

    #[test]
    fn invalid_reward_sum_fails() {
        let mut package = valid_package();
        package.drop_tables[0].numeric_payload.roll_groups[0].outcomes[0].weight += 1;
        let error = validate_references(&package).expect_err("invalid sum must fail");
        assert!(error.to_string().contains("total exactly 100"));
    }
}
