//! Fail-closed compiler for the unpromoted `GB-M03-03D` encounter/room content layer.

use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use content_schema::{
    ContentId, CoreEncounterRank, CoreEncounterRoomAssetManifest, CoreEncounterRoomCopyFile,
    CoreEncounterRoomDevelopmentTarget, CoreEncounterRoomRecords, CoreEncounterRoomTargetKind,
    CoreEncounterSourceKind, CoreFixedLayoutRecord, CoreRoomDoorSide, CoreRoomTemplateRecord,
    CoreRoomVolumeGeometry, CoreRoomVolumeKind, ReleaseStage, SCHEMA_VERSION,
};

use crate::{
    CompiledProductionItemCatalog, ContentPackage, CoreDevelopmentProgression,
    first_playable_bell_reed, first_playable_chain_sentry, first_playable_drowned_pilgrim,
    load_core_development_items, load_core_development_progression,
};

pub const CORE_ENCOUNTER_ROOM_TARGET_NAME: &str = "core-dev-encounter-rooms";
pub const CORE_ENCOUNTER_ROOM_TARGET_PATH: &str = "core_dev/encounter_rooms.json";
pub const CORE_ENCOUNTER_ROOM_RECORDS_PATH: &str = "core_dev/encounter_rooms.records.json";
pub const CORE_ENCOUNTER_ROOM_ASSETS_PATH: &str = "core_dev/encounter_rooms.assets.json";
pub const CORE_ENCOUNTER_ROOM_COPY_PATH: &str = "core_dev/encounter_rooms.en-US.json";

