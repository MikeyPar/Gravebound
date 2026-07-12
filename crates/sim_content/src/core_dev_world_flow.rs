//! Fail-closed compiler for the unpromoted `GB-M03-03A` world-flow subset.

use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use content_schema::{
    ContentId, CoreDevelopmentHeader, CoreDisabledWorldSystem, CoreGrayboxAssetKind,
    CoreGrayboxAssetManifest, CoreHubRecord, CoreMapOrigin, CoreProhibitedCreation,
    CoreRoadPolyline, CoreWorldFlowCopyFile, CoreWorldFlowDevelopmentTarget, CoreWorldFlowRecords,
    CoreWorldFlowTargetKind, CoreWorldObjectGeometry, CoreWorldObjectRecord, CoreWorldRecord,
    CoreWorldTerrain, MilliTileCircle, MilliTilePoint, MilliTileRectangle, ReleaseStage,
    SCHEMA_VERSION,
};

pub const CORE_WORLD_FLOW_TARGET_NAME: &str = "core-dev-world-flow";
pub const CORE_WORLD_FLOW_TARGET_PATH: &str = "core_dev/world_flow.json";
pub const CORE_WORLD_FLOW_RECORDS_PATH: &str = "core_dev/world_flow.records.json";
pub const CORE_WORLD_FLOW_ASSETS_PATH: &str = "core_dev/world_flow.assets.json";
pub const CORE_WORLD_FLOW_COPY_PATH: &str = "core_dev/world_flow.en-US.json";

