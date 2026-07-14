//! Fail-closed compiler for the unpromoted `GB-M03-03A` world-flow subset.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use content_schema::{
    ContentId, CoreDevelopmentHeader, CoreDisabledWorldSystem, CoreGrayboxAssetKind,
    CoreGrayboxAssetManifest, CoreHubRecord, CoreMapOrigin, CoreProhibitedCreation,
    CoreRoadPolyline, CoreWorldFlowCopyFile, CoreWorldFlowDevelopmentTarget, CoreWorldFlowRecords,
    CoreWorldFlowTargetKind, CoreWorldObjectGeometry, CoreWorldObjectRecord, CoreWorldRecord,
    CoreWorldTerrain, MilliTileCircle, MilliTilePoint, MilliTileRectangle, ReleaseStage,
    SCHEMA_VERSION,
};
use sim_core::{
    InteractionDefinition, MILLI_TILES_PER_TILE, PLAYER_COLLISION_RADIUS_MILLI_TILES,
    SceneCreationKind, SceneObjectCondition, SceneObjectGeometry, TilePoint, TileRectangle,
    WorldRoad, WorldSceneDefinition, WorldSceneKind, WorldSceneObject,
    duration_ms_to_ticks_nearest,
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
const RECORD_LOCALIZATION_KEYS: [&str; 24] = [
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
const LOCALIZATION_KEYS: [&str; 71] = [
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
    "transition.action.exit",
    "transition.action.retry",
    "transition.action.return_character_select",
    "transition.handshake.account_suspended",
    "transition.handshake.authentication_failed",
    "transition.handshake.content_mismatch",
    "transition.handshake.internal_retryable",
    "transition.handshake.maintenance",
    "transition.handshake.protocol_unsupported",
    "transition.handshake.rate_limited",
    "transition.handshake.region_full",
    "transition.handshake.update_required",
    "transition.transfer.character_dead",
    "transition.transfer.character_not_found",
    "transition.transfer.character_not_owned",
    "transition.transfer.content_disabled",
    "transition.transfer.content_mismatch",
    "transition.transfer.destination_disabled",
    "transition.transfer.idempotency_conflict",
    "transition.transfer.incomplete_restore_point",
    "transition.transfer.instance_unavailable",
    "transition.transfer.invalid_source",
    "transition.transfer.issued_at_invalid",
    "transition.transfer.no_selected_character",
    "transition.transfer.out_of_range",
    "transition.transfer.payload_hash_mismatch",
    "transition.transfer.rate_limited",
    "transition.transfer.service_unavailable",
    "transition.transfer.stage_disabled",
    "transition.transfer.state_version_mismatch",
    "transition.transfer.storage_resolution_required",
    "transition.transfer.transfer_in_progress",
    "transition.phase.awaiting_authoritative_state",
    "transition.phase.fatal_error",
    "transition.phase.link_lost",
    "transition.phase.loading_content",
    "transition.phase.ready",
    "transition.phase.reconnecting",
    "transition.phase.recoverable_error",
    "transition.phase.requesting_transfer",
    "transition.phase.resolved_to_character_select",
    "transition.phase.resolved_to_hall",
    "transition.phase.safe_origin",
    "transition.status.no_progress",
    "transition.status.prior_safe_state",
    "transition.status.reconnect_attempt",
    "transition.status.vulnerability_warning",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreWorldFlowHashes {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

/// Closed keys for native Core transfer, failure, and reconnect presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoreWorldTransitionCopyKey {
    ActionExit,
    ActionRetry,
    ActionReturnCharacterSelect,
    HandshakeAccountSuspended,
    HandshakeAuthenticationFailed,
    HandshakeContentMismatch,
    HandshakeInternalRetryable,
    HandshakeMaintenance,
    HandshakeProtocolUnsupported,
    HandshakeRateLimited,
    HandshakeRegionFull,
    HandshakeUpdateRequired,
    PhaseAwaitingAuthoritativeState,
    PhaseFatalError,
    PhaseLinkLost,
    PhaseLoadingContent,
    PhaseReady,
    PhaseReconnecting,
    PhaseRecoverableError,
    PhaseRequestingTransfer,
    PhaseResolvedToCharacterSelect,
    PhaseResolvedToHall,
    PhaseSafeOrigin,
    StatusNoProgress,
    StatusPriorSafeState,
    StatusReconnectAttempt,
    StatusVulnerabilityWarning,
}

impl CoreWorldTransitionCopyKey {
    pub const ALL: [Self; 27] = [
        Self::ActionExit,
        Self::ActionRetry,
        Self::ActionReturnCharacterSelect,
        Self::HandshakeAccountSuspended,
        Self::HandshakeAuthenticationFailed,
        Self::HandshakeContentMismatch,
        Self::HandshakeInternalRetryable,
        Self::HandshakeMaintenance,
        Self::HandshakeProtocolUnsupported,
        Self::HandshakeRateLimited,
        Self::HandshakeRegionFull,
        Self::HandshakeUpdateRequired,
        Self::PhaseAwaitingAuthoritativeState,
        Self::PhaseFatalError,
        Self::PhaseLinkLost,
        Self::PhaseLoadingContent,
        Self::PhaseReady,
        Self::PhaseReconnecting,
        Self::PhaseRecoverableError,
        Self::PhaseRequestingTransfer,
        Self::PhaseResolvedToCharacterSelect,
        Self::PhaseResolvedToHall,
        Self::PhaseSafeOrigin,
        Self::StatusNoProgress,
        Self::StatusPriorSafeState,
        Self::StatusReconnectAttempt,
        Self::StatusVulnerabilityWarning,
    ];

    #[must_use]
    pub const fn localization_key(self) -> &'static str {
        match self {
            Self::ActionExit => "transition.action.exit",
            Self::ActionRetry => "transition.action.retry",
            Self::ActionReturnCharacterSelect => "transition.action.return_character_select",
            Self::HandshakeAccountSuspended => "transition.handshake.account_suspended",
            Self::HandshakeAuthenticationFailed => "transition.handshake.authentication_failed",
            Self::HandshakeContentMismatch => "transition.handshake.content_mismatch",
            Self::HandshakeInternalRetryable => "transition.handshake.internal_retryable",
            Self::HandshakeMaintenance => "transition.handshake.maintenance",
            Self::HandshakeProtocolUnsupported => "transition.handshake.protocol_unsupported",
            Self::HandshakeRateLimited => "transition.handshake.rate_limited",
            Self::HandshakeRegionFull => "transition.handshake.region_full",
            Self::HandshakeUpdateRequired => "transition.handshake.update_required",
            Self::PhaseAwaitingAuthoritativeState => {
                "transition.phase.awaiting_authoritative_state"
            }
            Self::PhaseFatalError => "transition.phase.fatal_error",
            Self::PhaseLinkLost => "transition.phase.link_lost",
            Self::PhaseLoadingContent => "transition.phase.loading_content",
            Self::PhaseReady => "transition.phase.ready",
            Self::PhaseReconnecting => "transition.phase.reconnecting",
            Self::PhaseRecoverableError => "transition.phase.recoverable_error",
            Self::PhaseRequestingTransfer => "transition.phase.requesting_transfer",
            Self::PhaseResolvedToCharacterSelect => "transition.phase.resolved_to_character_select",
            Self::PhaseResolvedToHall => "transition.phase.resolved_to_hall",
            Self::PhaseSafeOrigin => "transition.phase.safe_origin",
            Self::StatusNoProgress => "transition.status.no_progress",
            Self::StatusPriorSafeState => "transition.status.prior_safe_state",
            Self::StatusReconnectAttempt => "transition.status.reconnect_attempt",
            Self::StatusVulnerabilityWarning => "transition.status.vulnerability_warning",
        }
    }
}

/// Immutable compiled development view. It deliberately has no serialization or release API.
#[derive(Debug, Clone)]
pub struct CoreDevelopmentWorldFlow {
    target_name: String,
    hub: CoreHubRecord,
    world: CoreWorldRecord,
    objects: Vec<CoreWorldObjectRecord>,
    hashes: CoreWorldFlowHashes,
    localization: BTreeMap<String, String>,
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

    #[must_use]
    pub fn localized(&self, key: &str) -> Option<&str> {
        self.localization.get(key).map(String::as_str)
    }

    /// Returns copy only from the compile-time closed transition-key set.
    #[must_use]
    pub fn transition_copy(&self, key: CoreWorldTransitionCopyKey) -> &str {
        self.localization
            .get(key.localization_key())
            .map(String::as_str)
            .expect("validated Core transition copy key must remain present")
    }

    /// Compiles the validated Lantern Halls record into renderer-independent simulation data.
    pub fn compile_hall_scene(&self) -> Result<WorldSceneDefinition> {
        let hub = &self.hub;
        WorldSceneDefinition {
            id: hub.header.id.to_string(),
            kind: WorldSceneKind::SafeHub,
            width_milli_tiles: tiles_to_milli(hub.width_tiles, "Hall width")?,
            height_milli_tiles: tiles_to_milli(hub.height_tiles, "Hall height")?,
            shell_thickness_milli_tiles: tiles_to_milli(hub.solid_shell_tiles, "Hall shell")?,
            player_radius_milli_tiles: positive_i32(
                hub.player_radius_milli_tiles,
                "Hall player radius",
            )?,
            capacity: None,
            player_spawn: simulation_point(hub.default_spawn),
            solid_rectangles: hub
                .solid_rectangles
                .iter()
                .copied()
                .map(simulation_rectangle)
                .collect::<Result<Vec<_>>>()?,
            roads: Vec::new(),
            objects: self.compile_scene_objects(hub.header.id.as_str())?,
            prohibited_creation: hub
                .prohibited_creation
                .iter()
                .copied()
                .map(simulation_prohibition)
                .collect(),
        }
        .validated()
        .context("compiled Lantern Halls scene is invalid")
    }

    /// Compiles the validated capacity-one Core microrealm into simulation data.
    pub fn compile_microrealm_scene(&self) -> Result<WorldSceneDefinition> {
        let world = &self.world;
        WorldSceneDefinition {
            id: world.header.id.to_string(),
            kind: WorldSceneKind::PrivateDanger,
            width_milli_tiles: tiles_to_milli(world.width_tiles, "microrealm width")?,
            height_milli_tiles: tiles_to_milli(world.height_tiles, "microrealm height")?,
            shell_thickness_milli_tiles: tiles_to_milli(
                world.solid_shell_tiles,
                "microrealm shell",
            )?,
            player_radius_milli_tiles: PLAYER_COLLISION_RADIUS_MILLI_TILES,
            capacity: Some(
                u16::try_from(world.capacity).context("microrealm capacity exceeds u16")?,
            ),
            player_spawn: simulation_point(world.player_spawn),
            solid_rectangles: Vec::new(),
            roads: world
                .roads
                .iter()
                .map(|road| {
                    Ok(WorldRoad {
                        width_milli_tiles: positive_i32(
                            road.width_milli_tiles,
                            "microrealm road width",
                        )?,
                        points: road.points.iter().copied().map(simulation_point).collect(),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            objects: self.compile_scene_objects(world.header.id.as_str())?,
            prohibited_creation: BTreeSet::new(),
        }
        .validated()
        .context("compiled Core microrealm scene is invalid")
    }

    fn compile_scene_objects(&self, parent_id: &str) -> Result<Vec<WorldSceneObject>> {
        self.objects
            .iter()
            .filter(|object| object.parent_id.as_str() == parent_id)
            .map(compile_scene_object)
            .collect()
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
        localization: copy
            .entries
            .iter()
            .map(|entry| (entry.key.to_string(), entry.value.clone()))
            .collect(),
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
        &target.required_localization_keys[..RECORD_LOCALIZATION_KEYS.len()],
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

fn compile_scene_object(object: &CoreWorldObjectRecord) -> Result<WorldSceneObject> {
    let id = object.header.id.as_str();
    let (geometry, interaction) = match object.geometry {
        CoreWorldObjectGeometry::PointInteractable {
            point,
            clear_radius_milli_tiles,
        } => {
            let hold_ticks = match id {
                "station.realm_gate" | "station.vault" | "station.overflow" => 0,
                "station.memorial_wall" | "station.oath_shrine" => {
                    u16::try_from(duration_ms_to_ticks_nearest(500))
                        .context("500 ms Hall interaction exceeds u16 ticks")?
                }
                _ => bail!("unauthorized Core point interactable {id}"),
            };
            (
                SceneObjectGeometry::PointInteractable {
                    point: simulation_point(point),
                    clear_radius_milli_tiles: positive_i32(
                        clear_radius_milli_tiles,
                        "station clear radius",
                    )?,
                },
                Some(InteractionDefinition {
                    range_milli_tiles: 1_500,
                    hold_ticks,
                }),
            )
        }
        CoreWorldObjectGeometry::RectangleLandmark { rectangle }
        | CoreWorldObjectGeometry::RectanglePortal { rectangle } => (
            SceneObjectGeometry::Rectangle(simulation_rectangle(rectangle)?),
            None,
        ),
        CoreWorldObjectGeometry::CircleLandmark { circle }
        | CoreWorldObjectGeometry::CirclePortal { circle } => (
            SceneObjectGeometry::Circle {
                center: simulation_point(circle.center),
                radius_milli_tiles: positive_i32(circle.radius, "scene circle radius")?,
            },
            None,
        ),
        CoreWorldObjectGeometry::SpawnAnchor { point } => {
            (SceneObjectGeometry::Point(simulation_point(point)), None)
        }
    };
    Ok(WorldSceneObject {
        id: id.to_owned(),
        geometry,
        interaction,
        integration_gate: object.integration_gate.clone(),
        condition: if id == "portal.dungeon.bell_sepulcher" {
            SceneObjectCondition::RequiresMicrorealmCleared
        } else {
            SceneObjectCondition::Always
        },
    })
}

const fn simulation_point(point: MilliTilePoint) -> TilePoint {
    TilePoint::new(point.x, point.y)
}

fn simulation_rectangle(rectangle: MilliTileRectangle) -> Result<TileRectangle> {
    Ok(TileRectangle::new(
        rectangle.x,
        rectangle.y,
        positive_i32(rectangle.width, "scene rectangle width")?,
        positive_i32(rectangle.height, "scene rectangle height")?,
    ))
}

fn simulation_prohibition(value: CoreProhibitedCreation) -> SceneCreationKind {
    match value {
        CoreProhibitedCreation::Hostile => SceneCreationKind::Hostile,
        CoreProhibitedCreation::Damage => SceneCreationKind::Damage,
        CoreProhibitedCreation::Projectile => SceneCreationKind::Projectile,
        CoreProhibitedCreation::Pickup => SceneCreationKind::Pickup,
        CoreProhibitedCreation::Drop => SceneCreationKind::Drop,
    }
}

fn tiles_to_milli(tiles: u32, field: &str) -> Result<i32> {
    positive_i32(tiles, field)?
        .checked_mul(MILLI_TILES_PER_TILE)
        .with_context(|| format!("{field} overflows milli-tile scale"))
}

fn positive_i32(value: u32, field: &str) -> Result<i32> {
    let converted = i32::try_from(value).with_context(|| format!("{field} exceeds i32"))?;
    if converted <= 0 {
        bail!("{field} must be positive");
    }
    Ok(converted)
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
    fn transition_copy_is_closed_typed_and_complete() {
        let compiled =
            load_core_development_world_flow(&content_root()).expect("valid Core world flow");
        assert_eq!(
            CoreWorldTransitionCopyKey::ALL
                .iter()
                .map(|key| key.localization_key())
                .collect::<BTreeSet<_>>()
                .len(),
            CoreWorldTransitionCopyKey::ALL.len()
        );
        for key in CoreWorldTransitionCopyKey::ALL {
            assert!(key.localization_key().starts_with("transition."));
            assert!(!compiled.transition_copy(key).trim().is_empty());
        }
    }

    #[test]
    fn exact_hall_and_microrealm_compile_for_simulation_without_runtime_defaults() {
        let compiled =
            load_core_development_world_flow(&content_root()).expect("valid Core world flow");
        let hall = compiled.compile_hall_scene().expect("Hall scene");
        assert_eq!(hall.id, "hub.lantern_halls_01");
        assert_eq!(
            (hall.width_milli_tiles, hall.height_milli_tiles),
            (64_000, 48_000)
        );
        assert_eq!(hall.player_spawn, TilePoint::new(32_000, 42_000));
        assert_eq!(hall.capacity, None);
        assert_eq!(hall.solid_rectangles.len(), 5);
        assert_eq!(hall.objects.len(), 6);
        assert_eq!(hall.prohibited_creation.len(), 5);
        assert!(!hall.can_occupy(TilePoint::new(30_000, 24_000)));
        assert!(hall.can_occupy(TilePoint::new(32_000, 42_000)));
        let return_spawn = hall
            .objects
            .iter()
            .find_map(|object| {
                (object.id == "spawn.hub.character_select_return").then_some(object.geometry)
            })
            .and_then(|geometry| match geometry {
                SceneObjectGeometry::Point(point) => Some(point),
                _ => None,
            })
            .expect("character-select return spawn");
        for station in hall
            .objects
            .iter()
            .filter(|object| object.interaction.is_some())
        {
            let SceneObjectGeometry::PointInteractable { point, .. } = station.geometry else {
                panic!("station {} must be point-interactable", station.id);
            };
            assert!(
                hall.has_grid_path(hall.player_spawn, point, 500)
                    .expect("default-spawn path"),
                "{} is unreachable from default spawn",
                station.id
            );
            assert!(
                hall.has_grid_path(return_spawn, point, 500)
                    .expect("return-spawn path"),
                "{} is unreachable from character-select return",
                station.id
            );
            assert_eq!(
                station.integration_gate.as_deref(),
                Some("core_world_flow_integration")
            );
            let hold_ticks = station.interaction.expect("interaction").hold_ticks;
            if matches!(
                station.id.as_str(),
                "station.realm_gate" | "station.vault" | "station.overflow"
            ) {
                assert_eq!(hold_ticks, 0, "{} must be instant", station.id);
            } else {
                assert_eq!(hold_ticks, 15, "{} must use the 500 ms hold", station.id);
            }
        }

        let microrealm = compiled
            .compile_microrealm_scene()
            .expect("microrealm scene");
        assert_eq!(microrealm.id, "world.core_microrealm_01");
        assert_eq!(
            (microrealm.width_milli_tiles, microrealm.height_milli_tiles),
            (48_000, 48_000)
        );
        assert_eq!(microrealm.player_spawn, TilePoint::new(8_500, 40_500));
        assert_eq!(microrealm.capacity, Some(1));
        assert_eq!(microrealm.roads.len(), 1);
        assert_eq!(microrealm.roads[0].width_milli_tiles, 5_000);
        assert_eq!(microrealm.objects.len(), 4);

        let bell_portal = microrealm
            .objects
            .iter()
            .find(|object| object.id == "portal.dungeon.bell_sepulcher")
            .expect("Bell portal");
        assert_eq!(
            bell_portal.condition,
            SceneObjectCondition::RequiresMicrorealmCleared
        );
        assert_eq!(
            bell_portal.integration_gate.as_deref(),
            Some("core_world_flow_integration")
        );
        assert_eq!(
            compiled.localized("hub.lantern_halls_01.name"),
            Some("Lantern Halls")
        );
        assert_ne!(
            hall.deterministic_digest().expect("Hall digest"),
            microrealm
                .deterministic_digest()
                .expect("microrealm digest")
        );
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
