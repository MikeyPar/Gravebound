//! Fail-closed compiler for the unpromoted Core identity subset (`GB-M03-01A`).

use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, bail};
use content_schema::{
    AbilityRecord, ClassPayload, ClassRecord, ContentId, CoreDevelopmentTarget,
    CoreDevelopmentTargetKind, FIRST_PLAYABLE_CONTENT_VERSION, SCHEMA_VERSION,
};

use crate::{
    ContentPackage, FIRST_PLAYABLE_CLASS_ID, FIRST_PLAYABLE_GRAVE_MARK_ID,
    FIRST_PLAYABLE_PRIMARY_ID, FIRST_PLAYABLE_SLIPSTEP_ID, FIRST_PLAYABLE_STILLNESS_ID,
};

/// Human-readable compiler target name. It is intentionally not a semantic bundle version.
pub const CORE_DEVELOPMENT_TARGET_NAME: &str = "core-dev";
/// Checked-in strict descriptor consumed by the Core development compiler.
pub const CORE_DEVELOPMENT_TARGET_PATH: &str = "core_dev/identity.json";
/// Only presentation asset authorized by `SPEC-CONFLICT-004` decision 1.
pub const CORE_DEVELOPMENT_BASE_SPRITE_ID: &str = "sprite.class.grave_arbalist";

const REQUIRED_CLASS_IDS: [&str; 1] = [FIRST_PLAYABLE_CLASS_ID];
const REQUIRED_ABILITY_IDS: [&str; 4] = [
    FIRST_PLAYABLE_PRIMARY_ID,
    FIRST_PLAYABLE_GRAVE_MARK_ID,
    FIRST_PLAYABLE_SLIPSTEP_ID,
    FIRST_PLAYABLE_STILLNESS_ID,
];
const PRESENTATION_ASSET_IDS: [&str; 1] = [CORE_DEVELOPMENT_BASE_SPRITE_ID];

/// Immutable compiled identity content for Core development.
///
/// This type intentionally does not implement serialization and does not contain a release
/// manifest, bundle ID, release stage, or promotion record. It is a runtime view over exact,
/// validated `fp.1.0.0` records while the complete Core bundle remains unfinished.
#[derive(Debug, Clone)]
pub struct CoreDevelopmentIdentity {
    source_content_version: String,
    class: ClassRecord,
    abilities: Vec<AbilityRecord>,
    base_sprite_id: ContentId,
}

impl CoreDevelopmentIdentity {
    /// Source version whose immutable stable records back this development view.
    #[must_use]
    pub fn source_content_version(&self) -> &str {
        &self.source_content_version
    }

    /// The one stage-legal class available to Core identity creation.
    #[must_use]
    pub const fn class(&self) -> &ClassRecord {
        &self.class
    }

    /// Exact primary, two active abilities, and passive in canonical class order.
    #[must_use]
    pub fn abilities(&self) -> &[AbilityRecord] {
        &self.abilities
    }

    /// Locked silhouette preview authorized for Core; this is not an appearance entitlement.
    #[must_use]
    pub const fn base_sprite_id(&self) -> &ContentId {
        &self.base_sprite_id
    }
}

/// Loads the strict descriptor and compiles it only from the fully validated FP package.
pub fn load_core_development_identity(root: &Path) -> Result<CoreDevelopmentIdentity> {
    let (source, _) = crate::load_and_validate(root)
        .context("Core development source package failed First Playable validation")?;
    let target: CoreDevelopmentTarget = crate::read_json(&root.join(CORE_DEVELOPMENT_TARGET_PATH))?;
    compile_core_development_identity(&source, &target)
}