const HUB_IDS: [&str; 1] = ["hub.lantern_halls_01"];
const WORLD_IDS: [&str; 1] = ["world.core_microrealm_01"];
const OBJECT_IDS: [&str; 10] = [
    "landmark.lantern_fork",
    "landmark.realm_gate",
    "portal.dungeon.bell_sepulcher",
    "portal.return.lantern_halls",
    "spawn.hub.character_select_return",
    "station.memorial_wall",
    "station.oath_shrine",
    "station.overflow",
    "station.realm_gate",
    "station.vault",
];
const ASSET_IDS: [&str; 11] = [
    "sprite.landmark.lantern_fork",
    "sprite.landmark.realm_gate",
    "sprite.portal.dungeon.bell_sepulcher",
    "sprite.portal.return.lantern_halls",
    "sprite.station.memorial_wall",
    "sprite.station.oath_shrine",
    "sprite.station.overflow",
    "sprite.station.realm_gate",
    "sprite.station.vault",
    "tilemap.hub.lantern_halls_01",
    "tilemap.world.core_microrealm_01",
];
const LOCALIZATION_KEYS: [&str; 24] = [
    "hub.lantern_halls_01.description",
    "hub.lantern_halls_01.name",
    "landmark.lantern_fork.description",
    "landmark.lantern_fork.name",
    "landmark.realm_gate.description",
    "landmark.realm_gate.name",
    "portal.dungeon.bell_sepulcher.description",
    "portal.dungeon.bell_sepulcher.name",
    "portal.return.lantern_halls.description",
    "portal.return.lantern_halls.name",
    "spawn.hub.character_select_return.description",
    "spawn.hub.character_select_return.name",
    "station.memorial_wall.description",
    "station.memorial_wall.name",
    "station.oath_shrine.description",
    "station.oath_shrine.name",
    "station.overflow.description",
    "station.overflow.name",
    "station.realm_gate.description",
    "station.realm_gate.name",
    "station.vault.description",
    "station.vault.name",
    "world.core_microrealm_01.description",
    "world.core_microrealm_01.name",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreWorldFlowHashes {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

/// Immutable compiled development view. It deliberately has no serialization or release API.
#[derive(Debug, Clone)]
pub struct CoreDevelopmentWorldFlow {
    target_name: String,
    hub: CoreHubRecord,
    world: CoreWorldRecord,
    objects: Vec<CoreWorldObjectRecord>,
    hashes: CoreWorldFlowHashes,
}

impl CoreDevelopmentWorldFlow {
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub const fn hub(&self) -> &CoreHubRecord {
        &self.hub
    }

    #[must_use]
    pub const fn world(&self) -> &CoreWorldRecord {
        &self.world
    }

    #[must_use]
    pub fn objects(&self) -> &[CoreWorldObjectRecord] {
        &self.objects
    }

    #[must_use]
    pub const fn hashes(&self) -> &CoreWorldFlowHashes {
        &self.hashes
    }
}

/// Loads and compiles the checked-in world-flow target after proving the frozen FP source remains
/// valid. Nothing returned here can activate a route or describe a promoted package.
pub fn load_core_development_world_flow(root: &Path) -> Result<CoreDevelopmentWorldFlow> {
    crate::load_and_validate(root).context("world-flow compilation requires valid fp.1.0.0")?;
    let target: CoreWorldFlowDevelopmentTarget =
        crate::read_json(&root.join(CORE_WORLD_FLOW_TARGET_PATH))?;
    let records: CoreWorldFlowRecords = crate::read_json(&root.join(CORE_WORLD_FLOW_RECORDS_PATH))?;
    let assets: CoreGrayboxAssetManifest =
        crate::read_json(&root.join(CORE_WORLD_FLOW_ASSETS_PATH))?;
    let copy: CoreWorldFlowCopyFile = crate::read_json(&root.join(CORE_WORLD_FLOW_COPY_PATH))?;
    let hashes = CoreWorldFlowHashes {
        records_blake3: hash_file(&root.join(CORE_WORLD_FLOW_RECORDS_PATH))?,
        assets_blake3: hash_file(&root.join(CORE_WORLD_FLOW_ASSETS_PATH))?,
        localization_blake3: hash_file(&root.join(CORE_WORLD_FLOW_COPY_PATH))?,
    };
    compile_core_development_world_flow(&target, &records, &assets, &copy, &hashes)
}

pub fn compile_core_development_world_flow(
    target: &CoreWorldFlowDevelopmentTarget,
    records: &CoreWorldFlowRecords,
    assets: &CoreGrayboxAssetManifest,
    copy: &CoreWorldFlowCopyFile,
    hashes: &CoreWorldFlowHashes,
) -> Result<CoreDevelopmentWorldFlow> {
    validate_target(target, hashes)?;
    validate_record_allowlists(target, records)?;
    validate_assets(target, records, assets)?;
    validate_copy(target, records, copy)?;
    let hub = records
        .hubs
        .first()
        .context("Core world flow requires one hub")?;
    let world = records
        .worlds
        .first()
        .context("Core world flow requires one world")?;
    validate_hub(hub, &records.objects)?;
    validate_world(world, &records.objects)?;
    validate_objects(&records.objects)?;
    validate_parent_closure(hub, world, &records.objects)?;
    Ok(CoreDevelopmentWorldFlow {
        target_name: target.target_name.clone(),
        hub: hub.clone(),
        world: world.clone(),
        objects: records.objects.clone(),
        hashes: hashes.clone(),
    })
}

fn validate_target(
    target: &CoreWorldFlowDevelopmentTarget,
    hashes: &CoreWorldFlowHashes,
) -> Result<()> {
    if target.schema_version != SCHEMA_VERSION
        || target.target_kind != CoreWorldFlowTargetKind::UnpromotedWorldFlowSubset
        || target.target_name != CORE_WORLD_FLOW_TARGET_NAME
    {
        bail!("Core world-flow target identity is not the approved unpromoted target");
    }
    require_exact_ids(&target.required_hub_ids, &HUB_IDS, "hub")?;
    require_exact_ids(&target.required_world_ids, &WORLD_IDS, "world")?;
    require_exact_ids(&target.required_object_ids, &OBJECT_IDS, "object")?;
    require_exact_ids(&target.required_asset_ids, &ASSET_IDS, "asset")?;
    require_exact_ids(
        &target.required_localization_keys,
        &LOCALIZATION_KEYS,
        "localization",
    )?;
    let expected = [
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
    ];
    for (wanted, actual, domain) in expected {
        if wanted != actual {
            bail!("Core world-flow {domain} BLAKE3 mismatch: expected {wanted}, actual {actual}");
        }
    }
    Ok(())
}

fn validate_record_allowlists(
    target: &CoreWorldFlowDevelopmentTarget,
    records: &CoreWorldFlowRecords,
) -> Result<()> {
    if records.schema_version != SCHEMA_VERSION {
        bail!("Core world-flow records use an unsupported schema version");
    }
    require_exact_ids(
        &records
            .hubs
            .iter()
            .map(|record| record.header.id.clone())
            .collect::<Vec<_>>(),
        &HUB_IDS,
        "source hub",
    )?;
    require_exact_ids(
        &records
            .worlds
            .iter()
            .map(|record| record.header.id.clone())
            .collect::<Vec<_>>(),
        &WORLD_IDS,
        "source world",
    )?;
    require_exact_ids(
        &records
            .objects
            .iter()
            .map(|record| record.header.id.clone())
            .collect::<Vec<_>>(),
        &OBJECT_IDS,
        "source object",
    )?;
    if target.required_hub_ids.len()
        + target.required_world_ids.len()
        + target.required_object_ids.len()
        != 12
    {
        bail!("Core world-flow target must contain exactly two parents and ten children");
    }
    Ok(())
}

fn validate_assets(
    target: &CoreWorldFlowDevelopmentTarget,
    records: &CoreWorldFlowRecords,
    assets: &CoreGrayboxAssetManifest,
) -> Result<()> {
    if assets.schema_version != SCHEMA_VERSION {
        bail!("Core graybox assets use an unsupported schema version");
    }
    let actual_ids = assets
        .assets
        .iter()
        .map(|entry| entry.asset_id.clone())
        .collect::<Vec<_>>();
    require_same_ids(&actual_ids, &target.required_asset_ids, "graybox asset")?;
    let record_ids = all_headers(records)
        .map(|header| header.id.as_str())
        .collect::<BTreeSet<_>>();
    for asset in &assets.assets {
        if !record_ids.contains(asset.source_record_id.as_str()) {
            bail!(
                "graybox asset {} has an unknown source record",
                asset.asset_id
            );
        }
        let tilemap = asset.asset_id.as_str().starts_with("tilemap.");
        let expected_kind = if tilemap {
            CoreGrayboxAssetKind::GeneratedCollisionTilemap
        } else {
            CoreGrayboxAssetKind::GrayboxMarker
        };
        if asset.kind != expected_kind {
            bail!(
                "graybox asset {} has the wrong resolution kind",
                asset.asset_id
            );
        }
    }
    let mut referenced = all_headers(records)
        .flat_map(|header| header.asset_ids.iter().cloned())
        .collect::<Vec<_>>();
    referenced.sort();
    require_same_ids(&referenced, &target.required_asset_ids, "referenced asset")?;
    Ok(())
}

fn validate_copy(
    target: &CoreWorldFlowDevelopmentTarget,
    records: &CoreWorldFlowRecords,
    copy: &CoreWorldFlowCopyFile,
) -> Result<()> {
    if copy.schema_version != SCHEMA_VERSION || copy.locale != "en-US" {
        bail!("Core world-flow copy must be schema 1 en-US");
    }
    let keys = copy
        .entries
        .iter()
        .map(|entry| entry.key.clone())
        .collect::<Vec<_>>();
    require_same_ids(&keys, &target.required_localization_keys, "localized copy")?;
    if copy
        .entries
        .iter()
        .any(|entry| entry.value.trim().is_empty())
    {
        bail!("Core world-flow copy cannot be empty");
    }
    let mut referenced = all_headers(records)
        .flat_map(|header| {
            [
                header.localization_description_key.clone(),
                header.localization_name_key.clone(),
            ]
        })
        .collect::<Vec<_>>();
    referenced.sort();
    require_same_ids(
        &referenced,
        &target.required_localization_keys,
        "referenced localization",
    )?;
    Ok(())
}

fn validate_hub(hub: &CoreHubRecord, objects: &[CoreWorldObjectRecord]) -> Result<()> {
    validate_header(
        &hub.header,
        "hub.lantern_halls_01",
        &["tilemap.hub.lantern_halls_01"],
        &["hub", "safe", "noncombat"],
        "CONT-HUB-001",
    )?;
    let expected_solids = [
        rect(6_000, 35_000, 12_000, 2_000),
        rect(46_000, 35_000, 12_000, 2_000),
        rect(6_000, 7_000, 14_000, 2_000),
        rect(44_000, 7_000, 14_000, 2_000),
        rect(29_000, 22_000, 6_000, 4_000),
    ];
    if hub.width_tiles != 64
        || hub.height_tiles != 48
        || hub.origin != CoreMapOrigin::Northwest
        || hub.solid_shell_tiles != 1
        || hub.player_radius_milli_tiles != 300
        || hub.minimum_aisle_width_milli_tiles != 3_000
        || !hub.safe_noncombat
        || hub.default_spawn != point(32_000, 42_000)
        || hub.character_select_return != point(32_000, 44_000)
        || hub.solid_rectangles != expected_solids
        || hub.prohibited_creation
            != [
                CoreProhibitedCreation::Hostile,
                CoreProhibitedCreation::Damage,
                CoreProhibitedCreation::Projectile,
                CoreProhibitedCreation::Pickup,
                CoreProhibitedCreation::Drop,
            ]
    {
        bail!("Lantern Halls geometry or safety contract drifted from CONT-HUB-001");
    }
    validate_hall_reachability(hub, objects)
}

fn validate_world(world: &CoreWorldRecord, objects: &[CoreWorldObjectRecord]) -> Result<()> {
    validate_header(
        &world.header,
        "world.core_microrealm_01",
        &["tilemap.world.core_microrealm_01"],
        &["world", "danger", "core_microrealm"],
        "CONT-WORLD-001",
    )?;
    let expected_road = CoreRoadPolyline {
        width_milli_tiles: 5_000,
        points: vec![
            point(8_500, 40_500),
            point(24_500, 40_500),
            point(24_500, 24_500),
            point(40_500, 24_500),
            point(40_500, 8_500),
        ],
    };
    let expected_anchors = vec![
        point(8_500, 8_500),
        point(16_500, 8_500),
        point(24_500, 8_500),
        point(8_500, 16_500),
        point(16_500, 16_500),
        point(32_500, 16_500),
        point(8_500, 24_500),
        point(16_500, 32_500),
        point(32_500, 32_500),
    ];
    if world.width_tiles != 48
        || world.height_tiles != 48
        || world.origin != CoreMapOrigin::Northwest
        || world.solid_shell_tiles != 1
        || world.base_terrain != CoreWorldTerrain::ClearMud
        || world.capacity != 1
        || world.disabled_systems
            != [
                CoreDisabledWorldSystem::MacroScheduler,
                CoreDisabledWorldSystem::RealmCycle,
                CoreDisabledWorldSystem::Siege,
                CoreDisabledWorldSystem::Retirement,
            ]
        || world.realm_gate != rect(4_000, 38_000, 10_000, 10_000)
        || world.player_spawn != point(8_500, 40_500)
        || world.lantern_fork_safe_area != circle(24_500, 24_500, 5_000)
        || world.bell_portal_area != circle(40_500, 8_500, 3_000)
        || world.roads != [expected_road]
        || world.candidate_spawn_anchors != expected_anchors
        || world.intentionally_excluded_anchor != point(32_500, 24_500)
        || world.enabled_spawn_anchor_count != 8
    {
        bail!("Core microrealm geometry or admission contract drifted from CONT-WORLD-001");
    }
    validate_world_anchors(world)?;
    let child_ids = objects
        .iter()
        .filter(|object| object.parent_id == world.header.id)
        .map(|object| object.header.id.as_str())
        .collect::<Vec<_>>();
    if child_ids
        != [
            "landmark.lantern_fork",
            "landmark.realm_gate",
            "portal.dungeon.bell_sepulcher",
            "portal.return.lantern_halls",
        ]
    {
        bail!("Core microrealm child closure drifted");
    }
    Ok(())
}

fn validate_objects(objects: &[CoreWorldObjectRecord]) -> Result<()> {
    for object in objects {
        let id = object.header.id.as_str();
        let (asset, tags, source, parent, geometry, gated) = expected_object(id)?;
        validate_header(&object.header, id, asset, tags, source)?;
        if object.parent_id.as_str() != parent
            || object.geometry != geometry
            || !object.authored_core_enabled
            || object.integration_gate.as_deref() != gated
        {
            bail!("Core world object {id} drifted from its approved contract");
        }
    }
    Ok(())
}

#[allow(clippy::type_complexity)]
fn expected_object(
    id: &str,
) -> Result<(
    &'static [&'static str],
    &'static [&'static str],
    &'static str,
    &'static str,
    CoreWorldObjectGeometry,
    Option<&'static str>,
)> {
    const GATE: Option<&str> = Some("core_world_flow_integration");
    let value = match id {
        "landmark.lantern_fork" => (
            &["sprite.landmark.lantern_fork"][..],
            &["landmark", "safe_zone"][..],
            "CONT-WORLD-001",
            "world.core_microrealm_01",
            CoreWorldObjectGeometry::CircleLandmark {
                circle: circle(24_500, 24_500, 5_000),
            },
            None,
        ),
        "landmark.realm_gate" => (
            &["sprite.landmark.realm_gate"][..],
            &["landmark", "realm_entry"][..],
            "CONT-WORLD-001",
            "world.core_microrealm_01",
            CoreWorldObjectGeometry::RectangleLandmark {
                rectangle: rect(4_000, 38_000, 10_000, 10_000),
            },
            None,
        ),
        "portal.dungeon.bell_sepulcher" => (
            &["sprite.portal.dungeon.bell_sepulcher"][..],
            &["portal", "dungeon_entry", "requires_microrealm_cleared"][..],
            "CONT-WORLD-001",
            "world.core_microrealm_01",
            CoreWorldObjectGeometry::CirclePortal {
                circle: circle(40_500, 8_500, 3_000),
            },
            GATE,
        ),
        "portal.return.lantern_halls" => (
            &["sprite.portal.return.lantern_halls"][..],
            &["portal", "hall_return", "safe_transfer"][..],
            "CONT-WORLD-001",
            "world.core_microrealm_01",
            CoreWorldObjectGeometry::RectanglePortal {
                rectangle: rect(4_000, 38_000, 10_000, 10_000),
            },
            GATE,
        ),
        "spawn.hub.character_select_return" => (
            &[][..],
            &["spawn_anchor", "nonvisual", "character_select_return"][..],
            "CONT-HUB-001",
            "hub.lantern_halls_01",
            CoreWorldObjectGeometry::SpawnAnchor {
                point: point(32_000, 44_000),
            },
            None,
        ),
        "station.memorial_wall" => station(
            "sprite.station.memorial_wall",
            point(10_000, 10_000),
            &["station"],
        ),
        "station.oath_shrine" => station(
            "sprite.station.oath_shrine",
            point(24_000, 18_000),
            &["station"],
        ),
        "station.overflow" => station(
            "sprite.station.overflow",
            point(15_000, 38_000),
            &["station"],
        ),
        "station.realm_gate" => station(
            "sprite.station.realm_gate",
            point(32_000, 3_000),
            &["station", "realm_entry", "instant_interaction"],
        ),
        "station.vault" => station("sprite.station.vault", point(10_000, 38_000), &["station"]),
        _ => bail!("unauthorized Core world object {id}"),
    };
    Ok(value)
}

