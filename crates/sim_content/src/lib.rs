//! Immutable, strict content loading and semantic validation for simulation consumers.

mod core_dev;
mod core_dev_copy;
mod core_dev_encounter_room;
mod core_dev_oath_bargain;
mod core_dev_progression;
mod core_dev_world_flow;
mod production_item;
mod prototype;

pub use core_dev::*;
pub use core_dev_copy::*;
pub use core_dev_encounter_room::*;
pub use core_dev_oath_bargain::*;
pub use core_dev_progression::*;
pub use core_dev_world_flow::*;
pub use production_item::*;
pub use prototype::*;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    num::NonZeroU64,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use content_schema::{
    AbilityPayload, AbilityRecord, ArenaRecord, AssetManifest, BossCueKind as ContentBossCueKind,
    BossMovementMode, BossPhase as ContentBossPhase, BossRecord, ClassRecord, CommonHeader,
    ContentId, DamageBand as ContentDamageBand, DamageType as ContentDamageType, DropTableRecord,
    EffectOperation, EnemyRecord, EnemyRole as ContentEnemyRole, EquipmentSlot,
    FIRST_PLAYABLE_CONTENT_VERSION, FeatureRegistry, ItemEffect, ItemPayload, ItemRarity,
    ItemRecord, PatternKind, PatternRecord, ReleaseManifest, ReleaseStage, SCHEMA_VERSION,
};
use sim_core::{
    ArenaAnchor, ArenaGeometry, AuthorityDefinitions, BELL_PROCTOR_CROSS_ID, BELL_PROCTOR_FAN_ID,
    BELL_PROCTOR_ID, BELL_PROCTOR_REWARD_ID, BELL_PROCTOR_RING_ID, BELL_REED_ID,
    BellProctorDefinition, BellProctorDefinitionParameters, BellReedDefinition,
    BellReedDefinitionParameters, BossCueKind, BossTimelineCue, CHAIN_SENTRY_ID,
    ChainSentryDefinition, ChainSentryDefinitionParameters, Counterplay, DROWNED_PILGRIM_ID,
    DamageBand, DamageType, DirectHitParameters, DirectHitRequest, DrownedPilgrimDefinition,
    DrownedPilgrimDefinitionParameters, EchoMemoryFamily, EnemyRole, EntityId, EntityIdAllocator,
    EquipmentItem, EquipmentSlot as SimulationEquipmentSlot, GraveMarkDefinition,
    GraveMarkDefinitionParameters, HostileDisposition, InventoryStack, ItemContentId,
    ItemInstanceId, LaneAttackDefinition, MILLI_TILES_PER_TILE, NORMAL_ENEMY_REWARD_TABLE_ID,
    NormalWaveDefinitions, NormalWaveEnemyKind, NormalWaveSpawn, PlayerCombatState,
    ProjectileAttackDefinition, RedTonicDefinition, RedTonicDefinitionParameters,
    SlipstepDefinition, SlipstepDefinitionParameters, SpawnInstanceId, StillnessDefinition,
    StillnessDefinitionParameters, TilePoint, TileRectangle, WeaponDefinition,
    WeaponDefinitionParameters, duration_ms_to_ticks_ceil, duration_ms_to_ticks_nearest,
    resolve_direct_hit, validate_damage_band,
};

/// Stable First Playable arena ID from `CONT-FP-001` and `CONT-FP-002`.
pub const FIRST_PLAYABLE_ARENA_ID: &str = "arena.prototype.bell_laboratory_01";
pub const FIRST_PLAYABLE_CLASS_ID: &str = "class.grave_arbalist";
pub const FIRST_PLAYABLE_PRIMARY_ID: &str = "ability.arbalist.primary_crossbow";
pub const FIRST_PLAYABLE_GRAVE_MARK_ID: &str = "ability.arbalist.grave_mark";
pub const FIRST_PLAYABLE_SLIPSTEP_ID: &str = "ability.arbalist.slipstep";
pub const FIRST_PLAYABLE_STILLNESS_ID: &str = "ability.arbalist.stillness";
pub const FIRST_PLAYABLE_WEAPON_ID: &str = "item.prototype.weapon.pine_crossbow";
pub const FIRST_PLAYABLE_RED_TONIC_ID: &str = "consumable.red_tonic";
pub const FIRST_PLAYABLE_UNDERTAKER_KNOT_ID: &str = "item.prototype.charm.undertaker_knot";
pub const FIRST_PLAYABLE_DROWNED_PILGRIM_PATTERN_ID: &str = "pattern.enemy.drowned_pilgrim.fan";
pub const FIRST_PLAYABLE_BELL_REED_PATTERN_ID: &str = "pattern.enemy.bell_reed.gap_ring";
pub const FIRST_PLAYABLE_CHAIN_SENTRY_PATTERN_ID: &str = "pattern.enemy.chain_sentry.cross_lanes";
pub const M02_COMBAT_TEST_REWARD_SEED: u64 = 7;
const COMMON_PROJECTILE_RADIUS_MILLI_TILES: u32 = 120;
const ABILITY_INPUT_BUFFER_MS: u64 = 100;
const GLOBAL_ABILITY_COOLDOWN_MS: u64 = 150;

/// Exact record counts for the M01 prototype bundle defined by `CONT-FP-001` through `CONT-FP-008`.
pub const FIRST_PLAYABLE_DOMAIN_COUNTS: [(&str, usize); 8] = [
    ("class", 1),
    ("ability", 4),
    ("enemy", 3),
    ("boss", 1),
    ("pattern", 6),
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
    pub bosses: Vec<BossRecord>,
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

#[derive(Debug, Clone)]
pub struct AuthorityCombatTestContent {
    pub definitions: AuthorityDefinitions,
    pub spawns: Vec<NormalWaveSpawn>,
    pub hostile_projectile_ids: EntityIdAllocator,
}

/// Compiles the M02 authority fixture exclusively from the validated `fp.1.0.0` package.
pub fn first_playable_authority_combat_test(
    package: &ContentPackage,
) -> Result<AuthorityCombatTestContent> {
    let arena = first_playable_arena(package)?;
    let class = package
        .classes
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_CLASS_ID)
        .context("First Playable Grave Arbalist class record is missing")?;
    let catalog = first_playable_equipment_catalog(package)?;
    let reedcloth = catalog
        .get("item.prototype.armor.reedcloth_wraps")
        .context("First Playable Reedcloth Wraps record is missing")?;
    let PrototypeItemBehavior::Armor {
        max_health_add,
        armor_add,
        veil_resistance_add_basis_points,
        ..
    } = reedcloth.behavior
    else {
        bail!("First Playable Reedcloth Wraps must compile as armor");
    };
    let maximum_health = checked_add_signed(
        class.numeric_payload.starting_max_health,
        max_health_add,
        "maximum health",
    )?;
    let starting_armor =
        checked_add_signed(class.numeric_payload.starting_armor, armor_add, "armor")?;
    let combat = PlayerCombatState::with_projectile_allocator(
        first_playable_weapon(package)?,
        first_playable_grave_mark(package)?,
        first_playable_slipstep(package)?,
        first_playable_stillness(package)?,
        EntityIdAllocator::starting_at(NonZeroU64::new(40_000).expect("nonzero fixture ID")),
    )?;
    let reward_catalog = first_playable_reward_catalog(package)?;
    let grants = reward_catalog.resolve(
        "reward.prototype.normal_enemy",
        &package.release_manifest.content_version,
        M02_COMBAT_TEST_REWARD_SEED,
        1,
    )?;
    let reward_stacks = grants
        .iter()
        .enumerate()
        .map(|(index, grant)| reward_stack(&catalog, index, grant))
        .collect::<Result<Vec<_>>>()?;
    if reward_stacks.is_empty() {
        bail!("M02 combat-test reward seed must resolve at least one personal pickup");
    }
    let spawn_point = arena
        .anchors
        .iter()
        .find(|anchor| anchor.id == "W1")
        .context("First Playable arena is missing W1")?
        .point;
    Ok(AuthorityCombatTestContent {
        definitions: AuthorityDefinitions {
            arena,
            wave: NormalWaveDefinitions {
                drowned_pilgrim: first_playable_drowned_pilgrim(package)?,
                bell_reed: first_playable_bell_reed(package)?,
                chain_sentry: first_playable_chain_sentry(package)?,
            },
            combat,
            red_tonic: first_playable_red_tonic(package)?,
            maximum_health,
            starting_armor,
            resistance_basis_points: veil_resistance_add_basis_points,
            reward_stacks,
        },
        spawns: vec![NormalWaveSpawn {
            instance_id: SpawnInstanceId {
                run_ordinal: 1,
                spawn_ordinal: 1,
            },
            kind: NormalWaveEnemyKind::DrownedPilgrim,
            position_milli_tiles: (spawn_point.x_milli_tiles, spawn_point.y_milli_tiles),
        }],
        hostile_projectile_ids: EntityIdAllocator::starting_at(
            NonZeroU64::new(20_000).expect("nonzero fixture ID"),
        ),
    })
}

fn checked_add_signed(base: u32, modifier: i32, label: &str) -> Result<u32> {
    let value = i64::from(base) + i64::from(modifier);
    u32::try_from(value).with_context(|| format!("First Playable {label} is outside u32 range"))
}

fn reward_stack(
    catalog: &PrototypeEquipmentCatalog,
    index: usize,
    grant: &PrototypeRewardGrant,
) -> Result<InventoryStack> {
    let ordinal = u64::try_from(index)
        .context("M02 reward stack index exceeds u64")?
        .checked_add(50_001)
        .context("M02 reward item identity overflow")?;
    let instance_id = ItemInstanceId::new(ordinal)?;
    if grant.item_id == FIRST_PLAYABLE_RED_TONIC_ID {
        let quantity = u8::try_from(grant.quantity).context("Red Tonic reward exceeds u8")?;
        return InventoryStack::red_tonic(instance_id, quantity).map_err(Into::into);
    }
    let definition = catalog
        .get(&grant.item_id)
        .with_context(|| format!("M02 reward references unknown item {}", grant.item_id))?;
    let slot = match definition.slot {
        EquipmentSlot::Weapon => SimulationEquipmentSlot::Weapon,
        EquipmentSlot::Relic => SimulationEquipmentSlot::Relic,
        EquipmentSlot::Armor => SimulationEquipmentSlot::Armor,
        EquipmentSlot::Charm => SimulationEquipmentSlot::Charm,
    };
    Ok(InventoryStack::Equipment(EquipmentItem::new(
        instance_id,
        ItemContentId::new(grant.item_id.clone())?,
        slot,
    )))
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
        bosses: read_json(&root.join("fp/bosses.json"))?,
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
    validate_first_playable_arena(package)?;
    validate_first_playable_weapon(package)?;
    validate_first_playable_grave_mark(package)?;
    validate_first_playable_slipstep(package)?;
    validate_first_playable_stillness(package)?;
    validate_first_playable_red_tonic(package)?;
    first_playable_red_tonic_with_undertaker_knot(package)?;
    first_playable_equipment_catalog(package)?;
    first_playable_reward_catalog(package)?;
    validate_first_playable_enemies(package)?;
    first_playable_bell_proctor(package)?;
    validate_first_playable_damage_bands(package)?;
    Ok(())
}

/// Returns the exact validated First Playable arena in simulation-owned units.
pub fn first_playable_arena(package: &ContentPackage) -> Result<ArenaGeometry> {
    let record = package
        .arenas
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_ARENA_ID)
        .context("First Playable arena record is missing")?;
    compile_arena_geometry(record)
}

/// Returns the exact validated Pine Crossbow used by the First Playable Grave Arbalist.
pub fn first_playable_weapon(package: &ContentPackage) -> Result<WeaponDefinition> {
    let class = package
        .classes
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_CLASS_ID)
        .context("First Playable Grave Arbalist class record is missing")?;
    if class.numeric_payload.weapon_family != "crossbow"
        || class.numeric_payload.primary_ability_id.as_str() != FIRST_PLAYABLE_PRIMARY_ID
    {
        bail!("First Playable class does not equip the required crossbow primary grammar");
    }
    let primary = package
        .abilities
        .iter()
        .find(|record| record.header.id == class.numeric_payload.primary_ability_id)
        .context("First Playable primary ability record is missing")?;
    let item = package
        .items
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_WEAPON_ID)
        .context("First Playable Pine Crossbow item record is missing")?;
    compile_primary_weapon(item, primary)
}

