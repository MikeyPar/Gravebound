//! Strict, transitive Core death-presentation catalog for `GB-M03-06D`.
//!
//! The catalog owns only death-specific copy. Class, hostile, item, Oath, and Bargain names are
//! resolved through their independently compiled packages, whose exact revisions are embedded in
//! the death records hash. Unknown stored IDs therefore fail closed instead of being prettified or
//! reconstructed by the client.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-020`, `UI-009`, `UI-030`),
//! `Gravebound_Content_Production_Spec_v1.md` (`CONT-HUB-001`, `CONT-HUB-002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-06`, `GB-M03-07`, M03 exit gate).

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use content_schema::{
    ContentId, CoreCaldusAssetManifest, CoreDeathViewAssetManifest, CoreDeathViewCopyKind,
    CoreDeathViewDependencyRevisions, CoreDeathViewDevelopmentTarget, CoreDeathViewRecords,
    CoreDeathViewTargetKind, CoreEncounterRoomAssetManifest, CoreGrayboxAssetManifest,
    CoreWorldFlowCopyFile, SCHEMA_VERSION,
};

use crate::{
    CORE_CALDUS_ASSETS_PATH, CORE_ENCOUNTER_ROOM_ASSETS_PATH, CORE_WORLD_FLOW_ASSETS_PATH,
    CompiledOathBargainCatalog, CompiledProductionItemCatalog, CoreDevelopmentCaldus,
    CoreDevelopmentEncounterRooms, CoreDevelopmentIdentityCopy, CoreDevelopmentWorldFlow,
    load_core_development_caldus, load_core_development_encounter_rooms,
    load_core_development_identity_copy, load_core_development_items,
    load_core_development_oaths_bargains, load_core_development_world_flow,
};

pub const CORE_DEATH_VIEW_TARGET_NAME: &str = "core-dev-death-view";
pub const CORE_DEATH_VIEW_TARGET_PATH: &str = "core_dev/death_view.json";
pub const CORE_DEATH_VIEW_RECORDS_PATH: &str = "core_dev/death_view.records.json";
pub const CORE_DEATH_VIEW_ASSETS_PATH: &str = "core_dev/death_view.assets.json";
pub const CORE_DEATH_VIEW_COPY_PATH: &str = "core_dev/death_view.en-US.json";