#[allow(clippy::type_complexity)]
fn station(
    asset: &'static str,
    point: MilliTilePoint,
    tags: &'static [&'static str],
) -> (
    &'static [&'static str],
    &'static [&'static str],
    &'static str,
    &'static str,
    CoreWorldObjectGeometry,
    Option<&'static str>,
) {
    let assets = match asset {
        "sprite.station.memorial_wall" => &["sprite.station.memorial_wall"][..],
        "sprite.station.oath_shrine" => &["sprite.station.oath_shrine"][..],
        "sprite.station.overflow" => &["sprite.station.overflow"][..],
        "sprite.station.realm_gate" => &["sprite.station.realm_gate"][..],
        "sprite.station.vault" => &["sprite.station.vault"][..],
        _ => &[][..],
    };
    (
        assets,
        tags,
        "CONT-HUB-001",
        "hub.lantern_halls_01",
        CoreWorldObjectGeometry::PointInteractable {
            point,
            clear_radius_milli_tiles: 2_000,
        },
        Some("core_world_flow_integration"),
    )
}

fn validate_parent_closure(
    hub: &CoreHubRecord,
    world: &CoreWorldRecord,
    objects: &[CoreWorldObjectRecord],
) -> Result<()> {
    let expected_hub = objects
        .iter()
        .filter(|object| object.parent_id == hub.header.id)
        .map(|object| object.header.id.clone())
        .collect::<Vec<_>>();
    let expected_world = objects
        .iter()
        .filter(|object| object.parent_id == world.header.id)
        .map(|object| object.header.id.clone())
        .collect::<Vec<_>>();
    if hub.object_ids != expected_hub || world.object_ids != expected_world {
        bail!("Core world-flow parent child allowlists are not exact");
    }
    Ok(())
}