/// Returns exact resolved Grave Mark values, including shared `CONT-013` defaults.
pub fn first_playable_grave_mark(package: &ContentPackage) -> Result<GraveMarkDefinition> {
    let class = package
        .classes
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_CLASS_ID)
        .context("First Playable Grave Arbalist class record is missing")?;
    let references = class
        .numeric_payload
        .active_ability_ids
        .iter()
        .filter(|id| id.as_str() == FIRST_PLAYABLE_GRAVE_MARK_ID)
        .count();
    if references != 1 {
        bail!("First Playable class must reference Grave Mark exactly once, found {references}");
    }
    let record = package
        .abilities
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_GRAVE_MARK_ID)
        .context("First Playable Grave Mark ability record is missing")?;
    compile_grave_mark(record)
}

/// Returns the exact validated First Playable Slipstep definition.
pub fn first_playable_slipstep(package: &ContentPackage) -> Result<SlipstepDefinition> {
    let class = package
        .classes
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_CLASS_ID)
        .context("First Playable Grave Arbalist class record is missing")?;
    let references = class
        .numeric_payload
        .active_ability_ids
        .iter()
        .filter(|id| id.as_str() == FIRST_PLAYABLE_SLIPSTEP_ID)
        .count();
    if references != 1 {
        bail!("First Playable class must reference Slipstep exactly once, found {references}");
    }
    let record = package
        .abilities
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_SLIPSTEP_ID)
        .context("First Playable Slipstep ability record is missing")?;
    compile_slipstep(record)
}

/// Compiles authored Slipstep values with shared ability timing.
pub fn compile_slipstep(record: &AbilityRecord) -> Result<SlipstepDefinition> {
    let AbilityPayload::Slipstep {
        cooldown_ms,
        travel_milli_tiles,
        travel_ms,
        direct_damage_reduction_basis_points,
        empowered_window_ms,
        projectile_speed_bonus_basis_points,
        pierce_bonus,
        exhaustion_ms,
    } = &record.numeric_payload
    else {
        bail!("{} is not Slipstep", record.header.id);
    };
    let compile_ticks = |milliseconds: u64, field: &str| {
        u32::try_from(duration_ms_to_ticks_nearest(milliseconds))
            .with_context(|| format!("compiled Slipstep {field} exceeds u32 ticks"))
    };
    SlipstepDefinition::new(SlipstepDefinitionParameters {
        content_id: record.header.id.to_string(),
        cooldown_ticks: compile_ticks(u64::from(*cooldown_ms), "cooldown")?,
        global_cooldown_ticks: compile_ticks(GLOBAL_ABILITY_COOLDOWN_MS, "global cooldown")?,
        input_buffer_ticks: compile_ticks(ABILITY_INPUT_BUFFER_MS, "input buffer")?,
        travel_milli_tiles: *travel_milli_tiles,
        travel_ticks: compile_ticks(u64::from(*travel_ms), "travel")?,
        direct_damage_reduction_basis_points: *direct_damage_reduction_basis_points,
        empowered_window_ticks: compile_ticks(u64::from(*empowered_window_ms), "empowered window")?,
        projectile_speed_bonus_basis_points: *projectile_speed_bonus_basis_points,
        pierce_bonus: *pierce_bonus,
        exhaustion_ticks: compile_ticks(u64::from(*exhaustion_ms), "Exhaustion")?,
    })
    .with_context(|| format!("{} failed simulation ability validation", record.header.id))
}

pub fn first_playable_stillness(package: &ContentPackage) -> Result<StillnessDefinition> {
    let class = package
        .classes
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_CLASS_ID)
        .context("First Playable Grave Arbalist class record is missing")?;
    if class.numeric_payload.passive_ability_id.as_str() != FIRST_PLAYABLE_STILLNESS_ID {
        bail!("First Playable class must reference Stillness as its passive");
    }
    let record = package
        .abilities
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_STILLNESS_ID)
        .context("First Playable Stillness ability record is missing")?;
    compile_stillness(record)
}

pub fn compile_stillness(record: &AbilityRecord) -> Result<StillnessDefinition> {
    let AbilityPayload::Stillness {
        activation_ms,
        movement_threshold_basis_points,
        projectile_speed_bonus_basis_points,
        primary_damage_bonus_basis_points,
        break_on_damage,
        break_on_slipstep,
    } = &record.numeric_payload
    else {
        bail!("{} is not Stillness", record.header.id);
    };
    StillnessDefinition::new(StillnessDefinitionParameters {
        content_id: record.header.id.to_string(),
        activation_ticks: u32::try_from(duration_ms_to_ticks_nearest(u64::from(*activation_ms)))
            .context("compiled Stillness activation exceeds u32 ticks")?,
        movement_threshold_basis_points: *movement_threshold_basis_points,
        projectile_speed_bonus_basis_points: *projectile_speed_bonus_basis_points,
        primary_damage_bonus_basis_points: *primary_damage_bonus_basis_points,
        break_on_damage: *break_on_damage,
        break_on_slipstep: *break_on_slipstep,
    })
    .with_context(|| format!("{} failed simulation ability validation", record.header.id))
}

/// Returns the exact validated Red Tonic used by the First Playable belt.
pub fn first_playable_red_tonic(package: &ContentPackage) -> Result<RedTonicDefinition> {
    let record = package
        .items
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_RED_TONIC_ID)
        .context("First Playable Red Tonic item record is missing")?;
    compile_red_tonic(record)
}

/// Resolves the exact Undertaker Knot override without weakening the base Red Tonic contract.
pub fn first_playable_red_tonic_with_undertaker_knot(
    package: &ContentPackage,
) -> Result<RedTonicDefinition> {
    validate_first_playable_red_tonic(package)?;
    let manifest_count = package
        .release_manifest
        .required_content_ids
        .iter()
        .filter(|id| id.as_str() == FIRST_PLAYABLE_UNDERTAKER_KNOT_ID)
        .count();
    if manifest_count != 1 {
        bail!(
            "fp.1.0.0 manifest must reference {FIRST_PLAYABLE_UNDERTAKER_KNOT_ID} exactly once, found {manifest_count}"
        );
    }
    let mut records = package
        .items
        .iter()
        .filter(|record| record.header.id.as_str() == FIRST_PLAYABLE_UNDERTAKER_KNOT_ID);
    let record = records
        .next()
        .context("First Playable Undertaker Knot item record is missing")?;
    if records.next().is_some() {
        bail!("First Playable Undertaker Knot item record is duplicated");
    }
    let ItemPayload::Equipment {
        slot,
        rarity,
        effects,
    } = &record.numeric_payload
    else {
        bail!("{FIRST_PLAYABLE_UNDERTAKER_KNOT_ID} is not equipment");
    };
    if *slot != EquipmentSlot::Charm || *rarity != ItemRarity::Oathed || effects.len() != 2 {
        bail!(
            "{FIRST_PLAYABLE_UNDERTAKER_KNOT_ID} must be an Oathed Charm with exactly two effects"
        );
    }
    let restore = exact_item_set_effect(effects, "red_tonic_restore_basis_points")?;
    let cooldown = exact_item_set_effect(effects, "shared_potion_cooldown_ms")?;
    if restore != 3_500 || cooldown != 2_500 {
        bail!("{FIRST_PLAYABLE_UNDERTAKER_KNOT_ID} differs from exact CONT-FP-006 values");
    }
    let resolved = RedTonicDefinition::with_undertaker_knot()
        .context("simulation rejected exact Undertaker Knot override")?;
    if resolved.restore_max_health_basis_points() != restore
        || resolved.shared_cooldown_ticks()
            != u32::try_from(duration_ms_to_ticks_nearest(u64::from(cooldown)))
                .context("Undertaker Knot cooldown exceeds u32 ticks")?
    {
        bail!("Undertaker Knot compiled values disagree with simulation factory");
    }
    Ok(resolved)
}

fn exact_item_set_effect(effects: &[ItemEffect], stat: &str) -> Result<u32> {
    let mut matches = effects.iter().filter(|effect| effect.stat == stat);
    let effect = matches
        .next()
        .with_context(|| format!("Undertaker Knot is missing required `{stat}` effect"))?;
    if matches.next().is_some() {
        bail!("Undertaker Knot contains duplicate `{stat}` effects");
    }
    if effect.operation != EffectOperation::Set {
        bail!("Undertaker Knot `{stat}` effect must use the `set` operation");
    }
    u32::try_from(effect.value)
        .with_context(|| format!("Undertaker Knot `{stat}` must be nonnegative"))
}

/// Compiles authored Red Tonic content into deterministic simulation ticks.
pub fn compile_red_tonic(record: &ItemRecord) -> Result<RedTonicDefinition> {
    let ItemPayload::Consumable {
        belt_stack_cap,
        restore_max_health_basis_points,
        restore_duration_ms,
        shared_cooldown_ms,
        damage_interrupts_restore,
        consumed_on_use,
    } = &record.numeric_payload
    else {
        bail!("{} is not a consumable", record.header.id);
    };
    let compile_ticks = |milliseconds: u32, field: &str| {
        u32::try_from(duration_ms_to_ticks_nearest(u64::from(milliseconds)))
            .with_context(|| format!("compiled Red Tonic {field} exceeds u32 ticks"))
    };
    RedTonicDefinition::new(RedTonicDefinitionParameters {
        content_id: record.header.id.to_string(),
        belt_stack_cap: u8::try_from(*belt_stack_cap)
            .context("Red Tonic belt stack cap exceeds u8")?,
        restore_max_health_basis_points: *restore_max_health_basis_points,
        restore_duration_ticks: compile_ticks(*restore_duration_ms, "restore duration")?,
        shared_cooldown_ticks: compile_ticks(*shared_cooldown_ms, "shared cooldown")?,
        damage_interrupts_restore: *damage_interrupts_restore,
        consumed_on_use: *consumed_on_use,
    })
    .with_context(|| {
        format!(
            "{} failed simulation consumable validation",
            record.header.id
        )
    })
}

/// Compiles the exact `fp.1.0.0` Drowned Pilgrim override and aimed-fan pattern.
pub fn first_playable_drowned_pilgrim(
    package: &ContentPackage,
) -> Result<DrownedPilgrimDefinition> {
    let (enemy, pattern) = first_playable_enemy_pair(
        package,
        DROWNED_PILGRIM_ID,
        FIRST_PLAYABLE_DROWNED_PILGRIM_PATTERN_ID,
    )?;
    require_exact_drowned_pilgrim(enemy, pattern)?;
    DrownedPilgrimDefinition::new(DrownedPilgrimDefinitionParameters {
        content_id: enemy.header.id.to_string(),
        role: EnemyRole::Fodder,
        health: enemy.numeric_payload.health,
        armor: enemy.numeric_payload.armor,
        hurtbox_radius_milli_tiles: enemy.numeric_payload.hurtbox_radius_milli_tiles,
        movement_speed_milli_tiles_per_second: enemy
            .numeric_payload
            .movement_speed_milli_tiles_per_second,
        aggro_radius_milli_tiles: enemy.numeric_payload.aggro_radius_milli_tiles,
        leash_radius_milli_tiles: enemy.numeric_payload.leash_radius_milli_tiles,
        spawn_telegraph_ticks: compile_hostile_telegraph(900, "Pilgrim spawn")?,
        approach_distance_milli_tiles: 5_000,
        windup_ticks: compile_hostile_telegraph(300, "Pilgrim fan")?,
        recover_ticks: compile_ordinary_duration(1_900, "Pilgrim recover")?,
        fan_offsets_degrees: [-15, 0, 15],
        origin_offset_milli_tiles: 450,
        attack: ProjectileAttackDefinition {
            pattern_id: FIRST_PLAYABLE_DROWNED_PILGRIM_PATTERN_ID,
            projectile_count: 3,
            speed_milli_tiles_per_second: 5_500,
            radius_milli_tiles: 120,
            lifetime_ticks: compile_ordinary_duration(2_200, "Pilgrim projectile")?,
            raw_damage: 8,
            damage_type: DamageType::Physical,
            damage_band: DamageBand::Chip,
            threat_cost: 3,
            memory_family: EchoMemoryFamily::FanProjectile,
            counterplay: Counterplay::Strafe,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players: false,
            maximum_active_instances: 6,
        },
        reward_table_id: NORMAL_ENEMY_REWARD_TABLE_ID.to_owned(),
    })
    .context("Drowned Pilgrim failed exact simulation-definition validation")
}