const NORMAL_IDS: [&str; 6] = [
    "enemy.drowned_pilgrim",
    "enemy.mire_leech",
    "enemy.bell_reed",
    "enemy.bell_acolyte",
    "enemy.chain_sentry",
    "enemy.choir_skull",
];
const MINIBOSS_IDS: [&str; 2] = ["miniboss.sepulcher_knight", "miniboss.choir_abbot"];
const PATTERN_IDS: [&str; 11] = [
    "pattern.enemy.drowned_pilgrim.fan",
    "pattern.enemy.mire_leech.charge",
    "pattern.enemy.bell_reed.gap_ring",
    "pattern.enemy.bell_acolyte.alternating_fan",
    "pattern.enemy.chain_sentry.cross_lanes",
    "pattern.enemy.choir_skull.rotor",
    "miniboss.sepulcher_knight.charge_lane",
    "miniboss.sepulcher_knight.stop_ring",
    "miniboss.sepulcher_knight.shield_fan",
    "miniboss.choir_abbot.rotor",
    "miniboss.choir_abbot.recovery_ring",
];
const ROOM_IDS: [&str; 9] = [
    "room.bell.vestibule_01",
    "room.bell.cross_01",
    "room.bell.nave_01",
    "room.bell.bridge_01",
    "room.bell.choir_01",
    "room.bell.knight_01",
    "room.bell.rest_01",
    "room.bell.secret_01",
    "arena.boss.caldus_01",
];
const ROOM_DIMENSIONS: [(u32, u32); 9] = [
    (13_000, 11_000),
    (17_000, 17_000),
    (15_000, 21_000),
    (23_000, 11_000),
    (19_000, 15_000),
    (19_000, 15_000),
    (15_000, 13_000),
    (11_000, 11_000),
    (18_000, 18_000),
];
const PACK_IDS: [&str; 1] = ["pack.bell.01"];
const LAYOUT_IDS: [&str; 1] = ["layout.core_private_life_01"];
const MAIN_CHAIN: [&str; 7] = ["B0", "B1", "B2", "B3", "B4", "B5", "B6"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreEncounterRoomHashes {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

/// Immutable compiled view. It intentionally exposes no release or promotion operation.
#[derive(Debug, Clone)]
pub struct CoreDevelopmentEncounterRooms {
    target_name: String,
    records: CoreEncounterRoomRecords,
    hashes: CoreEncounterRoomHashes,
    localization: std::collections::BTreeMap<String, String>,
}

impl CoreDevelopmentEncounterRooms {
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub fn roster(&self) -> &[content_schema::CoreEncounterRosterMember] {
        &self.records.roster
    }

    #[must_use]
    pub fn rooms(&self) -> &[CoreRoomTemplateRecord] {
        &self.records.rooms
    }

    #[must_use]
    pub fn pack_bell_01(&self) -> &content_schema::CoreEncounterPackRecord {
        &self.records.packs[0]
    }

    #[must_use]
    pub fn fixed_layout(&self) -> &CoreFixedLayoutRecord {
        &self.records.layouts[0]
    }

    #[must_use]
    pub const fn hashes(&self) -> &CoreEncounterRoomHashes {
        &self.hashes
    }

    #[must_use]
    pub fn localized(&self, key: &str) -> Option<&str> {
        self.localization.get(key).map(String::as_str)
    }
}

pub fn load_core_development_encounter_rooms(root: &Path) -> Result<CoreDevelopmentEncounterRooms> {
    let (source, _) = crate::load_and_validate(root)
        .context("encounter-room compilation requires valid fp.1.0.0")?;
    let items = load_core_development_items(root)
        .context("encounter-room reward references require production item content")?;
    let progression = load_core_development_progression(root)
        .context("encounter-room XP references require Core progression content")?;
    let target: CoreEncounterRoomDevelopmentTarget =
        crate::read_json(&root.join(CORE_ENCOUNTER_ROOM_TARGET_PATH))?;
    let records: CoreEncounterRoomRecords =
        crate::read_json(&root.join(CORE_ENCOUNTER_ROOM_RECORDS_PATH))?;
    let assets: CoreEncounterRoomAssetManifest =
        crate::read_json(&root.join(CORE_ENCOUNTER_ROOM_ASSETS_PATH))?;
    let copy: CoreEncounterRoomCopyFile =
        crate::read_json(&root.join(CORE_ENCOUNTER_ROOM_COPY_PATH))?;
    let hashes = CoreEncounterRoomHashes {
        records_blake3: hash_file(&root.join(CORE_ENCOUNTER_ROOM_RECORDS_PATH))?,
        assets_blake3: hash_file(&root.join(CORE_ENCOUNTER_ROOM_ASSETS_PATH))?,
        localization_blake3: hash_file(&root.join(CORE_ENCOUNTER_ROOM_COPY_PATH))?,
    };
    compile_core_development_encounter_rooms(
        &source,
        &items,
        &progression,
        &target,
        &records,
        &assets,
        &copy,
        &hashes,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn compile_core_development_encounter_rooms(
    source: &ContentPackage,
    items: &CompiledProductionItemCatalog,
    progression: &CoreDevelopmentProgression,
    target: &CoreEncounterRoomDevelopmentTarget,
    records: &CoreEncounterRoomRecords,
    assets: &CoreEncounterRoomAssetManifest,
    copy: &CoreEncounterRoomCopyFile,
    hashes: &CoreEncounterRoomHashes,
) -> Result<CoreDevelopmentEncounterRooms> {
    validate_target(target, hashes)?;
    validate_records(source, items, progression, target, records)?;
    validate_assets(target, records, assets)?;
    validate_copy(target, records, copy)?;
    Ok(CoreDevelopmentEncounterRooms {
        target_name: target.target_name.clone(),
        records: records.clone(),
        hashes: hashes.clone(),
        localization: copy
            .entries
            .iter()
            .map(|entry| (entry.key.to_string(), entry.value.clone()))
            .collect(),
    })
}

fn validate_target(
    target: &CoreEncounterRoomDevelopmentTarget,
    hashes: &CoreEncounterRoomHashes,
) -> Result<()> {
    if target.schema_version != SCHEMA_VERSION
        || target.target_kind != CoreEncounterRoomTargetKind::UnpromotedEncounterRoomSubset
        || target.target_name != CORE_ENCOUNTER_ROOM_TARGET_NAME
    {
        bail!("Core encounter-room target identity is not the approved unpromoted target");
    }
    require_exact_ids(
        &target.required_normal_enemy_ids,
        &NORMAL_IDS,
        "normal enemy",
    )?;
    require_exact_ids(&target.required_miniboss_ids, &MINIBOSS_IDS, "miniboss")?;
    require_exact_ids(&target.required_pattern_ids, &PATTERN_IDS, "pattern")?;
    require_exact_ids(&target.required_room_template_ids, &ROOM_IDS, "room")?;
    require_exact_ids(&target.required_pack_ids, &PACK_IDS, "pack")?;
    require_exact_ids(&target.required_layout_ids, &LAYOUT_IDS, "layout")?;
    require_unique(&target.required_asset_ids, "target asset")?;
    require_unique(
        &target.required_localization_keys,
        "target localization key",
    )?;
    for (expected, actual, domain) in [
        (
            &target.expected_records_blake3,
            &hashes.records_blake3,
            "records",
        ),
        (
            &target.expected_assets_blake3,
            &hashes.assets_blake3,
            "assets",
        ),
        (
            &target.expected_localization_blake3,
            &hashes.localization_blake3,
            "localization",
        ),
    ] {
        if expected != actual {
            bail!(
                "Core encounter-room {domain} BLAKE3 mismatch: expected {expected}, actual {actual}"
            );
        }
    }
    Ok(())
}

fn validate_records(
    source: &ContentPackage,
    items: &CompiledProductionItemCatalog,
    progression: &CoreDevelopmentProgression,
    target: &CoreEncounterRoomDevelopmentTarget,
    records: &CoreEncounterRoomRecords,
) -> Result<()> {
    if records.schema_version != SCHEMA_VERSION {
        bail!("Core encounter-room records use an unsupported schema version");
    }
    let roster_ids = records
        .roster
        .iter()
        .map(|record| record.header.id.as_str())
        .collect::<Vec<_>>();
    let mut expected_roster = NORMAL_IDS.to_vec();
    expected_roster.extend(MINIBOSS_IDS);
    if roster_ids != expected_roster {
        bail!("Core encounter roster must be the exact ordered 6/2 manifest");
    }
    require_unique_headers(
        records.roster.iter().map(|record| &record.header.id),
        "roster",
    )?;
    first_playable_drowned_pilgrim(source)?;
    first_playable_bell_reed(source)?;
    first_playable_chain_sentry(source)?;
    let xp_ids = progression
        .xp_profiles()
        .iter()
        .map(|profile| profile.header.id.as_str())
        .collect::<BTreeSet<_>>();
    for (index, member) in records.roster.iter().enumerate() {
        validate_header(&member.header)?;
        let normal = index < NORMAL_IDS.len();
        if member.rank
            != if normal {
                CoreEncounterRank::Normal
            } else {
                CoreEncounterRank::Miniboss
            }
        {
            bail!("{} has the wrong encounter rank", member.header.id);
        }
        let reused = matches!(
            member.header.id.as_str(),
            "enemy.drowned_pilgrim" | "enemy.bell_reed" | "enemy.chain_sentry"
        );
        let expected_source = if reused {
            CoreEncounterSourceKind::ImmutableFirstPlayable
        } else {
            CoreEncounterSourceKind::AuthoredCore
        };
        if member.source_kind != expected_source || !member.authored_core_enabled {
            bail!("{} has an invalid Core source boundary", member.header.id);
        }
        if member.header.asset_ids.len() != 2 {
            bail!("{} must bind one sprite and one portrait", member.header.id);
        }
        if !items
            .reward_tables()
            .contains_key(member.reward_profile_id.as_str())
        {
            bail!(
                "{} references an unavailable reward profile",
                member.header.id
            );
        }
        if !xp_ids.contains(member.xp_profile_id.as_str()) {
            bail!("{} references an unavailable XP profile", member.header.id);
        }
    }
    let flattened_patterns = records
        .roster
        .iter()
        .flat_map(|member| member.required_pattern_ids.iter())
        .map(ContentId::as_str)
        .collect::<Vec<_>>();
    if flattened_patterns != PATTERN_IDS {
        bail!("Core encounter roster pattern closure is not exact");
    }

    validate_rooms(target, &records.rooms)?;
    validate_pack(&records.packs)?;
    validate_layout(&records.layouts, &records.rooms)?;
    Ok(())
}

fn validate_rooms(
    target: &CoreEncounterRoomDevelopmentTarget,
    rooms: &[CoreRoomTemplateRecord],
) -> Result<()> {
    if rooms.len() != ROOM_IDS.len() {
        bail!("Core encounter-room records require exactly nine Bell templates");
    }
    for (index, room) in rooms.iter().enumerate() {
        validate_header(&room.header)?;
        if room.header.id.as_str() != ROOM_IDS[index]
            || (room.width_milli_tiles, room.height_milli_tiles) != ROOM_DIMENSIONS[index]
            || !room.authored_core_enabled
            || room.header.asset_ids.len() != 1
            || !target
                .required_asset_ids
                .contains(&room.header.asset_ids[0])
        {
            bail!(
                "Bell room {} does not match its exact manifest row",
                room.header.id
            );
        }
        require_unique_strings(room.doors.iter().map(|door| door.id.as_str()), "room door")?;
        require_unique_strings(
            room.volumes.iter().map(|volume| volume.id.as_str()),
            "room volume",
        )?;
        require_unique_strings(
            room.anchors.iter().map(|anchor| anchor.id.as_str()),
            "room anchor",
        )?;
        for door in &room.doors {
            if door.width_milli_tiles != 3_000 {
                bail!(
                    "{} door {} must be exactly three tiles wide",
                    room.header.id,
                    door.id
                );
            }
            let edge_length = match door.side {
                CoreRoomDoorSide::North | CoreRoomDoorSide::South => room.width_milli_tiles,
                CoreRoomDoorSide::East | CoreRoomDoorSide::West => room.height_milli_tiles,
            };
            if door.offset_milli_tiles > edge_length {
                bail!("{} door {} lies outside its edge", room.header.id, door.id);
            }
        }
        for anchor in &room.anchors {
            if anchor.point.x < 0
                || anchor.point.y < 0
                || u32::try_from(anchor.point.x).unwrap_or(u32::MAX) > room.width_milli_tiles
                || u32::try_from(anchor.point.y).unwrap_or(u32::MAX) > room.height_milli_tiles
            {
                bail!("{} anchor {} is out of bounds", room.header.id, anchor.id);
            }
        }
        for volume in &room.volumes {
            validate_volume(room, volume)?;
        }
    }
    if !rooms[0].safe_noncombat || !rooms[6].safe_noncombat {
        bail!("only the authored vestibule/rest safety rows may be noncombat");
    }
    if rooms
        .iter()
        .enumerate()
        .any(|(index, room)| !matches!(index, 0 | 6) && room.safe_noncombat)
    {
        bail!("a combat/secret/boss Bell room was marked safe");
    }
    Ok(())
}

fn validate_volume(
    room: &CoreRoomTemplateRecord,
    volume: &content_schema::CoreRoomVolume,
) -> Result<()> {
    match (&volume.kind, &volume.geometry) {
        (
            CoreRoomVolumeKind::Solid | CoreRoomVolumeKind::DeepWater,
            CoreRoomVolumeGeometry::Rectangle { rectangle },
        ) => {
            let right = i64::from(rectangle.x) + i64::from(rectangle.width);
            let bottom = i64::from(rectangle.y) + i64::from(rectangle.height);
            if rectangle.x < 0
                || rectangle.y < 0
                || right > i64::from(room.width_milli_tiles)
                || bottom > i64::from(room.height_milli_tiles)
            {
                bail!("{} volume {} is out of bounds", room.header.id, volume.id);
            }
        }
        (
            CoreRoomVolumeKind::WalkableBoundary | CoreRoomVolumeKind::ObjectiveArea,
            CoreRoomVolumeGeometry::Circle { circle },
        ) => {
            if circle.radius == 0 || circle.center.x < 0 || circle.center.y < 0 {
                bail!(
                    "{} volume {} has an invalid circle",
                    room.header.id,
                    volume.id
                );
            }
        }
        (
            CoreRoomVolumeKind::PatternLane,
            CoreRoomVolumeGeometry::Polyline {
                width_milli_tiles,
                points,
            },
        ) => {
            if *width_milli_tiles == 0
                || points.len() < 2
                || points.iter().any(|point| {
                    point.x < 0
                        || point.y < 0
                        || u32::try_from(point.x).unwrap_or(u32::MAX) > room.width_milli_tiles
                        || u32::try_from(point.y).unwrap_or(u32::MAX) > room.height_milli_tiles
                })
            {
                bail!(
                    "{} volume {} has an invalid lane",
                    room.header.id,
                    volume.id
                );
            }
        }
        _ => bail!(
            "{} volume {} uses an incompatible kind and geometry",
            room.header.id,
            volume.id
        ),
    }
    Ok(())
}

fn validate_pack(packs: &[content_schema::CoreEncounterPackRecord]) -> Result<()> {
    if packs.len() != 1 || packs[0].header.id.as_str() != PACK_IDS[0] {
        bail!("Core microrealm requires exactly pack.bell.01");
    }
    let pack = &packs[0];
    validate_header(&pack.header)?;
    validate_members(
        &pack.members,
        &[("enemy.drowned_pilgrim", 6, 1), ("enemy.bell_reed", 2, 3)],
        12,
    )?;
    if pack.warning_milliseconds != 900 || !pack.simultaneous_spawn || !pack.authored_core_enabled {
        bail!("pack.bell.01 timing/spawn policy is not exact");
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn validate_layout(
    layouts: &[CoreFixedLayoutRecord],
    rooms: &[CoreRoomTemplateRecord],
) -> Result<()> {
    if layouts.len() != 1 || layouts[0].header.id.as_str() != LAYOUT_IDS[0] {
        bail!("Core private life requires exactly layout.core_private_life_01");
    }
    let layout = &layouts[0];
    validate_header(&layout.header)?;
    if layout
        .main_chain_node_ids
        .iter()
        .map(String::as_str)
        .ne(MAIN_CHAIN)
        || layout.nodes.len() != 9
        || layout.edges.len() != 6
        || layout.disabled_branch_node_ids != ["BB1", "BS1"]
        || layout.branches_enabled
        || layout.seeded_selection_enabled
        || !layout.authored_core_enabled
    {
        bail!("Core private-life graph is not the exact fixed branch-disabled layout");
    }
    require_unique_strings(
        layout.nodes.iter().map(|node| node.node_id.as_str()),
        "layout node",
    )?;
    let room_ids = rooms
        .iter()
        .map(|room| room.header.id.as_str())
        .collect::<BTreeSet<_>>();
    for node in &layout.nodes {
        if !room_ids.contains(node.room_template_id.as_str())
            || !matches!(node.rotation_degrees, 0 | 90 | 180 | 270)
        {
            bail!(
                "layout node {} has an invalid room or rotation",
                node.node_id
            );
        }
    }
    if layout
        .nodes
        .iter()
        .filter(|node| node.counts_toward_six_room_total)
        .count()
        != 6
    {
        bail!("Core layout must count exactly B1 through B6 as the six-room dungeon");
    }
    let expected_rooms = [
        "room.bell.vestibule_01",
        "room.bell.cross_01",
        "room.bell.nave_01",
        "room.bell.knight_01",
        "room.bell.rest_01",
        "room.bell.bridge_01",
        "arena.boss.caldus_01",
        "room.bell.choir_01",
        "room.bell.secret_01",
    ];
    if layout
        .nodes
        .iter()
        .map(|node| node.room_template_id.as_str())
        .ne(expected_rooms)
    {
        bail!("Core layout room assignments are not exact");
    }
    validate_node_encounter(
        &layout.nodes[1],
        &[("enemy.drowned_pilgrim", 6, 1), ("enemy.bell_reed", 2, 3)],
        12,
    )?;
    validate_node_encounter(
        &layout.nodes[2],
        &[
            ("enemy.drowned_pilgrim", 6, 1),
            ("enemy.bell_acolyte", 2, 3),
            ("enemy.choir_skull", 1, 4),
        ],
        16,
    )?;
    validate_node_encounter(
        &layout.nodes[3],
        &[("miniboss.sepulcher_knight", 1, 10)],
        10,
    )?;
    if layout.nodes[4].encounter.is_some()
        || layout.nodes[6].encounter.is_some()
        || layout.nodes[8].encounter.is_some()
    {
        bail!("rest, 03E boss arena, and disabled secret fixture must not construct 03D hostiles");
    }
    validate_node_encounter(
        &layout.nodes[5],
        &[
            ("enemy.drowned_pilgrim", 6, 1),
            ("enemy.chain_sentry", 1, 6),
        ],
        12,
    )?;
    validate_node_encounter(&layout.nodes[7], &[("miniboss.choir_abbot", 1, 10)], 10)?;
    for (index, edge) in layout.edges.iter().enumerate() {
        if edge.from_node_id != MAIN_CHAIN[index]
            || edge.to_node_id != MAIN_CHAIN[index + 1]
            || edge.corridor_width_milli_tiles != 3_000
            || edge.corridor_length_tiles != 4
        {
            bail!("Core layout main-chain corridor {} is not exact", index + 1);
        }
    }
    Ok(())
}

fn validate_node_encounter(
    node: &content_schema::CoreFixedLayoutNode,
    expected: &[(&str, u16, u16)],
    budget: u16,
) -> Result<()> {
    let encounter = node
        .encounter
        .as_ref()
        .with_context(|| format!("layout node {} is missing its encounter", node.node_id))?;
    validate_members(&encounter.members, expected, budget)?;
    if encounter.warning_milliseconds != 900 {
        bail!(
            "layout node {} must use the 900 ms group warning",
            node.node_id
        );
    }
    Ok(())
}

fn validate_members(
    actual: &[content_schema::CoreEncounterPackMember],
    expected: &[(&str, u16, u16)],
    budget: u16,
) -> Result<()> {
    let actual_values = actual
        .iter()
        .map(|member| (member.enemy_id.as_str(), member.count, member.threat_each))
        .collect::<Vec<_>>();
    if actual_values != expected {
        bail!("encounter composition is not exact");
    }
    let actual_budget = actual.iter().try_fold(0_u16, |total, member| {
        total
            .checked_add(member.count.saturating_mul(member.threat_each))
            .context("encounter budget overflow")
    })?;
    if actual_budget != budget {
        bail!("encounter budget {actual_budget} does not match exact budget {budget}");
    }
    Ok(())
}

fn validate_assets(
    target: &CoreEncounterRoomDevelopmentTarget,
    records: &CoreEncounterRoomRecords,
    assets: &CoreEncounterRoomAssetManifest,
) -> Result<()> {
    if assets.schema_version != SCHEMA_VERSION {
        bail!("Core encounter-room assets use an unsupported schema version");
    }
    let actual_ids = assets
        .assets
        .iter()
        .map(|asset| asset.asset_id.as_str())
        .collect::<Vec<_>>();
    if actual_ids
        != target
            .required_asset_ids
            .iter()
            .map(ContentId::as_str)
            .collect::<Vec<_>>()
    {
        bail!("Core encounter-room asset manifest does not match its exact allowlist");
    }
    require_unique_headers(
        assets.assets.iter().map(|asset| &asset.asset_id),
        "encounter-room asset",
    )?;
    let record_ids = records
        .roster
        .iter()
        .map(|record| record.header.id.as_str())
        .chain(records.rooms.iter().map(|record| record.header.id.as_str()))
        .chain(PATTERN_IDS)
        .collect::<BTreeSet<_>>();
    for asset in &assets.assets {
        if !record_ids.contains(asset.source_record_id.as_str()) {
            bail!("asset {} has an unresolved source record", asset.asset_id);
        }
        let id = asset.asset_id.as_str();
        let kind_matches = match asset.kind {
            content_schema::CoreEncounterRoomAssetKind::EnemySilhouette => {
                id.starts_with("sprite.enemy.")
            }
            content_schema::CoreEncounterRoomAssetKind::EnemyPortrait => {
                id.starts_with("portrait.enemy.")
            }
            content_schema::CoreEncounterRoomAssetKind::MinibossSilhouette => {
                id.starts_with("sprite.miniboss.")
            }
            content_schema::CoreEncounterRoomAssetKind::MinibossPortrait => {
                id.starts_with("portrait.miniboss.")
            }
            content_schema::CoreEncounterRoomAssetKind::RoomTilemap => {
                id.starts_with("tilemap.room.") || id.starts_with("tilemap.arena.")
            }
            content_schema::CoreEncounterRoomAssetKind::Telegraph => id.starts_with("telegraph."),
            content_schema::CoreEncounterRoomAssetKind::WarningAudio => {
                id.starts_with("audio.") && id.ends_with(".warning")
            }
        };
        if !kind_matches {
            bail!("asset {} has an incompatible typed role", asset.asset_id);
        }
    }
    Ok(())
}

fn validate_copy(
    target: &CoreEncounterRoomDevelopmentTarget,
    records: &CoreEncounterRoomRecords,
    copy: &CoreEncounterRoomCopyFile,
) -> Result<()> {
    if copy.schema_version != SCHEMA_VERSION || copy.locale != "en-US" {
        bail!("Core encounter-room copy must be schema 1 en-US");
    }
    let keys = copy
        .entries
        .iter()
        .map(|entry| entry.key.as_str())
        .collect::<Vec<_>>();
    if keys
        != target
            .required_localization_keys
            .iter()
            .map(ContentId::as_str)
            .collect::<Vec<_>>()
    {
        bail!("Core encounter-room localization does not match its exact allowlist");
    }
    require_unique_headers(
        copy.entries.iter().map(|entry| &entry.key),
        "localization key",
    )?;
    if copy
        .entries
        .iter()
        .any(|entry| entry.value.trim().is_empty())
    {
        bail!("Core encounter-room localization contains blank copy");
    }
    let header_keys = records
        .roster
        .iter()
        .map(|record| &record.header)
        .chain(records.rooms.iter().map(|record| &record.header))
        .chain(records.packs.iter().map(|record| &record.header))
        .chain(records.layouts.iter().map(|record| &record.header))
        .flat_map(|header| {
            [
                header.localization_name_key.as_str(),
                header.localization_description_key.as_str(),
            ]
        });
    let available = keys.into_iter().collect::<BTreeSet<_>>();
    if header_keys.into_iter().any(|key| !available.contains(key)) {
        bail!("Core encounter-room record references missing localization");
    }
    Ok(())
}

fn validate_header(header: &content_schema::CoreDevelopmentHeader) -> Result<()> {
    if header.schema_version != SCHEMA_VERSION
        || !header.enabled
        || header.earliest_release_stage != ReleaseStage::Core
        || header.source_document_feature_id.trim().is_empty()
        || header.tags.is_empty()
    {
        bail!("Core encounter-room header {} is invalid", header.id);
    }
    Ok(())
}

fn require_exact_ids(actual: &[ContentId], expected: &[&str], domain: &str) -> Result<()> {
    require_unique(actual, domain)?;
    if actual
        .iter()
        .map(ContentId::as_str)
        .ne(expected.iter().copied())
    {
        bail!("Core encounter-room target has an unauthorized {domain} allowlist");
    }
    Ok(())
}

fn require_unique(actual: &[ContentId], domain: &str) -> Result<()> {
    require_unique_headers(actual.iter(), domain)
}

fn require_unique_headers<'a>(
    values: impl IntoIterator<Item = &'a ContentId>,
    domain: &str,
) -> Result<()> {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.iter().copied().collect::<BTreeSet<_>>().len() != values.len() {
        bail!("Core encounter-room content contains duplicate {domain} IDs");
    }
    Ok(())
}

fn require_unique_strings<'a>(
    values: impl IntoIterator<Item = &'a str>,
    domain: &str,
) -> Result<()> {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.iter().copied().collect::<BTreeSet<_>>().len() != values.len() {
        bail!("Core encounter-room content contains duplicate {domain} IDs");
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

    struct Fixture {
        source: ContentPackage,
        items: CompiledProductionItemCatalog,
        progression: CoreDevelopmentProgression,
        target: CoreEncounterRoomDevelopmentTarget,
        records: CoreEncounterRoomRecords,
        assets: CoreEncounterRoomAssetManifest,
        copy: CoreEncounterRoomCopyFile,
        hashes: CoreEncounterRoomHashes,
    }

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn fixture() -> Fixture {
        let root = content_root();
        let (source, _) = crate::load_and_validate(&root).expect("FP source");
        let items = load_core_development_items(&root).expect("items");
        let progression = load_core_development_progression(&root).expect("progression");
        let target: CoreEncounterRoomDevelopmentTarget =
            crate::read_json(&root.join(CORE_ENCOUNTER_ROOM_TARGET_PATH)).expect("target");
        let records =
            crate::read_json(&root.join(CORE_ENCOUNTER_ROOM_RECORDS_PATH)).expect("records");
        let assets = crate::read_json(&root.join(CORE_ENCOUNTER_ROOM_ASSETS_PATH)).expect("assets");
        let copy = crate::read_json(&root.join(CORE_ENCOUNTER_ROOM_COPY_PATH)).expect("copy");
        let hashes = CoreEncounterRoomHashes {
            records_blake3: target.expected_records_blake3.clone(),
            assets_blake3: target.expected_assets_blake3.clone(),
            localization_blake3: target.expected_localization_blake3.clone(),
        };
        Fixture {
            source,
            items,
            progression,
            target,
            records,
            assets,
            copy,
            hashes,
        }
    }

    fn compile_fixture(fixture: &Fixture) -> Result<CoreDevelopmentEncounterRooms> {
        compile_core_development_encounter_rooms(
            &fixture.source,
            &fixture.items,
            &fixture.progression,
            &fixture.target,
            &fixture.records,
            &fixture.assets,
            &fixture.copy,
            &fixture.hashes,
        )
    }

    #[test]
    fn checked_in_encounter_rooms_compile_exactly() {
        let compiled = load_core_development_encounter_rooms(&content_root())
            .expect("checked-in encounter rooms");
        assert_eq!(compiled.roster().len(), 8);
        assert_eq!(compiled.rooms().len(), 9);
        assert_eq!(compiled.pack_bell_01().base_budget, 12);
        assert_eq!(compiled.fixed_layout().main_chain_node_ids, MAIN_CHAIN);
    }

    #[test]
    fn schema_rejects_promotion_metadata() {
        let mut value: serde_json::Value = serde_json::from_slice(
            &fs::read(content_root().join(CORE_ENCOUNTER_ROOM_TARGET_PATH)).expect("target"),
        )
        .expect("valid target JSON");
        value
            .as_object_mut()
            .expect("target object")
            .insert("release_stage".to_owned(), serde_json::json!("core"));
        let error = serde_json::from_value::<CoreEncounterRoomDevelopmentTarget>(value)
            .expect_err("promotion metadata must fail");
        assert!(error.to_string().contains("unknown field `release_stage`"));
    }

    #[test]
    fn compiler_rejects_roster_pattern_or_source_drift() {
        let mut case = fixture();
        case.records.roster[1].source_kind = CoreEncounterSourceKind::ImmutableFirstPlayable;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.roster[3].required_pattern_ids[0] =
            ContentId::parse("pattern.enemy.bell_acolyte.invented").expect("test ID");
        assert!(compile_fixture(&case).is_err());
    }

    #[test]
    fn compiler_rejects_room_geometry_and_layout_drift() {
        let mut case = fixture();
        case.records.rooms[1].width_milli_tiles += 1;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.layouts[0].branches_enabled = true;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        let invented_boss_encounter = case.records.layouts[0].nodes[1].encounter.clone();
        case.records.layouts[0].nodes[6].encounter = invented_boss_encounter;
        assert!(compile_fixture(&case).is_err());
    }

    #[test]
    fn compiler_rejects_pack_asset_and_copy_drift() {
        let mut case = fixture();
        case.records.packs[0].members[0].count = 5;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.assets.assets[0].source_record_id =
            ContentId::parse("enemy.not_core").expect("test ID");
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.assets.assets[1].kind = content_schema::CoreEncounterRoomAssetKind::EnemySilhouette;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.copy.entries[0].value.clear();
        assert!(compile_fixture(&case).is_err());
    }
}