fn validate_hall_reachability(
    hub: &CoreHubRecord,
    objects: &[CoreWorldObjectRecord],
) -> Result<()> {
    let stations = objects
        .iter()
        .filter_map(|object| match object.geometry {
            CoreWorldObjectGeometry::PointInteractable { point, .. } => Some(point),
            _ => None,
        })
        .collect::<Vec<_>>();
    for spawn in [hub.default_spawn, hub.character_select_return] {
        if !hall_point_walkable(hub, spawn) {
            bail!("Lantern Halls spawn is not walkable for the validation player radius");
        }
        for station_point in &stations {
            if !hall_path_exists(hub, spawn, *station_point) {
                bail!("Lantern Halls station is unreachable from an authored spawn");
            }
        }
    }
    Ok(())
}

fn hall_path_exists(hub: &CoreHubRecord, start: MilliTilePoint, goal: MilliTilePoint) -> bool {
    const STEP: i32 = 500;
    let width = i32::try_from(hub.width_tiles).expect("small hub width") * 1_000 / STEP + 1;
    let height = i32::try_from(hub.height_tiles).expect("small hub height") * 1_000 / STEP + 1;
    let index = |point: MilliTilePoint| (point.x / STEP, point.y / STEP);
    let (start_x, start_y) = index(start);
    let (goal_x, goal_y) = index(goal);
    let mut frontier = std::collections::VecDeque::from([(start_x, start_y)]);
    let mut visited = BTreeSet::from([(start_x, start_y)]);
    while let Some((x, y)) = frontier.pop_front() {
        if (x - goal_x).abs() <= 4 && (y - goal_y).abs() <= 4 {
            return true;
        }
        for (next_x, next_y) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if next_x < 0 || next_y < 0 || next_x >= width || next_y >= height {
                continue;
            }
            let next = point(next_x * STEP, next_y * STEP);
            if hall_point_walkable(hub, next) && visited.insert((next_x, next_y)) {
                frontier.push_back((next_x, next_y));
            }
        }
    }
    false
}