/// Compiles the exact `fp.1.0.0` Bell Reed override and gap-ring pattern.
pub fn first_playable_bell_reed(package: &ContentPackage) -> Result<BellReedDefinition> {
    let (enemy, pattern) =
        first_playable_enemy_pair(package, BELL_REED_ID, FIRST_PLAYABLE_BELL_REED_PATTERN_ID)?;
    require_exact_bell_reed(enemy, pattern)?;
    BellReedDefinition::new(BellReedDefinitionParameters {
        content_id: enemy.header.id.to_string(),
        role: EnemyRole::Pressure,
        health: enemy.numeric_payload.health,
        armor: enemy.numeric_payload.armor,
        hurtbox_radius_milli_tiles: enemy.numeric_payload.hurtbox_radius_milli_tiles,
        movement_speed_milli_tiles_per_second: 0,
        aggro_radius_milli_tiles: enemy.numeric_payload.aggro_radius_milli_tiles,
        leash_radius_milli_tiles: enemy.numeric_payload.leash_radius_milli_tiles,
        spawn_telegraph_ticks: compile_hostile_telegraph(900, "Reed spawn")?,
        dormant_ticks: compile_ordinary_duration(500, "Reed dormant")?,
        cycle_ticks: compile_ordinary_duration(3_000, "Reed cycle")?,
        first_telegraph_ticks: compile_hostile_telegraph(450, "Reed first telegraph")?,
        repeated_telegraph_ticks: compile_hostile_telegraph(300, "Reed repeated telegraph")?,
        ring_index_count: 8,
        omitted_count: 2,
        omitted_start_advance: 3,
        attack: ProjectileAttackDefinition {
            pattern_id: FIRST_PLAYABLE_BELL_REED_PATTERN_ID,
            projectile_count: 6,
            speed_milli_tiles_per_second: 4_500,
            radius_milli_tiles: 130,
            lifetime_ticks: compile_ordinary_duration(3_000, "Reed projectile")?,
            raw_damage: 10,
            damage_type: DamageType::Veil,
            damage_band: DamageBand::Chip,
            threat_cost: 6,
            memory_family: EchoMemoryFamily::RadialProjectile,
            counterplay: Counterplay::FollowGap,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players: false,
            maximum_active_instances: 12,
        },
        reward_table_id: NORMAL_ENEMY_REWARD_TABLE_ID.to_owned(),
    })
    .context("Bell Reed failed exact simulation-definition validation")
}

/// Compiles the exact `fp.1.0.0` Chain Sentry override and cross-lane pattern.
pub fn first_playable_chain_sentry(package: &ContentPackage) -> Result<ChainSentryDefinition> {
    let (enemy, pattern) = first_playable_enemy_pair(
        package,
        CHAIN_SENTRY_ID,
        FIRST_PLAYABLE_CHAIN_SENTRY_PATTERN_ID,
    )?;
    require_exact_chain_sentry(enemy, pattern)?;
    ChainSentryDefinition::new(ChainSentryDefinitionParameters {
        content_id: enemy.header.id.to_string(),
        role: EnemyRole::Anchor,
        health: enemy.numeric_payload.health,
        armor: enemy.numeric_payload.armor,
        hurtbox_radius_milli_tiles: enemy.numeric_payload.hurtbox_radius_milli_tiles,
        movement_speed_milli_tiles_per_second: 0,
        aggro_radius_milli_tiles: enemy.numeric_payload.aggro_radius_milli_tiles,
        leash_radius_milli_tiles: enemy.numeric_payload.leash_radius_milli_tiles,
        spawn_telegraph_ticks: compile_hostile_telegraph(900, "Sentry spawn")?,
        dormant_ticks: compile_ordinary_duration(700, "Sentry dormant")?,
        cycle_ticks: compile_ordinary_duration(4_500, "Sentry cycle")?,
        first_telegraph_ticks: compile_hostile_telegraph(800, "Sentry first telegraph")?,
        repeated_telegraph_ticks: compile_hostile_telegraph(650, "Sentry repeated telegraph")?,
        attack: LaneAttackDefinition {
            pattern_id: FIRST_PLAYABLE_CHAIN_SENTRY_PATTERN_ID,
            lane_count: 2,
            width_milli_tiles: 900,
            active_ticks: compile_ordinary_duration(350, "Sentry active")?,
            raw_damage: 22,
            damage_type: DamageType::Physical,
            damage_band: DamageBand::Pressure,
            threat_cost_per_lane: 12,
            memory_family: EchoMemoryFamily::LaneOrBeam,
            counterplay: Counterplay::LeaveTelegraph,
            disposition: HostileDisposition::ExpireAtAuthoredEnd,
            maximum_active_instances: 2,
        },
        reward_table_id: NORMAL_ENEMY_REWARD_TABLE_ID.to_owned(),
    })
    .context("Chain Sentry failed exact simulation-definition validation")
}

fn validate_first_playable_enemies(package: &ContentPackage) -> Result<()> {
    first_playable_drowned_pilgrim(package)?;
    first_playable_bell_reed(package)?;
    first_playable_chain_sentry(package)?;
    Ok(())
}

/// Compiles the checked-in Bell Proctor record and its three shared attacks losslessly.
#[allow(clippy::too_many_lines)] // Explicit field mapping prevents hidden defaults in the strict content boundary.
pub fn first_playable_bell_proctor(package: &ContentPackage) -> Result<BellProctorDefinition> {
    let mut bosses = package
        .bosses
        .iter()
        .filter(|record| record.header.id.as_str() == BELL_PROCTOR_ID);
    let boss = bosses
        .next()
        .context("First Playable Bell Proctor is missing")?;
    if bosses.next().is_some() {
        bail!("First Playable Bell Proctor is duplicated");
    }
    let fan = required_pattern(package, BELL_PROCTOR_FAN_ID)?;
    let ring = required_pattern(package, BELL_PROCTOR_RING_ID)?;
    let cross = required_pattern(package, BELL_PROCTOR_CROSS_ID)?;
    require_exact_bell_proctor_records(boss, fan, ring, cross)?;

    let payload = &boss.numeric_payload;
    let timeline = |phase| {
        let matches: Vec<_> = payload
            .phase_timelines
            .iter()
            .filter(|candidate| candidate.phase == phase)
            .collect();
        if matches.len() != 1 {
            bail!("Bell Proctor must contain exactly one {phase:?} timeline");
        }
        matches[0]
            .cues
            .iter()
            .map(|cue| {
                Ok(BossTimelineCue {
                    kind: match cue.kind {
                        ContentBossCueKind::Fan => BossCueKind::Fan,
                        ContentBossCueKind::Ring => BossCueKind::Ring,
                        ContentBossCueKind::RingPreviewA => BossCueKind::RingPreviewA,
                        ContentBossCueKind::RingPreviewB => BossCueKind::RingPreviewB,
                        ContentBossCueKind::Cross => BossCueKind::Cross,
                    },
                    starts_at_offset_ticks: compile_ordinary_duration(
                        cue.starts_at_ms,
                        "Bell cue start",
                    )?,
                    resolves_at_offset_ticks: compile_hostile_telegraph(
                        cue.resolves_at_ms,
                        "Bell cue resolution",
                    )?,
                })
            })
            .collect::<Result<Vec<_>>>()
    };
    let loop_ms = |phase| {
        payload
            .phase_timelines
            .iter()
            .find(|candidate| candidate.phase == phase)
            .map(|candidate| candidate.loop_ms)
            .with_context(|| format!("Bell Proctor is missing {phase:?}"))
    };
    let threshold_health = |basis_points: u32| {
        payload
            .health
            .checked_mul(basis_points)
            .context("Bell Proctor health threshold overflow")
            .map(|scaled| scaled / 10_000)
    };
    let offsets: [i16; 5] = payload
        .fan_offsets_degrees
        .clone()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Bell Proctor fan must contain five offsets"))?;

    BellProctorDefinition::new(BellProctorDefinitionParameters {
        content_id: boss.header.id.to_string(),
        health: payload.health,
        armor: payload.armor,
        hurtbox_radius_milli_tiles: payload.hurtbox_radius_milli_tiles,
        position_x_milli_tiles: payload.position.x_milli_tiles,
        position_y_milli_tiles: payload.position.y_milli_tiles,
        target_solo_duration_min_ticks: compile_ordinary_duration(
            payload.target_solo_duration_min_ms,
            "Bell target minimum",
        )?,
        target_solo_duration_max_ticks: compile_ordinary_duration(
            payload.target_solo_duration_max_ms,
            "Bell target maximum",
        )?,
        soft_enrage_ticks: compile_ordinary_duration(payload.soft_enrage_ms, "Bell soft enrage")?,
        introduction_ticks: compile_ordinary_duration(
            payload.introduction_ms,
            "Bell introduction",
        )?,
        break_ticks: compile_ordinary_duration(payload.phase_break_ms, "Bell phase break")?,
        break_received_damage_multiplier_basis_points: payload
            .phase_break_received_damage_multiplier_basis_points,
        soft_enrage_downtime_multiplier_basis_points: payload
            .soft_enrage_downtime_multiplier_basis_points,
        phase1_loop_ticks: compile_ordinary_duration(
            loop_ms(ContentBossPhase::Phase1)?,
            "Bell phase 1 loop",
        )?,
        phase2_loop_ticks: compile_ordinary_duration(
            loop_ms(ContentBossPhase::Phase2)?,
            "Bell phase 2 loop",
        )?,
        phase3_loop_ticks: compile_ordinary_duration(
            loop_ms(ContentBossPhase::Phase3)?,
            "Bell phase 3 loop",
        )?,
        phase3_low_health_loop_ticks: compile_ordinary_duration(
            payload.phase_three_low_health_loop_ms,
            "Bell low-health loop",
        )?,
        phase_two_health: threshold_health(payload.phase_two_health_threshold_basis_points)?,
        phase_three_health: threshold_health(payload.phase_three_health_threshold_basis_points)?,
        low_health_restart: threshold_health(payload.low_health_restart_basis_points)?,
        fan_offsets_degrees: offsets,
        ring_index_count: u8::try_from(payload.ring_index_count)
            .context("Bell ring index count exceeds u8")?,
        ring_omitted_count: u8::try_from(payload.ring_omitted_count)
            .context("Bell ring omitted count exceeds u8")?,
        ring_gap_advance: u8::try_from(payload.ring_gap_advance)
            .context("Bell ring gap advance exceeds u8")?,
        phase3_second_gap_advance: u8::try_from(payload.phase_three_second_gap_advance)
            .context("Bell second gap advance exceeds u8")?,
        ring_preview_ticks: compile_hostile_telegraph(
            payload.ring_preview_ms,
            "Bell ring preview",
        )?,
        fan: compile_boss_projectile(fan)?,
        ring: compile_boss_projectile(ring)?,
        cross: compile_boss_cross(cross)?,
        phase1_timeline: timeline(ContentBossPhase::Phase1)?,
        phase2_timeline: timeline(ContentBossPhase::Phase2)?,
        phase3_timeline: timeline(ContentBossPhase::Phase3)?,
        reward_table_id: payload.reward_table_id.to_string(),
    })
    .context("Bell Proctor content differs from exact CONT-FP-005 simulation contract")
}

fn required_pattern<'a>(package: &'a ContentPackage, id: &str) -> Result<&'a PatternRecord> {
    let matches: Vec<_> = package
        .patterns
        .iter()
        .filter(|record| record.header.id.as_str() == id)
        .collect();
    match matches.as_slice() {
        [pattern] => Ok(*pattern),
        [] => bail!("required Bell Proctor pattern {id} is missing"),
        _ => bail!("required Bell Proctor pattern {id} is duplicated"),
    }
}

fn compile_boss_projectile(pattern: &PatternRecord) -> Result<ProjectileAttackDefinition> {
    let payload = &pattern.numeric_payload;
    let pattern_id = match pattern.header.id.as_str() {
        BELL_PROCTOR_FAN_ID => BELL_PROCTOR_FAN_ID,
        BELL_PROCTOR_RING_ID => BELL_PROCTOR_RING_ID,
        other => bail!("unsupported boss projectile pattern {other}"),
    };
    Ok(ProjectileAttackDefinition {
        pattern_id,
        projectile_count: u8::try_from(payload.projectile_count)
            .context("boss projectile count exceeds u8")?,
        speed_milli_tiles_per_second: payload
            .projectile_speed_milli_tiles_per_second
            .context("boss projectile speed is missing")?,
        radius_milli_tiles: payload
            .projectile_radius_milli_tiles
            .context("boss projectile radius is missing")?,
        lifetime_ticks: compile_ordinary_duration(
            payload
                .projectile_lifetime_ms
                .context("boss projectile lifetime is missing")?,
            "boss projectile lifetime",
        )?,
        raw_damage: payload.raw_damage,
        damage_type: compile_damage_type(payload.damage_type),
        damage_band: compile_damage_band(payload.damage_band),
        threat_cost: payload.threat_cost,
        memory_family: compile_memory_family(&payload.echo_memory_family)?,
        counterplay: compile_counterplay(&payload.counterplay)?,
        disposition: compile_disposition(&payload.projectile_disposition)?,
        pierces_players: false,
        maximum_active_instances: payload.maximum_active_instances,
    })
}