/// Compiles a non-promotable identity view from exact stable source records.
///
/// Callers cannot broaden this boundary with later Core content. Each additional Core domain gets
/// its own reviewed development target until formal `core.1.0.0` promotion is authorized.
pub fn compile_core_development_identity(
    source: &ContentPackage,
    target: &CoreDevelopmentTarget,
) -> Result<CoreDevelopmentIdentity> {
    crate::validate_package(source)
        .context("Core development compilation requires an exact validated fp.1.0.0 source")?;
    validate_target(target)?;

    let class = source
        .classes
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_CLASS_ID)
        .context("Core development source is missing class.grave_arbalist")?
        .clone();
    require_exact_stable_class(&class)?;

    let class_ability_ids = std::iter::once(&class.numeric_payload.primary_ability_id)
        .chain(class.numeric_payload.active_ability_ids.iter())
        .chain(std::iter::once(&class.numeric_payload.passive_ability_id))
        .map(ContentId::as_str)
        .collect::<Vec<_>>();
    if class_ability_ids != REQUIRED_ABILITY_IDS {
        bail!("Core development class ability references do not match the stable Arbalist subset");
    }
    if class.header.asset_ids.len() != 1
        || class.header.asset_ids[0].as_str() != CORE_DEVELOPMENT_BASE_SPRITE_ID
    {
        bail!("Core development class must expose only the approved base sprite");
    }
    if !source
        .asset_manifest
        .asset_ids
        .iter()
        .any(|id| id.as_str() == CORE_DEVELOPMENT_BASE_SPRITE_ID)
    {
        bail!("Core development base sprite does not resolve in the source asset manifest");
    }

    let abilities = REQUIRED_ABILITY_IDS
        .iter()
        .map(|required| {
            source
                .abilities
                .iter()
                .find(|record| record.header.id.as_str() == *required)
                .with_context(|| format!("Core development source is missing {required}"))
                .cloned()
        })
        .collect::<Result<Vec<_>>>()?;
    require_exact_stable_ability_headers(&abilities)?;

    Ok(CoreDevelopmentIdentity {
        source_content_version: target.source_content_version.clone(),
        class,
        abilities,
        base_sprite_id: target.presentation_asset_ids[0].clone(),
    })
}

fn require_exact_stable_class(class: &ClassRecord) -> Result<()> {
    let expected = ClassPayload {
        starting_max_health: 120,
        health_per_level: 4,
        starting_armor: 2,
        armor_growth_levels: vec![7, 14],
        movement_speed_milli_tiles_per_second: 5_100,
        weapon_family: "crossbow".to_owned(),
        primary_ability_id: ContentId::parse(FIRST_PLAYABLE_PRIMARY_ID)
            .expect("built-in primary ID is valid"),
        active_ability_ids: vec![
            ContentId::parse(FIRST_PLAYABLE_GRAVE_MARK_ID)
                .expect("built-in Grave Mark ID is valid"),
            ContentId::parse(FIRST_PLAYABLE_SLIPSTEP_ID).expect("built-in Slipstep ID is valid"),
        ],
        passive_ability_id: ContentId::parse(FIRST_PLAYABLE_STILLNESS_ID)
            .expect("built-in Stillness ID is valid"),
    };
    if class.numeric_payload != expected {
        bail!("Core development Grave Arbalist payload drifted from the stable CLS-020 subset");
    }
    if class.header.tags != ["class", "ranged", "precision"]
        || class.header.source_document_feature_id != "CLS-020"
    {
        bail!("Core development Grave Arbalist metadata drifted from the stable CLS-020 subset");
    }
    Ok(())
}

fn require_exact_stable_ability_headers(abilities: &[AbilityRecord]) -> Result<()> {
    let expected_tags: [&[&str]; 4] = [
        &["primary", "projectile", "crossbow"],
        &["active", "projectile", "mark"],
        &["active", "movement", "empower"],
        &["passive", "focused"],
    ];
    for ((record, id), tags) in abilities
        .iter()
        .zip(REQUIRED_ABILITY_IDS)
        .zip(expected_tags)
    {
        let expected_assets = [format!("vfx.{id}"), format!("audio.{id}")];
        if record.header.id.as_str() != id
            || record.header.source_document_feature_id != "CLS-020"
            || record
                .header
                .tags
                .iter()
                .map(String::as_str)
                .ne(tags.iter().copied())
            || record
                .header
                .asset_ids
                .iter()
                .map(ContentId::as_str)
                .ne(expected_assets.iter().map(String::as_str))
        {
            bail!("Core development ability {id} metadata drifted from the stable CLS-020 subset");
        }
    }
    Ok(())
}

