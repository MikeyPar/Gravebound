//! Fail-closed compiler for the unpromoted `GB-M03-03D` encounter/room content layer.

use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use content_schema::{
    ContentId, CoreEncounterRank, CoreEncounterRoomAssetManifest, CoreEncounterRoomCopyFile,
    CoreEncounterRoomDevelopmentTarget, CoreEncounterRoomRecords, CoreEncounterRoomTargetKind,
    CoreEncounterSourceKind, CoreFixedLayoutRecord, CoreRoomAnchorKind, CoreRoomDoorSide,
    CoreRoomTemplateRecord, CoreRoomVolumeGeometry, CoreRoomVolumeKind, ReleaseStage,
    SCHEMA_VERSION,
};
use sim_core::{
    BellReedDefinition, ChainSentryDefinition, CoreAttackGroupRule, CoreEnemyDefinition,
    CoreEnemyDefinitionParameters, CoreEnemyLocomotionParameters, CoreEnemyRole,
    CoreEnemyStateStage, CorePatternDefinition, CorePatternDefinitionParameters,
    CorePatternGeometryParameters, CorePatternWarningParameters, CoreRadialGapRelation,
    CoreTargetSelection, CoreTelegraphLock, Counterplay, DrownedPilgrimDefinition,
    DungeonAnchorKind, DungeonCorridor, DungeonDoorDefinition, DungeonDoorSide, DungeonRoomAnchor,
    DungeonRoomDefinition, DungeonRoomVolume, DungeonRoomVolumeGeometry, DungeonRoomVolumeKind,
    EchoMemoryFamily, FixedDungeonLayoutDefinition, HostileDisposition, MILLI_TILES_PER_TILE,
    PLAYER_COLLISION_RADIUS_MILLI_TILES, PlacedDungeonRoom, solve_core_authored_min_speed_paths,
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
const AUTHORED_BEHAVIOR_IDS: [&str; 5] = [
    "enemy.mire_leech",
    "enemy.bell_acolyte",
    "enemy.choir_skull",
    "miniboss.sepulcher_knight",
    "miniboss.choir_abbot",
];
const AUTHORED_PATTERN_IDS: [&str; 8] = [
    "pattern.enemy.mire_leech.charge",
    "pattern.enemy.bell_acolyte.alternating_fan",
    "pattern.enemy.choir_skull.rotor",
    "miniboss.sepulcher_knight.charge_lane",
    "miniboss.sepulcher_knight.stop_ring",
    "miniboss.sepulcher_knight.shield_fan",
    "miniboss.choir_abbot.rotor",
    "miniboss.choir_abbot.recovery_ring",
];
const MAJOR_PATTERN_IDS: [&str; 2] = [
    "miniboss.sepulcher_knight.charge_lane",
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

/// Validated behavior payload selected without constructing a runtime entity.
#[derive(Debug, Clone)]
pub enum CoreEncounterBehaviorDefinition {
    ImmutableDrownedPilgrim(DrownedPilgrimDefinition),
    ImmutableBellReed(BellReedDefinition),
    ImmutableChainSentry(ChainSentryDefinition),
    Authored(CoreEnemyDefinition),
}

/// One roster-ordered actor definition with its immutable reward and XP bindings.
#[derive(Debug, Clone)]
pub struct CoreEncounterActorDefinition {
    id: ContentId,
    rank: CoreEncounterRank,
    reward_profile_id: ContentId,
    xp_profile_id: ContentId,
    behavior: CoreEncounterBehaviorDefinition,
}

impl CoreEncounterActorDefinition {
    #[must_use]
    pub const fn id(&self) -> &ContentId {
        &self.id
    }

    #[must_use]
    pub const fn rank(&self) -> CoreEncounterRank {
        self.rank
    }

    #[must_use]
    pub const fn reward_profile_id(&self) -> &ContentId {
        &self.reward_profile_id
    }

    #[must_use]
    pub const fn xp_profile_id(&self) -> &ContentId {
        &self.xp_profile_id
    }

    #[must_use]
    pub const fn behavior(&self) -> &CoreEncounterBehaviorDefinition {
        &self.behavior
    }
}

/// Immutable compiled view. It intentionally exposes no release or promotion operation.
#[derive(Debug, Clone)]
pub struct CoreDevelopmentEncounterRooms {
    target_name: String,
    records: CoreEncounterRoomRecords,
    actor_definitions: Vec<CoreEncounterActorDefinition>,
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
    pub fn actor_definitions(&self) -> &[CoreEncounterActorDefinition] {
        &self.actor_definitions
    }

    /// Produces executable COM-006 evidence for all eight Core-authored patterns.
    pub fn solve_authored_min_speed_paths(
        &self,
    ) -> Result<sim_core::CoreAuthoredMinSpeedPaths, Vec<sim_core::CoreCounterplayDiagnostic>> {
        let authored = self
            .actor_definitions
            .iter()
            .filter_map(|actor| match &actor.behavior {
                CoreEncounterBehaviorDefinition::Authored(definition) => Some(definition.clone()),
                CoreEncounterBehaviorDefinition::ImmutableDrownedPilgrim(_)
                | CoreEncounterBehaviorDefinition::ImmutableBellReed(_)
                | CoreEncounterBehaviorDefinition::ImmutableChainSentry(_) => None,
            })
            .collect::<Vec<_>>();
        solve_core_authored_min_speed_paths(&authored)
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

    /// Compiles every authored Bell template into renderer-independent local geometry.
    pub fn compile_room_definitions(&self) -> Result<Vec<DungeonRoomDefinition>> {
        let definitions = self
            .records
            .rooms
            .iter()
            .map(compile_room)
            .collect::<Result<Vec<_>>>()?;
        let utility_kinds = [
            DungeonAnchorKind::SafeEntry,
            DungeonAnchorKind::Exit,
            DungeonAnchorKind::Stage,
            DungeonAnchorKind::Shrine,
            DungeonAnchorKind::Stabilization,
            DungeonAnchorKind::Chest,
            DungeonAnchorKind::Group,
        ];
        for definition in &definitions {
            definition
                .prove_navigation(
                    &utility_kinds,
                    u32::try_from(PLAYER_COLLISION_RADIUS_MILLI_TILES)
                        .expect("player collision radius is positive"),
                    500,
                )
                .with_context(|| {
                    format!("Bell room {} has no safe utility route", definition.id)
                })?;
        }
        Ok(definitions)
    }

    /// Places only the active B0→B6 main chain. Authored branch templates remain available above
    /// but cannot enter the runtime definition while their layout nodes are disabled.
    pub fn compile_fixed_layout_definition(&self) -> Result<FixedDungeonLayoutDefinition> {
        let definitions = self.compile_room_definitions()?;
        compile_fixed_layout(&self.records.layouts[0], &definitions)
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
    let actor_definitions = compile_actor_definitions(source, records)?;
    let compiled = CoreDevelopmentEncounterRooms {
        target_name: target.target_name.clone(),
        records: records.clone(),
        actor_definitions,
        hashes: hashes.clone(),
        localization: copy
            .entries
            .iter()
            .map(|entry| (entry.key.to_string(), entry.value.clone()))
            .collect(),
    };
    compiled.compile_fixed_layout_definition()?;
    compiled
        .solve_authored_min_speed_paths()
        .map_err(|diagnostics| {
            anyhow::anyhow!("Core authored COM-006 fixture failed: {diagnostics:?}")
        })?;
    Ok(compiled)
}

fn compile_actor_definitions(
    source: &ContentPackage,
    records: &CoreEncounterRoomRecords,
) -> Result<Vec<CoreEncounterActorDefinition>> {
    let authored = records
        .authored_behaviors
        .iter()
        .map(|behavior| (behavior.owner_id.as_str(), behavior))
        .collect::<std::collections::BTreeMap<_, _>>();
    records
        .roster
        .iter()
        .map(|member| {
            let behavior = match member.source_kind {
                CoreEncounterSourceKind::ImmutableFirstPlayable => {
                    match member.header.id.as_str() {
                        "enemy.drowned_pilgrim" => {
                            CoreEncounterBehaviorDefinition::ImmutableDrownedPilgrim(
                                first_playable_drowned_pilgrim(source)?,
                            )
                        }
                        "enemy.bell_reed" => CoreEncounterBehaviorDefinition::ImmutableBellReed(
                            first_playable_bell_reed(source)?,
                        ),
                        "enemy.chain_sentry" => {
                            CoreEncounterBehaviorDefinition::ImmutableChainSentry(
                                first_playable_chain_sentry(source)?,
                            )
                        }
                        id => bail!("{id} has no immutable First Playable behavior adapter"),
                    }
                }
                CoreEncounterSourceKind::AuthoredCore => {
                    let authored_behavior =
                        authored.get(member.header.id.as_str()).with_context(|| {
                            format!("{} has no authored behavior", member.header.id)
                        })?;
                    let patterns = authored_behavior
                        .pattern_ids
                        .iter()
                        .map(|pattern_id| {
                            records
                                .authored_patterns
                                .iter()
                                .find(|pattern| pattern.id == *pattern_id)
                                .cloned()
                                .with_context(|| {
                                    format!(
                                        "{} has no authored pattern {}",
                                        member.header.id, pattern_id
                                    )
                                })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    CoreEncounterBehaviorDefinition::Authored(compile_authored_actor_definition(
                        member,
                        authored_behavior,
                        &patterns,
                    )?)
                }
            };
            Ok(CoreEncounterActorDefinition {
                id: member.header.id.clone(),
                rank: member.rank,
                reward_profile_id: member.reward_profile_id.clone(),
                xp_profile_id: member.xp_profile_id.clone(),
                behavior,
            })
        })
        .collect()
}

fn compile_authored_actor_definition(
    member: &content_schema::CoreEncounterRosterMember,
    behavior: &content_schema::CoreAuthoredEnemyBehaviorRecord,
    patterns: &[content_schema::CoreAuthoredPatternRecord],
) -> Result<CoreEnemyDefinition> {
    let state_sequence = behavior
        .state_sequence
        .iter()
        .copied()
        .map(compile_enemy_state_stage)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| anyhow::anyhow!("{} has an invalid state sequence", behavior.owner_id))?;
    let patterns = patterns
        .iter()
        .map(compile_authored_pattern_definition)
        .collect::<Result<Vec<_>>>()?;
    CoreEnemyDefinition::new(CoreEnemyDefinitionParameters {
        content_id: behavior.owner_id.to_string(),
        role: compile_enemy_role(behavior.role),
        state_sequence,
        target_selection: CoreTargetSelection::NearestLivingDamageableInAggroTieLowestEntityId,
        telegraph_lock: CoreTelegraphLock::AimAndPositionAtTelegraphStart,
        maximum_health: behavior.maximum_health,
        armor: behavior.armor,
        collision_radius_milli_tiles: behavior.collision_radius_milli_tiles,
        hurtbox_radius_milli_tiles: behavior.hurtbox_radius_milli_tiles,
        aggro_radius_milli_tiles: behavior.aggro_radius_milli_tiles,
        leash_radius_milli_tiles: behavior.leash_radius_milli_tiles,
        target_reacquire_milliseconds: behavior.target_reacquire_milliseconds,
        no_target_reset_milliseconds: behavior.no_target_reset_milliseconds,
        spawn_warning_milliseconds: behavior.spawn_warning_milliseconds,
        spawn_invulnerability_milliseconds: behavior.spawn_invulnerability_milliseconds,
        introduction_milliseconds: behavior.introduction_milliseconds,
        contact_damage: behavior.contact_damage,
        drop_reward_on_reset: behavior.drop_reward_on_reset,
        locomotion: compile_locomotion_parameters(&behavior.locomotion),
        patterns,
        reward_profile_id: member.reward_profile_id.to_string(),
        xp_profile_id: member.xp_profile_id.to_string(),
    })
    .with_context(|| format!("{} failed 30 Hz definition compilation", behavior.owner_id))
}

fn compile_authored_pattern_definition(
    pattern: &content_schema::CoreAuthoredPatternRecord,
) -> Result<CorePatternDefinition> {
    CorePatternDefinition::new(CorePatternDefinitionParameters {
        id: pattern.id.to_string(),
        owner_id: pattern.owner_id.to_string(),
        telegraph_id: pattern.telegraph_id.to_string(),
        audio_cue_id: pattern.audio_cue_id.to_string(),
        major_audio_cue_id: pattern.major_audio_cue_id.as_ref().map(ToString::to_string),
        damage_type: crate::compile_damage_type(pattern.damage_type),
        damage_band: crate::compile_damage_band(pattern.damage_band),
        raw_damage: pattern.raw_damage,
        threat_cost: pattern.threat_cost,
        warning: compile_pattern_warning(&pattern.warning),
        cycle_milliseconds: pattern.cycle_milliseconds,
        quiet_milliseconds: pattern.quiet_milliseconds,
        geometry: compile_pattern_geometry(&pattern.geometry),
        counterplay: compile_pattern_counterplay(pattern.counterplay),
        memory_family: compile_pattern_memory(pattern.memory_family),
        disposition: compile_pattern_disposition(pattern.disposition),
        attack_group_rule: compile_attack_group_rule(pattern.attack_group_rule),
        acceleration_milli_tiles_per_second_squared: pattern
            .acceleration_milli_tiles_per_second_squared,
        pierces_players: pattern.pierces_players,
        status_count: pattern.statuses.len(),
        cancel_on_phase_change: pattern.cancel_on_phase_change,
        persisted_maximum_active_instances: pattern.maximum_active_instances,
    })
    .with_context(|| format!("{} failed 30 Hz pattern compilation", pattern.id))
}

const fn compile_enemy_role(role: content_schema::CoreEnemyRole) -> CoreEnemyRole {
    match role {
        content_schema::CoreEnemyRole::Fodder => CoreEnemyRole::Fodder,
        content_schema::CoreEnemyRole::Pressure => CoreEnemyRole::Pressure,
        content_schema::CoreEnemyRole::Disruptor => CoreEnemyRole::Disruptor,
        content_schema::CoreEnemyRole::Anchor => CoreEnemyRole::Anchor,
        content_schema::CoreEnemyRole::Elite => CoreEnemyRole::Elite,
    }
}

const fn compile_enemy_state_stage(
    stage: content_schema::CoreEnemyStateStage,
) -> CoreEnemyStateStage {
    match stage {
        content_schema::CoreEnemyStateStage::SpawnTelegraph => CoreEnemyStateStage::SpawnTelegraph,
        content_schema::CoreEnemyStateStage::Acquire => CoreEnemyStateStage::Acquire,
        content_schema::CoreEnemyStateStage::MoveOrPosition => CoreEnemyStateStage::MoveOrPosition,
        content_schema::CoreEnemyStateStage::Telegraph => CoreEnemyStateStage::Telegraph,
        content_schema::CoreEnemyStateStage::Attack => CoreEnemyStateStage::Attack,
        content_schema::CoreEnemyStateStage::Recover => CoreEnemyStateStage::Recover,
    }
}

fn compile_locomotion_parameters(
    locomotion: &content_schema::CoreEnemyLocomotion,
) -> CoreEnemyLocomotionParameters {
    match locomotion {
        content_schema::CoreEnemyLocomotion::RushRetreat {
            approach_speed_milli_tiles_per_second,
            trigger_distance_milli_tiles,
            charge_distance_milli_tiles,
            charge_duration_milliseconds,
            retreat_speed_milli_tiles_per_second,
            retreat_duration_milliseconds,
        } => CoreEnemyLocomotionParameters::RushRetreat {
            approach_speed_milli_tiles_per_second: *approach_speed_milli_tiles_per_second,
            trigger_distance_milli_tiles: *trigger_distance_milli_tiles,
            charge_distance_milli_tiles: *charge_distance_milli_tiles,
            charge_duration_milliseconds: *charge_duration_milliseconds,
            retreat_speed_milli_tiles_per_second: *retreat_speed_milli_tiles_per_second,
            retreat_duration_milliseconds: *retreat_duration_milliseconds,
        },
        content_schema::CoreEnemyLocomotion::MaintainDistance {
            movement_speed_milli_tiles_per_second,
            preferred_distance_milli_tiles,
        } => CoreEnemyLocomotionParameters::MaintainDistance {
            movement_speed_milli_tiles_per_second: *movement_speed_milli_tiles_per_second,
            preferred_distance_milli_tiles: *preferred_distance_milli_tiles,
        },
        content_schema::CoreEnemyLocomotion::OrbitAnchor {
            movement_speed_milli_tiles_per_second,
            orbit_radius_milli_tiles,
        } => CoreEnemyLocomotionParameters::OrbitAnchor {
            movement_speed_milli_tiles_per_second: *movement_speed_milli_tiles_per_second,
            orbit_radius_milli_tiles: *orbit_radius_milli_tiles,
        },
        content_schema::CoreEnemyLocomotion::PursueStopChargeHome {
            movement_speed_milli_tiles_per_second,
            stop_distance_milli_tiles,
        } => CoreEnemyLocomotionParameters::PursueStopChargeHome {
            movement_speed_milli_tiles_per_second: *movement_speed_milli_tiles_per_second,
            stop_distance_milli_tiles: *stop_distance_milli_tiles,
        },
        content_schema::CoreEnemyLocomotion::Stationary => {
            CoreEnemyLocomotionParameters::Stationary
        }
    }
}

fn compile_pattern_warning(
    warning: &content_schema::CorePatternWarning,
) -> CorePatternWarningParameters {
    match *warning {
        content_schema::CorePatternWarning::Standalone {
            first_milliseconds,
            repeated_milliseconds,
        } => CorePatternWarningParameters::Standalone {
            first_milliseconds,
            repeated_milliseconds,
        },
        content_schema::CorePatternWarning::ParentOnly => CorePatternWarningParameters::ParentOnly,
        content_schema::CorePatternWarning::RecoveryPreview {
            ground_origin_warning_milliseconds,
            directional_gap_preview_milliseconds,
            major_audio,
        } => CorePatternWarningParameters::RecoveryPreview {
            ground_origin_warning_milliseconds,
            directional_gap_preview_milliseconds,
            major_audio,
        },
    }
}

fn compile_pattern_geometry(
    geometry: &content_schema::CoreAuthoredPatternGeometry,
) -> CorePatternGeometryParameters {
    match geometry {
        content_schema::CoreAuthoredPatternGeometry::Charge {
            distance_milli_tiles,
            duration_milliseconds,
        } => CorePatternGeometryParameters::Charge {
            distance_milli_tiles: *distance_milli_tiles,
            duration_milliseconds: *duration_milliseconds,
        },
        content_schema::CoreAuthoredPatternGeometry::AlternatingFan {
            first_offsets_milli_degrees,
            second_offsets_milli_degrees,
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
        } => CorePatternGeometryParameters::AlternatingFan {
            first_offsets_milli_degrees: first_offsets_milli_degrees.clone(),
            second_offsets_milli_degrees: second_offsets_milli_degrees.clone(),
            projectile_speed_milli_tiles_per_second: *projectile_speed_milli_tiles_per_second,
            range_milli_tiles: *range_milli_tiles,
            projectile_radius_milli_tiles: *projectile_radius_milli_tiles,
        },
        content_schema::CoreAuthoredPatternGeometry::RotatingArms {
            arm_count,
            clockwise_milli_degrees_per_second,
            emission_interval_milliseconds,
            active_duration_milliseconds,
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
        } => CorePatternGeometryParameters::RotatingArms {
            arm_count: *arm_count,
            clockwise_milli_degrees_per_second: *clockwise_milli_degrees_per_second,
            emission_interval_milliseconds: *emission_interval_milliseconds,
            active_duration_milliseconds: *active_duration_milliseconds,
            projectile_speed_milli_tiles_per_second: *projectile_speed_milli_tiles_per_second,
            range_milli_tiles: *range_milli_tiles,
            projectile_radius_milli_tiles: *projectile_radius_milli_tiles,
        },
        content_schema::CoreAuthoredPatternGeometry::ChargeLane {
            width_milli_tiles,
            length_milli_tiles,
            charge_duration_milliseconds,
        } => CorePatternGeometryParameters::ChargeLane {
            width_milli_tiles: *width_milli_tiles,
            length_milli_tiles: *length_milli_tiles,
            charge_duration_milliseconds: *charge_duration_milliseconds,
        },
        content_schema::CoreAuthoredPatternGeometry::RadialGap {
            index_count,
            omitted_adjacent_count,
            relation,
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
        } => CorePatternGeometryParameters::RadialGap {
            index_count: *index_count,
            omitted_adjacent_count: *omitted_adjacent_count,
            relation: match relation {
                content_schema::CoreRadialGapRelation::TargetOpposite => {
                    CoreRadialGapRelation::TargetOpposite
                }
                content_schema::CoreRadialGapRelation::TargetFacing => {
                    CoreRadialGapRelation::TargetFacing
                }
            },
            projectile_speed_milli_tiles_per_second: *projectile_speed_milli_tiles_per_second,
            range_milli_tiles: *range_milli_tiles,
            projectile_radius_milli_tiles: *projectile_radius_milli_tiles,
        },
        content_schema::CoreAuthoredPatternGeometry::ProjectileFan {
            shot_count,
            total_arc_milli_degrees,
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
        } => CorePatternGeometryParameters::ProjectileFan {
            shot_count: *shot_count,
            total_arc_milli_degrees: *total_arc_milli_degrees,
            projectile_speed_milli_tiles_per_second: *projectile_speed_milli_tiles_per_second,
            range_milli_tiles: *range_milli_tiles,
            projectile_radius_milli_tiles: *projectile_radius_milli_tiles,
        },
    }
}

const fn compile_pattern_counterplay(
    counterplay: content_schema::CorePatternCounterplay,
) -> Counterplay {
    match counterplay {
        content_schema::CorePatternCounterplay::Strafe => Counterplay::Strafe,
        content_schema::CorePatternCounterplay::FollowGap => Counterplay::FollowGap,
        content_schema::CorePatternCounterplay::LeaveTelegraph => Counterplay::LeaveTelegraph,
        content_schema::CorePatternCounterplay::MoveWithRotation => Counterplay::MoveWithRotation,
    }
}

const fn compile_pattern_memory(
    memory: content_schema::CorePatternMemoryFamily,
) -> EchoMemoryFamily {
    match memory {
        content_schema::CorePatternMemoryFamily::ChargeOrContact => {
            EchoMemoryFamily::ChargeOrContact
        }
        content_schema::CorePatternMemoryFamily::FanProjectile => EchoMemoryFamily::FanProjectile,
        content_schema::CorePatternMemoryFamily::RotatingProjectile => {
            EchoMemoryFamily::RotatingProjectile
        }
        content_schema::CorePatternMemoryFamily::RadialProjectile => {
            EchoMemoryFamily::RadialProjectile
        }
    }
}

const fn compile_pattern_disposition(
    disposition: content_schema::CorePatternDisposition,
) -> HostileDisposition {
    match disposition {
        content_schema::CorePatternDisposition::OneContactHitPerCast => {
            HostileDisposition::OneContactHitPerCast
        }
        content_schema::CorePatternDisposition::ConsumeOnPlayerOrSolid => {
            HostileDisposition::ConsumeOnPlayerOrSolid
        }
    }
}

const fn compile_attack_group_rule(
    rule: content_schema::CoreAttackGroupRule,
) -> CoreAttackGroupRule {
    match rule {
        content_schema::CoreAttackGroupRule::DistinctProjectileHitGroups => {
            CoreAttackGroupRule::DistinctProjectileHitGroups
        }
        content_schema::CoreAttackGroupRule::OneContactHitPerCast => {
            CoreAttackGroupRule::OneContactHitPerCast
        }
    }
}

fn compile_room(record: &CoreRoomTemplateRecord) -> Result<DungeonRoomDefinition> {
    DungeonRoomDefinition {
        id: record.header.id.to_string(),
        width_milli_tiles: record.width_milli_tiles,
        height_milli_tiles: record.height_milli_tiles,
        doors: record
            .doors
            .iter()
            .map(|door| DungeonDoorDefinition {
                id: door.id.clone(),
                side: compile_door_side(door.side),
                offset_milli_tiles: door.offset_milli_tiles,
                width_milli_tiles: door.width_milli_tiles,
            })
            .collect(),
        volumes: record
            .volumes
            .iter()
            .map(|volume| DungeonRoomVolume {
                id: volume.id.clone(),
                kind: compile_volume_kind(volume.kind),
                geometry: compile_volume_geometry(&volume.geometry),
            })
            .collect(),
        anchors: record
            .anchors
            .iter()
            .map(|anchor| DungeonRoomAnchor {
                id: anchor.id.clone(),
                kind: compile_anchor_kind(anchor.kind),
                x_milli_tiles: anchor.point.x,
                y_milli_tiles: anchor.point.y,
                bound_content_id: anchor.bound_content_id.as_ref().map(ToString::to_string),
            })
            .collect(),
        safe_noncombat: record.safe_noncombat,
    }
    .validated()
    .with_context(|| format!("compiled Bell room {} is invalid", record.header.id))
}

fn compile_fixed_layout(
    layout: &CoreFixedLayoutRecord,
    definitions: &[DungeonRoomDefinition],
) -> Result<FixedDungeonLayoutDefinition> {
    let definition_by_id = definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect::<std::collections::BTreeMap<_, _>>();
    let node_by_id = layout
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect::<std::collections::BTreeMap<_, _>>();

    let first_node_id = layout
        .main_chain_node_ids
        .first()
        .context("fixed Core layout has no main-chain entry")?;
    let first_node = node_by_id
        .get(first_node_id.as_str())
        .context("fixed Core layout entry node is missing")?;
    let first_definition = definition_by_id
        .get(first_node.room_template_id.as_str())
        .context("fixed Core layout entry room is missing")?;
    let mut rooms = vec![PlacedDungeonRoom {
        node_id: first_node.node_id.clone(),
        room: first_definition.rotated(first_node.rotation_degrees)?,
        origin_x_milli_tiles: 0,
        origin_y_milli_tiles: 0,
        counts_toward_room_total: first_node.counts_toward_six_room_total,
    }];
    let mut corridors = Vec::with_capacity(layout.edges.len());

    for edge in &layout.edges {
        let from = rooms
            .last()
            .context("fixed Core layout placement lost its prior room")?;
        let (room, corridor) = place_layout_edge(from, edge, &node_by_id, &definition_by_id)?;
        corridors.push(corridor);
        rooms.push(room);
    }

    reject_room_overlap(&rooms)?;
    FixedDungeonLayoutDefinition {
        id: layout.header.id.to_string(),
        rooms,
        corridors,
        disabled_branch_node_ids: layout.disabled_branch_node_ids.clone(),
    }
    .validated()
    .context("compiled Core fixed layout is invalid")
}

fn place_layout_edge(
    from: &PlacedDungeonRoom,
    edge: &content_schema::CoreFixedLayoutEdge,
    node_by_id: &std::collections::BTreeMap<&str, &content_schema::CoreFixedLayoutNode>,
    definition_by_id: &std::collections::BTreeMap<&str, &DungeonRoomDefinition>,
) -> Result<(PlacedDungeonRoom, DungeonCorridor)> {
    if from.node_id != edge.from_node_id {
        bail!("fixed Core layout edges are not ordered with the main chain");
    }
    let start = from.world_door(&edge.from_door_id)?;
    let to_node = node_by_id
        .get(edge.to_node_id.as_str())
        .with_context(|| format!("fixed Core layout is missing node {}", edge.to_node_id))?;
    let to_definition = definition_by_id
        .get(to_node.room_template_id.as_str())
        .with_context(|| {
            format!(
                "fixed Core layout is missing room {}",
                to_node.room_template_id
            )
        })?;
    let rotated = to_definition.rotated(to_node.rotation_degrees)?;
    let to_door = rotated.door(&edge.to_door_id).with_context(|| {
        format!(
            "node {} is missing door {}",
            to_node.node_id, edge.to_door_id
        )
    })?;
    if start.side.opposite() != to_door.side
        || start.width_milli_tiles != edge.corridor_width_milli_tiles
        || to_door.width_milli_tiles != edge.corridor_width_milli_tiles
    {
        bail!(
            "fixed Core layout edge {} to {} has incompatible doors",
            edge.from_node_id,
            edge.to_node_id
        );
    }
    let length_milli_tiles = u32::from(edge.corridor_length_tiles)
        .checked_mul(u32::try_from(MILLI_TILES_PER_TILE).expect("milli-tiles fit u32"))
        .context("fixed Core corridor length overflow")?;
    let length_i32 =
        i32::try_from(length_milli_tiles).context("fixed Core corridor length exceeds i32")?;
    let (direction_x, direction_y) = start.side.outward_unit();
    let end_x = start
        .x_milli_tiles
        .checked_add(direction_x.saturating_mul(length_i32))
        .context("fixed Core corridor x overflow")?;
    let end_y = start
        .y_milli_tiles
        .checked_add(direction_y.saturating_mul(length_i32))
        .context("fixed Core corridor y overflow")?;
    let origin_x = end_x
        .checked_sub(to_door.x_milli_tiles)
        .context("fixed Core room x placement overflow")?;
    let origin_y = end_y
        .checked_sub(to_door.y_milli_tiles)
        .context("fixed Core room y placement overflow")?;
    Ok((
        PlacedDungeonRoom {
            node_id: to_node.node_id.clone(),
            room: rotated,
            origin_x_milli_tiles: origin_x,
            origin_y_milli_tiles: origin_y,
            counts_toward_room_total: to_node.counts_toward_six_room_total,
        },
        DungeonCorridor {
            from_node_id: edge.from_node_id.clone(),
            to_node_id: edge.to_node_id.clone(),
            start_x_milli_tiles: start.x_milli_tiles,
            start_y_milli_tiles: start.y_milli_tiles,
            end_x_milli_tiles: end_x,
            end_y_milli_tiles: end_y,
            width_milli_tiles: edge.corridor_width_milli_tiles,
            length_milli_tiles,
        },
    ))
}

fn reject_room_overlap(rooms: &[PlacedDungeonRoom]) -> Result<()> {
    for (index, left) in rooms.iter().enumerate() {
        for right in rooms.iter().skip(index + 1) {
            let left_right =
                i64::from(left.origin_x_milli_tiles) + i64::from(left.room.width_milli_tiles);
            let left_bottom =
                i64::from(left.origin_y_milli_tiles) + i64::from(left.room.height_milli_tiles);
            let right_right =
                i64::from(right.origin_x_milli_tiles) + i64::from(right.room.width_milli_tiles);
            let right_bottom =
                i64::from(right.origin_y_milli_tiles) + i64::from(right.room.height_milli_tiles);
            let overlaps = i64::from(left.origin_x_milli_tiles) < right_right
                && i64::from(right.origin_x_milli_tiles) < left_right
                && i64::from(left.origin_y_milli_tiles) < right_bottom
                && i64::from(right.origin_y_milli_tiles) < left_bottom;
            if overlaps {
                bail!(
                    "fixed Core rooms {} and {} overlap",
                    left.node_id,
                    right.node_id
                );
            }
        }
    }
    Ok(())
}

const fn compile_door_side(side: CoreRoomDoorSide) -> DungeonDoorSide {
    match side {
        CoreRoomDoorSide::North => DungeonDoorSide::North,
        CoreRoomDoorSide::East => DungeonDoorSide::East,
        CoreRoomDoorSide::South => DungeonDoorSide::South,
        CoreRoomDoorSide::West => DungeonDoorSide::West,
    }
}

const fn compile_volume_kind(kind: CoreRoomVolumeKind) -> DungeonRoomVolumeKind {
    match kind {
        CoreRoomVolumeKind::Solid => DungeonRoomVolumeKind::Solid,
        CoreRoomVolumeKind::DeepWater => DungeonRoomVolumeKind::DeepWater,
        CoreRoomVolumeKind::WalkableBoundary => DungeonRoomVolumeKind::WalkableBoundary,
        CoreRoomVolumeKind::PatternLane => DungeonRoomVolumeKind::PatternLane,
        CoreRoomVolumeKind::ObjectiveArea => DungeonRoomVolumeKind::ObjectiveArea,
    }
}

fn compile_volume_geometry(geometry: &CoreRoomVolumeGeometry) -> DungeonRoomVolumeGeometry {
    match geometry {
        CoreRoomVolumeGeometry::Rectangle { rectangle } => DungeonRoomVolumeGeometry::Rectangle {
            x: rectangle.x,
            y: rectangle.y,
            width: rectangle.width,
            height: rectangle.height,
        },
        CoreRoomVolumeGeometry::Circle { circle } => DungeonRoomVolumeGeometry::Circle {
            x: circle.center.x,
            y: circle.center.y,
            radius: circle.radius,
        },
        CoreRoomVolumeGeometry::Polyline {
            width_milli_tiles,
            points,
        } => DungeonRoomVolumeGeometry::Polyline {
            width_milli_tiles: *width_milli_tiles,
            points: points.iter().map(|point| (point.x, point.y)).collect(),
        },
    }
}

const fn compile_anchor_kind(kind: CoreRoomAnchorKind) -> DungeonAnchorKind {
    match kind {
        CoreRoomAnchorKind::SafeEntry => DungeonAnchorKind::SafeEntry,
        CoreRoomAnchorKind::Exit => DungeonAnchorKind::Exit,
        CoreRoomAnchorKind::Fodder => DungeonAnchorKind::Fodder,
        CoreRoomAnchorKind::Pressure => DungeonAnchorKind::Pressure,
        CoreRoomAnchorKind::Disruptor => DungeonAnchorKind::Disruptor,
        CoreRoomAnchorKind::AnchorEnemy => DungeonAnchorKind::AnchorEnemy,
        CoreRoomAnchorKind::Miniboss => DungeonAnchorKind::Miniboss,
        CoreRoomAnchorKind::Stage => DungeonAnchorKind::Stage,
        CoreRoomAnchorKind::Add => DungeonAnchorKind::Add,
        CoreRoomAnchorKind::Shrine => DungeonAnchorKind::Shrine,
        CoreRoomAnchorKind::Stabilization => DungeonAnchorKind::Stabilization,
        CoreRoomAnchorKind::Chest => DungeonAnchorKind::Chest,
        CoreRoomAnchorKind::Boss => DungeonAnchorKind::Boss,
        CoreRoomAnchorKind::ChargeEndpoint => DungeonAnchorKind::ChargeEndpoint,
        CoreRoomAnchorKind::Group => DungeonAnchorKind::Group,
    }
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
    validate_authored_behaviors(records)?;

    validate_rooms(target, &records.rooms)?;
    validate_pack(&records.packs)?;
    validate_layout(&records.layouts, &records.rooms)?;
    validate_encounter_anchor_capacity(records)?;
    Ok(())
}

fn validate_authored_behaviors(records: &CoreEncounterRoomRecords) -> Result<()> {
    let behavior_ids = records
        .authored_behaviors
        .iter()
        .map(|record| record.owner_id.as_str())
        .collect::<Vec<_>>();
    if behavior_ids != AUTHORED_BEHAVIOR_IDS {
        bail!("Core authored behavior allowlist is not exact");
    }
    let health = [70, 160, 150, 1_600, 1_900];
    let armor = [0, 2, 1, 8, 6];
    let roles = [
        content_schema::CoreEnemyRole::Fodder,
        content_schema::CoreEnemyRole::Pressure,
        content_schema::CoreEnemyRole::Disruptor,
        content_schema::CoreEnemyRole::Elite,
        content_schema::CoreEnemyRole::Elite,
    ];
    let state_sequence = [
        content_schema::CoreEnemyStateStage::SpawnTelegraph,
        content_schema::CoreEnemyStateStage::Acquire,
        content_schema::CoreEnemyStateStage::MoveOrPosition,
        content_schema::CoreEnemyStateStage::Telegraph,
        content_schema::CoreEnemyStateStage::Attack,
        content_schema::CoreEnemyStateStage::Recover,
        content_schema::CoreEnemyStateStage::Acquire,
    ];
    for (index, behavior) in records.authored_behaviors.iter().enumerate() {
        let miniboss = index >= 3;
        if behavior.role != roles[index]
            || behavior.state_sequence != state_sequence
            || behavior.target_selection
                != content_schema::CoreTargetSelection::NearestLivingDamageableInAggroTieLowestEntityId
            || behavior.telegraph_lock
                != content_schema::CoreTelegraphLock::AimAndPositionAtTelegraphStart
            || behavior.maximum_health != health[index]
            || behavior.armor != armor[index]
            || behavior.collision_radius_milli_tiles != if miniboss { 550 } else { 350 }
            || behavior.hurtbox_radius_milli_tiles != if miniboss { 480 } else { 300 }
            || behavior.aggro_radius_milli_tiles != 12_000
            || behavior.leash_radius_milli_tiles != 16_000
            || behavior.target_reacquire_milliseconds != 250
            || behavior.no_target_reset_milliseconds != 5_000
            || behavior.spawn_warning_milliseconds != 900
            || behavior.spawn_invulnerability_milliseconds != 1_000
            || behavior.introduction_milliseconds != if miniboss { 3_000 } else { 0 }
            || behavior.contact_damage != 0
            || behavior.drop_reward_on_reset
            || !locomotion_is_exact(index, &behavior.locomotion)
        {
            bail!(
                "authored behavior {} drifted from its exact Core row",
                behavior.owner_id
            );
        }
    }
    let flattened = records
        .authored_behaviors
        .iter()
        .flat_map(|behavior| behavior.pattern_ids.iter())
        .map(ContentId::as_str)
        .collect::<Vec<_>>();
    if flattened != AUTHORED_PATTERN_IDS {
        bail!("Core authored behavior pattern ownership is not exact");
    }
    validate_authored_patterns(&records.authored_patterns)
}

fn locomotion_is_exact(index: usize, locomotion: &content_schema::CoreEnemyLocomotion) -> bool {
    match (index, locomotion) {
        (
            0,
            content_schema::CoreEnemyLocomotion::RushRetreat {
                approach_speed_milli_tiles_per_second,
                trigger_distance_milli_tiles,
                charge_distance_milli_tiles,
                charge_duration_milliseconds,
                retreat_speed_milli_tiles_per_second,
                retreat_duration_milliseconds,
            },
        ) => {
            *approach_speed_milli_tiles_per_second == 3_000
                && *trigger_distance_milli_tiles == 2_500
                && *charge_distance_milli_tiles == 2_000
                && *charge_duration_milliseconds == 500
                && *retreat_speed_milli_tiles_per_second == 3_500
                && *retreat_duration_milliseconds == 1_500
        }
        (
            1,
            content_schema::CoreEnemyLocomotion::MaintainDistance {
                movement_speed_milli_tiles_per_second,
                preferred_distance_milli_tiles,
            },
        ) => {
            *movement_speed_milli_tiles_per_second == 3_000
                && *preferred_distance_milli_tiles == 6_000
        }
        (
            2,
            content_schema::CoreEnemyLocomotion::OrbitAnchor {
                movement_speed_milli_tiles_per_second,
                orbit_radius_milli_tiles,
            },
        ) => *movement_speed_milli_tiles_per_second == 2_800 && *orbit_radius_milli_tiles == 3_000,
        (
            3,
            content_schema::CoreEnemyLocomotion::PursueStopChargeHome {
                movement_speed_milli_tiles_per_second,
                stop_distance_milli_tiles,
            },
        ) => *movement_speed_milli_tiles_per_second == 2_400 && *stop_distance_milli_tiles == 3_500,
        (4, content_schema::CoreEnemyLocomotion::Stationary) => true,
        _ => false,
    }
}

#[allow(clippy::too_many_lines)]
fn validate_authored_patterns(
    patterns: &[content_schema::CoreAuthoredPatternRecord],
) -> Result<()> {
    let ids = patterns
        .iter()
        .map(|pattern| pattern.id.as_str())
        .collect::<Vec<_>>();
    if ids != AUTHORED_PATTERN_IDS {
        bail!("Core authored pattern allowlist is not exact");
    }
    let owners = [
        "enemy.mire_leech",
        "enemy.bell_acolyte",
        "enemy.choir_skull",
        "miniboss.sepulcher_knight",
        "miniboss.sepulcher_knight",
        "miniboss.sepulcher_knight",
        "miniboss.choir_abbot",
        "miniboss.choir_abbot",
    ];
    let damage_types = [
        content_schema::DamageType::Physical,
        content_schema::DamageType::Veil,
        content_schema::DamageType::Veil,
        content_schema::DamageType::Physical,
        content_schema::DamageType::Physical,
        content_schema::DamageType::Physical,
        content_schema::DamageType::Veil,
        content_schema::DamageType::Veil,
    ];
    let bands = [
        content_schema::DamageBand::Pressure,
        content_schema::DamageBand::Pressure,
        content_schema::DamageBand::Pressure,
        content_schema::DamageBand::Major,
        content_schema::DamageBand::Pressure,
        content_schema::DamageBand::Pressure,
        content_schema::DamageBand::Pressure,
        content_schema::DamageBand::Major,
    ];
    let damage = [12, 16, 14, 34, 20, 18, 18, 26];
    let threat = [2, 7, 10, 8, 8, 5, 12, 12];
    let cycle = [2_500, 1_800, 6_000, 6_000, 6_000, 2_200, 6_000, 6_000];
    let quiet = [1_500, 0, 2_000, 0, 0, 0, 2_500, 0];
    let active = [1, 5, 8, 1, 8, 5, 10, 12];
    let counterplay = [
        content_schema::CorePatternCounterplay::LeaveTelegraph,
        content_schema::CorePatternCounterplay::Strafe,
        content_schema::CorePatternCounterplay::MoveWithRotation,
        content_schema::CorePatternCounterplay::LeaveTelegraph,
        content_schema::CorePatternCounterplay::FollowGap,
        content_schema::CorePatternCounterplay::Strafe,
        content_schema::CorePatternCounterplay::MoveWithRotation,
        content_schema::CorePatternCounterplay::FollowGap,
    ];
    let memories = [
        content_schema::CorePatternMemoryFamily::ChargeOrContact,
        content_schema::CorePatternMemoryFamily::FanProjectile,
        content_schema::CorePatternMemoryFamily::RotatingProjectile,
        content_schema::CorePatternMemoryFamily::ChargeOrContact,
        content_schema::CorePatternMemoryFamily::RadialProjectile,
        content_schema::CorePatternMemoryFamily::FanProjectile,
        content_schema::CorePatternMemoryFamily::RotatingProjectile,
        content_schema::CorePatternMemoryFamily::RadialProjectile,
    ];
    for (index, pattern) in patterns.iter().enumerate() {
        let charge = matches!(index, 0 | 3);
        let pattern_id = pattern.id.as_str();
        let expected_telegraph = format!("{pattern_id}.telegraph");
        let expected_warning = format!("{pattern_id}.warning");
        let expected_major_warning = format!("{expected_warning}.major");
        if pattern.owner_id.as_str() != owners[index]
            || pattern.telegraph_id.as_str() != expected_telegraph
            || pattern.audio_cue_id.as_str() != expected_warning
            || pattern.major_audio_cue_id.as_ref().map(ContentId::as_str)
                != bands[index]
                    .eq(&content_schema::DamageBand::Major)
                    .then_some(expected_major_warning.as_str())
            || pattern.damage_type != damage_types[index]
            || pattern.damage_band != bands[index]
            || pattern.raw_damage != damage[index]
            || pattern.threat_cost != threat[index]
            || pattern.cycle_milliseconds != cycle[index]
            || pattern.quiet_milliseconds != quiet[index]
            || pattern.counterplay != counterplay[index]
            || pattern.memory_family != memories[index]
            || pattern.disposition
                != if charge {
                    content_schema::CorePatternDisposition::OneContactHitPerCast
                } else {
                    content_schema::CorePatternDisposition::ConsumeOnPlayerOrSolid
                }
            || pattern.attack_group_rule
                != if charge {
                    content_schema::CoreAttackGroupRule::OneContactHitPerCast
                } else {
                    content_schema::CoreAttackGroupRule::DistinctProjectileHitGroups
                }
            || pattern.acceleration_milli_tiles_per_second_squared != 0
            || pattern.pierces_players
            || !pattern.statuses.is_empty()
            || !pattern.cancel_on_phase_change
            || pattern.maximum_active_instances != active[index]
            || !warning_is_exact(index, &pattern.warning)
            || !pattern_geometry_is_exact(index, &pattern.geometry)
        {
            bail!(
                "authored pattern {} drifted from its exact Core row",
                pattern.id
            );
        }
    }
    Ok(())
}

fn warning_is_exact(index: usize, warning: &content_schema::CorePatternWarning) -> bool {
    matches!(
        (index, warning),
        (
            0 | 1 | 5,
            content_schema::CorePatternWarning::Standalone {
                first_milliseconds: 400,
                repeated_milliseconds: 300,
            },
        ) | (
            2 | 6,
            content_schema::CorePatternWarning::Standalone {
                first_milliseconds: 650,
                repeated_milliseconds: 500,
            },
        ) | (
            3,
            content_schema::CorePatternWarning::Standalone {
                first_milliseconds: 900,
                repeated_milliseconds: 900,
            },
        ) | (4, content_schema::CorePatternWarning::ParentOnly)
            | (
                7,
                content_schema::CorePatternWarning::RecoveryPreview {
                    ground_origin_warning_milliseconds: 2_500,
                    directional_gap_preview_milliseconds: 650,
                    major_audio: true,
                },
            )
    )
}

#[allow(clippy::too_many_lines)]
fn pattern_geometry_is_exact(
    index: usize,
    geometry: &content_schema::CoreAuthoredPatternGeometry,
) -> bool {
    if let (
        1,
        content_schema::CoreAuthoredPatternGeometry::AlternatingFan {
            first_offsets_milli_degrees,
            second_offsets_milli_degrees,
            projectile_speed_milli_tiles_per_second: 6_000,
            range_milli_tiles: 9_000,
            projectile_radius_milli_tiles: 110,
        },
    ) = (index, geometry)
    {
        return first_offsets_milli_degrees == &[-50_000, -35_000, -20_000, -5_000, 10_000]
            && second_offsets_milli_degrees == &[-10_000, 5_000, 20_000, 35_000, 50_000];
    }
    matches!(
        (index, geometry),
        (
            0,
            content_schema::CoreAuthoredPatternGeometry::Charge {
                distance_milli_tiles: 2_000,
                duration_milliseconds: 500,
            },
        ) | (
            3,
            content_schema::CoreAuthoredPatternGeometry::ChargeLane {
                width_milli_tiles: 1_000,
                length_milli_tiles: 5_000,
                charge_duration_milliseconds: 550,
            },
        ) | (
            5,
            content_schema::CoreAuthoredPatternGeometry::ProjectileFan {
                shot_count: 5,
                total_arc_milli_degrees: 50_000,
                projectile_speed_milli_tiles_per_second: 6_000,
                range_milli_tiles: 8_000,
                projectile_radius_milli_tiles: 120,
            },
        ) | (
            2,
            content_schema::CoreAuthoredPatternGeometry::RotatingArms {
                arm_count: 2,
                clockwise_milli_degrees_per_second: 35_000,
                emission_interval_milliseconds: 400,
                active_duration_milliseconds: 4_000,
                projectile_speed_milli_tiles_per_second: 4_500,
                range_milli_tiles: 7_000,
                projectile_radius_milli_tiles: 120,
            },
        ) | (
            6,
            content_schema::CoreAuthoredPatternGeometry::RotatingArms {
                arm_count: 2,
                clockwise_milli_degrees_per_second: 35_000,
                emission_interval_milliseconds: 350,
                active_duration_milliseconds: 3_500,
                projectile_speed_milli_tiles_per_second: 4_500,
                range_milli_tiles: 7_000,
                projectile_radius_milli_tiles: 120,
            },
        ) | (
            4,
            content_schema::CoreAuthoredPatternGeometry::RadialGap {
                index_count: 10,
                omitted_adjacent_count: 2,
                relation: content_schema::CoreRadialGapRelation::TargetOpposite,
                projectile_speed_milli_tiles_per_second: 5_000,
                range_milli_tiles: 8_000,
                projectile_radius_milli_tiles: 120,
            },
        ) | (
            7,
            content_schema::CoreAuthoredPatternGeometry::RadialGap {
                index_count: 16,
                omitted_adjacent_count: 4,
                relation: content_schema::CoreRadialGapRelation::TargetFacing,
                projectile_speed_milli_tiles_per_second: 4_500,
                range_milli_tiles: 8_000,
                projectile_radius_milli_tiles: 120,
            },
        )
    )
}

fn validate_encounter_anchor_capacity(records: &CoreEncounterRoomRecords) -> Result<()> {
    let room_by_id = records
        .rooms
        .iter()
        .map(|room| (room.header.id.as_str(), room))
        .collect::<std::collections::BTreeMap<_, _>>();
    let roster_by_id = records
        .roster
        .iter()
        .map(|member| (member.header.id.as_str(), member))
        .collect::<std::collections::BTreeMap<_, _>>();
    for node in &records.layouts[0].nodes {
        let Some(encounter) = &node.encounter else {
            continue;
        };
        let room = room_by_id
            .get(node.room_template_id.as_str())
            .with_context(|| format!("node {} references a missing room", node.node_id))?;
        for member in &encounter.members {
            let roster = roster_by_id
                .get(member.enemy_id.as_str())
                .with_context(|| {
                    format!("node {} references a missing roster member", node.node_id)
                })?;
            let available = if roster.rank == CoreEncounterRank::Miniboss {
                room.anchors
                    .iter()
                    .filter(|anchor| {
                        anchor.kind == CoreRoomAnchorKind::Miniboss
                            || anchor.bound_content_id.as_ref() == Some(&member.enemy_id)
                    })
                    .count()
            } else {
                let required_kind = if roster.header.tags.iter().any(|tag| tag == "fodder") {
                    CoreRoomAnchorKind::Fodder
                } else if roster.header.tags.iter().any(|tag| tag == "pressure") {
                    CoreRoomAnchorKind::Pressure
                } else if roster.header.tags.iter().any(|tag| tag == "disruptor") {
                    CoreRoomAnchorKind::Disruptor
                } else if roster.header.tags.iter().any(|tag| tag == "anchor") {
                    CoreRoomAnchorKind::AnchorEnemy
                } else {
                    bail!("{} has no supported room role tag", roster.header.id);
                };
                room.anchors
                    .iter()
                    .filter(|anchor| anchor.kind == required_kind)
                    .count()
            };
            if available < usize::from(member.count) {
                bail!(
                    "node {} lacks role-compatible anchors for {}",
                    node.node_id,
                    member.enemy_id
                );
            }
        }
    }
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
        let source_id = asset.source_record_id.as_str();
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
            content_schema::CoreEncounterRoomAssetKind::Telegraph => {
                id == format!("{source_id}.telegraph")
            }
            content_schema::CoreEncounterRoomAssetKind::WarningAudio => {
                id == format!("{source_id}.warning")
            }
            content_schema::CoreEncounterRoomAssetKind::MajorWarningAudio => {
                MAJOR_PATTERN_IDS.contains(&source_id) && id == format!("{source_id}.warning.major")
            }
        };
        if !kind_matches {
            bail!("asset {} has an incompatible typed role", asset.asset_id);
        }
    }
    let pattern_asset_sources = |kind| {
        assets
            .assets
            .iter()
            .filter(|asset| asset.kind == kind)
            .map(|asset| asset.source_record_id.as_str())
            .collect::<Vec<_>>()
    };
    if pattern_asset_sources(content_schema::CoreEncounterRoomAssetKind::Telegraph) != PATTERN_IDS
        || pattern_asset_sources(content_schema::CoreEncounterRoomAssetKind::WarningAudio)
            != PATTERN_IDS
    {
        bail!("Core pattern telegraph and warning-audio closure is not exact");
    }
    let major_sources = assets
        .assets
        .iter()
        .filter(|asset| asset.kind == content_schema::CoreEncounterRoomAssetKind::MajorWarningAudio)
        .map(|asset| asset.source_record_id.as_str())
        .collect::<Vec<_>>();
    if major_sources != MAJOR_PATTERN_IDS {
        bail!("Core Major pattern audio closure is not exact");
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
    use sim_core::{
        CoreEnemyKitEvent, CoreEnemyKitScheduler, CorePatternGeometryDefinition,
        CorePatternWarningDefinition, EnemyHealthActor, EnemyHealthKind, EntityId,
        SimulationVector, Tick,
    };

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

    fn authored_definition(
        compiled: &CoreDevelopmentEncounterRooms,
        content_id: &str,
    ) -> sim_core::CoreEnemyDefinition {
        compiled
            .actor_definitions()
            .iter()
            .find(|actor| actor.id().as_str() == content_id)
            .and_then(|actor| match actor.behavior() {
                CoreEncounterBehaviorDefinition::Authored(definition) => Some(definition.clone()),
                CoreEncounterBehaviorDefinition::ImmutableDrownedPilgrim(_)
                | CoreEncounterBehaviorDefinition::ImmutableBellReed(_)
                | CoreEncounterBehaviorDefinition::ImmutableChainSentry(_) => None,
            })
            .expect("exact authored actor")
    }

    fn trace_kit(mut scheduler: CoreEnemyKitScheduler, final_tick: u64) -> Vec<CoreEnemyKitEvent> {
        let mut events = Vec::new();
        while scheduler.tick().0 <= final_tick {
            events.extend(scheduler.advance(true).expect("kit tick"));
        }
        events
    }

    #[test]
    fn authored_room_actors_enter_shared_health_and_reward_authority_exactly() {
        let compiled =
            load_core_development_encounter_rooms(&content_root()).expect("encounter rooms");
        let cases = [
            (
                "enemy.mire_leech",
                EnemyHealthKind::MireLeech,
                70,
                0,
                "reward.normal_outer",
            ),
            (
                "enemy.bell_acolyte",
                EnemyHealthKind::BellAcolyte,
                160,
                2,
                "reward.normal_outer",
            ),
            (
                "enemy.choir_skull",
                EnemyHealthKind::ChoirSkull,
                150,
                1,
                "reward.normal_outer",
            ),
            (
                "miniboss.sepulcher_knight",
                EnemyHealthKind::SepulcherKnight,
                1_600,
                8,
                "reward.miniboss_t1",
            ),
            (
                "miniboss.choir_abbot",
                EnemyHealthKind::ChoirAbbot,
                1_900,
                6,
                "reward.miniboss_t1",
            ),
        ];
        for (index, (content_id, kind, health, armor, reward)) in cases.into_iter().enumerate() {
            let actor = EnemyHealthActor::core_authored(
                EntityId::new(1_000 + u64::try_from(index).expect("index")).expect("entity ID"),
                &authored_definition(&compiled, content_id),
                SimulationVector::new(5.0, 5.0),
                Tick(100),
            )
            .expect("authored health actor");
            assert_eq!(actor.kind(), kind);
            assert_eq!(actor.max_health(), health);
            assert_eq!(actor.armor(), armor);
            assert_eq!(actor.reward_table_id(), reward);
            assert_eq!(actor.damageable_at(), Tick(130));
        }
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the exact room, roster, tick, and scheduler signatures stay in one integration fixture"
    )]
    fn checked_in_encounter_rooms_compile_exactly() {
        let compiled = load_core_development_encounter_rooms(&content_root())
            .expect("checked-in encounter rooms");
        assert_eq!(compiled.roster().len(), 8);
        assert_eq!(compiled.actor_definitions().len(), 8);
        assert_eq!(
            compiled
                .actor_definitions()
                .iter()
                .map(|definition| definition.id().as_str())
                .collect::<Vec<_>>(),
            NORMAL_IDS
                .into_iter()
                .chain(MINIBOSS_IDS)
                .collect::<Vec<_>>()
        );
        assert!(matches!(
            compiled.actor_definitions()[0].behavior(),
            CoreEncounterBehaviorDefinition::ImmutableDrownedPilgrim(_)
        ));
        assert!(matches!(
            compiled.actor_definitions()[2].behavior(),
            CoreEncounterBehaviorDefinition::ImmutableBellReed(_)
        ));
        assert!(matches!(
            compiled.actor_definitions()[4].behavior(),
            CoreEncounterBehaviorDefinition::ImmutableChainSentry(_)
        ));
        let authored_pattern_counts = compiled
            .actor_definitions()
            .iter()
            .filter_map(|definition| match definition.behavior() {
                CoreEncounterBehaviorDefinition::Authored(authored) => {
                    Some(authored.parameters().patterns.len())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(authored_pattern_counts, [1, 1, 1, 3, 2]);
        let authored = compiled
            .actor_definitions()
            .iter()
            .filter_map(|definition| match definition.behavior() {
                CoreEncounterBehaviorDefinition::Authored(authored) => Some(authored),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(authored.iter().all(|definition| {
            definition.target_reacquire_ticks() == 8
                && definition.no_target_reset_ticks() == 150
                && definition.spawn_warning_ticks() == 27
                && definition.spawn_invulnerability_ticks() == 30
        }));
        assert_eq!(
            authored
                .iter()
                .map(|definition| definition.introduction_ticks())
                .collect::<Vec<_>>(),
            [0, 0, 0, 90, 90]
        );
        let patterns = authored
            .iter()
            .flat_map(|definition| definition.parameters().patterns.iter())
            .collect::<Vec<_>>();
        assert_eq!(
            patterns
                .iter()
                .map(|pattern| pattern.cycle_ticks())
                .collect::<Vec<_>>(),
            [75, 54, 180, 180, 180, 66, 180, 180]
        );
        assert_eq!(
            patterns
                .iter()
                .map(|pattern| pattern.quiet_ticks())
                .collect::<Vec<_>>(),
            [45, 0, 60, 0, 0, 0, 75, 0]
        );
        assert_eq!(
            patterns
                .iter()
                .map(|pattern| pattern.traced_maximum_active_instances())
                .collect::<Vec<_>>(),
            [1, 5, 8, 1, 8, 5, 10, 12]
        );
        assert!(matches!(
            patterns[0].geometry(),
            CorePatternGeometryDefinition::Charge {
                duration_ticks: 15,
                ..
            }
        ));
        assert!(matches!(
            patterns[1].geometry(),
            CorePatternGeometryDefinition::AlternatingFan {
                projectile_lifetime_ticks: 45,
                ..
            }
        ));
        assert!(matches!(
            patterns[2].geometry(),
            CorePatternGeometryDefinition::RotatingArms {
                emission_interval_ticks: 12,
                active_ticks: 120,
                projectile_lifetime_ticks: 47,
                ..
            }
        ));
        assert!(matches!(
            patterns[3].geometry(),
            CorePatternGeometryDefinition::ChargeLane {
                charge_ticks: 17,
                ..
            }
        ));
        assert!(matches!(
            patterns[6].geometry(),
            CorePatternGeometryDefinition::RotatingArms {
                emission_interval_ticks: 11,
                active_ticks: 105,
                projectile_lifetime_ticks: 47,
                ..
            }
        ));
        assert!(matches!(
            patterns[7].geometry(),
            CorePatternGeometryDefinition::RadialGap {
                projectile_lifetime_ticks: 54,
                ..
            }
        ));
        assert!(matches!(
            patterns[2].warning(),
            CorePatternWarningDefinition::Standalone {
                first_ticks: 20,
                repeated_ticks: 15,
            }
        ));
        assert!(matches!(
            patterns[7].warning(),
            CorePatternWarningDefinition::RecoveryPreview {
                ground_origin_warning_ticks: 75,
                directional_gap_preview_ticks: 20,
                major_audio: true,
            }
        ));
        let com006 = compiled
            .solve_authored_min_speed_paths()
            .expect("Core authored COM-006 routes");
        assert_eq!(com006.player_speed_milli_tiles_per_second, 4_500);
        assert_eq!(com006.player_hurtbox_radius_milli_tiles, 250);
        assert_eq!(com006.round_trip_latency_milliseconds, 120);
        assert!(!com006.movement_ability_used);
        assert_eq!(com006.routes.len(), 8);
        assert_eq!(
            com006
                .routes
                .iter()
                .map(|route| route.safe_corridor_milli_tiles)
                .collect::<Vec<_>>(),
            [800, 1_346, 800, 800, 1_378, 800, 800, 1_422]
        );
        assert_eq!(
            com006
                .routes
                .iter()
                .filter_map(|route| route
                    .projectile
                    .as_ref()
                    .map(|proof| proof.projectile_arrival_milliseconds))
                .collect::<Vec<_>>(),
            [1_000, 667, 350, 584, 667, 350]
        );
        assert!(com006.routes.iter().all(|route| {
            route.encounter_projectile_cap == 300
                && route.projectile.as_ref().is_none_or(|proof| {
                    proof.minimum_start_distance_milli_tiles >= 1_250
                        || proof.ground_origin_warning_milliseconds >= 750
                })
        }));
        assert_eq!(compiled.rooms().len(), 9);
        assert_eq!(compiled.pack_bell_01().base_budget, 12);
        assert_eq!(compiled.fixed_layout().main_chain_node_ids, MAIN_CHAIN);
        let room_definitions = compiled
            .compile_room_definitions()
            .expect("room definitions");
        assert_eq!(room_definitions.len(), 9);
        let layout = compiled
            .compile_fixed_layout_definition()
            .expect("fixed layout definition");
        assert_eq!(layout.rooms.len(), 7);
        assert_eq!(layout.corridors.len(), 6);
        assert_eq!(layout.disabled_branch_node_ids, ["BB1", "BS1"]);
        assert_eq!(
            (
                layout.rooms[2].room.width_milli_tiles,
                layout.rooms[2].room.height_milli_tiles,
                layout.rooms[2].room.rotation_degrees,
            ),
            (21_000, 15_000, 90)
        );
        assert!(layout.corridors.iter().all(|corridor| {
            corridor.width_milli_tiles == 3_000 && corridor.length_milli_tiles == 4_000
        }));
        assert_eq!(
            layout
                .rooms
                .iter()
                .filter(|room| room.counts_toward_room_total)
                .count(),
            6
        );
        assert_eq!(
            layout.deterministic_digest(),
            "0de16de0531d5a1eee7bdc139c4a34e5b7ce83be68bea8676abab2bc32a8a88c"
        );
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
    fn compiler_rejects_authored_behavior_and_pattern_drift() {
        let mut case = fixture();
        case.records.authored_behaviors[0].spawn_invulnerability_milliseconds += 1;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.authored_behaviors[0].state_sequence.swap(3, 4);
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.authored_behaviors[1].locomotion =
            content_schema::CoreEnemyLocomotion::Stationary;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.authored_patterns[0].raw_damage += 1;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.authored_patterns[0].attack_group_rule =
            content_schema::CoreAttackGroupRule::DistinctProjectileHitGroups;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.authored_patterns[1].telegraph_id =
            ContentId::parse("pattern.enemy.bell_acolyte.alternating_fan.invented")
                .expect("test ID");
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.authored_patterns[2].warning = content_schema::CorePatternWarning::ParentOnly;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.authored_patterns[3].geometry =
            content_schema::CoreAuthoredPatternGeometry::ChargeLane {
                width_milli_tiles: 1_001,
                length_milli_tiles: 5_000,
                charge_duration_milliseconds: 550,
            };
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.authored_patterns[6].maximum_active_instances += 1;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.authored_patterns[7].major_audio_cue_id = None;
        assert!(compile_fixture(&case).is_err());
    }

    #[test]
    fn compiler_rejects_room_geometry_and_layout_drift() {
        let mut case = fixture();
        case.records.rooms[1].width_milli_tiles += 1;
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.rooms[1]
            .anchors
            .retain(|anchor| anchor.id != "p2");
        assert!(compile_fixture(&case).is_err());

        let mut case = fixture();
        case.records.rooms[1]
            .volumes
            .push(content_schema::CoreRoomVolume {
                id: "solid.partition".to_owned(),
                kind: CoreRoomVolumeKind::Solid,
                geometry: CoreRoomVolumeGeometry::Rectangle {
                    rectangle: content_schema::MilliTileRectangle {
                        x: 8_000,
                        y: 0,
                        width: 1_000,
                        height: 17_000,
                    },
                },
            });
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

    #[test]
    fn mire_and_acolyte_schedulers_preserve_exact_cycles_and_first_use_state() {
        let compiled = load_core_development_encounter_rooms(&content_root()).expect("content");
        let mire = trace_kit(
            CoreEnemyKitScheduler::new(authored_definition(&compiled, "enemy.mire_leech"))
                .expect("Mire scheduler"),
            84,
        );
        assert_eq!(
            mire,
            vec![
                CoreEnemyKitEvent::TelegraphDue {
                    tick: Tick(0),
                    pattern_index: 0,
                    warning_ticks: 12,
                    first_use: true,
                },
                CoreEnemyKitEvent::MireChargeDue {
                    tick: Tick(12),
                    pattern_index: 0,
                    distance_milli_tiles: 2_000,
                    duration_ticks: 15,
                },
                CoreEnemyKitEvent::MireRetreatDue {
                    tick: Tick(27),
                    speed_milli_tiles_per_second: 3_500,
                    duration_ticks: 45,
                },
                CoreEnemyKitEvent::TelegraphDue {
                    tick: Tick(75),
                    pattern_index: 0,
                    warning_ticks: 9,
                    first_use: false,
                },
                CoreEnemyKitEvent::MireChargeDue {
                    tick: Tick(84),
                    pattern_index: 0,
                    distance_milli_tiles: 2_000,
                    duration_ticks: 15,
                },
            ]
        );

        let acolyte = trace_kit(
            CoreEnemyKitScheduler::new(authored_definition(&compiled, "enemy.bell_acolyte"))
                .expect("Acolyte scheduler"),
            117,
        );
        assert_eq!(
            acolyte,
            vec![
                CoreEnemyKitEvent::TelegraphDue {
                    tick: Tick(0),
                    pattern_index: 0,
                    warning_ticks: 12,
                    first_use: true,
                },
                CoreEnemyKitEvent::AcolyteFanDue {
                    tick: Tick(12),
                    pattern_index: 0,
                    offsets_milli_degrees: vec![-50_000, -35_000, -20_000, -5_000, 10_000],
                },
                CoreEnemyKitEvent::TelegraphDue {
                    tick: Tick(54),
                    pattern_index: 0,
                    warning_ticks: 9,
                    first_use: false,
                },
                CoreEnemyKitEvent::AcolyteFanDue {
                    tick: Tick(63),
                    pattern_index: 0,
                    offsets_milli_degrees: vec![-10_000, 5_000, 20_000, 35_000, 50_000],
                },
                CoreEnemyKitEvent::TelegraphDue {
                    tick: Tick(108),
                    pattern_index: 0,
                    warning_ticks: 9,
                    first_use: false,
                },
                CoreEnemyKitEvent::AcolyteFanDue {
                    tick: Tick(117),
                    pattern_index: 0,
                    offsets_milli_degrees: vec![-50_000, -35_000, -20_000, -5_000, 10_000],
                },
            ]
        );
    }

    #[test]
    fn knight_scheduler_resets_fan_phase_inside_each_charge_loop() {
        let compiled = load_core_development_encounter_rooms(&content_root()).expect("content");
        let events = trace_kit(
            CoreEnemyKitScheduler::new(authored_definition(&compiled, "miniboss.sepulcher_knight"))
                .expect("Knight scheduler"),
            207,
        );
        let telegraphs = events
            .iter()
            .filter_map(|event| match event {
                CoreEnemyKitEvent::TelegraphDue {
                    tick,
                    pattern_index,
                    warning_ticks,
                    first_use,
                } => Some((tick.0, *pattern_index, *warning_ticks, *first_use)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            telegraphs,
            [
                (0, 0, 27, true),
                (66, 2, 12, true),
                (132, 2, 9, false),
                (180, 0, 27, false),
            ]
        );
        assert!(events.contains(&CoreEnemyKitEvent::KnightChargeDue {
            tick: Tick(27),
            pattern_index: 0,
            charge_ticks: 17,
        }));
        assert!(events.contains(&CoreEnemyKitEvent::KnightStopRingDue {
            tick: Tick(44),
            pattern_index: 1,
        }));
        assert!(events.contains(&CoreEnemyKitEvent::KnightShieldFanDue {
            tick: Tick(78),
            pattern_index: 2,
        }));
        assert!(events.contains(&CoreEnemyKitEvent::KnightShieldFanDue {
            tick: Tick(141),
            pattern_index: 2,
        }));
        assert!(events.contains(&CoreEnemyKitEvent::KnightChargeDue {
            tick: Tick(207),
            pattern_index: 0,
            charge_ticks: 17,
        }));
    }

    #[test]
    fn rotor_schedulers_hold_release_cadence_and_independent_volley_rounding() {
        let compiled = load_core_development_encounter_rooms(&content_root()).expect("content");
        let skull = trace_kit(
            CoreEnemyKitScheduler::new(authored_definition(&compiled, "enemy.choir_skull"))
                .expect("Skull scheduler"),
            212,
        );
        assert_eq!(rotor_starts(&skull), [(20, 0, 120), (200, 1, 120)]);
        assert_eq!(
            rotor_volleys(&skull),
            [
                (32, 0, 0),
                (44, 0, 1),
                (56, 0, 2),
                (68, 0, 3),
                (80, 0, 4),
                (92, 0, 5),
                (104, 0, 6),
                (116, 0, 7),
                (128, 0, 8),
                (140, 0, 9),
                (212, 1, 0),
            ]
        );
        assert!(skull.contains(&CoreEnemyKitEvent::RotorRecoveryStarted {
            tick: Tick(140),
            pattern_index: 0,
            recovery_ticks: 60,
        }));
        assert!(skull.contains(&CoreEnemyKitEvent::TelegraphDue {
            tick: Tick(185),
            pattern_index: 0,
            warning_ticks: 15,
            first_use: false,
        }));

        let abbot = trace_kit(
            CoreEnemyKitScheduler::new(authored_definition(&compiled, "miniboss.choir_abbot"))
                .expect("Abbot scheduler"),
            211,
        );
        assert_eq!(rotor_starts(&abbot), [(20, 0, 105), (200, 1, 105)]);
        assert_eq!(
            rotor_volleys(&abbot),
            [
                (31, 0, 0),
                (41, 0, 1),
                (52, 0, 2),
                (62, 0, 3),
                (73, 0, 4),
                (83, 0, 5),
                (94, 0, 6),
                (104, 0, 7),
                (115, 0, 8),
                (125, 0, 9),
                (211, 1, 0),
            ]
        );
        assert!(abbot.contains(&CoreEnemyKitEvent::RecoveryWarningDue {
            tick: Tick(125),
            pattern_index: 1,
            warning_ticks: 75,
            directional_preview_ticks: 20,
        }));
        assert!(
            abbot.contains(&CoreEnemyKitEvent::DirectionalGapPreviewDue {
                tick: Tick(180),
                pattern_index: 1,
                warning_ticks: 20,
            })
        );
        let boundary = abbot
            .iter()
            .filter(|event| match event {
                CoreEnemyKitEvent::AbbotRecoveryRingDue { tick, .. }
                | CoreEnemyKitEvent::RotorStarted { tick, .. } => *tick == Tick(200),
                _ => false,
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            boundary.as_slice(),
            [
                CoreEnemyKitEvent::AbbotRecoveryRingDue { .. },
                CoreEnemyKitEvent::RotorStarted { .. }
            ]
        ));
    }

    fn rotor_starts(events: &[CoreEnemyKitEvent]) -> Vec<(u64, u32, u32)> {
        events
            .iter()
            .filter_map(|event| match event {
                CoreEnemyKitEvent::RotorStarted {
                    tick,
                    cycle_index,
                    active_ticks,
                    ..
                } => Some((tick.0, *cycle_index, *active_ticks)),
                _ => None,
            })
            .collect()
    }

    fn rotor_volleys(events: &[CoreEnemyKitEvent]) -> Vec<(u64, u32, u8)> {
        events
            .iter()
            .filter_map(|event| match event {
                CoreEnemyKitEvent::RotorVolleyDue {
                    tick,
                    cycle_index,
                    volley_index,
                    ..
                } => Some((tick.0, *cycle_index, *volley_index)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn kit_readiness_and_reset_reanchor_without_losing_first_use_contracts() {
        let compiled = load_core_development_encounter_rooms(&content_root()).expect("content");
        let mut scheduler =
            CoreEnemyKitScheduler::new(authored_definition(&compiled, "enemy.bell_acolyte"))
                .expect("Acolyte scheduler");
        for _ in 0..5 {
            assert!(scheduler.advance(false).expect("gated tick").is_empty());
        }
        assert_eq!(
            scheduler.advance(true).expect("ready tick"),
            [CoreEnemyKitEvent::TelegraphDue {
                tick: Tick(5),
                pattern_index: 0,
                warning_ticks: 12,
                first_use: true,
            }]
        );
        while scheduler.tick() < Tick(18) {
            scheduler.advance(true).expect("release progression");
        }
        scheduler.reset().expect("exact reset");
        assert_eq!(
            scheduler.advance(true).expect("reset tick"),
            [CoreEnemyKitEvent::TelegraphDue {
                tick: Tick(18),
                pattern_index: 0,
                warning_ticks: 12,
                first_use: true,
            }]
        );
    }
}