fn compile_boss_cross(pattern: &PatternRecord) -> Result<LaneAttackDefinition> {
    let payload = &pattern.numeric_payload;
    Ok(LaneAttackDefinition {
        pattern_id: BELL_PROCTOR_CROSS_ID,
        lane_count: u8::try_from(payload.projectile_count).context("boss lane count exceeds u8")?,
        width_milli_tiles: payload
            .lane_width_milli_tiles
            .context("boss lane width is missing")?,
        active_ticks: compile_ordinary_duration(
            payload
                .active_ms
                .context("boss lane active time is missing")?,
            "boss lane active",
        )?,
        raw_damage: payload.raw_damage,
        damage_type: compile_damage_type(payload.damage_type),
        damage_band: compile_damage_band(payload.damage_band),
        threat_cost_per_lane: payload.threat_cost,
        memory_family: compile_memory_family(&payload.echo_memory_family)?,
        counterplay: compile_counterplay(&payload.counterplay)?,
        disposition: compile_disposition(&payload.projectile_disposition)?,
        maximum_active_instances: payload.maximum_active_instances,
    })
}

const fn compile_damage_type(value: ContentDamageType) -> DamageType {
    match value {
        ContentDamageType::Physical => DamageType::Physical,
        ContentDamageType::Veil => DamageType::Veil,
    }
}

const fn compile_damage_band(value: ContentDamageBand) -> DamageBand {
    match value {
        ContentDamageBand::Chip => DamageBand::Chip,
        ContentDamageBand::Pressure => DamageBand::Pressure,
        ContentDamageBand::Major => DamageBand::Major,
    }
}

fn compile_memory_family(value: &str) -> Result<EchoMemoryFamily> {
    match value {
        "fan_projectile" => Ok(EchoMemoryFamily::FanProjectile),
        "radial_projectile" => Ok(EchoMemoryFamily::RadialProjectile),
        "lane_or_beam" => Ok(EchoMemoryFamily::LaneOrBeam),
        _ => bail!("unsupported boss memory family {value}"),
    }
}

fn compile_counterplay(value: &str) -> Result<Counterplay> {
    match value {
        "strafe" => Ok(Counterplay::Strafe),
        "follow_gap" => Ok(Counterplay::FollowGap),
        "leave_telegraph" => Ok(Counterplay::LeaveTelegraph),
        _ => bail!("unsupported boss counterplay {value}"),
    }
}

fn compile_disposition(value: &str) -> Result<HostileDisposition> {
    match value {
        "consume_on_player_or_solid" => Ok(HostileDisposition::ConsumeOnPlayerOrSolid),
        "expire_at_authored_end" => Ok(HostileDisposition::ExpireAtAuthoredEnd),
        _ => bail!("unsupported boss disposition {value}"),
    }
}

fn require_exact_bell_proctor_records(
    boss: &BossRecord,
    fan: &PatternRecord,
    ring: &PatternRecord,
    cross: &PatternRecord,
) -> Result<()> {
    let payload = &boss.numeric_payload;
    if [
        payload.health,
        payload.armor,
        payload.hurtbox_radius_milli_tiles,
    ] != [3_000, 4, 650]
        || [
            payload.position.x_milli_tiles,
            payload.position.y_milli_tiles,
        ] != [24_000, 12_000]
        || [
            payload.target_solo_duration_min_ms,
            payload.target_solo_duration_max_ms,
            payload.soft_enrage_ms,
            payload.introduction_ms,
            payload.phase_break_ms,
            payload.phase_three_low_health_loop_ms,
            payload.ring_preview_ms,
        ] != [75_000, 110_000, 180_000, 2_000, 3_000, 9_000, 500]
        || [
            payload.phase_break_received_damage_multiplier_basis_points,
            payload.soft_enrage_downtime_multiplier_basis_points,
            payload.phase_two_health_threshold_basis_points,
            payload.phase_three_health_threshold_basis_points,
            payload.low_health_restart_basis_points,
        ] != [12_000, 8_500, 7_000, 3_500, 2_000]
        || [
            payload.ring_index_count,
            payload.ring_omitted_count,
            payload.ring_gap_advance,
            payload.phase_three_second_gap_advance,
        ] != [16, 4, 5, 4]
        || payload.fan_offsets_degrees != [-20, -10, 0, 10, 20]
        || payload.movement_mode != BossMovementMode::Fixed
        || payload.summons_enabled
        || !payload.status_effect_ids.is_empty()
        || payload.cross_axis_sets_degrees != [[0, 90], [45, 135]]
        || payload.fan_pattern_id.as_str() != BELL_PROCTOR_FAN_ID
        || payload.ring_pattern_id.as_str() != BELL_PROCTOR_RING_ID
        || payload.cross_pattern_id.as_str() != BELL_PROCTOR_CROSS_ID
        || payload.reward_table_id.as_str() != BELL_PROCTOR_REWARD_ID
        || payload.phase_timelines.len() != 3
    {
        bail!("{BELL_PROCTOR_ID} differs from exact CONT-FP-005 ownership tuple");
    }
    require_exact_boss_timeline(payload)?;
    require_projectile_pattern(
        fan,
        PatternKind::AimedFan,
        [7_200, 400, 400],
        [5, 6_000, 120, 3_000],
        [12, 5, 10],
        ContentDamageType::Veil,
        ContentDamageBand::Chip,
        "fan_projectile",
        "strafe",
    )?;
    require_projectile_pattern(
        ring,
        PatternKind::GapRing,
        [10_000, 650, 650],
        [12, 4_500, 130, 4_000],
        [15, 12, 24],
        ContentDamageType::Veil,
        ContentDamageBand::Pressure,
        "radial_projectile",
        "follow_gap",
    )?;
    let p = &cross.numeric_payload;
    if p.pattern_kind != PatternKind::CrossLanes
        || [p.cycle_ms, p.first_telegraph_ms, p.repeated_telegraph_ms] != [10_000, 900, 900]
        || p.projectile_count != 2
        || p.lane_width_milli_tiles != Some(1_000)
        || p.active_ms != Some(500)
        || [p.raw_damage, p.threat_cost, p.maximum_active_instances] != [28, 12, 2]
        || p.damage_type != ContentDamageType::Physical
        || p.damage_band != ContentDamageBand::Major
        || p.echo_memory_family != "lane_or_beam"
        || p.counterplay != "leave_telegraph"
        || p.projectile_disposition != "expire_at_authored_end"
    {
        bail!("{BELL_PROCTOR_CROSS_ID} differs from exact CONT-FP-005 pattern tuple");
    }
    Ok(())
}

fn require_exact_boss_timeline(payload: &content_schema::BossPayload) -> Result<()> {
    let expected = [
        (
            ContentBossPhase::Phase1,
            7_200,
            vec![
                (ContentBossCueKind::Fan, 0, 400),
                (ContentBossCueKind::Fan, 2_400, 2_800),
                (ContentBossCueKind::Ring, 5_600, 6_250),
            ],
        ),
        (
            ContentBossPhase::Phase2,
            10_000,
            vec![
                (ContentBossCueKind::Fan, 0, 400),
                (ContentBossCueKind::Fan, 2_400, 2_800),
                (ContentBossCueKind::Ring, 4_200, 4_850),
                (ContentBossCueKind::Cross, 7_000, 7_900),
            ],
        ),
        (
            ContentBossPhase::Phase3,
            10_000,
            vec![
                (ContentBossCueKind::RingPreviewA, 0, 900),
                (ContentBossCueKind::RingPreviewB, 1_000, 1_800),
                (ContentBossCueKind::Fan, 4_000, 4_400),
                (ContentBossCueKind::Cross, 6_500, 7_400),
                (ContentBossCueKind::Fan, 8_400, 8_800),
            ],
        ),
    ];
    for (phase, loop_ms, cues) in expected {
        let Some(actual) = payload
            .phase_timelines
            .iter()
            .find(|candidate| candidate.phase == phase)
        else {
            bail!("Bell Proctor is missing {phase:?}");
        };
        let actual_cues: Vec<_> = actual
            .cues
            .iter()
            .map(|cue| (cue.kind, cue.starts_at_ms, cue.resolves_at_ms))
            .collect();
        if actual.loop_ms != loop_ms || actual_cues != cues {
            bail!("Bell Proctor {phase:?} differs from exact CONT-FP-005 timeline");
        }
    }
    Ok(())
}

fn validate_first_playable_damage_bands(package: &ContentPackage) -> Result<()> {
    let pilgrim = first_playable_drowned_pilgrim(package)?;
    let reed = first_playable_bell_reed(package)?;
    let sentry = first_playable_chain_sentry(package)?;
    let boss = first_playable_bell_proctor(package)?;
    let attacks = [
        (
            pilgrim.parameters().attack.raw_damage,
            pilgrim.parameters().attack.damage_band,
        ),
        (
            reed.parameters().attack.raw_damage,
            reed.parameters().attack.damage_band,
        ),
        (
            sentry.parameters().attack.raw_damage,
            sentry.parameters().attack.damage_band,
        ),
        (
            boss.parameters().fan.raw_damage,
            boss.parameters().fan.damage_band,
        ),
        (
            boss.parameters().ring.raw_damage,
            boss.parameters().ring.damage_band,
        ),
        (
            boss.parameters().cross.raw_damage,
            boss.parameters().cross.damage_band,
        ),
    ];
    for (index, (raw_damage, declared_band)) in attacks.into_iter().enumerate() {
        let source = EntityId::new(u64::try_from(index + 1).expect("six attacks fit u64"))
            .expect("reference source is nonzero");
        let target = EntityId::new(100).expect("reference target is nonzero");
        let damage = resolve_direct_hit(&DirectHitRequest::new(DirectHitParameters {
            source,
            target,
            collision_confirmed: true,
            target_is_immune: false,
            raw_damage,
            damage_type: DamageType::Veil,
            attacker_multiplier_basis_points: 10_000,
            target_resistance_basis_points: 0,
            direct_damage_reductions_basis_points: Vec::new(),
            armor: 2,
            current_barrier: 0,
            health_damage_cap_basis_points: None,
            current_health: 128,
            max_health: 128,
        })?)?;
        validate_damage_band(declared_band, damage.health_damage_applied, 128, false)
            .with_context(|| format!("First Playable attack index {index} damage band mismatch"))?;
    }
    Ok(())
}

fn first_playable_enemy_pair<'a>(
    package: &'a ContentPackage,
    enemy_id: &str,
    pattern_id: &str,
) -> Result<(&'a EnemyRecord, &'a PatternRecord)> {
    for required in [enemy_id, pattern_id, NORMAL_ENEMY_REWARD_TABLE_ID] {
        let count = package
            .release_manifest
            .required_content_ids
            .iter()
            .filter(|candidate| candidate.as_str() == required)
            .count();
        if count != 1 {
            bail!("fp.1.0.0 manifest must reference {required} exactly once, found {count}");
        }
    }
    let mut enemies = package
        .enemies
        .iter()
        .filter(|record| record.header.id.as_str() == enemy_id);
    let enemy = enemies
        .next()
        .with_context(|| format!("First Playable enemy {enemy_id} is missing"))?;
    if enemies.next().is_some() {
        bail!("First Playable enemy {enemy_id} is duplicated");
    }
    if enemy.numeric_payload.pattern_ids.len() != 1
        || enemy.numeric_payload.pattern_ids[0].as_str() != pattern_id
    {
        bail!("{enemy_id} must reference exactly one pattern, {pattern_id}");
    }
    if enemy.numeric_payload.reward_table_id.as_str() != NORMAL_ENEMY_REWARD_TABLE_ID {
        bail!("{enemy_id} must reference {NORMAL_ENEMY_REWARD_TABLE_ID}");
    }
    let reward_count = package
        .drop_tables
        .iter()
        .filter(|record| record.header.id.as_str() == NORMAL_ENEMY_REWARD_TABLE_ID)
        .count();
    if reward_count != 1 {
        bail!("expected one {NORMAL_ENEMY_REWARD_TABLE_ID}, found {reward_count}");
    }
    let mut patterns = package
        .patterns
        .iter()
        .filter(|record| record.header.id.as_str() == pattern_id);
    let pattern = patterns
        .next()
        .with_context(|| format!("First Playable pattern {pattern_id} is missing"))?;
    if patterns.next().is_some() {
        bail!("First Playable pattern {pattern_id} is duplicated");
    }
    Ok((enemy, pattern))
}