fn validate_target(target: &CoreDevelopmentTarget) -> Result<()> {
    if target.schema_version != SCHEMA_VERSION {
        bail!("Core development target schema version must be {SCHEMA_VERSION}");
    }
    if target.target_kind != CoreDevelopmentTargetKind::UnpromotedIdentitySubset {
        bail!("Core development target must remain an unpromoted identity subset");
    }
    if target.source_content_version != FIRST_PLAYABLE_CONTENT_VERSION {
        bail!("Core development identity must reuse immutable fp.1.0.0 records");
    }

    reject_forbidden_domains(target)?;
    require_unique_exact_ids(&target.required_class_ids, &REQUIRED_CLASS_IDS, "class")?;
    require_unique_exact_ids(
        &target.required_ability_ids,
        &REQUIRED_ABILITY_IDS,
        "ability",
    )?;
    require_unique_exact_ids(
        &target.presentation_asset_ids,
        &PRESENTATION_ASSET_IDS,
        "presentation asset",
    )?;
    Ok(())
}

fn reject_forbidden_domains(target: &CoreDevelopmentTarget) -> Result<()> {
    for id in target
        .required_class_ids
        .iter()
        .chain(target.required_ability_ids.iter())
        .chain(target.presentation_asset_ids.iter())
    {
        let value = id.as_str();
        if value.starts_with("item.")
            || value.starts_with("arena.")
            || value.starts_with("reward.")
            || value.contains(".prototype.")
        {
            bail!("Core development identity rejects item/arena/reward or prototype leakage: {id}");
        }
    }
    Ok(())
}