fn hall_point_walkable(hub: &CoreHubRecord, point: MilliTilePoint) -> bool {
    let radius = i32::try_from(hub.player_radius_milli_tiles).expect("small radius");
    let shell = i32::try_from(hub.solid_shell_tiles).expect("small shell") * 1_000;
    let max_x = i32::try_from(hub.width_tiles).expect("small width") * 1_000;
    let max_y = i32::try_from(hub.height_tiles).expect("small height") * 1_000;
    if point.x < shell + radius
        || point.y < shell + radius
        || point.x > max_x - shell - radius
        || point.y > max_y - shell - radius
    {
        return false;
    }
    hub.solid_rectangles.iter().all(|solid| {
        let right = solid.x + i32::try_from(solid.width).expect("small rectangle");
        let bottom = solid.y + i32::try_from(solid.height).expect("small rectangle");
        point.x < solid.x - radius
            || point.x > right + radius
            || point.y < solid.y - radius
            || point.y > bottom + radius
    })
}

fn validate_world_anchors(world: &CoreWorldRecord) -> Result<()> {
    if world
        .candidate_spawn_anchors
        .contains(&world.intentionally_excluded_anchor)
    {
        bail!("road-conflicting (32.5,24.5) must not be a spawn candidate");
    }
    let mut legal = world.candidate_spawn_anchors.clone();
    legal.retain(|anchor| {
        !point_in_circle(*anchor, world.lantern_fork_safe_area)
            && !point_in_circle(*anchor, world.bell_portal_area)
            && !point_on_road(*anchor, &world.roads)
    });
    legal.sort_by_key(|anchor| (anchor.y, anchor.x));
    let count = usize::try_from(world.enabled_spawn_anchor_count).context("anchor count")?;
    if legal.len() != 9
        || legal
            .get(..count)
            .context("enabled anchor count exceeds legal anchors")?
            != world
                .candidate_spawn_anchors
                .get(..count)
                .context("candidate count")?
    {
        bail!("Core microrealm spawn filtering or (y,x) ordering drifted");
    }
    Ok(())
}

