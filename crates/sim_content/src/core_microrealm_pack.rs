//! Cross-document construction boundary for the M03 Core microrealm encounter.
//!
//! The world-flow package owns legal anchors, the encounter-room package owns `pack.bell.01`, and
//! the microrealm lifecycle owns the warning event. This module joins those immutable authorities
//! without teaching any one package about the others' source records.

use content_schema::ContentId;
use sim_core::{
    CORE_MICROREALM_PACK_WARNING_TICKS, CoreMicrorealmEvent, EntityId, NormalWaveEnemyKind,
    NormalWaveEntityIdError, NormalWaveSpawn, SpawnInstanceId, Tick, TilePoint,
    normal_wave_entity_id,
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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
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
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{load_core_development_encounter_rooms, load_core_development_world_flow};

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
        assert_eq!(
            construct_core_microrealm_pack(
                &encounters,
                &world,
                Tick(1),
                CoreMicrorealmEvent::ResetPack,
                1,
            ),
            Err(CoreMicrorealmPackError::UnexpectedLifecycleEvent)
        );
        assert_eq!(
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
        );
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
}