fn require_state_machine(enemy: &EnemyRecord, expected: &[(&str, Option<u32>)]) -> Result<()> {
    if enemy.numeric_payload.state_machine.len() != expected.len()
        || enemy
            .numeric_payload
            .state_machine
            .iter()
            .zip(expected)
            .any(|(step, (state, duration))| step.state != *state || step.duration_ms != *duration)
    {
        bail!(
            "{} state machine differs from exact CONT-FP-004 order/durations",
            enemy.header.id
        );
    }
    Ok(())
}

fn require_exact_drowned_pilgrim(enemy: &EnemyRecord, pattern: &PatternRecord) -> Result<()> {
    require_state_machine(
        enemy,
        &[
            ("spawn_telegraph", Some(900)),
            ("acquire", None),
            ("approach_until_distance_5", None),
            ("attack_windup", Some(300)),
            ("fire_fan", None),
            ("recover", Some(1_900)),
        ],
    )?;
    if enemy.numeric_payload.role != ContentEnemyRole::Fodder
        || enemy.numeric_payload.health != 85
        || enemy.numeric_payload.armor != 0
        || enemy.numeric_payload.hurtbox_radius_milli_tiles != 340
        || enemy.numeric_payload.movement_speed_milli_tiles_per_second != 2_200
        || enemy.numeric_payload.aggro_radius_milli_tiles != 10_000
        || enemy.numeric_payload.leash_radius_milli_tiles != 12_000
        || enemy.numeric_payload.spawn_telegraph_ms != 900
    {
        bail!("{DROWNED_PILGRIM_ID} differs from exact CONT-FP-004 enemy tuple");
    }
    require_projectile_pattern(
        pattern,
        PatternKind::AimedFan,
        [2_200, 300, 300],
        [3, 5_500, 120, 2_200],
        [8, 3, 6],
        ContentDamageType::Physical,
        ContentDamageBand::Chip,
        "fan_projectile",
        "strafe",
    )
}

fn require_exact_bell_reed(enemy: &EnemyRecord, pattern: &PatternRecord) -> Result<()> {
    require_state_machine(
        enemy,
        &[
            ("spawn_telegraph", Some(900)),
            ("dormant", Some(500)),
            ("ring_telegraph", None),
            ("fire_gap_ring", None),
            ("recover_until_cycle", Some(3_000)),
        ],
    )?;
    if enemy.numeric_payload.role != ContentEnemyRole::Pressure
        || enemy.numeric_payload.health != 130
        || enemy.numeric_payload.armor != 2
        || enemy.numeric_payload.hurtbox_radius_milli_tiles != 420
        || enemy.numeric_payload.movement_speed_milli_tiles_per_second != 0
        || enemy.numeric_payload.aggro_radius_milli_tiles != 11_000
        || enemy.numeric_payload.leash_radius_milli_tiles != 12_000
        || enemy.numeric_payload.spawn_telegraph_ms != 900
    {
        bail!("{BELL_REED_ID} differs from exact CONT-FP-004 enemy tuple");
    }
    require_projectile_pattern(
        pattern,
        PatternKind::GapRing,
        [3_000, 450, 300],
        [6, 4_500, 130, 3_000],
        [10, 6, 12],
        ContentDamageType::Veil,
        ContentDamageBand::Chip,
        "radial_projectile",
        "follow_gap",
    )
}