fn point_in_circle(point: MilliTilePoint, circle: MilliTileCircle) -> bool {
    let dx = i64::from(point.x - circle.center.x);
    let dy = i64::from(point.y - circle.center.y);
    dx * dx + dy * dy <= i64::from(circle.radius).pow(2)
}

fn point_on_road(point: MilliTilePoint, roads: &[CoreRoadPolyline]) -> bool {
    roads.iter().any(|road| {
        let half_width = i32::try_from(road.width_milli_tiles / 2).expect("small road");
        road.points.windows(2).any(|segment| {
            let a = segment[0];
            let b = segment[1];
            if a.x == b.x {
                (point.x - a.x).abs() <= half_width
                    && point.y >= a.y.min(b.y) - half_width
                    && point.y <= a.y.max(b.y) + half_width
            } else if a.y == b.y {
                (point.y - a.y).abs() <= half_width
                    && point.x >= a.x.min(b.x) - half_width
                    && point.x <= a.x.max(b.x) + half_width
            } else {
                true
            }
        })
    })
}

fn validate_header(
    header: &CoreDevelopmentHeader,
    id: &str,
    assets: &[&str],
    tags: &[&str],
    source: &str,
) -> Result<()> {
    let expected_name = format!("{id}.name");
    let expected_description = format!("{id}.description");
    if header.id.as_str() != id
        || header.schema_version != SCHEMA_VERSION
        || !header.enabled
        || header.earliest_release_stage != ReleaseStage::Core
        || header.localization_name_key.as_str() != expected_name
        || header.localization_description_key.as_str() != expected_description
        || header
            .asset_ids
            .iter()
            .map(ContentId::as_str)
            .ne(assets.iter().copied())
        || header
            .tags
            .iter()
            .map(String::as_str)
            .ne(tags.iter().copied())
        || header.source_document_feature_id != source
    {
        bail!("Core world-flow header {id} drifted from its approved derivation");
    }
    Ok(())
}

