//! Exact initial encounter plans for the four M03 Bell Sepulcher combat rooms.

use std::collections::BTreeSet;

use content_schema::{ContentId, CoreFixedLayoutNode};
use sim_core::{
    DungeonAnchorKind, DungeonRoomDefinition, EntityId, FixedRoomError, FixedRoomSimulation,
    NormalWaveEntityIdError, SpawnInstanceId, normal_wave_entity_id,
};
use thiserror::Error;

use crate::CoreDevelopmentEncounterRooms;

const FIXED_COMBAT_NODE_IDS: [&str; 4] = ["B1", "B2", "B3", "B5"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreFixedRoomActorRuntimeKind {
    DrownedPilgrim,
    BellReed,
    BellAcolyte,
    ChoirSkull,
    ChainSentry,
    SepulcherKnight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFixedRoomAssignment {
    pub instance_id: SpawnInstanceId,
    pub entity_id: EntityId,
    pub enemy_id: ContentId,
    pub runtime_kind: CoreFixedRoomActorRuntimeKind,
    pub reward_profile_id: ContentId,
    pub xp_profile_id: ContentId,
    pub anchor_id: String,
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFixedRoomEncounterPlan {
    pub node_id: String,
    pub room_template_id: ContentId,
    pub rotation_degrees: u16,
    pub base_budget: u16,
    pub warning_ticks: u64,
    pub first_spawn_ordinal: u16,
    assignments: Vec<CoreFixedRoomAssignment>,
}

impl CoreFixedRoomEncounterPlan {
    #[must_use]
    pub fn assignments(&self) -> &[CoreFixedRoomAssignment] {
        &self.assignments
    }

    pub fn new_authority(&self) -> Result<FixedRoomSimulation, FixedRoomError> {
        FixedRoomSimulation::new(
            u16::try_from(self.assignments.len()).map_err(|_| FixedRoomError::EmptyEncounter)?,
            0,
        )
    }
}

/// Compiles the four exact initial room attempts with one monotonic run-local identity sequence.
pub fn compile_core_fixed_room_encounters(
    content: &CoreDevelopmentEncounterRooms,
    run_ordinal: u32,
) -> Result<Vec<CoreFixedRoomEncounterPlan>, CoreFixedRoomEncounterError> {
    if run_ordinal == 0 {
        return Err(CoreFixedRoomEncounterError::EntityId(
            NormalWaveEntityIdError::ZeroRunOrdinal,
        ));
    }
    let definitions = content.compile_room_definitions()?;
    let mut next_spawn_ordinal = 1_u16;
    let mut plans = Vec::with_capacity(FIXED_COMBAT_NODE_IDS.len());
    for expected_node_id in FIXED_COMBAT_NODE_IDS {
        let node = content
            .fixed_layout()
            .nodes
            .iter()
            .find(|node| node.node_id == expected_node_id)
            .ok_or(CoreFixedRoomEncounterError::DefinitionDrift)?;
        let plan = compile_node_plan(content, &definitions, node, run_ordinal, next_spawn_ordinal)?;
        next_spawn_ordinal = next_spawn_ordinal
            .checked_add(
                u16::try_from(plan.assignments.len())
                    .map_err(|_| CoreFixedRoomEncounterError::DefinitionDrift)?,
            )
            .ok_or(CoreFixedRoomEncounterError::DefinitionDrift)?;
        plans.push(plan);
    }
    Ok(plans)
}

fn compile_node_plan(
    content: &CoreDevelopmentEncounterRooms,
    definitions: &[DungeonRoomDefinition],
    node: &CoreFixedLayoutNode,
    run_ordinal: u32,
    first_spawn_ordinal: u16,
) -> Result<CoreFixedRoomEncounterPlan, CoreFixedRoomEncounterError> {
    let encounter = node
        .encounter
        .as_ref()
        .ok_or(CoreFixedRoomEncounterError::DefinitionDrift)?;
    if encounter.warning_milliseconds != 900 {
        return Err(CoreFixedRoomEncounterError::DefinitionDrift);
    }
    let definition = definitions
        .iter()
        .find(|room| room.id == node.room_template_id.as_str())
        .ok_or(CoreFixedRoomEncounterError::DefinitionDrift)?;
    let rotated = definition.rotated(node.rotation_degrees)?;

    let mut units = Vec::new();
    let mut budget = 0_u16;
    for member in &encounter.members {
        for occurrence in 0..member.count {
            units.push((member.enemy_id.clone(), occurrence));
            budget = budget
                .checked_add(member.threat_each)
                .ok_or(CoreFixedRoomEncounterError::DefinitionDrift)?;
        }
    }
    units.sort_by(|left, right| {
        left.0
            .as_str()
            .cmp(right.0.as_str())
            .then_with(|| left.1.cmp(&right.1))
    });
    if budget != encounter.base_budget || units.is_empty() {
        return Err(CoreFixedRoomEncounterError::DefinitionDrift);
    }

    let mut used_anchor_ids = BTreeSet::new();
    let mut assignments = Vec::with_capacity(units.len());
    for (index, (enemy_id, _)) in units.into_iter().enumerate() {
        let (runtime_kind, anchor_kind) = runtime_and_anchor_kind(enemy_id.as_str())?;
        let actor = content
            .actor_definitions()
            .iter()
            .find(|actor| actor.id() == &enemy_id)
            .ok_or(CoreFixedRoomEncounterError::DefinitionDrift)?;
        let anchor = rotated
            .anchors
            .iter()
            .filter(|anchor| {
                anchor.kind == anchor_kind
                    && anchor
                        .bound_content_id
                        .as_deref()
                        .is_none_or(|bound| bound == enemy_id.as_str())
                    && !used_anchor_ids.contains(anchor.id.as_str())
            })
            .min_by_key(|anchor| (anchor.y_milli_tiles, anchor.x_milli_tiles, &anchor.id))
            .ok_or_else(|| CoreFixedRoomEncounterError::MissingCompatibleAnchor {
                node_id: node.node_id.clone(),
                enemy_id: enemy_id.to_string(),
            })?;
        used_anchor_ids.insert(anchor.id.clone());
        let offset =
            u16::try_from(index).map_err(|_| CoreFixedRoomEncounterError::DefinitionDrift)?;
        let instance_id = SpawnInstanceId {
            run_ordinal,
            spawn_ordinal: first_spawn_ordinal
                .checked_add(offset)
                .ok_or(CoreFixedRoomEncounterError::DefinitionDrift)?,
        };
        assignments.push(CoreFixedRoomAssignment {
            instance_id,
            entity_id: normal_wave_entity_id(instance_id)?,
            enemy_id,
            runtime_kind,
            reward_profile_id: actor.reward_profile_id().clone(),
            xp_profile_id: actor.xp_profile_id().clone(),
            anchor_id: anchor.id.clone(),
            x_milli_tiles: anchor.x_milli_tiles,
            y_milli_tiles: anchor.y_milli_tiles,
        });
    }
    Ok(CoreFixedRoomEncounterPlan {
        node_id: node.node_id.clone(),
        room_template_id: node.room_template_id.clone(),
        rotation_degrees: node.rotation_degrees,
        base_budget: encounter.base_budget,
        warning_ticks: 27,
        first_spawn_ordinal,
        assignments,
    })
}

fn runtime_and_anchor_kind(
    enemy_id: &str,
) -> Result<(CoreFixedRoomActorRuntimeKind, DungeonAnchorKind), CoreFixedRoomEncounterError> {
    match enemy_id {
        "enemy.drowned_pilgrim" => Ok((
            CoreFixedRoomActorRuntimeKind::DrownedPilgrim,
            DungeonAnchorKind::Fodder,
        )),
        "enemy.bell_reed" => Ok((
            CoreFixedRoomActorRuntimeKind::BellReed,
            DungeonAnchorKind::Pressure,
        )),
        "enemy.bell_acolyte" => Ok((
            CoreFixedRoomActorRuntimeKind::BellAcolyte,
            DungeonAnchorKind::Pressure,
        )),
        "enemy.choir_skull" => Ok((
            CoreFixedRoomActorRuntimeKind::ChoirSkull,
            DungeonAnchorKind::Disruptor,
        )),
        "enemy.chain_sentry" => Ok((
            CoreFixedRoomActorRuntimeKind::ChainSentry,
            DungeonAnchorKind::AnchorEnemy,
        )),
        "miniboss.sepulcher_knight" => Ok((
            CoreFixedRoomActorRuntimeKind::SepulcherKnight,
            DungeonAnchorKind::Miniboss,
        )),
        _ => Err(CoreFixedRoomEncounterError::DefinitionDrift),
    }
}

#[derive(Debug, Error)]
pub enum CoreFixedRoomEncounterError {
    #[error("fixed Core room content drifted from the exact B1/B2/B3/B5 contract")]
    DefinitionDrift,
    #[error("room {node_id} has no compatible unused anchor for {enemy_id}")]
    MissingCompatibleAnchor { node_id: String, enemy_id: String },
    #[error(transparent)]
    EntityId(#[from] NormalWaveEntityIdError),
    #[error(transparent)]
    Room(#[from] sim_core::DungeonRoomError),
    #[error(transparent)]
    Content(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::load_core_development_encounter_rooms;

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn four_fixed_room_plans_are_exact_ordered_and_identity_disjoint() {
        let content = load_core_development_encounter_rooms(&content_root()).expect("content");
        let plans = compile_core_fixed_room_encounters(&content, 1).expect("plans");
        assert_eq!(
            plans
                .iter()
                .map(|plan| plan.node_id.as_str())
                .collect::<Vec<_>>(),
            FIXED_COMBAT_NODE_IDS
        );
        assert_eq!(
            plans
                .iter()
                .map(|plan| (plan.assignments.len(), plan.base_budget, plan.warning_ticks))
                .collect::<Vec<_>>(),
            [(8, 12, 27), (9, 16, 27), (1, 10, 27), (7, 12, 27)]
        );
        assert_eq!(
            plans
                .iter()
                .flat_map(|plan| plan.assignments.iter())
                .map(|assignment| assignment.instance_id.spawn_ordinal)
                .collect::<Vec<_>>(),
            (1..=25).collect::<Vec<_>>()
        );
        assert!(plans.iter().all(|plan| {
            plan.assignments
                .iter()
                .map(|assignment| assignment.anchor_id.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                == plan.assignments.len()
        }));
    }

    #[test]
    fn assignments_preserve_runtime_reward_xp_and_rotated_anchor_contracts() {
        let content = load_core_development_encounter_rooms(&content_root()).expect("content");
        let plans = compile_core_fixed_room_encounters(&content, 7).expect("plans");
        let b2 = &plans[1];
        assert_eq!(b2.rotation_degrees, 90);
        assert_eq!(b2.first_spawn_ordinal, 9);
        assert_eq!(
            b2.assignments
                .iter()
                .map(|assignment| assignment.enemy_id.as_str())
                .collect::<Vec<_>>(),
            [
                "enemy.bell_acolyte",
                "enemy.bell_acolyte",
                "enemy.choir_skull",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
                "enemy.drowned_pilgrim",
            ]
        );
        assert!(b2.assignments.iter().all(|assignment| {
            !assignment.reward_profile_id.as_str().is_empty()
                && !assignment.xp_profile_id.as_str().is_empty()
                && assignment.instance_id.run_ordinal == 7
        }));
        let knight = &plans[2].assignments[0];
        assert_eq!(knight.enemy_id.as_str(), "miniboss.sepulcher_knight");
        assert_eq!(knight.anchor_id, "miniboss");
        assert_eq!(
            (knight.x_milli_tiles, knight.y_milli_tiles),
            (13_500, 7_500)
        );
        assert_eq!(knight.reward_profile_id.as_str(), "reward.miniboss_t1");
        assert_eq!(knight.xp_profile_id.as_str(), "xp.miniboss_t1");
        assert_eq!(
            plans[0]
                .new_authority()
                .expect("authority")
                .activation_ordinal(),
            0
        );
    }
}