const ACTION_IDS: &[&str] = &[
    "death.action.back",
    "death.action.character_select",
    "death.action.create_successor",
    "death.action.inspect_trace",
    "death.action.load_more",
    "death.action.memorial",
    "death.action.retry",
    "death.action.successor_unavailable",
];
const ATTACK_IDS: &[&str] = &[
    "attack.environment.core_hazard",
    "attack.network.disconnect",
];
const CAUSE_IDS: &[&str] = &[
    "death.cause.damage_over_time",
    "death.cause.direct_hit",
    "death.cause.disconnect",
    "death.cause.environment",
];
const DAMAGE_TYPE_IDS: &[&str] = &["death.damage_type.physical", "death.damage_type.veil"];
const DEED_IDS: &[&str] = &[
    "deed.core.sepulcher_knight_defeated",
    "deed.core.sir_caldus_defeated",
    "deed.none",
];
const ECHO_OUTCOME_IDS: &[&str] = &[
    "death.echo.available",
    "death.echo.dormant",
    "death.echo.not_eligible",
];
const ERROR_IDS: &[&str] = &[
    "death.error.content_mismatch",
    "death.error.corrupt_record",
    "death.error.death_not_found",
    "death.error.death_not_owned",
    "death.error.feature_disabled",
    "death.error.page_out_of_range",
    "death.error.service_unavailable",
    "death.error.title",
    "death.error.unauthenticated",
];
const FIELD_IDS: &[&str] = &[
    "death.field.attack",
    "death.field.cause",
    "death.field.class",
    "death.field.damage",
    "death.field.damage_type",
    "death.field.final_deed",
    "death.field.killer",
    "death.field.level",
    "death.field.lifetime",
    "death.field.network",
    "death.field.recall",
    "death.field.source_position",
];
const HERO_IDS: &[&str] = &["hero.core.grave_arbalist"];
const MATERIAL_IDS: &[&str] = &[
    "material.bell_brass",
    "material.echo_ember",
    "material.funeral_root",
    "material.saltglass_shard",
];
const MEMORIAL_PRESENTATION_IDS: &[&str] = &["memorial.presentation.core_default"];
const NETWORK_IDS: &[&str] = &[
    "death.network.connected",
    "death.network.degraded",
    "death.network.link_lost",
    "death.network.reattached",
];
const PATTERN_IDS: &[&str] = &[
    "boss.caldus.bell_ring",
    "boss.caldus.charge_lane",
    "boss.caldus.charge_stop_ring",
    "boss.caldus.shield_arc",
    "miniboss.choir_abbot.recovery_ring",
    "miniboss.choir_abbot.rotor",
    "miniboss.sepulcher_knight.charge_lane",
    "miniboss.sepulcher_knight.shield_fan",
    "miniboss.sepulcher_knight.stop_ring",
    "pattern.enemy.bell_acolyte.alternating_fan",
    "pattern.enemy.bell_reed.gap_ring",
    "pattern.enemy.chain_sentry.cross_lanes",
    "pattern.enemy.choir_skull.rotor",
    "pattern.enemy.drowned_pilgrim.fan",
    "pattern.enemy.mire_leech.charge",
];
const PROJECTION_IDS: &[&str] = &[
    "projection.created.echo",
    "projection.created.memorial",
    "projection.preserved.account_records",
    "projection.preserved.cosmetics",
    "projection.preserved.currency",
    "projection.preserved.recipes",
    "projection.preserved.vault",
];
const RECALL_IDS: &[&str] = &[
    "death.recall.channeling",
    "death.recall.completion_pending",
    "death.recall.inactive",
];
const SECTION_IDS: &[&str] = &[
    "death.section.created",
    "death.section.hero",
    "death.section.lost",
    "death.section.network",
    "death.section.preserved",
    "death.section.timeline",
    "death.section.what_happened",
];
const SOURCE_IDS: &[&str] = &["environment.core.hazard", "network.disconnect"];
const STATE_IDS: &[&str] = &[
    "death.state.awaiting_commit",
    "death.state.awaiting_commit_detail",
    "death.state.loading_memorial",
    "death.state.loading_summary",
    "death.state.loading_trace",
];
const STATUS_IDS: &[&str] = &[
    "status.bleed",
    "status.exhaustion",
    "status.frostbind",
    "status.guardbreak",
    "status.hex",
    "status.marked",
    "status.silence",
];
const SURFACE_IDS: &[&str] = &[
    "death.memorial.empty",
    "death.memorial.title",
    "death.summary.eyebrow",
    "death.summary.title",
];
const ASSET_IDS: &[&str] = &[
    "portrait.boss.sir_caldus",
    "portrait.miniboss.sepulcher_knight",
    "sprite.station.memorial_wall",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDeathViewHashes {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

/// Immutable renderer-independent presentation authority.
#[derive(Debug, Clone)]
pub struct CoreDevelopmentDeathView {
    target_name: String,
    hashes: CoreDeathViewHashes,
    item_content_revision: String,
    death_owned_copy: BTreeMap<String, CoreDeathViewCopyValue>,
    dependency_names: CoreDeathViewDependencyNames,
    asset_ids: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct CoreDeathViewCopyValue {
    kind: CoreDeathViewCopyKind,
    value: String,
}

#[derive(Debug, Clone, Default)]
struct CoreDeathViewDependencyNames {
    classes: BTreeMap<String, String>,
    items: BTreeMap<String, String>,
    sources: BTreeMap<String, String>,
    oaths: BTreeMap<String, String>,
    bargains: BTreeMap<String, String>,
}

impl CoreDeathViewDependencyNames {
    fn contains(&self, content_id: &str) -> bool {
        self.classes.contains_key(content_id)
            || self.items.contains_key(content_id)
            || self.sources.contains_key(content_id)
            || self.oaths.contains_key(content_id)
            || self.bargains.contains_key(content_id)
    }
}

impl CoreDevelopmentDeathView {
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub const fn hashes(&self) -> &CoreDeathViewHashes {
        &self.hashes
    }

    #[must_use]
    pub fn item_content_revision(&self) -> &str {
        &self.item_content_revision
    }

    /// Resolves a death-owned label only when its semantic domain matches the requesting field.
    /// This prevents a valid item, class, or source ID from being accepted as unrelated copy.
    #[must_use]
    pub fn resolve_copy(&self, kind: CoreDeathViewCopyKind, content_id: &str) -> Option<&str> {
        self.death_owned_copy
            .get(content_id)
            .filter(|entry| entry.kind == kind)
            .map(|entry| entry.value.as_str())
    }

    #[must_use]
    pub fn resolve_class(&self, content_id: &str) -> Option<&str> {
        self.dependency_names
            .classes
            .get(content_id)
            .map(String::as_str)
    }

    #[must_use]
    pub fn resolve_item(&self, content_id: &str) -> Option<&str> {
        self.dependency_names
            .items
            .get(content_id)
            .map(String::as_str)
    }

    #[must_use]
    pub fn resolve_source(&self, content_id: &str) -> Option<&str> {
        self.resolve_copy(CoreDeathViewCopyKind::Source, content_id)
            .or_else(|| {
                self.dependency_names
                    .sources
                    .get(content_id)
                    .map(String::as_str)
            })
    }

    /// Core combat records use their canonical pattern ID as the attack ID. Environment and
    /// disconnect deaths use the two explicit non-pattern attack IDs.
    #[must_use]
    pub fn resolve_attack(&self, content_id: &str) -> Option<&str> {
        self.resolve_copy(CoreDeathViewCopyKind::Attack, content_id)
            .or_else(|| self.resolve_copy(CoreDeathViewCopyKind::Pattern, content_id))
    }

    #[must_use]
    pub fn resolve_pattern(&self, content_id: &str) -> Option<&str> {
        self.resolve_copy(CoreDeathViewCopyKind::Pattern, content_id)
    }

    #[must_use]
    pub fn resolve_status(&self, content_id: &str) -> Option<&str> {
        self.resolve_copy(CoreDeathViewCopyKind::Status, content_id)
    }

    #[must_use]
    pub fn resolve_oath(&self, content_id: &str) -> Option<&str> {
        self.dependency_names
            .oaths
            .get(content_id)
            .map(String::as_str)
    }

    #[must_use]
    pub fn resolve_bargain(&self, content_id: &str) -> Option<&str> {
        self.dependency_names
            .bargains
            .get(content_id)
            .map(String::as_str)
    }

    #[must_use]
    pub fn contains_asset(&self, asset_id: &str) -> bool {
        self.asset_ids.contains(asset_id)
    }
}

/// Loads every dependency first and then proves the death catalog matches their exact revisions.
pub fn load_core_development_death_view(root: &Path) -> Result<CoreDevelopmentDeathView> {
    let target: CoreDeathViewDevelopmentTarget =
        crate::read_json(&root.join(CORE_DEATH_VIEW_TARGET_PATH))?;
    let records: CoreDeathViewRecords = crate::read_json(&root.join(CORE_DEATH_VIEW_RECORDS_PATH))?;
    let assets: CoreDeathViewAssetManifest =
        crate::read_json(&root.join(CORE_DEATH_VIEW_ASSETS_PATH))?;
    let copy: CoreWorldFlowCopyFile = crate::read_json(&root.join(CORE_DEATH_VIEW_COPY_PATH))?;
    let hashes = CoreDeathViewHashes {
        records_blake3: hash_file(&root.join(CORE_DEATH_VIEW_RECORDS_PATH))?,
        assets_blake3: hash_file(&root.join(CORE_DEATH_VIEW_ASSETS_PATH))?,
        localization_blake3: hash_file(&root.join(CORE_DEATH_VIEW_COPY_PATH))?,
    };

    let world = load_core_development_world_flow(root)?;
    let identity = load_core_development_identity_copy(root)?;
    let items = load_core_development_items(root)?;
    let encounters = load_core_development_encounter_rooms(root)?;
    let caldus = load_core_development_caldus(root)?;
    let oaths = load_core_development_oaths_bargains(root)?;

    validate_target(&target, &hashes)?;
    validate_records(
        root,
        &target,
        &records,
        &world,
        &items,
        &encounters,
        &caldus,
        &oaths,
    )?;
    let death_owned_copy = validate_copy(&target, &records, &copy)?;
    let asset_ids = validate_assets(root, &target, &assets)?;
    let dependency_names = dependency_names(&identity, &items, &encounters, &caldus, &oaths)?;

    if death_owned_copy
        .keys()
        .any(|id| dependency_names.contains(id))
    {
        bail!("Core death-view copy duplicates dependency-owned content");
    }

    Ok(CoreDevelopmentDeathView {
        target_name: target.target_name,
        hashes,
        item_content_revision: records.content_revision,
        death_owned_copy,
        dependency_names,
        asset_ids,
    })
}

fn validate_target(
    target: &CoreDeathViewDevelopmentTarget,
    hashes: &CoreDeathViewHashes,
) -> Result<()> {
    if target.schema_version != SCHEMA_VERSION
        || target.target_kind != CoreDeathViewTargetKind::UnpromotedDeathViewSubset
        || target.target_name != CORE_DEATH_VIEW_TARGET_NAME
    {
        bail!("Core death-view target identity is invalid");
    }
    let required = required_copy_ids();
    if target
        .required_copy_ids
        .iter()
        .map(ContentId::as_str)
        .collect::<Vec<_>>()
        != required
        || target
            .required_asset_ids
            .iter()
            .map(ContentId::as_str)
            .collect::<Vec<_>>()
            != ASSET_IDS
    {
        bail!("Core death-view copy or asset allowlist drifted");
    }
    if target.expected_records_blake3 != hashes.records_blake3
        || target.expected_assets_blake3 != hashes.assets_blake3
        || target.expected_localization_blake3 != hashes.localization_blake3
    {
        bail!(
            "Core death-view source hashes do not match the reviewed target: records={}, assets={}, localization={}",
            hashes.records_blake3,
            hashes.assets_blake3,
            hashes.localization_blake3
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_records(
    root: &Path,
    target: &CoreDeathViewDevelopmentTarget,
    records: &CoreDeathViewRecords,
    world: &CoreDevelopmentWorldFlow,
    items: &CompiledProductionItemCatalog,
    encounters: &CoreDevelopmentEncounterRooms,
    caldus: &CoreDevelopmentCaldus,
    oaths: &CompiledOathBargainCatalog,
) -> Result<()> {
    if records.schema_version != SCHEMA_VERSION
        || records.content_revision != items.revision_label()
        || records.copy_bindings.len() != target.required_copy_ids.len()
    {
        bail!("Core death-view record identity or item revision drifted");
    }
    let actual_dependencies = dependency_revisions(root, world, items, encounters, caldus, oaths)?;
    if records.dependencies != actual_dependencies {
        bail!(
            "Core death-view dependency revision closure drifted: expected={:?}, actual={actual_dependencies:?}",
            records.dependencies
        );
    }
    let reachable_patterns = encounters
        .roster()
        .iter()
        .flat_map(|member| member.required_pattern_ids.iter())
        .map(ContentId::as_str)
        .chain(caldus.patterns().iter().map(|pattern| pattern.id.as_str()))
        .collect::<BTreeSet<_>>();
    let death_patterns = PATTERN_IDS.iter().copied().collect::<BTreeSet<_>>();
    if reachable_patterns != death_patterns {
        bail!(
            "Core death-view pattern closure drifted: death={death_patterns:?}, reachable={reachable_patterns:?}"
        );
    }
    for (binding, required_id) in records.copy_bindings.iter().zip(&target.required_copy_ids) {
        if &binding.content_id != required_id
            || binding.localization_key != binding.content_id
            || binding.kind != expected_kind(binding.content_id.as_str())?
        {
            bail!("Core death-view binding {} drifted", binding.content_id);
        }
    }
    Ok(())
}

fn validate_copy(
    target: &CoreDeathViewDevelopmentTarget,
    records: &CoreDeathViewRecords,
    copy: &CoreWorldFlowCopyFile,
) -> Result<BTreeMap<String, CoreDeathViewCopyValue>> {
    if copy.schema_version != SCHEMA_VERSION
        || copy.locale != "en-US"
        || copy.entries.len() != records.copy_bindings.len()
    {
        bail!("Core death-view copy schema, locale, or count drifted");
    }
    let mut values = BTreeMap::new();
    for ((entry, binding), required_id) in copy
        .entries
        .iter()
        .zip(&records.copy_bindings)
        .zip(&target.required_copy_ids)
    {
        if &entry.key != required_id
            || entry.key != binding.localization_key
            || entry.value.trim().is_empty()
            || entry
                .value
                .chars()
                .any(|character| character.is_control() && character != '\n')
            || values
                .insert(
                    entry.key.to_string(),
                    CoreDeathViewCopyValue {
                        kind: binding.kind,
                        value: entry.value.clone(),
                    },
                )
                .is_some()
        {
            bail!("Core death-view localization closure is invalid");
        }
    }
    Ok(values)
}

fn validate_assets(
    root: &Path,
    target: &CoreDeathViewDevelopmentTarget,
    assets: &CoreDeathViewAssetManifest,
) -> Result<BTreeSet<String>> {
    if assets.schema_version != SCHEMA_VERSION
        || assets.assets.len() != target.required_asset_ids.len()
    {
        bail!("Core death-view asset manifest identity drifted");
    }
    let world_assets: CoreGrayboxAssetManifest =
        crate::read_json(&root.join(CORE_WORLD_FLOW_ASSETS_PATH))?;
    let encounter_assets: CoreEncounterRoomAssetManifest =
        crate::read_json(&root.join(CORE_ENCOUNTER_ROOM_ASSETS_PATH))?;
    let caldus_assets: CoreCaldusAssetManifest =
        crate::read_json(&root.join(CORE_CALDUS_ASSETS_PATH))?;
    let available = world_assets
        .assets
        .iter()
        .map(|asset| (asset.asset_id.as_str(), asset.source_record_id.as_str()))
        .chain(
            encounter_assets
                .assets
                .iter()
                .map(|asset| (asset.asset_id.as_str(), asset.source_record_id.as_str())),
        )
        .chain(
            caldus_assets
                .assets
                .iter()
                .map(|asset| (asset.asset_id.as_str(), asset.source_record_id.as_str())),
        )
        .collect::<BTreeSet<_>>();
    let mut output = BTreeSet::new();
    for (asset, required_id) in assets.assets.iter().zip(&target.required_asset_ids) {
        if &asset.asset_id != required_id
            || !available.contains(&(asset.asset_id.as_str(), asset.source_content_id.as_str()))
            || !output.insert(asset.asset_id.to_string())
        {
            bail!(
                "Core death-view asset {} is not dependency-backed",
                asset.asset_id
            );
        }
    }
    Ok(output)
}

fn dependency_revisions(
    root: &Path,
    world: &CoreDevelopmentWorldFlow,
    items: &CompiledProductionItemCatalog,
    encounters: &CoreDevelopmentEncounterRooms,
    caldus: &CoreDevelopmentCaldus,
    oaths: &CompiledOathBargainCatalog,
) -> Result<CoreDeathViewDependencyRevisions> {
    Ok(CoreDeathViewDependencyRevisions {
        identity_manifest_blake3: identity_manifest_hash(root)?,
        world_records_blake3: world.hashes().records_blake3.clone(),
        world_assets_blake3: world.hashes().assets_blake3.clone(),
        world_localization_blake3: world.hashes().localization_blake3.clone(),
        item_manifest_blake3: items.hashes().manifest_blake3.clone(),
        encounter_records_blake3: encounters.hashes().records_blake3.clone(),
        encounter_assets_blake3: encounters.hashes().assets_blake3.clone(),
        encounter_localization_blake3: encounters.hashes().localization_blake3.clone(),
        caldus_records_blake3: caldus.hashes().records_blake3.clone(),
        caldus_assets_blake3: caldus.hashes().assets_blake3.clone(),
        caldus_localization_blake3: caldus.hashes().localization_blake3.clone(),
        oath_bargain_manifest_blake3: oaths.hashes().manifest_blake3.clone(),
    })
}

fn dependency_names(
    identity: &CoreDevelopmentIdentityCopy,
    items: &CompiledProductionItemCatalog,
    encounters: &CoreDevelopmentEncounterRooms,
    caldus: &CoreDevelopmentCaldus,
    oaths: &CompiledOathBargainCatalog,
) -> Result<CoreDeathViewDependencyNames> {
    let mut output = CoreDeathViewDependencyNames::default();
    insert_name(
        &mut output.classes,
        "class.grave_arbalist",
        identity.class_name(),
    )?;
    for item_id in items.items().keys() {
        insert_name(
            &mut output.items,
            item_id,
            items
                .localized_item_name(item_id)
                .with_context(|| format!("missing Core item name {item_id}"))?,
        )?;
    }
    for actor in encounters.actor_definitions() {
        let actor_id = actor.id().as_str();
        let key = format!("{actor_id}.name");
        insert_name(
            &mut output.sources,
            actor_id,
            encounters
                .localized(&key)
                .with_context(|| format!("missing Core hostile name {actor_id}"))?,
        )?;
    }
    let boss = caldus.boss();
    insert_name(
        &mut output.sources,
        boss.header.id.as_str(),
        caldus
            .localized(boss.header.localization_name_key.as_str())
            .context("missing Sir Caldus name")?,
    )?;
    for oath in oaths.oaths().values() {
        insert_name(
            &mut output.oaths,
            oath.header.id.as_str(),
            oaths
                .localized(oath.header.localization_name_key.as_str())
                .with_context(|| format!("missing Core Oath name {}", oath.header.id))?,
        )?;
    }
    for bargain in oaths.bargains().values() {
        insert_name(
            &mut output.bargains,
            bargain.header.id.as_str(),
            oaths
                .localized(bargain.header.localization_name_key.as_str())
                .with_context(|| format!("missing Core Bargain name {}", bargain.header.id))?,
        )?;
    }
    Ok(output)
}

fn insert_name(output: &mut BTreeMap<String, String>, id: &str, name: &str) -> Result<()> {
    if name.trim().is_empty() || output.insert(id.to_owned(), name.to_owned()).is_some() {
        bail!("Core death-view dependency name {id} is empty or duplicated");
    }
    Ok(())
}

fn required_copy_ids() -> Vec<&'static str> {
    let mut ids = copy_groups()
        .into_iter()
        .flat_map(|(ids, _)| ids.iter().copied())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

fn expected_kind(id: &str) -> Result<CoreDeathViewCopyKind> {
    copy_groups()
        .into_iter()
        .find_map(|(ids, kind)| ids.contains(&id).then_some(kind))
        .with_context(|| format!("unknown Core death-view copy ID {id}"))
}

#[allow(clippy::type_complexity)]
fn copy_groups() -> [(&'static [&'static str], CoreDeathViewCopyKind); 20] {
    [
        (ACTION_IDS, CoreDeathViewCopyKind::Action),
        (ATTACK_IDS, CoreDeathViewCopyKind::Attack),
        (CAUSE_IDS, CoreDeathViewCopyKind::Cause),
        (DAMAGE_TYPE_IDS, CoreDeathViewCopyKind::DamageType),
        (DEED_IDS, CoreDeathViewCopyKind::Deed),
        (ECHO_OUTCOME_IDS, CoreDeathViewCopyKind::EchoOutcome),
        (ERROR_IDS, CoreDeathViewCopyKind::Error),
        (FIELD_IDS, CoreDeathViewCopyKind::Field),
        (HERO_IDS, CoreDeathViewCopyKind::HeroLabel),
        (MATERIAL_IDS, CoreDeathViewCopyKind::Material),
        (
            MEMORIAL_PRESENTATION_IDS,
            CoreDeathViewCopyKind::MemorialPresentation,
        ),
        (NETWORK_IDS, CoreDeathViewCopyKind::NetworkState),
        (PATTERN_IDS, CoreDeathViewCopyKind::Pattern),
        (PROJECTION_IDS, CoreDeathViewCopyKind::Projection),
        (RECALL_IDS, CoreDeathViewCopyKind::RecallState),
        (SECTION_IDS, CoreDeathViewCopyKind::Section),
        (SOURCE_IDS, CoreDeathViewCopyKind::Source),
        (STATE_IDS, CoreDeathViewCopyKind::State),
        (STATUS_IDS, CoreDeathViewCopyKind::Status),
        (SURFACE_IDS, CoreDeathViewCopyKind::Surface),
    ]
}

fn identity_manifest_hash(root: &Path) -> Result<String> {
    const PATHS: [&str; 5] = [
        "core_dev/identity.en-US.json",
        "core_dev/identity.json",
        "fp/abilities.json",
        "fp/classes.json",
        "localization/en-US.json",
    ];
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gravebound.core-death.identity-dependency.v1\0");
    for relative in PATHS {
        let bytes = fs::read(root.join(relative))
            .with_context(|| format!("failed to read death-view dependency {relative}"))?;
        hasher.update(&(relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn checked_in_death_view_catalog_is_complete_and_transitive() {
        let catalog = load_core_development_death_view(&content_root()).unwrap();
        assert_eq!(catalog.target_name(), CORE_DEATH_VIEW_TARGET_NAME);
        assert_eq!(
            catalog.resolve_class("class.grave_arbalist"),
            Some("Grave Arbalist")
        );
        assert_eq!(
            catalog.resolve_source("miniboss.sepulcher_knight"),
            Some("Sepulcher Knight")
        );
        assert_eq!(
            catalog.resolve_item("item.weapon.crossbow.pine_crossbow"),
            Some("Pine Crossbow")
        );
        assert_eq!(
            catalog.resolve_attack("boss.caldus.bell_ring"),
            Some("Bell Ring")
        );
        assert_eq!(
            catalog.resolve_copy(CoreDeathViewCopyKind::Deed, "deed.none"),
            Some("No final deed recorded.")
        );
        assert_eq!(
            catalog.resolve_pattern("boss.caldus.bell_ring"),
            Some("Bell Ring")
        );
        assert_eq!(catalog.resolve_attack("attack.caldus.bell_ring"), None);
        assert_eq!(catalog.resolve_attack("class.grave_arbalist"), None);
        assert_eq!(
            catalog.resolve_status("item.weapon.crossbow.pine_crossbow"),
            None
        );
        for pattern_id in PATTERN_IDS {
            assert!(catalog.resolve_attack(pattern_id).is_some(), "{pattern_id}");
            assert!(
                catalog.resolve_pattern(pattern_id).is_some(),
                "{pattern_id}"
            );
        }
        for asset_id in ASSET_IDS {
            assert!(catalog.contains_asset(asset_id));
        }
    }

    #[test]
    fn every_enabled_core_trace_producer_resolves_before_danger_admission() {
        let root = content_root();
        let catalog = load_core_development_death_view(&root).unwrap();
        let encounters = load_core_development_encounter_rooms(&root).unwrap();
        let caldus = load_core_development_caldus(&root).unwrap();

        let reachable_sources = encounters
            .actor_definitions()
            .iter()
            .map(|actor| actor.id().as_str())
            .chain(std::iter::once(caldus.boss().header.id.as_str()))
            .chain(SOURCE_IDS.iter().copied())
            .collect::<BTreeSet<_>>();
        for source_id in reachable_sources {
            assert!(catalog.resolve_source(source_id).is_some(), "{source_id}");
        }
        for attack_id in ATTACK_IDS
            .iter()
            .copied()
            .chain(PATTERN_IDS.iter().copied())
        {
            assert!(catalog.resolve_attack(attack_id).is_some(), "{attack_id}");
        }
        for pattern_id in PATTERN_IDS {
            assert!(
                catalog.resolve_pattern(pattern_id).is_some(),
                "{pattern_id}"
            );
        }
        for status_id in STATUS_IDS {
            assert!(catalog.resolve_status(status_id).is_some(), "{status_id}");
        }
    }

    #[test]
    fn every_death_owned_id_has_one_semantic_kind() {
        let ids = required_copy_ids();
        assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len());
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
        for id in ids {
            assert!(expected_kind(id).is_ok(), "{id}");
        }
    }

    #[test]
    fn dependency_copy_and_binding_drift_fail_closed() {
        let root = content_root();
        let target: CoreDeathViewDevelopmentTarget =
            crate::read_json(&root.join(CORE_DEATH_VIEW_TARGET_PATH)).unwrap();
        let records: CoreDeathViewRecords =
            crate::read_json(&root.join(CORE_DEATH_VIEW_RECORDS_PATH)).unwrap();
        let copy: CoreWorldFlowCopyFile =
            crate::read_json(&root.join(CORE_DEATH_VIEW_COPY_PATH)).unwrap();
        let world = load_core_development_world_flow(&root).unwrap();
        let items = load_core_development_items(&root).unwrap();
        let encounters = load_core_development_encounter_rooms(&root).unwrap();
        let caldus = load_core_development_caldus(&root).unwrap();
        let oaths = load_core_development_oaths_bargains(&root).unwrap();

        let mut changed_dependencies = records.clone();
        changed_dependencies.dependencies.item_manifest_blake3 = "a".repeat(64);
        assert!(
            validate_records(
                &root,
                &target,
                &changed_dependencies,
                &world,
                &items,
                &encounters,
                &caldus,
                &oaths,
            )
            .is_err()
        );

        let mut changed_binding = records.clone();
        changed_binding.copy_bindings[0].kind = CoreDeathViewCopyKind::Cause;
        assert!(
            validate_records(
                &root,
                &target,
                &changed_binding,
                &world,
                &items,
                &encounters,
                &caldus,
                &oaths,
            )
            .is_err()
        );

        let mut changed_copy = copy;
        changed_copy.entries[0].value.clear();
        assert!(validate_copy(&target, &records, &changed_copy).is_err());
    }
}
