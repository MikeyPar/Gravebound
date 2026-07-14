//! Fail-closed compiler for the unpromoted `GB-M03-03E` Sir Caldus subset.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};
use content_schema::{
    ContentId, CoreCaldusAssetKind, CoreCaldusAssetManifest, CoreCaldusAttackGroupRule,
    CoreCaldusCopyFile, CoreCaldusCounterplay, CoreCaldusDamageBand, CoreCaldusDevelopmentTarget,
    CoreCaldusDisposition, CoreCaldusMemoryFamily, CoreCaldusPatternPayload, CoreCaldusRecords,
    CoreCaldusSafeArrival, CoreCaldusTargetKind, DamageType, MilliTilePoint, ReleaseStage,
    SCHEMA_VERSION,
};

use crate::{
    CompiledProductionItemCatalog, CoreDevelopmentEncounterRooms, CoreDevelopmentProgression,
    load_core_development_encounter_rooms, load_core_development_items,
    load_core_development_progression,
};

pub const CORE_CALDUS_TARGET_NAME: &str = "core-dev-caldus";
pub const CORE_CALDUS_TARGET_PATH: &str = "core_dev/caldus.json";
pub const CORE_CALDUS_RECORDS_PATH: &str = "core_dev/caldus.records.json";
pub const CORE_CALDUS_ASSETS_PATH: &str = "core_dev/caldus.assets.json";
pub const CORE_CALDUS_COPY_PATH: &str = "core_dev/caldus.en-US.json";

