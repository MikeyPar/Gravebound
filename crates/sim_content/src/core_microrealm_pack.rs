//! Cross-document construction boundary for the M03 Core microrealm encounter.
//!
//! The world-flow package owns legal anchors, the encounter-room package owns `pack.bell.01`, and
//! the microrealm lifecycle owns the warning event. This module joins those immutable authorities
//! without teaching any one package about the others' source records.

use content_schema::ContentId;
use sim_core::{
    ArenaAnchor, ArenaGeometry, ArenaGeometryError, CORE_MICROREALM_PACK_WARNING_TICKS,
    CoreMicrorealmEvent, EnemyLabPlayer, EntityId, EntityIdAllocator, NormalWaveDefinitions,
    NormalWaveEnemyKind, NormalWaveEntityIdError, NormalWaveError, NormalWaveSimulation,
    NormalWaveSpawn, SpawnInstanceId, Tick, TilePoint, normal_wave_entity_id,
};
use thiserror::Error;

use crate::{CoreDevelopmentEncounterRooms, CoreDevelopmentWorldFlow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreMicrorealmPackAssignment {
    pub instance_id: SpawnInstanceId,
    pub entity_id: EntityId,
    pub enemy_id: ContentId,
    pub kind: NormalWaveEnemyKind,
    pub anchor: TilePoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreMicrorealmPackPlan {
    pub pack_id: ContentId,
    pub warning_started_at: Tick,
    pub activates_at: Tick,
    pub base_budget: u16,
    assignments: Vec<CoreMicrorealmPackAssignment>,
}

impl CoreMicrorealmPackPlan {
    #[must_use]
    pub fn assignments(&self) -> &[CoreMicrorealmPackAssignment] {
        &self.assignments
    }

    #[must_use]
    pub fn normal_wave_spawns(&self) -> Vec<NormalWaveSpawn> {
        self.assignments
            .iter()
            .map(|assignment| NormalWaveSpawn {
                instance_id: assignment.instance_id,
                kind: assignment.kind,
                position_milli_tiles: (
                    assignment.anchor.x_milli_tiles,
                    assignment.anchor.y_milli_tiles,
                ),
            })
            .collect()
    }
}

/// Constructs the exact eight-member encounter only in response to the lifecycle warning seam.
pub fn construct_core_microrealm_pack(
    encounters: &CoreDevelopmentEncounterRooms,
    world_flow: &CoreDevelopmentWorldFlow,
    warning_started_at: Tick,
    event: CoreMicrorealmEvent,
    run_ordinal: u32,
) -> Result<CoreMicrorealmPackPlan, CoreMicrorealmPackError> {
    let CoreMicrorealmEvent::BeginPackWarning { warning_ticks } = event else {
        return Err(CoreMicrorealmPackError::UnexpectedLifecycleEvent);
    };
    if warning_ticks != CORE_MICROREALM_PACK_WARNING_TICKS {
        return Err(CoreMicrorealmPackError::WarningDrift {
            expected: CORE_MICROREALM_PACK_WARNING_TICKS,
            actual: warning_ticks,
        });
    }

    let pack = encounters.pack_bell_01();
    let world = world_flow.world();
    if pack.header.id.as_str() != "pack.bell.01"
        || pack.warning_milliseconds != 900
        || !pack.simultaneous_spawn
        || pack.base_budget != 12
        || world.header.id.as_str() != "world.core_microrealm_01"
        || world.enabled_spawn_anchor_count != 8
    {
        return Err(CoreMicrorealmPackError::DefinitionDrift);
    }

    let mut members = Vec::new();
    for member in &pack.members {
        let kind = match member.enemy_id.as_str() {
            "enemy.drowned_pilgrim" if member.threat_each == 1 => {
                NormalWaveEnemyKind::DrownedPilgrim
            }
            "enemy.bell_reed" if member.threat_each == 3 => NormalWaveEnemyKind::BellReed,
            _ => return Err(CoreMicrorealmPackError::DefinitionDrift),
        };
        for _ in 0..member.count {
            members.push((member.enemy_id.clone(), kind, member.threat_each));
        }
    }
    members.sort_by(|left, right| left.0.as_str().cmp(right.0.as_str()));

    let enabled_count = usize::try_from(world.enabled_spawn_anchor_count)
        .map_err(|_| CoreMicrorealmPackError::DefinitionDrift)?;
    let mut anchors = world
        .candidate_spawn_anchors
        .iter()
        .take(enabled_count)
        .map(|point| TilePoint::new(point.x, point.y))
        .collect::<Vec<_>>();
    anchors.sort_by_key(|point| (point.y_milli_tiles, point.x_milli_tiles));
    if members.len() != enabled_count || anchors.len() != enabled_count {
        return Err(CoreMicrorealmPackError::DefinitionDrift);
    }

    let budget = members
        .iter()
        .try_fold(0_u16, |sum, (_, _, cost)| sum.checked_add(*cost))
        .ok_or(CoreMicrorealmPackError::DefinitionDrift)?;
    if budget != pack.base_budget {
        return Err(CoreMicrorealmPackError::DefinitionDrift);
    }

    let assignments = members
        .into_iter()
        .zip(anchors)
        .enumerate()
        .map(|(index, ((enemy_id, kind, _), anchor))| {
            let spawn_ordinal =
                u16::try_from(index + 1).map_err(|_| CoreMicrorealmPackError::DefinitionDrift)?;
            let instance_id = SpawnInstanceId {
                run_ordinal,
                spawn_ordinal,
            };
            let entity_id = normal_wave_entity_id(instance_id)?;
            Ok(CoreMicrorealmPackAssignment {
                instance_id,
                entity_id,
                enemy_id,
                kind,
                anchor,
            })
        })
        .collect::<Result<Vec<_>, CoreMicrorealmPackError>>()?;
    let activates_at = warning_started_at
        .0
        .checked_add(warning_ticks)
        .map(Tick)
        .ok_or(CoreMicrorealmPackError::TickOverflow)?;

    Ok(CoreMicrorealmPackPlan {
        pack_id: pack.header.id.clone(),
        warning_started_at,
        activates_at,
        base_budget: pack.base_budget,
        assignments,
    })
}

/// Instantiates the immutable plan with the existing authoritative First Playable enemy runtime.
pub fn instantiate_core_microrealm_pack(
    plan: &CoreMicrorealmPackPlan,
    world_flow: &CoreDevelopmentWorldFlow,
    player: EnemyLabPlayer,
    hostile_projectile_ids: EntityIdAllocator,
) -> Result<NormalWaveSimulation, CoreMicrorealmPackError> {
    if plan.pack_id.as_str() != "pack.bell.01"
        || plan.base_budget != 12
        || plan.assignments.len() != 8
        || plan.activates_at.0
            != plan
                .warning_started_at
                .0
                .checked_add(CORE_MICROREALM_PACK_WARNING_TICKS)
                .ok_or(CoreMicrorealmPackError::TickOverflow)?
    {
        return Err(CoreMicrorealmPackError::DefinitionDrift);
    }
    let arena = microrealm_combat_arena(world_flow)?;
    let wave = NormalWaveSimulation::new(
        NormalWaveDefinitions::first_playable(),
        arena,
        plan.normal_wave_spawns(),
        player,
        hostile_projectile_ids,
        plan.warning_started_at,
    )?;
    if wave.starts_at() != plan.warning_started_at
        || !matches!(
            wave.phase(),
            sim_core::NormalWavePhase::DormantTelegraph { activates_at }
                if activates_at == plan.activates_at
        )
    {
        return Err(CoreMicrorealmPackError::DefinitionDrift);
    }
    Ok(wave)
}

fn microrealm_combat_arena(
    world_flow: &CoreDevelopmentWorldFlow,
) -> Result<ArenaGeometry, CoreMicrorealmPackError> {
    let world = world_flow.world();
    if world.header.id.as_str() != "world.core_microrealm_01"
        || world.width_tiles != 48
        || world.height_tiles != 48
        || world.solid_shell_tiles != 1
    {
        return Err(CoreMicrorealmPackError::DefinitionDrift);
    }
    let enabled_count = usize::try_from(world.enabled_spawn_anchor_count)
        .map_err(|_| CoreMicrorealmPackError::DefinitionDrift)?;
    ArenaGeometry {
        id: world.header.id.to_string(),
        width_milli_tiles: i32::try_from(world.width_tiles)
            .ok()
            .and_then(|tiles| tiles.checked_mul(1_000))
            .ok_or(CoreMicrorealmPackError::DefinitionDrift)?,
        height_milli_tiles: i32::try_from(world.height_tiles)
            .ok()
            .and_then(|tiles| tiles.checked_mul(1_000))
            .ok_or(CoreMicrorealmPackError::DefinitionDrift)?,
        shell_thickness_milli_tiles: i32::try_from(world.solid_shell_tiles)
            .ok()
            .and_then(|tiles| tiles.checked_mul(1_000))
            .ok_or(CoreMicrorealmPackError::DefinitionDrift)?,
        player_spawn: TilePoint::new(world.player_spawn.x, world.player_spawn.y),
        // `ArenaGeometry` carries a compatibility boss point; the Core microrealm has no boss.
        boss_spawn: TilePoint::new(
            world.bell_portal_area.center.x,
            world.bell_portal_area.center.y,
        ),
        pillars: Vec::new(),
        anchors: world
            .candidate_spawn_anchors
            .iter()
            .take(enabled_count)
            .enumerate()
            .map(|(index, point)| ArenaAnchor {
                id: format!("pack.{:02}", index + 1),
                point: TilePoint::new(point.x, point.y),
            })
            .collect(),
    }
    .validated()
    .map_err(Into::into)
}

#[derive(Debug, Error)]
pub enum CoreMicrorealmPackError {
    #[error("Core microrealm pack construction requires BeginPackWarning")]
    UnexpectedLifecycleEvent,
    #[error("Core microrealm warning drifted: expected {expected} ticks, received {actual}")]
    WarningDrift { expected: u64, actual: u64 },
    #[error("Core microrealm world or pack definition drifted from its exact contract")]
    DefinitionDrift,
    #[error("Core microrealm pack activation tick overflowed")]
    TickOverflow,
    #[error(transparent)]
    EntityId(#[from] NormalWaveEntityIdError),
    #[error(transparent)]
    Arena(#[from] ArenaGeometryError),
    #[error(transparent)]
    Wave(#[from] NormalWaveError),
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, path::Path};

    use super::*;
    use crate::{load_core_development_encounter_rooms, load_core_development_world_flow};
    use sim_core::{
        CombatStep, HostileTargetState, NormalWavePhase, PlayerVitals, RedTonicSimulation,
        SimulationVector, TonicBelt,
    };

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn warning_event_constructs_exact_sorted_pack_atomically() {
        let root = content_root();
        let encounters = load_core_development_encounter_rooms(&root).expect("encounters");
        let world = load_core_development_world_flow(&root).expect("world");
        let plan = construct_core_microrealm_pack(
            &encounters,
            &world,
            Tick(32),
            CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 },
            1,
        )
        .expect("exact pack");

        assert_eq!(plan.pack_id.as_str(), "pack.bell.01");
        assert_eq!(plan.warning_started_at, Tick(32));
        assert_eq!(plan.activates_at, Tick(59));
        assert_eq!(plan.base_budget, 12);
        assert_eq!(plan.assignments().len(), 8);
        assert_eq!(
            plan.assignments()
                .iter()
                .map(|assignment| assignment.enemy_id.as_str())
                .collect::<Vec<_>>(),
            [
                "enemy.bell_reed",
                "enemy.bell_reed",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
            ]
        );
        assert_eq!(
            plan.assignments()
                .iter()
                .map(|assignment| assignment.anchor)
                .collect::<Vec<_>>(),
            [
                TilePoint::new(8_500, 8_500),
                TilePoint::new(16_500, 8_500),
                TilePoint::new(24_500, 8_500),
                TilePoint::new(8_500, 16_500),
                TilePoint::new(16_500, 16_500),
                TilePoint::new(32_500, 16_500),
                TilePoint::new(8_500, 24_500),
                TilePoint::new(16_500, 32_500),
            ]
        );
        assert_eq!(
            plan.assignments()
                .iter()
                .map(|assignment| assignment.entity_id.get())
                .collect::<Vec<_>>(),
            (30_001..=30_008).collect::<Vec<_>>()
        );
        assert_eq!(
            plan.normal_wave_spawns()
                .iter()
                .map(|spawn| spawn.instance_id.spawn_ordinal)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<_>>()
        );
    }

    #[test]
    fn nonwarning_drift_and_zero_run_fail_without_a_partial_plan() {
        let root = content_root();
        let encounters = load_core_development_encounter_rooms(&root).expect("encounters");
        let world = load_core_development_world_flow(&root).expect("world");
        assert!(matches!(
            construct_core_microrealm_pack(
                &encounters,
                &world,
                Tick(1),
                CoreMicrorealmEvent::ResetPack,
                1,
            ),
            Err(CoreMicrorealmPackError::UnexpectedLifecycleEvent)
        ));
        assert!(matches!(
            construct_core_microrealm_pack(
                &encounters,
                &world,
                Tick(1),
                CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 26 },
                1,
            ),
            Err(CoreMicrorealmPackError::WarningDrift {
                expected: 27,
                actual: 26,
            })
        ));
        assert!(matches!(
            construct_core_microrealm_pack(
                &encounters,
                &world,
                Tick(1),
                CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 },
                0,
            ),
            Err(CoreMicrorealmPackError::EntityId(
                NormalWaveEntityIdError::ZeroRunOrdinal
            ))
        ));
    }

    #[test]
    fn plan_instantiates_existing_wave_runtime_at_the_same_activation_boundary() {
        let root = content_root();
        let encounters = load_core_development_encounter_rooms(&root).expect("encounters");
        let world = load_core_development_world_flow(&root).expect("world");
        let plan = construct_core_microrealm_pack(
            &encounters,
            &world,
            Tick(32),
            CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 },
            1,
        )
        .expect("plan");
        let (source, _) = crate::load_and_validate(&root).expect("FP source");
        let fixture = crate::first_playable_authority_combat_test(&source).expect("FP fixture");
        let definitions = fixture.definitions;
        let player_id = EntityId::new(900).expect("player ID");
        let player = EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: player_id,
                position: SimulationVector::new(8.5, 40.5),
                target_is_immune: false,
                resistance_basis_points: definitions.resistance_basis_points,
                additional_direct_damage_reductions_basis_points: Vec::new(),
                armor: definitions.starting_armor,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
            },
            consumables: RedTonicSimulation::new(
                definitions.red_tonic,
                PlayerVitals::new(definitions.maximum_health, definitions.maximum_health)
                    .expect("vitals"),
                TonicBelt::first_playable(),
            )
            .expect("tonic"),
            combat: definitions.combat,
        };
        let mut wave = instantiate_core_microrealm_pack(
            &plan,
            &world,
            player,
            EntityIdAllocator::starting_at(NonZeroU64::new(20_000).expect("projectile ID")),
        )
        .expect("wave");
        assert_eq!(
            wave.phase(),
            NormalWavePhase::DormantTelegraph {
                activates_at: Tick(59)
            }
        );
        for tick in 32..59 {
            let step = wave
                .step(&CombatStep {
                    tick: Tick(tick),
                    ..CombatStep::default()
                })
                .expect("warning tick");
            assert!(!step.activated);
            assert!(step.hostile_spawn_events.is_empty());
        }
        let step = wave
            .step(&CombatStep {
                tick: Tick(59),
                ..CombatStep::default()
            })
            .expect("activation tick");
        assert!(step.activated);
        assert_eq!(wave.phase(), NormalWavePhase::Active);
        assert_eq!(wave.snapshots().len(), 8);
    }
}
