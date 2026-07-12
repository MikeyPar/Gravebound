//! Fail-closed compiler for the unpromoted `GB-M03-04A` progression subset.

use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use content_schema::{
    ContentId, CoreClassProgressionRecord, CoreDevelopmentHeader, CoreProgressionDevelopmentTarget,
    CoreProgressionRecords, CoreProgressionTargetKind, CoreWorldFlowCopyFile,
    CoreXpEligibilityKind, CoreXpProfileRecord, CoreXpSourceBinding, CoreXpSourceKind,
    ReleaseStage, SCHEMA_VERSION,
};
use sim_core::{
    CORE_LEVEL_COUNT, GraveArbalistProgressionDefinition, LevelCurve, grave_arbalist_level_stats,
};

pub const CORE_PROGRESSION_TARGET_NAME: &str = "core-dev-progression";
pub const CORE_PROGRESSION_TARGET_PATH: &str = "core_dev/progression.json";
pub const CORE_PROGRESSION_RECORDS_PATH: &str = "core_dev/progression.records.json";
pub const CORE_PROGRESSION_COPY_PATH: &str = "core_dev/progression.en-US.json";

const CURVE_IDS: [&str; 1] = ["progression.curve.core_1_10"];
const CLASS_PROGRESSION_IDS: [&str; 1] = ["progression.class.grave_arbalist.core"];
const XP_PROFILE_IDS: [&str; 13] = [
    "xp.normal_t1",
    "xp.normal_t2",
    "xp.normal_t3",
    "xp.realm_elite",
    "xp.miniboss_t1",
    "xp.miniboss_t2",
    "xp.miniboss_t3",
    "xp.event_minor",
    "xp.event_major",
    "xp.boss_caldus",
    "xp.boss_veyr",
    "xp.boss_confessor",
    "xp.world_warden",
];
const BINDING_IDS: [&str; 6] = [
    "xp.binding.enemy.drowned_pilgrim",
    "xp.binding.enemy.bell_reed",
    "xp.binding.enemy.chain_sentry",
    "xp.binding.miniboss.sepulcher_knight",
    "xp.binding.miniboss.choir_abbot",
    "xp.binding.boss.sir_caldus",
];
type ExpectedXpProfile = (
    &'static str,
    bool,
    ReleaseStage,
    CoreXpSourceKind,
    CoreXpEligibilityKind,
    u32,
    u16,
);
const EXPECTED_XP_PROFILES: [ExpectedXpProfile; 13] = [
    (
        "xp.normal_t1",
        true,
        ReleaseStage::Core,
        CoreXpSourceKind::NormalEnemy,
        CoreXpEligibilityKind::OrdinaryEnemy,
        5,
        0,
    ),
    (
        "xp.normal_t2",
        false,
        ReleaseStage::Slice,
        CoreXpSourceKind::NormalEnemy,
        CoreXpEligibilityKind::OrdinaryEnemy,
        10,
        0,
    ),
    (
        "xp.normal_t3",
        false,
        ReleaseStage::Alpha,
        CoreXpSourceKind::NormalEnemy,
        CoreXpEligibilityKind::OrdinaryEnemy,
        15,
        0,
    ),
    (
        "xp.realm_elite",
        false,
        ReleaseStage::Slice,
        CoreXpSourceKind::RealmElite,
        CoreXpEligibilityKind::EncounterContribution,
        60,
        0,
    ),
    (
        "xp.miniboss_t1",
        true,
        ReleaseStage::Core,
        CoreXpSourceKind::Miniboss,
        CoreXpEligibilityKind::EncounterContribution,
        120,
        0,
    ),
    (
        "xp.miniboss_t2",
        false,
        ReleaseStage::Slice,
        CoreXpSourceKind::Miniboss,
        CoreXpEligibilityKind::EncounterContribution,
        220,
        0,
    ),
    (
        "xp.miniboss_t3",
        false,
        ReleaseStage::Alpha,
        CoreXpSourceKind::Miniboss,
        CoreXpEligibilityKind::EncounterContribution,
        350,
        0,
    ),
    (
        "xp.event_minor",
        false,
        ReleaseStage::Slice,
        CoreXpSourceKind::Event,
        CoreXpEligibilityKind::EncounterContribution,
        120,
        0,
    ),
    (
        "xp.event_major",
        false,
        ReleaseStage::Slice,
        CoreXpSourceKind::Event,
        CoreXpEligibilityKind::EncounterContribution,
        300,
        0,
    ),
    (
        "xp.boss_caldus",
        true,
        ReleaseStage::Core,
        CoreXpSourceKind::Boss,
        CoreXpEligibilityKind::EncounterContribution,
        450,
        5_000,
    ),
    (
        "xp.boss_veyr",
        false,
        ReleaseStage::Slice,
        CoreXpSourceKind::Boss,
        CoreXpEligibilityKind::EncounterContribution,
        800,
        5_000,
    ),
    (
        "xp.boss_confessor",
        false,
        ReleaseStage::Alpha,
        CoreXpSourceKind::Boss,
        CoreXpEligibilityKind::EncounterContribution,
        1_200,
        5_000,
    ),
    (
        "xp.world_warden",
        false,
        ReleaseStage::Alpha,
        CoreXpSourceKind::WorldClimax,
        CoreXpEligibilityKind::EncounterContribution,
        1_500,
        5_000,
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreProgressionHashes {
    pub records_blake3: String,
    pub localization_blake3: String,
}

/// Immutable compiled development view with no release or promotion API.
#[derive(Debug, Clone)]
pub struct CoreDevelopmentProgression {
    target_name: String,
    level_curve: LevelCurve,
    arbalist: GraveArbalistProgressionDefinition,
    xp_profiles: Vec<CoreXpProfileRecord>,
    source_bindings: Vec<CoreXpSourceBinding>,
    hashes: CoreProgressionHashes,
}

impl CoreDevelopmentProgression {
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub const fn level_curve(&self) -> LevelCurve {
        self.level_curve
    }

    #[must_use]
    pub const fn arbalist(&self) -> &GraveArbalistProgressionDefinition {
        &self.arbalist
    }

    #[must_use]
    pub fn xp_profiles(&self) -> &[CoreXpProfileRecord] {
        &self.xp_profiles
    }

    #[must_use]
    pub fn source_bindings(&self) -> &[CoreXpSourceBinding] {
        &self.source_bindings
    }

    #[must_use]
    pub const fn hashes(&self) -> &CoreProgressionHashes {
        &self.hashes
    }
}

pub fn load_core_development_progression(root: &Path) -> Result<CoreDevelopmentProgression> {
    crate::load_and_validate(root).context("progression compilation requires valid fp.1.0.0")?;
    let target: CoreProgressionDevelopmentTarget =
        crate::read_json(&root.join(CORE_PROGRESSION_TARGET_PATH))?;
    let records: CoreProgressionRecords =
        crate::read_json(&root.join(CORE_PROGRESSION_RECORDS_PATH))?;
    let copy: CoreWorldFlowCopyFile = crate::read_json(&root.join(CORE_PROGRESSION_COPY_PATH))?;
    let hashes = CoreProgressionHashes {
        records_blake3: hash_file(&root.join(CORE_PROGRESSION_RECORDS_PATH))?,
        localization_blake3: hash_file(&root.join(CORE_PROGRESSION_COPY_PATH))?,
    };
    compile_core_development_progression(&target, &records, &copy, &hashes)
}

pub fn compile_core_development_progression(
    target: &CoreProgressionDevelopmentTarget,
    records: &CoreProgressionRecords,
    copy: &CoreWorldFlowCopyFile,
    hashes: &CoreProgressionHashes,
) -> Result<CoreDevelopmentProgression> {
    validate_target(target, hashes)?;
    validate_allowlists(target, records)?;
    validate_headers(records)?;
    let curve = validate_curve(records)?;
    let arbalist = validate_class_progression(records, curve)?;
    validate_profiles(records)?;
    validate_bindings(records)?;
    validate_copy(records, copy)?;
    Ok(CoreDevelopmentProgression {
        target_name: target.target_name.clone(),
        level_curve: curve,
        arbalist,
        xp_profiles: records.xp_profiles.clone(),
        source_bindings: records.source_bindings.clone(),
        hashes: hashes.clone(),
    })
}

fn validate_target(
    target: &CoreProgressionDevelopmentTarget,
    hashes: &CoreProgressionHashes,
) -> Result<()> {
    if target.schema_version != SCHEMA_VERSION
        || target.target_kind != CoreProgressionTargetKind::UnpromotedProgressionSubset
        || target.target_name != CORE_PROGRESSION_TARGET_NAME
    {
        bail!("Core progression target identity is not the approved unpromoted target");
    }
    require_exact_ids(&target.required_curve_ids, &CURVE_IDS, "curve")?;
    require_exact_ids(
        &target.required_class_progression_ids,
        &CLASS_PROGRESSION_IDS,
        "class progression",
    )?;
    require_exact_ids(
        &target.required_xp_profile_ids,
        &XP_PROFILE_IDS,
        "XP profile",
    )?;
    require_exact_ids(
        &target.required_source_binding_ids,
        &BINDING_IDS,
        "source binding",
    )?;
    if target.expected_records_blake3 != hashes.records_blake3 {
        bail!(
            "Core progression records BLAKE3 mismatch: expected {}, actual {}",
            target.expected_records_blake3,
            hashes.records_blake3
        );
    }
    if target.expected_localization_blake3 != hashes.localization_blake3 {
        bail!(
            "Core progression localization BLAKE3 mismatch: expected {}, actual {}",
            target.expected_localization_blake3,
            hashes.localization_blake3
        );
    }
    Ok(())
}

fn validate_allowlists(
    target: &CoreProgressionDevelopmentTarget,
    records: &CoreProgressionRecords,
) -> Result<()> {
    if records.schema_version != SCHEMA_VERSION {
        bail!("Core progression records use an unsupported schema version");
    }
    require_exact_ids(
        &records
            .level_curves
            .iter()
            .map(|record| record.header.id.clone())
            .collect::<Vec<_>>(),
        &CURVE_IDS,
        "source curve",
    )?;
    require_exact_ids(
        &records
            .class_progression
            .iter()
            .map(|record| record.header.id.clone())
            .collect::<Vec<_>>(),
        &CLASS_PROGRESSION_IDS,
        "source class progression",
    )?;
    require_exact_ids(
        &records
            .xp_profiles
            .iter()
            .map(|record| record.header.id.clone())
            .collect::<Vec<_>>(),
        &XP_PROFILE_IDS,
        "source XP profile",
    )?;
    require_exact_ids(
        &records
            .source_bindings
            .iter()
            .map(|record| record.header.id.clone())
            .collect::<Vec<_>>(),
        &BINDING_IDS,
        "source binding",
    )?;
    if target.required_curve_ids.len()
        + target.required_class_progression_ids.len()
        + target.required_xp_profile_ids.len()
        + target.required_source_binding_ids.len()
        != 21
    {
        bail!("Core progression target must contain exactly 21 records");
    }
    Ok(())
}

fn validate_headers(records: &CoreProgressionRecords) -> Result<()> {
    for header in records
        .level_curves
        .iter()
        .map(|record| &record.header)
        .chain(
            records
                .class_progression
                .iter()
                .map(|record| &record.header),
        )
        .chain(records.xp_profiles.iter().map(|record| &record.header))
        .chain(records.source_bindings.iter().map(|record| &record.header))
    {
        validate_header(header)?;
    }
    Ok(())
}

fn validate_header(header: &CoreDevelopmentHeader) -> Result<()> {
    if header.schema_version != SCHEMA_VERSION
        || !header.asset_ids.is_empty()
        || header.tags.is_empty()
        || header.source_document_feature_id.trim().is_empty()
        || header.localization_name_key.as_str() != format!("{}.name", header.id)
        || header.localization_description_key.as_str() != format!("{}.description", header.id)
    {
        bail!(
            "Core progression record {} has invalid complete metadata",
            header.id
        );
    }
    Ok(())
}

fn validate_curve(records: &CoreProgressionRecords) -> Result<LevelCurve> {
    let source = records
        .level_curves
        .first()
        .context("Core progression requires one level curve")?;
    let cumulative_xp: [u32; CORE_LEVEL_COUNT] = source
        .cumulative_xp
        .clone()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Core level curve must contain exactly ten thresholds"))?;
    let curve = LevelCurve { cumulative_xp };
    curve.validate().map_err(|error| anyhow::anyhow!(error))?;
    if cumulative_xp != [0, 100, 250, 450, 700, 1_000, 1_350, 1_750, 2_200, 2_700]
        || !source.header.enabled
        || source.header.earliest_release_stage != ReleaseStage::Core
        || source.header.source_document_feature_id != "PROG-002"
    {
        bail!("Core level curve drifted from PROG-002 and approved SPEC-CONFLICT-007");
    }
    Ok(curve)
}

fn validate_class_progression(
    records: &CoreProgressionRecords,
    curve: LevelCurve,
) -> Result<GraveArbalistProgressionDefinition> {
    let source: &CoreClassProgressionRecord = records
        .class_progression
        .first()
        .context("Core progression requires one class progression record")?;
    let definition = GraveArbalistProgressionDefinition {
        starting_maximum_health: source.starting_maximum_health,
        health_per_level_after_one: source.health_per_level_after_one,
        starting_armor: source.starting_armor,
        armor_growth_levels: source.armor_growth_levels.clone(),
        movement_milli_tiles_per_second: source.movement_milli_tiles_per_second,
        level_damage_growth_basis_points: source.level_damage_growth_basis_points,
    };
    definition
        .validate()
        .map_err(|error| anyhow::anyhow!(error))?;
    if source.class_id.as_str() != "class.grave_arbalist"
        || source.level_curve_id.as_str() != CURVE_IDS[0]
        || definition.starting_maximum_health != 120
        || definition.health_per_level_after_one != 4
        || definition.starting_armor != 2
        || definition.armor_growth_levels != [7, 14]
        || definition.movement_milli_tiles_per_second != 5_100
        || definition.level_damage_growth_basis_points != 150
        || !source.header.enabled
        || source.header.earliest_release_stage != ReleaseStage::Core
    {
        bail!("Core Grave Arbalist progression drifted from CLS-002/020");
    }
    let level_ten =
        grave_arbalist_level_stats(&definition, 10).map_err(|error| anyhow::anyhow!(error))?;
    if curve.xp_cap() != 2_700
        || level_ten.maximum_health != 156
        || level_ten.armor != 3
        || level_ten.damage_multiplier_basis_points != 11_350
    {
        bail!("Core Grave Arbalist level-ten projection is inconsistent");
    }
    Ok(definition)
}

fn validate_profiles(records: &CoreProgressionRecords) -> Result<()> {
    for (record, expected) in records.xp_profiles.iter().zip(EXPECTED_XP_PROFILES) {
        if (
            record.header.id.as_str(),
            record.header.enabled,
            record.header.earliest_release_stage,
            record.source_kind,
            record.eligibility_kind,
            record.base_xp,
            record.first_account_clear_bonus_basis_points,
        ) != expected
        {
            bail!("Core XP profile {} drifted from PROG-003", record.header.id);
        }
    }
    Ok(())
}

fn validate_bindings(records: &CoreProgressionRecords) -> Result<()> {
    let expected = [
        ("enemy.drowned_pilgrim", "xp.normal_t1"),
        ("enemy.bell_reed", "xp.normal_t1"),
        ("enemy.chain_sentry", "xp.normal_t1"),
        ("miniboss.sepulcher_knight", "xp.miniboss_t1"),
        ("miniboss.choir_abbot", "xp.miniboss_t1"),
        ("boss.sir_caldus", "xp.boss_caldus"),
    ];
    let enabled_profiles = records
        .xp_profiles
        .iter()
        .filter(|profile| profile.header.enabled)
        .map(|profile| profile.header.id.as_str())
        .collect::<BTreeSet<_>>();
    for (binding, (source, profile)) in records.source_bindings.iter().zip(expected) {
        if !binding.header.enabled
            || binding.header.earliest_release_stage != ReleaseStage::Core
            || !binding.authored_core_enabled
            || binding.source_id.as_str() != source
            || binding.xp_profile_id.as_str() != profile
            || !enabled_profiles.contains(profile)
        {
            bail!(
                "Core XP binding {} is invalid or resolves a disabled profile",
                binding.header.id
            );
        }
    }
    Ok(())
}

fn validate_copy(records: &CoreProgressionRecords, copy: &CoreWorldFlowCopyFile) -> Result<()> {
    if copy.schema_version != SCHEMA_VERSION || copy.locale != "en-US" {
        bail!("Core progression copy must be schema 1 en-US");
    }
    let required = records
        .level_curves
        .iter()
        .map(|record| &record.header)
        .chain(
            records
                .class_progression
                .iter()
                .map(|record| &record.header),
        )
        .chain(records.xp_profiles.iter().map(|record| &record.header))
        .chain(records.source_bindings.iter().map(|record| &record.header))
        .flat_map(|header| {
            [
                header.localization_description_key.as_str(),
                header.localization_name_key.as_str(),
            ]
        })
        .collect::<BTreeSet<_>>();
    let actual = copy
        .entries
        .iter()
        .map(|entry| entry.key.as_str())
        .collect::<BTreeSet<_>>();
    if required != actual
        || actual.len() != copy.entries.len()
        || copy
            .entries
            .iter()
            .any(|entry| entry.value.trim().is_empty())
    {
        bail!("Core progression localization must resolve every record exactly once");
    }
    Ok(())
}

fn require_exact_ids(actual: &[ContentId], expected: &[&str], domain: &str) -> Result<()> {
    if actual.len() != expected.len()
        || actual
            .iter()
            .map(ContentId::as_str)
            .ne(expected.iter().copied())
        || actual.iter().collect::<BTreeSet<_>>().len() != actual.len()
    {
        bail!("Core progression target has an invalid {domain} allowlist");
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn checked_in_progression_compiles_exact_core_and_disabled_future_profiles() {
        let compiled = load_core_development_progression(&content_root()).unwrap();
        assert_eq!(compiled.target_name(), CORE_PROGRESSION_TARGET_NAME);
        assert_eq!(compiled.level_curve().xp_cap(), 2_700);
        assert_eq!(
            compiled
                .xp_profiles()
                .iter()
                .filter(|profile| profile.header.enabled)
                .count(),
            3
        );
        assert_eq!(compiled.source_bindings().len(), 6);
    }

    #[test]
    fn profile_or_stage_drift_fails_closed() {
        let root = content_root();
        let target: CoreProgressionDevelopmentTarget =
            crate::read_json(&root.join(CORE_PROGRESSION_TARGET_PATH)).unwrap();
        let mut records: CoreProgressionRecords =
            crate::read_json(&root.join(CORE_PROGRESSION_RECORDS_PATH)).unwrap();
        let copy: CoreWorldFlowCopyFile =
            crate::read_json(&root.join(CORE_PROGRESSION_COPY_PATH)).unwrap();
        let hashes = CoreProgressionHashes {
            records_blake3: target.expected_records_blake3.clone(),
            localization_blake3: target.expected_localization_blake3.clone(),
        };
        records.xp_profiles[0].base_xp = 6;
        assert!(compile_core_development_progression(&target, &records, &copy, &hashes).is_err());
        records.xp_profiles[0].base_xp = 5;
        records.xp_profiles[3].header.enabled = true;
        assert!(compile_core_development_progression(&target, &records, &copy, &hashes).is_err());
    }
}