fn require_exact_chain_sentry(enemy: &EnemyRecord, pattern: &PatternRecord) -> Result<()> {
    require_state_machine(
        enemy,
        &[
            ("spawn_telegraph", Some(900)),
            ("dormant", Some(700)),
            ("lane_telegraph", None),
            ("lane_impact", Some(350)),
            ("recover_until_cycle", Some(4_500)),
            ("toggle_orientation", None),
        ],
    )?;
    if enemy.numeric_payload.role != ContentEnemyRole::Anchor
        || enemy.numeric_payload.health != 300
        || enemy.numeric_payload.armor != 5
        || enemy.numeric_payload.hurtbox_radius_milli_tiles != 550
        || enemy.numeric_payload.movement_speed_milli_tiles_per_second != 0
        || enemy.numeric_payload.aggro_radius_milli_tiles != 13_000
        || enemy.numeric_payload.leash_radius_milli_tiles != 13_000
        || enemy.numeric_payload.spawn_telegraph_ms != 900
    {
        bail!("{CHAIN_SENTRY_ID} differs from exact CONT-FP-004 enemy tuple");
    }
    let payload = &pattern.numeric_payload;
    if payload.pattern_kind != PatternKind::CrossLanes
        || [
            payload.cycle_ms,
            payload.first_telegraph_ms,
            payload.repeated_telegraph_ms,
        ] != [4_500, 800, 650]
        || payload.projectile_count != 2
        || payload.projectile_speed_milli_tiles_per_second.is_some()
        || payload.projectile_radius_milli_tiles.is_some()
        || payload.projectile_lifetime_ms.is_some()
        || payload.lane_width_milli_tiles != Some(900)
        || payload.active_ms != Some(350)
        || [
            payload.raw_damage,
            payload.threat_cost,
            payload.maximum_active_instances,
        ] != [22, 12, 2]
        || payload.damage_type != ContentDamageType::Physical
        || payload.damage_band != ContentDamageBand::Pressure
        || payload.echo_memory_family != "lane_or_beam"
        || payload.counterplay != "leave_telegraph"
        || payload.projectile_disposition != "expire_at_authored_end"
        || payload.telegraph_id.as_str() != "pattern.enemy.chain_sentry.cross_lanes.telegraph"
        || payload.audio_cue_id.as_str() != "pattern.enemy.chain_sentry.cross_lanes.warning"
    {
        bail!(
            "{FIRST_PLAYABLE_CHAIN_SENTRY_PATTERN_ID} differs from exact CONT-FP-004 pattern tuple"
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn require_projectile_pattern(
    pattern: &PatternRecord,
    kind: PatternKind,
    timings: [u32; 3],
    projectile: [u32; 4],
    combat: [u32; 3],
    damage_type: ContentDamageType,
    damage_band: ContentDamageBand,
    memory: &str,
    counterplay: &str,
) -> Result<()> {
    let payload = &pattern.numeric_payload;
    let expected_telegraph = format!("{}.telegraph", pattern.header.id);
    let expected_audio = format!("{}.warning", pattern.header.id);
    if payload.pattern_kind != kind
        || [
            payload.cycle_ms,
            payload.first_telegraph_ms,
            payload.repeated_telegraph_ms,
        ] != timings
        || payload.projectile_count != projectile[0]
        || payload.projectile_speed_milli_tiles_per_second != Some(projectile[1])
        || payload.projectile_radius_milli_tiles != Some(projectile[2])
        || payload.projectile_lifetime_ms != Some(projectile[3])
        || payload.lane_width_milli_tiles.is_some()
        || payload.active_ms.is_some()
        || [
            payload.raw_damage,
            payload.threat_cost,
            payload.maximum_active_instances,
        ] != combat
        || payload.damage_type != damage_type
        || payload.damage_band != damage_band
        || payload.echo_memory_family != memory
        || payload.counterplay != counterplay
        || payload.projectile_disposition != "consume_on_player_or_solid"
        || payload.telegraph_id.as_str() != expected_telegraph
        || payload.audio_cue_id.as_str() != expected_audio
    {
        bail!(
            "{} differs from exact CONT-FP-004 pattern tuple",
            pattern.header.id
        );
    }
    Ok(())
}

fn compile_hostile_telegraph(milliseconds: u32, field: &str) -> Result<u32> {
    u32::try_from(duration_ms_to_ticks_ceil(u64::from(milliseconds)))
        .with_context(|| format!("compiled {field} exceeds u32 ticks"))
}

fn compile_ordinary_duration(milliseconds: u32, field: &str) -> Result<u32> {
    u32::try_from(duration_ms_to_ticks_nearest(u64::from(milliseconds)))
        .with_context(|| format!("compiled {field} exceeds u32 ticks"))
}

/// Compiles an authored Grave Mark record with normative shared timing and projectile defaults.
pub fn compile_grave_mark(record: &AbilityRecord) -> Result<GraveMarkDefinition> {
    let AbilityPayload::GraveMark {
        cooldown_ms,
        projectile_speed_milli_tiles_per_second,
        range_milli_tiles,
        weapon_damage_multiplier_basis_points,
        duration_ms,
        marked_primary_bonus_basis_points,
        maximum_marked_targets,
    } = &record.numeric_payload
    else {
        bail!("{} is not Grave Mark", record.header.id);
    };
    let compile_ticks = |milliseconds: u64, field: &str| {
        u32::try_from(duration_ms_to_ticks_nearest(milliseconds))
            .with_context(|| format!("compiled Grave Mark {field} exceeds u32 ticks"))
    };
    GraveMarkDefinition::new(GraveMarkDefinitionParameters {
        content_id: record.header.id.to_string(),
        cooldown_ticks: compile_ticks(u64::from(*cooldown_ms), "cooldown")?,
        global_cooldown_ticks: compile_ticks(GLOBAL_ABILITY_COOLDOWN_MS, "global cooldown")?,
        input_buffer_ticks: compile_ticks(ABILITY_INPUT_BUFFER_MS, "input buffer")?,
        projectile_speed_milli_tiles_per_second: *projectile_speed_milli_tiles_per_second,
        range_milli_tiles: *range_milli_tiles,
        projectile_radius_milli_tiles: COMMON_PROJECTILE_RADIUS_MILLI_TILES,
        weapon_damage_multiplier_basis_points: *weapon_damage_multiplier_basis_points,
        duration_ticks: compile_ticks(u64::from(*duration_ms), "duration")?,
        marked_primary_bonus_basis_points: *marked_primary_bonus_basis_points,
        maximum_marked_targets: *maximum_marked_targets,
        consumes_on_solid: true,
    })
    .with_context(|| format!("{} failed simulation ability validation", record.header.id))
}

/// Compiles one strict weapon item and its class-primary grammar into simulation values.
pub fn compile_primary_weapon(
    item: &ItemRecord,
    primary: &AbilityRecord,
) -> Result<WeaponDefinition> {
    let ItemPayload::Equipment { slot, effects, .. } = &item.numeric_payload else {
        bail!("{} is not equipment", item.header.id);
    };
    if *slot != EquipmentSlot::Weapon {
        bail!("{} is not a weapon", item.header.id);
    }
    let AbilityPayload::Primary {
        range_milli_tiles: primary_range,
        attacks_per_second_basis_points,
        projectile_radius_milli_tiles: primary_radius,
        stops_on_first_enemy,
    } = &primary.numeric_payload
    else {
        bail!("{} is not a primary ability", primary.header.id);
    };

    let raw_damage = required_set_effect(effects, "primary_damage")?;
    let attack_interval_ms = required_set_effect(effects, "attack_interval_ms")?;
    let range_milli_tiles = required_set_effect(effects, "range_milli_tiles")?;
    let projectile_speed_milli_tiles_per_second =
        required_set_effect(effects, "projectile_speed_milli_tiles_per_second")?;
    let projectile_radius_milli_tiles =
        required_set_effect(effects, "projectile_radius_milli_tiles")?;
    let projectile_count = required_set_effect(effects, "projectile_count")?;
    let pierce = required_set_effect(effects, "pierce")?;

    if *attacks_per_second_basis_points == 0 {
        bail!("{} primary attack rate must be positive", primary.header.id);
    }
    let derived_interval_ms = 10_000_000_u64
        .checked_add(u64::from(*attacks_per_second_basis_points) / 2)
        .context("primary interval rounding overflowed")?
        / u64::from(*attacks_per_second_basis_points);
    if u64::from(attack_interval_ms) != derived_interval_ms
        || range_milli_tiles != *primary_range
        || projectile_radius_milli_tiles != *primary_radius
    {
        bail!(
            "{} item values disagree with {} primary grammar",
            item.header.id,
            primary.header.id
        );
    }
    let attack_interval_ticks =
        u32::try_from(duration_ms_to_ticks_nearest(u64::from(attack_interval_ms)))
            .context("compiled attack interval exceeds u32 ticks")?;
    WeaponDefinition::new(WeaponDefinitionParameters {
        content_id: item.header.id.to_string(),
        raw_damage,
        attack_interval_ticks,
        range_milli_tiles,
        projectile_speed_milli_tiles_per_second,
        projectile_radius_milli_tiles,
        projectile_count,
        projectile_directions_millionths: vec![(1_000_000, 0)],
        max_projectiles_per_target: 1,
        pierce,
        stops_on_first_enemy: *stops_on_first_enemy,
    })
    .with_context(|| format!("{} failed simulation weapon validation", item.header.id))
}

fn required_set_effect(effects: &[ItemEffect], stat: &str) -> Result<u32> {
    let mut matching = effects.iter().filter(|effect| effect.stat == stat);
    let effect = matching
        .next()
        .with_context(|| format!("weapon is missing required `{stat}` effect"))?;
    if matching.next().is_some() {
        bail!("weapon contains duplicate `{stat}` effects");
    }
    if effect.operation != EffectOperation::Set {
        bail!("weapon `{stat}` effect must use the `set` operation");
    }
    u32::try_from(effect.value).with_context(|| format!("weapon `{stat}` must be nonnegative"))
}

/// Compiles one strict content record into renderer-independent simulation geometry.
pub fn compile_arena_geometry(record: &ArenaRecord) -> Result<ArenaGeometry> {
    let payload = &record.numeric_payload;
    let mut pillars = payload
        .pillars
        .iter()
        .map(|rectangle| {
            Ok(TileRectangle::new(
                rectangle.x_milli_tiles,
                rectangle.y_milli_tiles,
                u32_to_i32(rectangle.width_milli_tiles, "pillar width")?,
                u32_to_i32(rectangle.height_milli_tiles, "pillar height")?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    pillars.sort_by_key(|rectangle| {
        (
            rectangle.y_milli_tiles,
            rectangle.x_milli_tiles,
            rectangle.width_milli_tiles,
            rectangle.height_milli_tiles,
        )
    });

    let mut anchors: Vec<_> = payload
        .anchors
        .iter()
        .map(|anchor| ArenaAnchor {
            id: anchor.id.clone(),
            point: content_point(anchor.point),
        })
        .collect();
    anchors.sort_by(|first, second| first.id.as_bytes().cmp(second.id.as_bytes()));

    ArenaGeometry {
        id: record.header.id.to_string(),
        width_milli_tiles: whole_tiles_to_milli(payload.width_tiles, "arena width")?,
        height_milli_tiles: whole_tiles_to_milli(payload.height_tiles, "arena height")?,
        shell_thickness_milli_tiles: whole_tiles_to_milli(
            payload.shell_thickness_tiles,
            "shell thickness",
        )?,
        player_spawn: content_point(payload.player_spawn),
        boss_spawn: content_point(payload.boss_spawn),
        pillars,
        anchors,
    }
    .validated()
    .context("arena geometry invariants failed")
}

fn content_point(point: content_schema::Point) -> TilePoint {
    TilePoint::new(point.x_milli_tiles, point.y_milli_tiles)
}

fn whole_tiles_to_milli(tiles: u32, field: &str) -> Result<i32> {
    let tiles = u32_to_i32(tiles, field)?;
    tiles
        .checked_mul(MILLI_TILES_PER_TILE)
        .with_context(|| format!("{field} exceeds fixed-point range"))
}

fn u32_to_i32(value: u32, field: &str) -> Result<i32> {
    i32::try_from(value).with_context(|| format!("{field} exceeds signed geometry range"))
}

fn validate_first_playable_arena(package: &ContentPackage) -> Result<()> {
    let actual = first_playable_arena(package)?;
    let expected = expected_first_playable_arena()?;
    if actual != expected {
        bail!("{FIRST_PLAYABLE_ARENA_ID} does not exactly match CONT-FP-002 geometry");
    }
    Ok(())
}

fn expected_first_playable_arena() -> Result<ArenaGeometry> {
    ArenaGeometry {
        id: FIRST_PLAYABLE_ARENA_ID.to_owned(),
        width_milli_tiles: 32_000,
        height_milli_tiles: 24_000,
        shell_thickness_milli_tiles: 1_000,
        player_spawn: TilePoint::new(4_000, 12_000),
        boss_spawn: TilePoint::new(24_000, 12_000),
        pillars: vec![
            TileRectangle::new(10_000, 5_000, 2_000, 3_000),
            TileRectangle::new(20_000, 5_000, 2_000, 3_000),
            TileRectangle::new(10_000, 16_000, 2_000, 3_000),
            TileRectangle::new(20_000, 16_000, 2_000, 3_000),
        ],
        anchors: vec![
            ArenaAnchor {
                id: "C".to_owned(),
                point: TilePoint::new(16_000, 12_000),
            },
            ArenaAnchor {
                id: "E1".to_owned(),
                point: TilePoint::new(29_000, 8_000),
            },
            ArenaAnchor {
                id: "E2".to_owned(),
                point: TilePoint::new(29_000, 16_000),
            },
            ArenaAnchor {
                id: "N1".to_owned(),
                point: TilePoint::new(8_000, 3_000),
            },
            ArenaAnchor {
                id: "N2".to_owned(),
                point: TilePoint::new(16_000, 3_000),
            },
            ArenaAnchor {
                id: "N3".to_owned(),
                point: TilePoint::new(24_000, 3_000),
            },
            ArenaAnchor {
                id: "S1".to_owned(),
                point: TilePoint::new(8_000, 21_000),
            },
            ArenaAnchor {
                id: "S2".to_owned(),
                point: TilePoint::new(16_000, 21_000),
            },
            ArenaAnchor {
                id: "S3".to_owned(),
                point: TilePoint::new(24_000, 21_000),
            },
            ArenaAnchor {
                id: "W1".to_owned(),
                point: TilePoint::new(3_000, 8_000),
            },
            ArenaAnchor {
                id: "W2".to_owned(),
                point: TilePoint::new(3_000, 16_000),
            },
            ArenaAnchor {
                id: "reward_pedestal".to_owned(),
                point: TilePoint::new(4_000, 4_000),
            },
            ArenaAnchor {
                id: "tonic_refill".to_owned(),
                point: TilePoint::new(4_000, 20_000),
            },
        ],
    }
    .validated()
    .context("built-in CONT-FP-002 fixture is invalid")
}

fn validate_first_playable_weapon(package: &ContentPackage) -> Result<()> {
    let actual = first_playable_weapon(package)?;
    let expected = expected_first_playable_weapon()?;
    if actual != expected {
        bail!("{FIRST_PLAYABLE_WEAPON_ID} does not exactly match CONT-FP-006 values");
    }
    Ok(())
}

fn validate_first_playable_grave_mark(package: &ContentPackage) -> Result<()> {
    let record = package
        .abilities
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_GRAVE_MARK_ID)
        .context("First Playable Grave Mark ability record is missing")?;
    let AbilityPayload::GraveMark {
        cooldown_ms,
        projectile_speed_milli_tiles_per_second,
        range_milli_tiles,
        weapon_damage_multiplier_basis_points,
        duration_ms,
        marked_primary_bonus_basis_points,
        maximum_marked_targets,
    } = &record.numeric_payload
    else {
        bail!("{FIRST_PLAYABLE_GRAVE_MARK_ID} is not Grave Mark");
    };
    if (
        *cooldown_ms,
        *projectile_speed_milli_tiles_per_second,
        *range_milli_tiles,
        *weapon_damage_multiplier_basis_points,
        *duration_ms,
        *marked_primary_bonus_basis_points,
        *maximum_marked_targets,
    ) != (5_000, 12_000, 11_000, 18_000, 4_000, 1_500, 1)
    {
        bail!("{FIRST_PLAYABLE_GRAVE_MARK_ID} does not exactly match CLS-020 authored values");
    }
    let actual = first_playable_grave_mark(package)?;
    let expected = expected_first_playable_grave_mark()?;
    if actual != expected {
        bail!("{FIRST_PLAYABLE_GRAVE_MARK_ID} does not exactly match CLS-020/CONT-013 values");
    }
    Ok(())
}

fn expected_first_playable_grave_mark() -> Result<GraveMarkDefinition> {
    GraveMarkDefinition::new(GraveMarkDefinitionParameters {
        content_id: FIRST_PLAYABLE_GRAVE_MARK_ID.to_owned(),
        cooldown_ticks: 150,
        global_cooldown_ticks: 5,
        input_buffer_ticks: 3,
        projectile_speed_milli_tiles_per_second: 12_000,
        range_milli_tiles: 11_000,
        projectile_radius_milli_tiles: 120,
        weapon_damage_multiplier_basis_points: 18_000,
        duration_ticks: 120,
        marked_primary_bonus_basis_points: 1_500,
        maximum_marked_targets: 1,
        consumes_on_solid: true,
    })
    .context("built-in CLS-020 Grave Mark fixture is invalid")
}

fn validate_first_playable_slipstep(package: &ContentPackage) -> Result<()> {
    let record = package
        .abilities
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_SLIPSTEP_ID)
        .context("First Playable Slipstep ability record is missing")?;
    let AbilityPayload::Slipstep {
        cooldown_ms,
        travel_milli_tiles,
        travel_ms,
        direct_damage_reduction_basis_points,
        empowered_window_ms,
        projectile_speed_bonus_basis_points,
        pierce_bonus,
        exhaustion_ms,
    } = &record.numeric_payload
    else {
        bail!("{FIRST_PLAYABLE_SLIPSTEP_ID} is not Slipstep");
    };
    if (
        *cooldown_ms,
        *travel_milli_tiles,
        *travel_ms,
        *direct_damage_reduction_basis_points,
        *empowered_window_ms,
        *projectile_speed_bonus_basis_points,
        *pierce_bonus,
        *exhaustion_ms,
    ) != (8_000, 2_000, 180, 2_500, 1_500, 3_000, 1, 1_500)
    {
        bail!("{FIRST_PLAYABLE_SLIPSTEP_ID} does not exactly match CLS-020 authored values");
    }
    if first_playable_slipstep(package)? != expected_first_playable_slipstep()? {
        bail!("{FIRST_PLAYABLE_SLIPSTEP_ID} does not exactly match CLS-020 timing values");
    }
    Ok(())
}

fn expected_first_playable_slipstep() -> Result<SlipstepDefinition> {
    SlipstepDefinition::new(SlipstepDefinitionParameters {
        content_id: FIRST_PLAYABLE_SLIPSTEP_ID.to_owned(),
        cooldown_ticks: 240,
        global_cooldown_ticks: 5,
        input_buffer_ticks: 3,
        travel_milli_tiles: 2_000,
        travel_ticks: 5,
        direct_damage_reduction_basis_points: 2_500,
        empowered_window_ticks: 45,
        projectile_speed_bonus_basis_points: 3_000,
        pierce_bonus: 1,
        exhaustion_ticks: 45,
    })
    .context("built-in CLS-020 Slipstep fixture is invalid")
}

fn validate_first_playable_stillness(package: &ContentPackage) -> Result<()> {
    let record = package
        .abilities
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_STILLNESS_ID)
        .context("First Playable Stillness ability record is missing")?;
    let AbilityPayload::Stillness {
        activation_ms,
        movement_threshold_basis_points,
        projectile_speed_bonus_basis_points,
        primary_damage_bonus_basis_points,
        break_on_damage,
        break_on_slipstep,
    } = &record.numeric_payload
    else {
        bail!("{FIRST_PLAYABLE_STILLNESS_ID} is not Stillness");
    };
    if (
        *activation_ms,
        *movement_threshold_basis_points,
        *projectile_speed_bonus_basis_points,
        *primary_damage_bonus_basis_points,
        *break_on_damage,
        *break_on_slipstep,
    ) != (600, 2_000, 1_000, 800, true, true)
    {
        bail!("{FIRST_PLAYABLE_STILLNESS_ID} does not exactly match CLS-020 authored values");
    }
    if first_playable_stillness(package)? != expected_first_playable_stillness()? {
        bail!("{FIRST_PLAYABLE_STILLNESS_ID} does not exactly match CLS-020 timing values");
    }
    Ok(())
}

fn expected_first_playable_stillness() -> Result<StillnessDefinition> {
    StillnessDefinition::new(StillnessDefinitionParameters {
        content_id: FIRST_PLAYABLE_STILLNESS_ID.to_owned(),
        activation_ticks: 18,
        movement_threshold_basis_points: 2_000,
        projectile_speed_bonus_basis_points: 1_000,
        primary_damage_bonus_basis_points: 800,
        break_on_damage: true,
        break_on_slipstep: true,
    })
    .context("built-in CLS-020 Stillness fixture is invalid")
}

fn validate_first_playable_red_tonic(package: &ContentPackage) -> Result<()> {
    let record = package
        .items
        .iter()
        .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_RED_TONIC_ID)
        .context("First Playable Red Tonic item record is missing")?;
    let ItemPayload::Consumable {
        belt_stack_cap,
        restore_max_health_basis_points,
        restore_duration_ms,
        shared_cooldown_ms,
        damage_interrupts_restore,
        consumed_on_use,
    } = &record.numeric_payload
    else {
        bail!("{FIRST_PLAYABLE_RED_TONIC_ID} is not a consumable");
    };
    if (
        *belt_stack_cap,
        *restore_max_health_basis_points,
        *restore_duration_ms,
        *shared_cooldown_ms,
        *damage_interrupts_restore,
        *consumed_on_use,
    ) != (6, 3_000, 400, 2_000, false, true)
    {
        bail!("{FIRST_PLAYABLE_RED_TONIC_ID} does not exactly match CONT-FP-007 values");
    }
    if first_playable_red_tonic(package)? != expected_first_playable_red_tonic()? {
        bail!("{FIRST_PLAYABLE_RED_TONIC_ID} does not exactly match CONT-FP-007 timing values");
    }
    Ok(())
}

fn expected_first_playable_red_tonic() -> Result<RedTonicDefinition> {
    RedTonicDefinition::new(RedTonicDefinitionParameters {
        content_id: FIRST_PLAYABLE_RED_TONIC_ID.to_owned(),
        belt_stack_cap: 6,
        restore_max_health_basis_points: 3_000,
        restore_duration_ticks: 12,
        shared_cooldown_ticks: 60,
        damage_interrupts_restore: false,
        consumed_on_use: true,
    })
    .context("built-in CONT-FP-007 Red Tonic fixture is invalid")
}

fn expected_first_playable_weapon() -> Result<WeaponDefinition> {
    WeaponDefinition::new(WeaponDefinitionParameters {
        content_id: FIRST_PLAYABLE_WEAPON_ID.to_owned(),
        raw_damage: 20,
        attack_interval_ticks: 14,
        range_milli_tiles: 9_500,
        projectile_speed_milli_tiles_per_second: 12_000,
        projectile_radius_milli_tiles: 100,
        projectile_count: 1,
        projectile_directions_millionths: vec![(1_000_000, 0)],
        max_projectiles_per_target: 1,
        pierce: 0,
        stops_on_first_enemy: true,
    })
    .context("built-in CONT-FP-006 Pine Crossbow fixture is invalid")
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
    let bosses: BTreeSet<_> = package
        .bosses
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
    for boss in &package.bosses {
        for pattern_id in [
            &boss.numeric_payload.fan_pattern_id,
            &boss.numeric_payload.ring_pattern_id,
            &boss.numeric_payload.cross_pattern_id,
        ] {
            require_ref(&boss.header.id, pattern_id, &patterns, "pattern")?;
        }
        require_ref(
            &boss.header.id,
            &boss.numeric_payload.reward_table_id,
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
        for boss_id in &arena.numeric_payload.allowed_boss_ids {
            require_ref(&arena.header.id, boss_id, &bosses, "boss")?;
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
        ("boss", package.bosses.len()),
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
        &package.bosses,
        &[BELL_PROCTOR_ID],
        |record| &record.header.id,
        "boss",
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
            BELL_PROCTOR_FAN_ID,
            BELL_PROCTOR_CROSS_ID,
            BELL_PROCTOR_RING_ID,
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
        .chain(package.bosses.iter().map(|record| &record.header))
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
        assert_eq!(report.record_count, 34);
    }

    #[test]
    fn m02_authority_fixture_compiles_only_from_checked_in_content() {
        let package = valid_package();
        let rewards = first_playable_reward_catalog(&package).expect("rewards");
        let first_seed = (0..1_000_u64)
            .find(|seed| {
                !rewards
                    .resolve(
                        "reward.prototype.normal_enemy",
                        &package.release_manifest.content_version,
                        *seed,
                        1,
                    )
                    .expect("reward resolution")
                    .is_empty()
            })
            .expect("a bounded seed resolves a reward");
        assert_eq!(first_seed, M02_COMBAT_TEST_REWARD_SEED);
        let fixture =
            first_playable_authority_combat_test(&package).expect("M02 authority content");
        assert_eq!(fixture.definitions.maximum_health, 128);
        assert_eq!(fixture.definitions.starting_armor, 2);
        assert_eq!(fixture.definitions.reward_stacks.len(), 1);
        assert_eq!(fixture.spawns.len(), 1);
        assert_eq!(fixture.spawns[0].kind, NormalWaveEnemyKind::DrownedPilgrim);
        assert_eq!(fixture.spawns[0].position_milli_tiles, (3_000, 8_000));
    }

    #[test]
    fn bell_proctor_content_compiles_losslessly_and_rejects_authored_drift() {
        let package = valid_package();
        let compiled = first_playable_bell_proctor(&package).expect("compiled Bell Proctor");
        assert_eq!(compiled, BellProctorDefinition::first_playable());
        let parameters = compiled.parameters();
        assert_eq!(parameters.fan.damage_band, DamageBand::Chip);
        assert_eq!(parameters.fan.maximum_active_instances, 10);
        assert_eq!(
            (parameters.phase_two_health, parameters.phase_three_health),
            (2_100, 1_050)
        );

        let mut timing_drift = package.clone();
        timing_drift.bosses[0].numeric_payload.introduction_ms += 1;
        assert!(first_playable_bell_proctor(&timing_drift).is_err());

        let mut pattern_drift = package;
        pattern_drift
            .patterns
            .iter_mut()
            .find(|record| record.header.id.as_str() == BELL_PROCTOR_FAN_ID)
            .expect("fan")
            .numeric_payload
            .maximum_active_instances = 5;
        assert!(first_playable_bell_proctor(&pattern_drift).is_err());
    }

    #[test]
    fn bell_proctor_references_and_manifest_are_fail_closed() {
        let mut missing_pattern = valid_package();
        missing_pattern.bosses[0].numeric_payload.ring_pattern_id =
            ContentId::parse("pattern.prototype.bell_proctor.missing").expect("valid ID");
        assert!(validate_references(&missing_pattern).is_err());

        let mut missing_manifest = valid_package();
        missing_manifest
            .release_manifest
            .required_content_ids
            .retain(|id| id.as_str() != BELL_PROCTOR_ID);
        assert!(validate_manifest(&missing_manifest).is_err());
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

    #[test]
    fn first_playable_arena_compiles_exactly_and_order_independently() {
        let mut package = valid_package();
        let expected = expected_first_playable_arena().expect("fixture");
        assert_eq!(first_playable_arena(&package).expect("compiled"), expected);

        package.arenas[0].numeric_payload.pillars.reverse();
        package.arenas[0].numeric_payload.anchors.reverse();
        assert_eq!(first_playable_arena(&package).expect("reordered"), expected);
    }

    #[test]
    fn first_playable_arena_mismatch_fails_content_validation() {
        let mut package = valid_package();
        package.arenas[0].numeric_payload.player_spawn.x_milli_tiles += 1;
        let error = validate_first_playable_arena(&package).expect_err("mismatch must fail");
        assert!(error.to_string().contains("CONT-FP-002"));
    }

    #[test]
    fn first_playable_pine_crossbow_compiles_exactly() {
        let package = valid_package();
        let weapon = first_playable_weapon(&package).expect("compiled weapon");
        assert_eq!(
            weapon,
            expected_first_playable_weapon().expect("weapon fixture")
        );
        assert_eq!(weapon.projectile_lifetime_ticks(), 24);
    }

    #[test]
    fn malformed_weapon_effects_fail_closed() {
        let mut package = valid_package();
        let ItemPayload::Equipment { effects, .. } = &mut package
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_WEAPON_ID)
            .expect("pine crossbow")
            .numeric_payload
        else {
            panic!("pine crossbow must be equipment");
        };
        effects.push(
            effects
                .iter()
                .find(|effect| effect.stat == "primary_damage")
                .expect("damage effect")
                .clone(),
        );
        let error = first_playable_weapon(&package).expect_err("duplicate must fail");
        assert!(error.to_string().contains("duplicate `primary_damage`"));

        let mut package = valid_package();
        let ItemPayload::Equipment { effects, .. } = &mut package
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_WEAPON_ID)
            .expect("pine crossbow")
            .numeric_payload
        else {
            panic!("pine crossbow must be equipment");
        };
        effects
            .iter_mut()
            .find(|effect| effect.stat == "attack_interval_ms")
            .expect("interval effect")
            .operation = EffectOperation::Add;
        let error = first_playable_weapon(&package).expect_err("wrong operation must fail");
        assert!(error.to_string().contains("must use the `set` operation"));

        let mut package = valid_package();
        let ItemPayload::Equipment { effects, .. } = &mut package
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_WEAPON_ID)
            .expect("pine crossbow")
            .numeric_payload
        else {
            panic!("pine crossbow must be equipment");
        };
        effects.retain(|effect| effect.stat != "projectile_count");
        let error = first_playable_weapon(&package).expect_err("missing effect must fail");
        assert!(
            error
                .to_string()
                .contains("missing required `projectile_count`")
        );

        let mut package = valid_package();
        let ItemPayload::Equipment { effects, .. } = &mut package
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_WEAPON_ID)
            .expect("pine crossbow")
            .numeric_payload
        else {
            panic!("pine crossbow must be equipment");
        };
        effects
            .iter_mut()
            .find(|effect| effect.stat == "primary_damage")
            .expect("damage effect")
            .value = -1;
        let error = first_playable_weapon(&package).expect_err("negative effect must fail");
        assert!(error.to_string().contains("must be nonnegative"));
    }

    #[test]
    fn item_and_primary_grammar_mismatch_fails_closed() {
        let mut package = valid_package();
        let primary = package
            .abilities
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_PRIMARY_ID)
            .expect("primary");
        let AbilityPayload::Primary {
            range_milli_tiles, ..
        } = &mut primary.numeric_payload
        else {
            panic!("primary payload");
        };
        *range_milli_tiles += 1;
        let error = first_playable_weapon(&package).expect_err("mismatch must fail");
        assert!(error.to_string().contains("disagree"));
    }

    #[test]
    fn consistently_mutated_weapon_still_fails_exact_fp_contract() {
        let mut package = valid_package();
        let primary = package
            .abilities
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_PRIMARY_ID)
            .expect("primary");
        let AbilityPayload::Primary {
            range_milli_tiles, ..
        } = &mut primary.numeric_payload
        else {
            panic!("primary payload");
        };
        *range_milli_tiles += 1;
        let ItemPayload::Equipment { effects, .. } = &mut package
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_WEAPON_ID)
            .expect("pine crossbow")
            .numeric_payload
        else {
            panic!("pine crossbow must be equipment");
        };
        effects
            .iter_mut()
            .find(|effect| effect.stat == "range_milli_tiles")
            .expect("range effect")
            .value += 1;
        let error =
            validate_first_playable_weapon(&package).expect_err("exact fixture must reject change");
        assert!(error.to_string().contains("CONT-FP-006"));
    }

    #[test]
    fn first_playable_grave_mark_compiles_exact_shared_defaults() {
        let package = valid_package();
        let ability = first_playable_grave_mark(&package).expect("compiled Grave Mark");
        assert_eq!(
            ability,
            expected_first_playable_grave_mark().expect("Grave Mark fixture")
        );
        assert_eq!(ability.cooldown_ticks(), 150);
        assert_eq!(ability.global_cooldown_ticks(), 5);
        assert_eq!(ability.input_buffer_ticks(), 3);
        assert_eq!(ability.projectile_lifetime_ticks(), 28);
        assert!((ability.projectile_radius_tiles() - 0.12).abs() < f32::EPSILON);
        assert!(ability.consumes_on_solid());
    }

    #[test]
    fn grave_mark_reference_and_exact_values_fail_closed() {
        let mut package = valid_package();
        package.classes[0]
            .numeric_payload
            .active_ability_ids
            .retain(|id| id.as_str() != FIRST_PLAYABLE_GRAVE_MARK_ID);
        let error = first_playable_grave_mark(&package).expect_err("missing class ref must fail");
        assert!(error.to_string().contains("exactly once"));

        let mut package = valid_package();
        let record = package
            .abilities
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_GRAVE_MARK_ID)
            .expect("Grave Mark");
        let AbilityPayload::GraveMark { cooldown_ms, .. } = &mut record.numeric_payload else {
            panic!("Grave Mark payload");
        };
        *cooldown_ms += 1;
        let error =
            validate_first_playable_grave_mark(&package).expect_err("exact fixture must reject");
        assert!(error.to_string().contains("CLS-020"));
    }

    #[test]
    fn first_playable_slipstep_compiles_exact_values_and_rejects_drift() {
        let package = valid_package();
        let ability = first_playable_slipstep(&package).expect("compiled Slipstep");
        assert_eq!(
            ability,
            expected_first_playable_slipstep().expect("fixture")
        );
        assert_eq!(ability.travel_ticks(), 5);
        assert_eq!(ability.cooldown_ticks(), 240);
        assert_eq!(ability.exhaustion_ticks(), 45);

        let mut package = valid_package();
        let record = package
            .abilities
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_SLIPSTEP_ID)
            .expect("Slipstep");
        let AbilityPayload::Slipstep { travel_ms, .. } = &mut record.numeric_payload else {
            panic!("Slipstep payload");
        };
        *travel_ms += 1;
        let error = validate_first_playable_slipstep(&package).expect_err("drift must fail");
        assert!(error.to_string().contains("CLS-020"));
    }

    #[test]
    fn first_playable_stillness_compiles_exact_values_and_rejects_drift() {
        let package = valid_package();
        let passive = first_playable_stillness(&package).expect("compiled Stillness");
        assert_eq!(
            passive,
            expected_first_playable_stillness().expect("fixture")
        );
        assert_eq!(passive.activation_ticks(), 18);

        let mut package = valid_package();
        let record = package
            .abilities
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_STILLNESS_ID)
            .expect("Stillness");
        let AbilityPayload::Stillness { activation_ms, .. } = &mut record.numeric_payload else {
            panic!("Stillness payload");
        };
        *activation_ms += 1;
        let error = validate_first_playable_stillness(&package).expect_err("drift must fail");
        assert!(error.to_string().contains("CLS-020"));
    }

    #[test]
    fn first_playable_red_tonic_compiles_exact_values() {
        let package = valid_package();
        let tonic = first_playable_red_tonic(&package).expect("compiled Red Tonic");
        assert_eq!(tonic, expected_first_playable_red_tonic().expect("fixture"));
        assert_eq!(tonic.belt_stack_cap(), 6);
        assert_eq!(tonic.restore_max_health_basis_points(), 3_000);
        assert_eq!(tonic.restore_duration_ticks(), 12);
        assert_eq!(tonic.shared_cooldown_ticks(), 60);
        assert!(!tonic.damage_interrupts_restore());
        assert!(tonic.consumed_on_use());
    }

    #[test]
    fn first_playable_red_tonic_rejects_authored_drift() {
        let mut package = valid_package();
        let record = package
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_RED_TONIC_ID)
            .expect("Red Tonic");
        let ItemPayload::Consumable {
            shared_cooldown_ms, ..
        } = &mut record.numeric_payload
        else {
            panic!("Red Tonic payload");
        };
        *shared_cooldown_ms += 1;
        let error =
            validate_first_playable_red_tonic(&package).expect_err("drift must fail closed");
        assert!(error.to_string().contains("CONT-FP-007"));
    }

    #[test]
    fn first_playable_red_tonic_rejects_wrong_kind_and_manifest_reference() {
        let mut wrong_kind = valid_package();
        let equipment_payload = wrong_kind
            .items
            .iter()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_WEAPON_ID)
            .expect("Pine Crossbow")
            .numeric_payload
            .clone();
        wrong_kind
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_RED_TONIC_ID)
            .expect("Red Tonic")
            .numeric_payload = equipment_payload;
        let error = first_playable_red_tonic(&wrong_kind).expect_err("wrong kind must fail");
        assert!(error.to_string().contains("not a consumable"));

        let mut wrong_reference = valid_package();
        wrong_reference
            .release_manifest
            .required_content_ids
            .retain(|id| id.as_str() != FIRST_PLAYABLE_RED_TONIC_ID);
        let error = validate_manifest(&wrong_reference).expect_err("missing reference must fail");
        assert!(error.to_string().contains("do not exactly match"));
    }

    #[test]
    fn undertaker_knot_compiles_exact_override_and_is_effect_order_independent() {
        let mut package = valid_package();
        let resolved = first_playable_red_tonic_with_undertaker_knot(&package)
            .expect("Undertaker Knot override");
        assert_eq!(resolved.restore_max_health_basis_points(), 3_500);
        assert_eq!(resolved.restore_duration_ticks(), 12);
        assert_eq!(resolved.shared_cooldown_ticks(), 75);

        let record = package
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_UNDERTAKER_KNOT_ID)
            .expect("Undertaker Knot");
        let ItemPayload::Equipment { effects, .. } = &mut record.numeric_payload else {
            panic!("Undertaker Knot equipment payload");
        };
        effects.reverse();
        assert_eq!(
            first_playable_red_tonic_with_undertaker_knot(&package)
                .expect("semantic effect ordering"),
            resolved
        );
    }

    #[test]
    fn undertaker_knot_rejects_value_operation_shape_and_base_tonic_drift() {
        let mut value_drift = valid_package();
        let record = value_drift
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_UNDERTAKER_KNOT_ID)
            .expect("Knot");
        let ItemPayload::Equipment { effects, .. } = &mut record.numeric_payload else {
            panic!("Knot payload");
        };
        effects[0].value += 1;
        assert!(
            first_playable_red_tonic_with_undertaker_knot(&value_drift)
                .expect_err("value drift")
                .to_string()
                .contains("CONT-FP-006")
        );

        let mut operation_drift = valid_package();
        let record = operation_drift
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_UNDERTAKER_KNOT_ID)
            .expect("Knot");
        let ItemPayload::Equipment { effects, .. } = &mut record.numeric_payload else {
            panic!("Knot payload");
        };
        effects[1].operation = EffectOperation::Add;
        assert!(
            first_playable_red_tonic_with_undertaker_knot(&operation_drift)
                .expect_err("operation drift")
                .to_string()
                .contains("`set` operation")
        );

        let mut shape_drift = valid_package();
        let record = shape_drift
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_UNDERTAKER_KNOT_ID)
            .expect("Knot");
        let ItemPayload::Equipment { effects, .. } = &mut record.numeric_payload else {
            panic!("Knot payload");
        };
        effects.push(effects[0].clone());
        assert!(
            first_playable_red_tonic_with_undertaker_knot(&shape_drift)
                .expect_err("extra effect")
                .to_string()
                .contains("exactly two effects")
        );

        let mut base_drift = valid_package();
        let record = base_drift
            .items
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_RED_TONIC_ID)
            .expect("Tonic");
        let ItemPayload::Consumable {
            restore_duration_ms,
            ..
        } = &mut record.numeric_payload
        else {
            panic!("Tonic payload");
        };
        *restore_duration_ms += 1;
        assert!(
            first_playable_red_tonic_with_undertaker_knot(&base_drift)
                .expect_err("base drift")
                .to_string()
                .contains("CONT-FP-007")
        );
    }

    #[test]
    fn all_first_playable_enemies_compile_to_exact_simulation_definitions() {
        let package = valid_package();
        assert_eq!(
            first_playable_drowned_pilgrim(&package).expect("Pilgrim"),
            DrownedPilgrimDefinition::first_playable()
        );
        assert_eq!(
            first_playable_bell_reed(&package).expect("Reed"),
            BellReedDefinition::first_playable()
        );
        assert_eq!(
            first_playable_chain_sentry(&package).expect("Sentry"),
            ChainSentryDefinition::first_playable()
        );
    }

    #[test]
    fn enemy_authored_millisecond_drift_fails_even_when_tick_rounding_is_unchanged() {
        let mut package = valid_package();
        let pilgrim = package
            .enemies
            .iter_mut()
            .find(|record| record.header.id.as_str() == DROWNED_PILGRIM_ID)
            .expect("Pilgrim");
        pilgrim.numeric_payload.spawn_telegraph_ms = 899;
        assert_eq!(
            duration_ms_to_ticks_ceil(899),
            duration_ms_to_ticks_ceil(900)
        );
        assert!(
            first_playable_drowned_pilgrim(&package)
                .expect_err("authored drift")
                .to_string()
                .contains("CONT-FP-004 enemy tuple")
        );

        let mut package = valid_package();
        let pattern = package
            .patterns
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_BELL_REED_PATTERN_ID)
            .expect("Reed pattern");
        pattern.numeric_payload.projectile_lifetime_ms = Some(3_001);
        assert_eq!(
            duration_ms_to_ticks_nearest(3_001),
            duration_ms_to_ticks_nearest(3_000)
        );
        assert!(
            first_playable_bell_reed(&package)
                .expect_err("pattern drift")
                .to_string()
                .contains("CONT-FP-004 pattern tuple")
        );
    }

    #[test]
    fn enemy_state_machine_order_kind_and_damage_metadata_fail_closed() {
        let mut state_drift = valid_package();
        let sentry = state_drift
            .enemies
            .iter_mut()
            .find(|record| record.header.id.as_str() == CHAIN_SENTRY_ID)
            .expect("Sentry");
        sentry.numeric_payload.state_machine.swap(1, 2);
        assert!(
            first_playable_chain_sentry(&state_drift)
                .expect_err("state order")
                .to_string()
                .contains("state machine")
        );

        let mut kind_drift = valid_package();
        let pattern = kind_drift
            .patterns
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_DROWNED_PILGRIM_PATTERN_ID)
            .expect("Pilgrim pattern");
        pattern.numeric_payload.pattern_kind = PatternKind::GapRing;
        assert!(
            first_playable_drowned_pilgrim(&kind_drift)
                .expect_err("kind")
                .to_string()
                .contains("pattern tuple")
        );

        let mut damage_drift = valid_package();
        let pattern = damage_drift
            .patterns
            .iter_mut()
            .find(|record| record.header.id.as_str() == FIRST_PLAYABLE_BELL_REED_PATTERN_ID)
            .expect("Reed pattern");
        pattern.numeric_payload.damage_band = ContentDamageBand::Pressure;
        assert!(
            first_playable_bell_reed(&damage_drift)
                .expect_err("band")
                .to_string()
                .contains("pattern tuple")
        );
    }

    #[test]
    fn enemy_pattern_reward_manifest_and_duplicate_references_fail_closed() {
        let mut wrong_pattern = valid_package();
        let pilgrim = wrong_pattern
            .enemies
            .iter_mut()
            .find(|record| record.header.id.as_str() == DROWNED_PILGRIM_ID)
            .expect("Pilgrim");
        pilgrim.numeric_payload.pattern_ids[0] =
            ContentId::parse(FIRST_PLAYABLE_BELL_REED_PATTERN_ID).expect("ID");
        assert!(
            first_playable_drowned_pilgrim(&wrong_pattern)
                .expect_err("wrong pattern")
                .to_string()
                .contains("exactly one pattern")
        );

        let mut wrong_reward = valid_package();
        let pilgrim = wrong_reward
            .enemies
            .iter_mut()
            .find(|record| record.header.id.as_str() == DROWNED_PILGRIM_ID)
            .expect("Pilgrim");
        pilgrim.numeric_payload.reward_table_id =
            ContentId::parse("reward.prototype.wave_1").expect("ID");
        assert!(
            first_playable_drowned_pilgrim(&wrong_reward)
                .expect_err("wrong reward")
                .to_string()
                .contains(NORMAL_ENEMY_REWARD_TABLE_ID)
        );

        let mut missing_manifest = valid_package();
        missing_manifest
            .release_manifest
            .required_content_ids
            .retain(|id| id.as_str() != CHAIN_SENTRY_ID);
        assert!(
            first_playable_chain_sentry(&missing_manifest)
                .expect_err("manifest")
                .to_string()
                .contains("manifest must reference")
        );

        let mut duplicate = valid_package();
        let extra = duplicate.enemies[0].clone();
        duplicate.enemies.push(extra);
        assert!(
            first_playable_drowned_pilgrim(&duplicate)
                .expect_err("duplicate")
                .to_string()
                .contains("duplicated")
        );
    }
}