const BOSS_IDS: [&str; 1] = ["boss.sir_caldus"];
const PATTERN_IDS: [&str; 4] = [
    "boss.caldus.shield_arc",
    "boss.caldus.bell_ring",
    "boss.caldus.charge_lane",
    "boss.caldus.charge_stop_ring",
];
const EXIT_IDS: [&str; 1] = ["portal.exit.dungeon.bell_sepulcher"];
const ASSET_IDS: [&str; 15] = [
    "sprite.boss.sir_caldus",
    "portrait.boss.sir_caldus",
    "boss.caldus.shield_arc.telegraph",
    "boss.caldus.shield_arc.warning",
    "boss.caldus.shield_arc.warning.major",
    "boss.caldus.bell_ring.telegraph",
    "boss.caldus.bell_ring.warning",
    "boss.caldus.bell_ring.warning.major",
    "boss.caldus.charge_lane.telegraph",
    "boss.caldus.charge_lane.warning",
    "boss.caldus.charge_lane.warning.major",
    "boss.caldus.charge_stop_ring.telegraph",
    "boss.caldus.charge_stop_ring.warning",
    "boss.caldus.charge_stop_ring.warning.major",
    "sprite.portal.exit.dungeon.bell_sepulcher",
];
const COPY_KEYS: [&str; 4] = [
    "boss.sir_caldus.name",
    "boss.sir_caldus.description",
    "portal.exit.dungeon.bell_sepulcher.name",
    "portal.exit.dungeon.bell_sepulcher.description",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusHashes {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

/// Immutable development-only view. It exposes no promotion or production-ingress operation.
#[derive(Debug, Clone)]
pub struct CoreDevelopmentCaldus {
    target_name: String,
    records: CoreCaldusRecords,
    hashes: CoreCaldusHashes,
    localization: BTreeMap<String, String>,
}

impl CoreDevelopmentCaldus {
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub fn boss(&self) -> &content_schema::CoreCaldusBossRecord {
        &self.records.bosses[0]
    }

    #[must_use]
    pub fn patterns(&self) -> &[content_schema::CoreCaldusPatternRecord] {
        &self.records.patterns
    }

    #[must_use]
    pub fn exit(&self) -> &content_schema::CoreCaldusExitRecord {
        &self.records.exits[0]
    }

    #[must_use]
    pub fn room_binding(&self) -> &content_schema::CoreCaldusRoomBindingRecord {
        &self.records.room_bindings[0]
    }

    #[must_use]
    pub const fn hashes(&self) -> &CoreCaldusHashes {
        &self.hashes
    }

    #[must_use]
    pub fn localized(&self, key: &str) -> Option<&str> {
        self.localization.get(key).map(String::as_str)
    }

    /// Exact WRLD-006 round-half-up health scaling for the immutable participant lock.
    pub fn scaled_maximum_health(&self, locked_participants: u8) -> Result<u32> {
        if !(1..=8).contains(&locked_participants) {
            bail!("Caldus locked participant count must be within 1..=8");
        }
        let boss = self.boss();
        let factor = 10_000_u64
            + u64::from(boss.additional_participant_health_basis_points)
                * u64::from(locked_participants - 1);
        let numerator = u64::from(boss.base_health)
            .checked_mul(factor)
            .context("Caldus health scaling overflowed")?;
        u32::try_from((numerator + 5_000) / 10_000).context("Caldus scaled health exceeds u32")
    }
}

pub fn load_core_development_caldus(root: &Path) -> Result<CoreDevelopmentCaldus> {
    let rooms = load_core_development_encounter_rooms(root)
        .context("Caldus compilation requires the validated 03D Bell rooms")?;
    let items = load_core_development_items(root)
        .context("Caldus compilation requires the validated Core reward catalog")?;
    let progression = load_core_development_progression(root)
        .context("Caldus compilation requires the validated Core progression catalog")?;
    let target: CoreCaldusDevelopmentTarget =
        crate::read_json(&root.join(CORE_CALDUS_TARGET_PATH))?;
    let records: CoreCaldusRecords = crate::read_json(&root.join(CORE_CALDUS_RECORDS_PATH))?;
    let assets: CoreCaldusAssetManifest = crate::read_json(&root.join(CORE_CALDUS_ASSETS_PATH))?;
    let copy: CoreCaldusCopyFile = crate::read_json(&root.join(CORE_CALDUS_COPY_PATH))?;
    let hashes = CoreCaldusHashes {
        records_blake3: hash_file(&root.join(CORE_CALDUS_RECORDS_PATH))?,
        assets_blake3: hash_file(&root.join(CORE_CALDUS_ASSETS_PATH))?,
        localization_blake3: hash_file(&root.join(CORE_CALDUS_COPY_PATH))?,
    };
    compile_core_development_caldus(
        &target,
        &records,
        &assets,
        &copy,
        &hashes,
        &rooms,
        &items,
        &progression,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn compile_core_development_caldus(
    target: &CoreCaldusDevelopmentTarget,
    records: &CoreCaldusRecords,
    assets: &CoreCaldusAssetManifest,
    copy: &CoreCaldusCopyFile,
    hashes: &CoreCaldusHashes,
    rooms: &CoreDevelopmentEncounterRooms,
    items: &CompiledProductionItemCatalog,
    progression: &CoreDevelopmentProgression,
) -> Result<CoreDevelopmentCaldus> {
    validate_target(target, hashes)?;
    validate_records(target, records, rooms, items, progression)?;
    validate_assets(target, records, assets)?;
    let localization = validate_copy(target, records, copy)?;
    Ok(CoreDevelopmentCaldus {
        target_name: target.target_name.clone(),
        records: records.clone(),
        hashes: hashes.clone(),
        localization,
    })
}

fn validate_target(target: &CoreCaldusDevelopmentTarget, hashes: &CoreCaldusHashes) -> Result<()> {
    if target.schema_version != SCHEMA_VERSION
        || target.target_kind != CoreCaldusTargetKind::UnpromotedCaldusSubset
        || target.target_name != CORE_CALDUS_TARGET_NAME
    {
        bail!("Caldus target identity is not the approved unpromoted target");
    }
    require_exact_ids(&target.required_boss_ids, &BOSS_IDS, "boss")?;
    require_exact_ids(&target.required_pattern_ids, &PATTERN_IDS, "pattern")?;
    require_exact_ids(&target.required_exit_ids, &EXIT_IDS, "exit")?;
    require_exact_ids(&target.required_asset_ids, &ASSET_IDS, "asset")?;
    require_exact_ids(
        &target.required_localization_keys,
        &COPY_KEYS,
        "localization",
    )?;
    if target.expected_records_blake3 != hashes.records_blake3
        || target.expected_assets_blake3 != hashes.assets_blake3
        || target.expected_localization_blake3 != hashes.localization_blake3
    {
        bail!("Caldus source hash drifted from the reviewed target");
    }
    Ok(())
}

fn validate_records(
    target: &CoreCaldusDevelopmentTarget,
    records: &CoreCaldusRecords,
    rooms: &CoreDevelopmentEncounterRooms,
    items: &CompiledProductionItemCatalog,
    progression: &CoreDevelopmentProgression,
) -> Result<()> {
    if records.schema_version != SCHEMA_VERSION
        || records.bosses.len() != 1
        || records.patterns.len() != 4
        || records.exits.len() != 1
        || records.room_bindings.len() != 1
    {
        bail!(
            "Caldus records require exactly one boss, four patterns, one exit, and one B6 binding"
        );
    }
    require_exact_ids(
        &records
            .bosses
            .iter()
            .map(|record| record.header.id.clone())
            .collect::<Vec<_>>(),
        &BOSS_IDS,
        "record boss",
    )?;
    require_exact_ids(
        &records
            .patterns
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>(),
        &PATTERN_IDS,
        "record pattern",
    )?;
    require_exact_ids(
        &records
            .exits
            .iter()
            .map(|record| record.header.id.clone())
            .collect::<Vec<_>>(),
        &EXIT_IDS,
        "record exit",
    )?;
    if records.bosses[0].pattern_ids != target.required_pattern_ids {
        bail!("Caldus pattern order drifted from the target allowlist");
    }
    validate_boss(&records.bosses[0])?;
    validate_patterns(&records.patterns)?;
    validate_exit(&records.exits[0])?;
    validate_binding(records, rooms)?;
    if !items.reward_tables().contains_key("reward.boss_caldus") {
        bail!("Caldus reward profile is missing from the validated Core item catalog");
    }
    let xp = progression
        .xp_profiles()
        .iter()
        .find(|profile| profile.header.id.as_str() == "xp.boss_caldus")
        .context("Caldus XP profile is missing")?;
    if xp.base_xp != 450 || xp.first_account_clear_bonus_basis_points != 5_000 {
        bail!("Caldus XP profile drifted from 450 plus the 50 percent first-clear bonus");
    }
    if !progression.source_bindings().iter().any(|binding| {
        binding.source_id.as_str() == "boss.sir_caldus"
            && binding.xp_profile_id.as_str() == "xp.boss_caldus"
            && binding.authored_core_enabled
    }) {
        bail!("Caldus XP source binding is missing or disabled");
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn validate_boss(boss: &content_schema::CoreCaldusBossRecord) -> Result<()> {
    validate_header(
        &boss.header,
        "boss.sir_caldus",
        &["sprite.boss.sir_caldus", "portrait.boss.sir_caldus"],
        &["boss", "major_boss", "bell", "wipeable_core"],
        "ENC-010",
    )?;
    let lock = &boss.participant_lock;
    if boss.arena_id.as_str() != "arena.boss.caldus_01"
        || boss.reward_profile_id.as_str() != "reward.boss_caldus"
        || boss.xp_profile_id.as_str() != "xp.boss_caldus"
        || boss.exit_id.as_str() != "portal.exit.dungeon.bell_sepulcher"
        || boss.base_health != 7_200
        || boss.additional_participant_health_basis_points != 7_200
        || boss.armor != 10
        || boss.recommended_level != 10
        || boss.recommended_item_level != 8
        || boss.target_solo_duration_minimum_milliseconds != 150_000
        || boss.target_solo_duration_maximum_milliseconds != 210_000
        || boss.collision_radius_milli_tiles != 700
        || boss.hurtbox_radius_milli_tiles != 620
        || boss.contact_damage != 0
        || boss.resistance_basis_points != 0
        || boss.spawn != point(9_000, 9_000)
        || boss.stage != point(2_500, 9_000)
        || boss.arena_center != point(9_000, 9_000)
        || boss.group_anchors != [point(2_500, 6_000), point(2_500, 12_000)]
        || boss.charge_endpoints
            != [
                point(1_000, 9_000),
                point(17_000, 9_000),
                point(9_000, 1_000),
                point(9_000, 17_000),
            ]
        || lock.load_timeout_milliseconds != 10_000
        || lock.ready_countdown_milliseconds != 5_000
        || lock.introduction_milliseconds != 2_500
        || lock.empty_reset_milliseconds != 5_000
        || (
            lock.minimum_locked_participants,
            lock.maximum_locked_participants,
            lock.runtime_capacity,
        ) != (1, 8, 1)
        || lock.safe_entrance_radius_milli_tiles != 3_000
        || lock.late_entry_allowed
        || lock.death_or_recall_rescales
        || !lock.recall_allowed
        || (
            boss.phase_two_threshold_percent,
            boss.phase_three_threshold_percent,
            boss.low_health_threshold_percent,
        ) != (70, 35, 20)
        || boss.phase_break_milliseconds != 4_000
        || boss.break_incoming_damage_basis_points != 12_500
        || boss.soft_enrage_milliseconds != 360_000
        || boss.soft_enrage_downtime_basis_points != 8_500
        || boss.stationary_phase_numbers != [1, 3]
        || !boss.phase_two_movement_is_charge_and_center_return_only
        || !boss.authored_core_enabled
    {
        bail!("Caldus scalar, geometry, participant-lock, phase, or movement contract drifted");
    }
    if boss.phase_one.loop_milliseconds != 7_800
        || boss.phase_one.shield_starts_milliseconds != [0, 1_800, 3_600]
        || boss.phase_one.bell_ring_start_milliseconds != 6_000
        || (
            boss.phase_one.ring_gap_initial_index,
            boss.phase_one.ring_gap_advance,
        ) != (0, 5)
        || boss.phase_two.loop_milliseconds != 15_000
        || boss.phase_two.charge_starts_milliseconds != [0, 7_500]
        || boss.phase_two.shield_starts_milliseconds != [3_000, 5_200, 10_500, 12_700]
        || (
            boss.phase_two.charge_direction_lock_milliseconds,
            boss.phase_two.charge_movement_start_milliseconds,
            boss.phase_two.charge_end_milliseconds,
        ) != (700, 1_000, 1_550)
        || boss.phase_two.center_return_speed_milli_tiles_per_second != 2_000
        || boss.phase_two.center_stop_radius_milli_tiles != 250
        || boss.phase_three.loop_milliseconds != 8_000
        || boss.phase_three.low_health_loop_milliseconds != 7_200
        || boss.phase_three.preview_windows_milliseconds != [[0, 600], [600, 1_200], [1_200, 1_800]]
        || boss.phase_three.ring_emissions_milliseconds != [2_200, 3_000, 3_800]
        || boss.phase_three.shield_start_milliseconds != 6_000
    {
        bail!("Caldus authored scheduler drifted");
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn validate_patterns(patterns: &[content_schema::CoreCaldusPatternRecord]) -> Result<()> {
    for (index, pattern) in patterns.iter().enumerate() {
        let id = PATTERN_IDS[index];
        if pattern.id.as_str() != id
            || pattern.owner_id.as_str() != "boss.sir_caldus"
            || pattern.telegraph_id.as_str() != format!("{id}.telegraph")
            || pattern.audio_cue_id.as_str() != format!("{id}.warning")
            || pattern.major_audio_cue_id.as_str() != format!("{id}.warning.major")
            || pattern.acceleration_milli_tiles_per_second_squared != 0
            || pattern.pierces_players
            || !pattern.statuses.is_empty()
            || !pattern.cancel_on_phase_change
        {
            bail!("Caldus pattern {id} normalized metadata drifted");
        }
    }
    let shield = &patterns[0];
    if shield.damage_type != DamageType::Physical
        || shield.damage_band != CoreCaldusDamageBand::Major
        || (shield.raw_damage, shield.threat_cost) != (24, 5)
        || shield.counterplay != CoreCaldusCounterplay::Strafe
        || shield.memory_family != CoreCaldusMemoryFamily::FanProjectile
        || shield.disposition != CoreCaldusDisposition::ConsumeOnPlayerOrSolid
        || shield.attack_group_rule != CoreCaldusAttackGroupRule::DistinctProjectileHitGroups
        || !shield.fevered_repeat_eligible
        || shield.maximum_active_instances != 25
        || !matches!(&shield.payload, CoreCaldusPatternPayload::ShieldArc { warning_milliseconds: 650, projectile_count: 5, total_arc_milli_degrees: 60_000, projectile_speed_milli_tiles_per_second: 7_000, range_milli_tiles: 17_500, projectile_radius_milli_tiles: 120, group_target_thresholds, group_stagger_milliseconds: 400 } if group_target_thresholds == &[[1,1],[4,2],[7,3]])
    {
        bail!("Caldus Shield Arc drifted");
    }
    let ring = &patterns[1];
    if ring.damage_type != DamageType::Veil
        || ring.damage_band != CoreCaldusDamageBand::Major
        || (ring.raw_damage, ring.threat_cost) != (32, 15)
        || ring.counterplay != CoreCaldusCounterplay::FollowGap
        || ring.memory_family != CoreCaldusMemoryFamily::RadialProjectile
        || ring.disposition != CoreCaldusDisposition::ConsumeOnPlayerOrSolid
        || ring.attack_group_rule != CoreCaldusAttackGroupRule::DistinctProjectileHitGroups
        || !ring.fevered_repeat_eligible
        || ring.maximum_active_instances != 45
        || !matches!(
            ring.payload,
            CoreCaldusPatternPayload::BellRing {
                warning_milliseconds: 800,
                index_count: 18,
                omitted_adjacent_count: 3,
                projectile_speed_milli_tiles_per_second: 5_000,
                range_milli_tiles: 20_000,
                projectile_radius_milli_tiles: 130
            }
        )
    {
        bail!("Caldus Bell Ring drifted");
    }
    let charge = &patterns[2];
    if charge.damage_type != DamageType::Physical
        || charge.damage_band != CoreCaldusDamageBand::Severe
        || (charge.raw_damage, charge.threat_cost) != (48, 18)
        || charge.counterplay != CoreCaldusCounterplay::LeaveTelegraph
        || charge.memory_family != CoreCaldusMemoryFamily::ChargeOrContact
        || charge.disposition != CoreCaldusDisposition::OneContactHitPerCast
        || charge.attack_group_rule != CoreCaldusAttackGroupRule::OneContactHitPerCast
        || charge.fevered_repeat_eligible
        || charge.maximum_active_instances != 1
        || !matches!(
            charge.payload,
            CoreCaldusPatternPayload::ChargeLane {
                warning_milliseconds: 1_000,
                width_milli_tiles: 1_200,
                travel_milli_tiles: 6_500,
                travel_milliseconds: 550,
                maximum_hits_per_player_per_cast: 1
            }
        )
    {
        bail!("Caldus Charge Lane drifted");
    }
    let stop = &patterns[3];
    if stop.damage_type != DamageType::Physical
        || stop.damage_band != CoreCaldusDamageBand::Major
        || (stop.raw_damage, stop.threat_cost) != (28, 12)
        || stop.counterplay != CoreCaldusCounterplay::FollowGap
        || stop.memory_family != CoreCaldusMemoryFamily::RadialProjectile
        || stop.disposition != CoreCaldusDisposition::ConsumeOnPlayerOrSolid
        || stop.attack_group_rule != CoreCaldusAttackGroupRule::DistinctProjectileHitGroups
        || !stop.fevered_repeat_eligible
        || stop.maximum_active_instances != 12
        || !matches!(&stop.payload, CoreCaldusPatternPayload::ChargeStopRing { parent_pattern_id, index_count: 14, omitted_adjacent_count: 2, projectile_speed_milli_tiles_per_second: 5_000, range_milli_tiles: 18_000, projectile_radius_milli_tiles: 130 } if parent_pattern_id.as_str() == "boss.caldus.charge_lane")
    {
        bail!("Caldus Charge Stop Ring drifted");
    }
    Ok(())
}

fn validate_exit(exit: &content_schema::CoreCaldusExitRecord) -> Result<()> {
    validate_header(
        &exit.header,
        "portal.exit.dungeon.bell_sepulcher",
        &["sprite.portal.exit.dungeon.bell_sepulcher"],
        &[
            "portal",
            "dungeon_exit",
            "successful_extraction",
            "requires_committed_boss_reward",
        ],
        "CONT-BOSS-001",
    )?;
    if exit.arena_id.as_str() != "arena.boss.caldus_01"
        || exit.boss_id.as_str() != "boss.sir_caldus"
        || exit.required_reward_profile_id.as_str() != "reward.boss_caldus"
        || exit.point != point(2_500, 9_000)
        || exit.destination_content_id.as_str() != "hub.lantern_halls_01"
        || exit.arrival != CoreCaldusSafeArrival::HallDefault
        || !exit.requires_committed_extraction_receipt
        || !exit.authored_core_enabled
    {
        bail!("Caldus stable exit drifted");
    }
    Ok(())
}

fn validate_binding(
    records: &CoreCaldusRecords,
    rooms: &CoreDevelopmentEncounterRooms,
) -> Result<()> {
    let binding = &records.room_bindings[0];
    if binding.layout_id.as_str() != "layout.core_private_life_01"
        || binding.node_id != "B6"
        || binding.arena_id.as_str() != "arena.boss.caldus_01"
        || binding.boss_id.as_str() != "boss.sir_caldus"
        || binding.reward_profile_id.as_str() != "reward.boss_caldus"
        || binding.exit_id.as_str() != "portal.exit.dungeon.bell_sepulcher"
    {
        bail!("Caldus B6 binding drifted");
    }
    let node = rooms
        .fixed_layout()
        .nodes
        .iter()
        .find(|node| node.node_id == "B6")
        .context("03D fixed layout is missing B6")?;
    if node.room_template_id.as_str() != "arena.boss.caldus_01" || node.encounter.is_some() {
        bail!("B6 must remain the major-boss-owned arena rather than an ordinary room encounter");
    }
    let room = rooms
        .rooms()
        .iter()
        .find(|room| room.header.id.as_str() == "arena.boss.caldus_01")
        .context("03D Bell rooms are missing the Caldus arena")?;
    let boss_anchor = room
        .anchors
        .iter()
        .find(|anchor| anchor.id == "boss")
        .context("Caldus arena is missing its boss anchor")?;
    let stage_anchor = room
        .anchors
        .iter()
        .find(|anchor| anchor.id == "stage")
        .context("Caldus arena is missing its stage anchor")?;
    if boss_anchor.point != records.bosses[0].spawn
        || boss_anchor.bound_content_id.as_ref().map(ContentId::as_str) != Some("boss.sir_caldus")
        || stage_anchor.point != records.bosses[0].stage
        || stage_anchor.point != records.exits[0].point
    {
        bail!("Caldus B6 anchors do not match boss spawn and stable exit");
    }
    Ok(())
}

fn validate_assets(
    target: &CoreCaldusDevelopmentTarget,
    records: &CoreCaldusRecords,
    assets: &CoreCaldusAssetManifest,
) -> Result<()> {
    if assets.schema_version != SCHEMA_VERSION || assets.assets.len() != ASSET_IDS.len() {
        bail!("Caldus asset manifest count or schema drifted");
    }
    require_exact_ids(
        &assets
            .assets
            .iter()
            .map(|asset| asset.asset_id.clone())
            .collect::<Vec<_>>(),
        &ASSET_IDS,
        "manifest asset",
    )?;
    if target.required_asset_ids
        != assets
            .assets
            .iter()
            .map(|asset| asset.asset_id.clone())
            .collect::<Vec<_>>()
    {
        bail!("Caldus asset target and manifest order differ");
    }
    let expected_kinds = [
        CoreCaldusAssetKind::BossSilhouette,
        CoreCaldusAssetKind::BossPortrait,
        CoreCaldusAssetKind::Telegraph,
        CoreCaldusAssetKind::WarningAudio,
        CoreCaldusAssetKind::MajorWarningAudio,
        CoreCaldusAssetKind::Telegraph,
        CoreCaldusAssetKind::WarningAudio,
        CoreCaldusAssetKind::MajorWarningAudio,
        CoreCaldusAssetKind::Telegraph,
        CoreCaldusAssetKind::WarningAudio,
        CoreCaldusAssetKind::MajorWarningAudio,
        CoreCaldusAssetKind::Telegraph,
        CoreCaldusAssetKind::WarningAudio,
        CoreCaldusAssetKind::MajorWarningAudio,
        CoreCaldusAssetKind::ExitMarker,
    ];
    for (index, asset) in assets.assets.iter().enumerate() {
        let expected_source = match index {
            0 | 1 => "boss.sir_caldus",
            2..=4 => PATTERN_IDS[0],
            5..=7 => PATTERN_IDS[1],
            8..=10 => PATTERN_IDS[2],
            11..=13 => PATTERN_IDS[3],
            14 => EXIT_IDS[0],
            _ => unreachable!(),
        };
        if asset.kind != expected_kinds[index] || asset.source_record_id.as_str() != expected_source
        {
            bail!("Caldus asset kind or source drifted at index {index}");
        }
    }
    let header_assets = records.bosses[0]
        .header
        .asset_ids
        .iter()
        .chain(records.exits[0].header.asset_ids.iter())
        .map(ContentId::as_str)
        .collect::<Vec<_>>();
    if header_assets != [ASSET_IDS[0], ASSET_IDS[1], ASSET_IDS[14]] {
        bail!("Caldus record asset references drifted");
    }
    Ok(())
}

fn validate_copy(
    target: &CoreCaldusDevelopmentTarget,
    records: &CoreCaldusRecords,
    copy: &CoreCaldusCopyFile,
) -> Result<BTreeMap<String, String>> {
    if copy.schema_version != SCHEMA_VERSION
        || copy.locale != "en-US"
        || copy.entries.len() != COPY_KEYS.len()
    {
        bail!("Caldus localization schema, locale, or count drifted");
    }
    let keys = copy
        .entries
        .iter()
        .map(|entry| entry.key.clone())
        .collect::<Vec<_>>();
    require_exact_ids(&keys, &COPY_KEYS, "copy")?;
    if keys != target.required_localization_keys
        || copy
            .entries
            .iter()
            .any(|entry| entry.value.trim().is_empty())
        || records.bosses[0].header.localization_name_key != keys[0]
        || records.bosses[0].header.localization_description_key != keys[1]
        || records.exits[0].header.localization_name_key != keys[2]
        || records.exits[0].header.localization_description_key != keys[3]
    {
        bail!("Caldus localization references or values drifted");
    }
    Ok(copy
        .entries
        .iter()
        .map(|entry| (entry.key.to_string(), entry.value.clone()))
        .collect())
}

fn validate_header(
    header: &content_schema::CoreDevelopmentHeader,
    id: &str,
    assets: &[&str],
    tags: &[&str],
    source: &str,
) -> Result<()> {
    if header.id.as_str() != id
        || header.schema_version != SCHEMA_VERSION
        || !header.enabled
        || header.earliest_release_stage != ReleaseStage::Core
        || header
            .asset_ids
            .iter()
            .map(ContentId::as_str)
            .collect::<Vec<_>>()
            != assets
        || header.tags.iter().map(String::as_str).collect::<Vec<_>>() != tags
        || header.source_document_feature_id != source
    {
        bail!("Caldus header {id} drifted");
    }
    Ok(())
}

fn require_exact_ids(actual: &[ContentId], expected: &[&str], domain: &str) -> Result<()> {
    if actual.iter().map(ContentId::as_str).collect::<Vec<_>>() != expected {
        bail!("Caldus {domain} allowlist drifted");
    }
    Ok(())
}

const fn point(x: i32, y: i32) -> MilliTilePoint {
    MilliTilePoint { x, y }
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        target: CoreCaldusDevelopmentTarget,
        records: CoreCaldusRecords,
        assets: CoreCaldusAssetManifest,
        copy: CoreCaldusCopyFile,
        hashes: CoreCaldusHashes,
        rooms: CoreDevelopmentEncounterRooms,
        items: CompiledProductionItemCatalog,
        progression: CoreDevelopmentProgression,
    }

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn case() -> Case {
        let root = content_root();
        let target: CoreCaldusDevelopmentTarget =
            crate::read_json(&root.join(CORE_CALDUS_TARGET_PATH)).expect("target");
        let records = crate::read_json(&root.join(CORE_CALDUS_RECORDS_PATH)).expect("records");
        let assets = crate::read_json(&root.join(CORE_CALDUS_ASSETS_PATH)).expect("assets");
        let copy = crate::read_json(&root.join(CORE_CALDUS_COPY_PATH)).expect("copy");
        let hashes = CoreCaldusHashes {
            records_blake3: target.expected_records_blake3.clone(),
            assets_blake3: target.expected_assets_blake3.clone(),
            localization_blake3: target.expected_localization_blake3.clone(),
        };
        Case {
            target,
            records,
            assets,
            copy,
            hashes,
            rooms: load_core_development_encounter_rooms(&root).expect("rooms"),
            items: load_core_development_items(&root).expect("items"),
            progression: load_core_development_progression(&root).expect("progression"),
        }
    }

    fn compile(case: &Case) -> Result<CoreDevelopmentCaldus> {
        compile_core_development_caldus(
            &case.target,
            &case.records,
            &case.assets,
            &case.copy,
            &case.hashes,
            &case.rooms,
            &case.items,
            &case.progression,
        )
    }

    #[test]
    fn checked_in_caldus_content_is_exact_and_unpromoted() {
        let compiled = load_core_development_caldus(&content_root()).expect("Caldus content");
        assert_eq!(compiled.target_name(), CORE_CALDUS_TARGET_NAME);
        assert_eq!(compiled.boss().header.id.as_str(), "boss.sir_caldus");
        assert_eq!(compiled.patterns().len(), 4);
        assert_eq!(compiled.exit().arrival, CoreCaldusSafeArrival::HallDefault);
        assert_eq!(compiled.room_binding().node_id, "B6");
        assert_eq!(
            (1..=8)
                .map(|count| compiled
                    .scaled_maximum_health(count)
                    .expect("scaled health"))
                .collect::<Vec<_>>(),
            [
                7_200, 12_384, 17_568, 22_752, 27_936, 33_120, 38_304, 43_488
            ]
        );
        assert!(compiled.scaled_maximum_health(0).is_err());
        assert!(compiled.scaled_maximum_health(9).is_err());
        assert_eq!(
            compiled.localized("boss.sir_caldus.name"),
            Some("Sir Caldus, Bell-Bound Knight")
        );
    }

    #[test]
    fn caldus_schema_rejects_release_metadata() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../../content/core_dev/caldus.json"))
                .expect("target JSON");
        value.as_object_mut().expect("object").insert(
            "content_version".to_owned(),
            serde_json::json!("core.1.0.0"),
        );
        assert!(serde_json::from_value::<CoreCaldusDevelopmentTarget>(value).is_err());
    }

    #[test]
    fn caldus_target_hash_and_boss_mutations_fail_closed() {
        let mut changed = case();
        changed.hashes.records_blake3 = "0".repeat(64);
        assert!(compile(&changed).is_err());

        let mut changed = case();
        changed.records.bosses[0].base_health += 1;
        assert!(compile(&changed).is_err());

        let mut changed = case();
        changed.records.bosses[0].phase_two.charge_end_milliseconds += 1;
        assert!(compile(&changed).is_err());

        let mut changed = case();
        changed.records.bosses[0]
            .participant_lock
            .late_entry_allowed = true;
        assert!(compile(&changed).is_err());
    }

    #[test]
    fn every_caldus_pattern_domain_is_strict() {
        let mut changed = case();
        changed.records.patterns[0].maximum_active_instances += 1;
        assert!(compile(&changed).is_err());

        let mut changed = case();
        changed.records.patterns[1].raw_damage += 1;
        assert!(compile(&changed).is_err());

        let mut changed = case();
        changed.records.patterns[2].counterplay = CoreCaldusCounterplay::Strafe;
        assert!(compile(&changed).is_err());

        let mut changed = case();
        let CoreCaldusPatternPayload::ChargeStopRing {
            omitted_adjacent_count,
            ..
        } = &mut changed.records.patterns[3].payload
        else {
            panic!("stop ring payload");
        };
        *omitted_adjacent_count += 1;
        assert!(compile(&changed).is_err());
    }

    #[test]
    fn caldus_binding_exit_assets_and_copy_are_strict() {
        let mut changed = case();
        changed.records.room_bindings[0].node_id = "B5".to_owned();
        assert!(compile(&changed).is_err());

        let mut changed = case();
        changed.records.exits[0].requires_committed_extraction_receipt = false;
        assert!(compile(&changed).is_err());

        let mut changed = case();
        changed.assets.assets[14].kind = CoreCaldusAssetKind::Telegraph;
        assert!(compile(&changed).is_err());

        let mut changed = case();
        changed.copy.entries[0].value.clear();
        assert!(compile(&changed).is_err());
    }
}