fn all_headers(records: &CoreWorldFlowRecords) -> impl Iterator<Item = &CoreDevelopmentHeader> {
    records
        .hubs
        .iter()
        .map(|record| &record.header)
        .chain(records.worlds.iter().map(|record| &record.header))
        .chain(records.objects.iter().map(|record| &record.header))
}

fn require_exact_ids(actual: &[ContentId], expected: &[&str], domain: &str) -> Result<()> {
    let unique = actual.iter().collect::<BTreeSet<_>>();
    if unique.len() != actual.len() {
        bail!("Core world-flow target contains duplicate {domain} IDs");
    }
    if actual
        .iter()
        .map(ContentId::as_str)
        .ne(expected.iter().copied())
    {
        bail!("Core world-flow target has an unauthorized or reordered {domain} allowlist");
    }
    Ok(())
}

fn require_same_ids(actual: &[ContentId], expected: &[ContentId], domain: &str) -> Result<()> {
    if actual != expected || actual.iter().collect::<BTreeSet<_>>().len() != actual.len() {
        bail!("Core world-flow {domain} closure is missing, duplicated, extra, or reordered");
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

const fn point(x: i32, y: i32) -> MilliTilePoint {
    MilliTilePoint { x, y }
}

const fn rect(x: i32, y: i32, width: u32, height: u32) -> MilliTileRectangle {
    MilliTileRectangle {
        x,
        y,
        width,
        height,
    }
}

const fn circle(x: i32, y: i32, radius: u32) -> MilliTileCircle {
    MilliTileCircle {
        center: point(x, y),
        radius,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn checked_in_world_flow_compiles_without_a_release_artifact() {
        let compiled =
            load_core_development_world_flow(&content_root()).expect("valid Core world flow");
        assert_eq!(compiled.target_name(), CORE_WORLD_FLOW_TARGET_NAME);
        assert_eq!(compiled.hub().header.id.as_str(), HUB_IDS[0]);
        assert_eq!(compiled.world().header.id.as_str(), WORLD_IDS[0]);
        assert_eq!(compiled.objects().len(), OBJECT_IDS.len());
        for forbidden in [
            "manifests/core.1.0.0.json",
            "promotions/core.1.0.0.json",
            "packages/core.1.0.0.json",
        ] {
            assert!(!content_root().join(forbidden).exists());
        }
    }

    #[test]
    fn route_encounter_room_and_secret_bindings_are_absent() {
        let records = fs::read_to_string(content_root().join(CORE_WORLD_FLOW_RECORDS_PATH))
            .expect("world-flow source");
        for forbidden in [
            "destination_id",
            "pack.bell.01",
            "room.bell.",
            "layout.core_private_life_01",
            "encounter.secret.bell_01",
        ] {
            assert!(!records.contains(forbidden), "03A leaked {forbidden}");
        }
    }

    #[test]
    fn stale_version_capacity_and_geometry_drift_fail_closed() {
        let root = content_root();
        let target: CoreWorldFlowDevelopmentTarget =
            crate::read_json(&root.join(CORE_WORLD_FLOW_TARGET_PATH)).expect("target");
        let records: CoreWorldFlowRecords =
            crate::read_json(&root.join(CORE_WORLD_FLOW_RECORDS_PATH)).expect("records");
        let assets: CoreGrayboxAssetManifest =
            crate::read_json(&root.join(CORE_WORLD_FLOW_ASSETS_PATH)).expect("assets");
        let copy: CoreWorldFlowCopyFile =
            crate::read_json(&root.join(CORE_WORLD_FLOW_COPY_PATH)).expect("copy");
        let hashes = CoreWorldFlowHashes {
            records_blake3: target.expected_records_blake3.clone(),
            assets_blake3: target.expected_assets_blake3.clone(),
            localization_blake3: target.expected_localization_blake3.clone(),
        };

        let mut changed = records.clone();
        changed.worlds[0].capacity = 2;
        assert!(
            compile_core_development_world_flow(&target, &changed, &assets, &copy, &hashes)
                .expect_err("public capacity forbidden")
                .to_string()
                .contains("admission contract")
        );

        let mut changed = records.clone();
        changed.hubs[0].solid_rectangles[0].width += 1;
        assert!(
            compile_core_development_world_flow(&target, &changed, &assets, &copy, &hashes)
                .expect_err("geometry drift")
                .to_string()
                .contains("geometry")
        );

        let mut changed = target.clone();
        changed.target_name = "core.1.0.0".to_owned();
        assert!(
            compile_core_development_world_flow(&changed, &records, &assets, &copy, &hashes)
                .expect_err("promotion relabel")
                .to_string()
                .contains("unpromoted")
        );
    }

    #[test]
    fn allowlist_asset_copy_and_parent_drift_fail_closed() {
        let root = content_root();
        let target: CoreWorldFlowDevelopmentTarget =
            crate::read_json(&root.join(CORE_WORLD_FLOW_TARGET_PATH)).expect("target");
        let records: CoreWorldFlowRecords =
            crate::read_json(&root.join(CORE_WORLD_FLOW_RECORDS_PATH)).expect("records");
        let assets: CoreGrayboxAssetManifest =
            crate::read_json(&root.join(CORE_WORLD_FLOW_ASSETS_PATH)).expect("assets");
        let copy: CoreWorldFlowCopyFile =
            crate::read_json(&root.join(CORE_WORLD_FLOW_COPY_PATH)).expect("copy");
        let hashes = CoreWorldFlowHashes {
            records_blake3: target.expected_records_blake3.clone(),
            assets_blake3: target.expected_assets_blake3.clone(),
            localization_blake3: target.expected_localization_blake3.clone(),
        };

        let mut changed = target.clone();
        changed.required_object_ids.swap(0, 1);
        assert!(
            compile_core_development_world_flow(&changed, &records, &assets, &copy, &hashes)
                .is_err()
        );

        let mut changed = assets.clone();
        changed.assets.pop();
        assert!(
            compile_core_development_world_flow(&target, &records, &changed, &copy, &hashes)
                .is_err()
        );

        let mut changed = copy.clone();
        changed.entries[0].value.clear();
        assert!(
            compile_core_development_world_flow(&target, &records, &assets, &changed, &hashes)
                .is_err()
        );

        let mut changed = records;
        changed.objects[0].parent_id = ContentId::parse("hub.lantern_halls_01").expect("ID");
        assert!(
            compile_core_development_world_flow(&target, &changed, &assets, &copy, &hashes)
                .is_err()
        );
    }
}