fn require_unique_exact_ids(actual: &[ContentId], expected: &[&str], domain: &str) -> Result<()> {
    let unique = actual.iter().collect::<BTreeSet<_>>();
    if unique.len() != actual.len() {
        bail!("Core development target contains duplicate {domain} IDs");
    }
    if actual
        .iter()
        .map(ContentId::as_str)
        .ne(expected.iter().copied())
    {
        bail!("Core development target has an unauthorized {domain} allowlist");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn source_and_target() -> (ContentPackage, CoreDevelopmentTarget) {
        let (source, _) = crate::load_and_validate(&content_root()).expect("valid FP source");
        let target = crate::read_json(&content_root().join(CORE_DEVELOPMENT_TARGET_PATH))
            .expect("valid Core development descriptor");
        (source, target)
    }

    #[test]
    fn checked_in_core_development_identity_compiles_exact_stable_subset() {
        let compiled =
            load_core_development_identity(&content_root()).expect("valid Core development target");
        assert_eq!(compiled.source_content_version(), "fp.1.0.0");
        assert_eq!(compiled.class().header.id.as_str(), FIRST_PLAYABLE_CLASS_ID);
        assert_eq!(
            compiled
                .abilities()
                .iter()
                .map(|record| record.header.id.as_str())
                .collect::<Vec<_>>(),
            REQUIRED_ABILITY_IDS
        );
        assert_eq!(
            compiled.base_sprite_id().as_str(),
            CORE_DEVELOPMENT_BASE_SPRITE_ID
        );
        assert!(!compiled.class().header.id.as_str().contains("prototype"));
        assert!(
            compiled.abilities().iter().all(|record| !record
                .header
                .id
                .as_str()
                .contains("prototype"))
        );
    }

    #[test]
    fn prototype_item_arena_and_reward_ids_fail_before_allowlist_resolution() {
        let (source, target) = source_and_target();
        for leaked in [
            "item.prototype.weapon.pine_crossbow",
            "arena.prototype.bell_laboratory_01",
            "reward.prototype.boss",
        ] {
            let mut changed = target.clone();
            changed.required_ability_ids[0] = ContentId::parse(leaked).expect("valid leaked ID");
            let error = compile_core_development_identity(&source, &changed)
                .expect_err("prototype domain must fail closed");
            assert!(
                error.to_string().contains("rejects item/arena/reward"),
                "{error:#}"
            );
        }
    }

    #[test]
    fn missing_extra_reordered_and_duplicate_ids_fail_closed() {
        let (source, target) = source_and_target();

        let mut missing = target.clone();
        missing.required_ability_ids.pop();
        assert!(compile_core_development_identity(&source, &missing).is_err());

        let mut extra = target.clone();
        extra
            .required_ability_ids
            .push(ContentId::parse("ability.arbalist.future").expect("valid ID"));
        assert!(compile_core_development_identity(&source, &extra).is_err());

        let mut reordered = target.clone();
        reordered.required_ability_ids.swap(0, 1);
        assert!(compile_core_development_identity(&source, &reordered).is_err());

        let mut duplicate = target;
        duplicate.required_ability_ids[1] = duplicate.required_ability_ids[0].clone();
        let error = compile_core_development_identity(&source, &duplicate)
            .expect_err("duplicates must fail closed");
        assert!(error.to_string().contains("duplicate ability"), "{error:#}");
    }

    #[test]
    fn source_version_and_stable_record_drift_fail_closed() {
        let (source, target) = source_and_target();

        let mut relabeled = target.clone();
        relabeled.source_content_version = "core.1.0.0".to_owned();
        assert!(compile_core_development_identity(&source, &relabeled).is_err());

        let mut changed_source = source;
        changed_source.classes[0]
            .numeric_payload
            .starting_max_health += 1;
        let error = compile_core_development_identity(&changed_source, &target)
            .expect_err("source payload drift must fail exact FP validation");
        assert!(error.to_string().contains("payload drifted"), "{error:#}");

        let (mut changed_source, target) = source_and_target();
        changed_source.abilities[0].header.tags[0] = "prototype".to_owned();
        let error = compile_core_development_identity(&changed_source, &target)
            .expect_err("source metadata drift must fail exact Core compilation");
        assert!(error.to_string().contains("metadata drifted"), "{error:#}");
    }

    #[test]
    fn no_core_release_or_promotion_artifact_exists() {
        for forbidden in [
            "manifests/core.1.0.0.json",
            "promotions/core.1.0.0.json",
            "packages/core.1.0.0.json",
        ] {
            assert!(
                !content_root().join(forbidden).exists(),
                "unpromoted Core work must not emit {forbidden}"
            );
        }
    }

    #[test]
    fn fp_tracked_content_bytes_match_the_reviewed_baseline() {
        const BASELINE: [(&str, &str); 11] = [
            (
                "fp/abilities.json",
                "5180ef75f4cea4574e6dfe156a5a2f2582456b7775ca0121d86dc73cf28ab4cb",
            ),
            (
                "fp/arenas.json",
                "d0d317e03fd5f39605c70f04a2aa08c658637a9415c83f4de6c2c61373cf6ce5",
            ),
            (
                "fp/bosses.json",
                "97336c226f295235a86555d6112f16c4cce270bb74fbb4a927d30d2f8cabb118",
            ),
            (
                "fp/classes.json",
                "e5db05b1b930228ce6ea0c60ecbae33a0655ff65e114d25154d26e97added42d",
            ),
            (
                "fp/drop_tables.json",
                "3aefc70216d62086d91c38f5e8de7246e9a240587c6cea8c2dc9288b72837852",
            ),
            (
                "fp/enemies.json",
                "b766faed52941d5dbeef5722796501912847b65ae0e52046483324b7b79c6f09",
            ),
            (
                "fp/items.json",
                "e794a1102cdc8106b0d7db43388e3350d979c62ad40fdcb46b647b41f8fb0853",
            ),
            (
                "fp/patterns.json",
                "998fd27d43be46512f1a82fc8f6cbd9a6a7fe1a5f79aea8f3afe3c328b940437",
            ),
            (
                "localization/en-US.json",
                "c067bec900e0e1e2bef59c28aae2e63d2dd9bc8096be20866dff0be46afdd8f5",
            ),
            (
                "manifests/assets.fp.json",
                "577534e292d0e3628eb3a29129ad7db005ed2c32c307728d5e441efe69a7fe81",
            ),
            (
                "manifests/fp.1.0.0.json",
                "5f88f6e8f8673ade03095b27e85890e1d939065ae520b8b57896f3b90afc56cc",
            ),
        ];
        let mismatches = BASELINE
            .iter()
            .filter_map(|(path, expected)| {
                let bytes = fs::read(content_root().join(path)).expect("frozen FP content file");
                let actual = blake3::hash(&bytes).to_hex().to_string();
                (actual != *expected).then(|| format!("{path}={actual}"))
            })
            .collect::<Vec<_>>();
        assert!(
            mismatches.is_empty(),
            "fp.1.0.0 tracked bytes changed or need baseline initialization:\n{}",
            mismatches.join("\n")
        );
    }
}
