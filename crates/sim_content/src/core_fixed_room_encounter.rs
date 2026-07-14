//! Exact initial encounter plans for the four M03 Bell Sepulcher combat rooms.

use std::collections::BTreeSet;

use content_schema::{ContentId, CoreFixedLayoutNode};
use sim_core::{
    ArenaAnchor, ArenaGeometry, CombatStep, DungeonAnchorKind, DungeonRoomDefinition,
    DungeonRoomVolumeGeometry, DungeonRoomVolumeKind, EnemyLabPlayer, EntityId, EntityIdAllocator,
    FixedRoomError, FixedRoomEvent, FixedRoomInput, FixedRoomPhase, FixedRoomSimulation,
    NormalWaveClearedHostiles, NormalWaveDefinitions, NormalWaveEnemyKind, NormalWaveEntityIdError,
    NormalWaveError, NormalWaveHandoff, NormalWaveSimulation, NormalWaveSpawn, NormalWaveStep,
    RotatedDungeonRoom, SpawnInstanceId, Tick, TilePoint, TileRectangle, normal_wave_entity_id,
};
use thiserror::Error;

use crate::CoreDevelopmentEncounterRooms;

const FIXED_COMBAT_NODE_IDS: [&str; 4] = ["B1", "B2", "B3", "B5"];
const INITIAL_FIXED_ROUTE_ACTOR_COUNT: u16 = 25;

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
    arena: ArenaGeometry,
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

    #[must_use]
    pub const fn arena(&self) -> &ArenaGeometry {
        &self.arena
    }
}

/// Instantiates B1/B5 through the immutable First Playable combat owner. Mixed/authored rooms fail
/// closed until their dedicated owner is supplied.
pub fn instantiate_immutable_fixed_room_wave(
    plan: &CoreFixedRoomEncounterPlan,
    player: EnemyLabPlayer,
    hostile_projectile_ids: EntityIdAllocator,
    warning_started_at: Tick,
) -> Result<NormalWaveSimulation, CoreFixedRoomEncounterError> {
    instantiate_immutable_fixed_room_wave_at_ordinal(
        plan,
        player,
        hostile_projectile_ids,
        warning_started_at,
        plan.first_spawn_ordinal,
    )
}

fn instantiate_immutable_fixed_room_wave_at_ordinal(
    plan: &CoreFixedRoomEncounterPlan,
    player: EnemyLabPlayer,
    hostile_projectile_ids: EntityIdAllocator,
    warning_started_at: Tick,
    first_spawn_ordinal: u16,
) -> Result<NormalWaveSimulation, CoreFixedRoomEncounterError> {
    let run_ordinal = plan
        .assignments
        .first()
        .ok_or(CoreFixedRoomEncounterError::DefinitionDrift)?
        .instance_id
        .run_ordinal;
    let spawns = plan
        .assignments
        .iter()
        .enumerate()
        .map(|(index, assignment)| {
            let kind = match assignment.runtime_kind {
                CoreFixedRoomActorRuntimeKind::DrownedPilgrim => {
                    NormalWaveEnemyKind::DrownedPilgrim
                }
                CoreFixedRoomActorRuntimeKind::BellReed => NormalWaveEnemyKind::BellReed,
                CoreFixedRoomActorRuntimeKind::ChainSentry => NormalWaveEnemyKind::ChainSentry,
                CoreFixedRoomActorRuntimeKind::BellAcolyte
                | CoreFixedRoomActorRuntimeKind::ChoirSkull
                | CoreFixedRoomActorRuntimeKind::SepulcherKnight => {
                    return Err(CoreFixedRoomEncounterError::AuthoredRuntimeRequired {
                        node_id: plan.node_id.clone(),
                    });
                }
            };
            let offset =
                u16::try_from(index).map_err(|_| CoreFixedRoomEncounterError::DefinitionDrift)?;
            let spawn_ordinal = first_spawn_ordinal
                .checked_add(offset)
                .ok_or(CoreFixedRoomEncounterError::IdentityOverflow)?;
            Ok(NormalWaveSpawn {
                instance_id: SpawnInstanceId {
                    run_ordinal,
                    spawn_ordinal,
                },
                kind,
                position_milli_tiles: (assignment.x_milli_tiles, assignment.y_milli_tiles),
            })
        })
        .collect::<Result<Vec<_>, CoreFixedRoomEncounterError>>()?;
    NormalWaveSimulation::new(
        NormalWaveDefinitions::first_playable(),
        plan.arena.clone(),
        spawns,
        player,
        hostile_projectile_ids,
        warning_started_at,
    )
    .map_err(Into::into)
}

#[derive(Debug, Clone)]
pub struct CoreImmutableFixedRoomInput {
    pub crossed_activation_boundary: bool,
    pub living_inside: u16,
    pub living_party_outside: u16,
    pub doorway_hurtbox_blocked: bool,
    pub combat_step: Option<CombatStep>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreImmutableFixedRoomStep {
    pub tick: Tick,
    pub phase_after: FixedRoomPhase,
    pub required_hostiles_remaining: u16,
    pub lifecycle_events: Vec<FixedRoomEvent>,
    pub wave_step: Option<NormalWaveStep>,
    pub reset_cleared_hostiles: Option<NormalWaveClearedHostiles>,
}

/// Owns the complete `DNG-005` lifecycle for immutable B1/B5 combat rooms.
///
/// Required-hostile progress is derived from the wave snapshots and cannot be supplied by a
/// caller. Reactivations advance by the complete 25-actor initial-route stride, preserving the
/// disjoint B1/B2/B3/B5 identity ranges across every attempt.
#[derive(Debug, Clone)]
pub struct CoreImmutableFixedRoomSimulation {
    plan: CoreFixedRoomEncounterPlan,
    authority: FixedRoomSimulation,
    next_spawn_ordinal: u16,
    participant: Option<NormalWaveHandoff>,
    wave: Option<NormalWaveSimulation>,
}

impl CoreImmutableFixedRoomSimulation {
    pub fn new(
        plan: CoreFixedRoomEncounterPlan,
        player: EnemyLabPlayer,
        hostile_projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CoreFixedRoomEncounterError> {
        if plan.assignments.is_empty()
            || plan.assignments.iter().any(|assignment| {
                matches!(
                    assignment.runtime_kind,
                    CoreFixedRoomActorRuntimeKind::BellAcolyte
                        | CoreFixedRoomActorRuntimeKind::ChoirSkull
                        | CoreFixedRoomActorRuntimeKind::SepulcherKnight
                )
            })
        {
            return Err(CoreFixedRoomEncounterError::AuthoredRuntimeRequired {
                node_id: plan.node_id.clone(),
            });
        }
        let next_spawn_ordinal = plan.first_spawn_ordinal;
        let authority = plan.new_authority()?;
        Ok(Self {
            plan,
            authority,
            next_spawn_ordinal,
            participant: Some(NormalWaveHandoff {
                player,
                hostile_projectile_ids,
            }),
            wave: None,
        })
    }

    #[must_use]
    pub const fn phase(&self) -> FixedRoomPhase {
        self.authority.phase()
    }

    #[must_use]
    pub const fn activation_ordinal(&self) -> u32 {
        self.authority.activation_ordinal()
    }

    #[must_use]
    pub const fn wave(&self) -> Option<&NormalWaveSimulation> {
        self.wave.as_ref()
    }

    pub fn step(
        &mut self,
        tick: Tick,
        input: &CoreImmutableFixedRoomInput,
    ) -> Result<CoreImmutableFixedRoomStep, CoreFixedRoomEncounterError> {
        let mut staged = self.clone();
        let step = staged.step_inner(tick, input)?;
        *self = staged;
        Ok(step)
    }

    fn step_inner(
        &mut self,
        tick: Tick,
        input: &CoreImmutableFixedRoomInput,
    ) -> Result<CoreImmutableFixedRoomStep, CoreFixedRoomEncounterError> {
        let mut wave_step = None;
        if let Some(wave) = &mut self.wave {
            let combat_step = match input.combat_step.as_ref() {
                Some(step) => step.clone(),
                None if input.living_inside == 0 => CombatStep {
                    tick,
                    ..CombatStep::default()
                },
                None => return Err(CoreFixedRoomEncounterError::MissingCombatStep),
            };
            wave_step = Some(wave.step(&combat_step)?);
        }

        let required_hostiles_remaining = self.required_hostiles_remaining()?;
        let lifecycle_events = self.authority.step(
            tick,
            FixedRoomInput {
                crossed_activation_boundary: input.crossed_activation_boundary,
                living_inside: input.living_inside,
                living_party_outside: input.living_party_outside,
                doorway_hurtbox_blocked: input.doorway_hurtbox_blocked,
                required_hostiles_remaining,
                required_objectives_remaining: 0,
            },
        )?;

        let mut reset_cleared_hostiles = None;
        for event in lifecycle_events.iter().copied() {
            match event {
                FixedRoomEvent::BeginGroupWarning { .. } => {
                    let participant = self
                        .participant
                        .take()
                        .ok_or(CoreFixedRoomEncounterError::MissingParticipantHandoff)?;
                    let mut wave = instantiate_immutable_fixed_room_wave_at_ordinal(
                        &self.plan,
                        participant.player,
                        participant.hostile_projectile_ids,
                        tick,
                        self.next_spawn_ordinal,
                    )?;
                    let initial_combat = input.combat_step.clone().unwrap_or(CombatStep {
                        tick,
                        ..CombatStep::default()
                    });
                    wave_step = Some(wave.step(&initial_combat)?);
                    self.next_spawn_ordinal = self
                        .next_spawn_ordinal
                        .checked_add(INITIAL_FIXED_ROUTE_ACTOR_COUNT)
                        .ok_or(CoreFixedRoomEncounterError::IdentityOverflow)?;
                    self.wave = Some(wave);
                }
                FixedRoomEvent::CompletionCommitted { .. } => {
                    self.participant = Some(
                        self.wave
                            .take()
                            .ok_or(CoreFixedRoomEncounterError::MissingWave)?
                            .into_handoff()?,
                    );
                }
                FixedRoomEvent::RoomReset => {
                    if let Some(wave) = self.wave.take() {
                        let reset = wave.into_reset_handoff()?;
                        self.participant = Some(reset.participant);
                        reset_cleared_hostiles = Some(reset.cleared_hostiles);
                    } else if self.participant.is_none() {
                        return Err(CoreFixedRoomEncounterError::MissingParticipantHandoff);
                    }
                }
                _ => {}
            }
        }

        Ok(CoreImmutableFixedRoomStep {
            tick,
            phase_after: self.authority.phase(),
            required_hostiles_remaining,
            lifecycle_events,
            wave_step,
            reset_cleared_hostiles,
        })
    }

    fn required_hostiles_remaining(&self) -> Result<u16, CoreFixedRoomEncounterError> {
        if let Some(wave) = &self.wave {
            return u16::try_from(
                wave.snapshots()
                    .iter()
                    .filter(|snapshot| snapshot.health.alive)
                    .count(),
            )
            .map_err(|_| CoreFixedRoomEncounterError::DefinitionDrift);
        }
        if matches!(
            self.authority.phase(),
            FixedRoomPhase::Quiet | FixedRoomPhase::Cleared
        ) {
            Ok(0)
        } else {
            u16::try_from(self.plan.assignments.len())
                .map_err(|_| CoreFixedRoomEncounterError::DefinitionDrift)
        }
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
    let arena = combat_arena(&rotated)?;

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
    let mut used_anchor_positions = BTreeSet::new();
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
                    && !used_anchor_positions
                        .contains(&(anchor.x_milli_tiles, anchor.y_milli_tiles))
            })
            .min_by_key(|anchor| (anchor.y_milli_tiles, anchor.x_milli_tiles, &anchor.id))
            .ok_or_else(|| CoreFixedRoomEncounterError::MissingCompatibleAnchor {
                node_id: node.node_id.clone(),
                enemy_id: enemy_id.to_string(),
            })?;
        used_anchor_ids.insert(anchor.id.clone());
        used_anchor_positions.insert((anchor.x_milli_tiles, anchor.y_milli_tiles));
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
        arena,
        assignments,
    })
}

fn combat_arena(room: &RotatedDungeonRoom) -> Result<ArenaGeometry, CoreFixedRoomEncounterError> {
    let center = TilePoint::new(
        i32::try_from(room.width_milli_tiles / 2)
            .map_err(|_| CoreFixedRoomEncounterError::DefinitionDrift)?,
        i32::try_from(room.height_milli_tiles / 2)
            .map_err(|_| CoreFixedRoomEncounterError::DefinitionDrift)?,
    );
    let player_spawn = room.doors.first().map_or(center, |door| {
        TilePoint::new(door.x_milli_tiles, door.y_milli_tiles)
    });
    ArenaGeometry {
        id: format!("{}.combat", room.room_id),
        width_milli_tiles: i32::try_from(room.width_milli_tiles)
            .map_err(|_| CoreFixedRoomEncounterError::DefinitionDrift)?,
        height_milli_tiles: i32::try_from(room.height_milli_tiles)
            .map_err(|_| CoreFixedRoomEncounterError::DefinitionDrift)?,
        shell_thickness_milli_tiles: 1_000,
        player_spawn,
        boss_spawn: center,
        pillars: room
            .volumes
            .iter()
            .filter_map(|volume| match (&volume.kind, &volume.geometry) {
                (
                    DungeonRoomVolumeKind::Solid | DungeonRoomVolumeKind::DeepWater,
                    DungeonRoomVolumeGeometry::Rectangle {
                        x,
                        y,
                        width,
                        height,
                    },
                ) => Some(TileRectangle::new(
                    *x,
                    *y,
                    i32::try_from(*width).ok()?,
                    i32::try_from(*height).ok()?,
                )),
                _ => None,
            })
            .collect(),
        anchors: room
            .anchors
            .iter()
            .map(|anchor| ArenaAnchor {
                id: anchor.id.clone(),
                point: TilePoint::new(anchor.x_milli_tiles, anchor.y_milli_tiles),
            })
            .collect(),
    }
    .validated()
    .map_err(Into::into)
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
    #[error("fixed room {node_id} requires its Core-authored combat owner")]
    AuthoredRuntimeRequired { node_id: String },
    #[error("an occupied immutable fixed room requires an authoritative combat step")]
    MissingCombatStep,
    #[error("fixed-room lifecycle has no participant handoff")]
    MissingParticipantHandoff,
    #[error("fixed-room lifecycle requested completion without an owned wave")]
    MissingWave,
    #[error("fixed-room spawn identity sequence overflowed")]
    IdentityOverflow,
    #[error(transparent)]
    FixedRoom(#[from] FixedRoomError),
    #[error(transparent)]
    EntityId(#[from] NormalWaveEntityIdError),
    #[error(transparent)]
    Room(#[from] sim_core::DungeonRoomError),
    #[error(transparent)]
    Arena(#[from] sim_core::ArenaGeometryError),
    #[error(transparent)]
    Wave(#[from] NormalWaveError),
    #[error(transparent)]
    Content(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, path::Path};

    use super::*;
    use crate::load_core_development_encounter_rooms;
    use sim_core::{
        CollisionTarget, FixedRoomEvent, FriendlyProjectileSource, HostileTargetState,
        NormalWavePhase, PlayerVitals, ProjectileCollision, RawDamageIntent, RawDamageIntentSource,
        RedTonicSimulation, SimulationVector, TonicBelt,
    };

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn player_fixture() -> (EnemyLabPlayer, EntityIdAllocator) {
        let root = content_root();
        let (source, _) = crate::load_and_validate(&root).expect("FP source");
        let fixture = crate::first_playable_authority_combat_test(&source).expect("FP fixture");
        let definitions = fixture.definitions;
        (
            EnemyLabPlayer {
                target: HostileTargetState {
                    entity_id: EntityId::new(900).expect("player ID"),
                    position: SimulationVector::new(3.0, 8.5),
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
            },
            EntityIdAllocator::starting_at(NonZeroU64::new(20_000).expect("projectile allocator")),
        )
    }

    fn room_input(living_inside: u16, tick: u64) -> CoreImmutableFixedRoomInput {
        CoreImmutableFixedRoomInput {
            crossed_activation_boundary: false,
            living_inside,
            living_party_outside: u16::from(living_inside == 0),
            doorway_hurtbox_blocked: false,
            combat_step: (living_inside > 0).then_some(CombatStep {
                tick: Tick(tick),
                ..CombatStep::default()
            }),
        }
    }

    fn lethal_step(wave: &NormalWaveSimulation, tick: u64) -> CombatStep {
        let mut combat = CombatStep {
            tick: Tick(tick),
            ..CombatStep::default()
        };
        for (index, target) in wave
            .snapshots()
            .into_iter()
            .map(|snapshot| snapshot.entity_id)
            .enumerate()
        {
            let projectile_id = EntityId::new(60_000 + u64::try_from(index).expect("index"))
                .expect("projectile ID");
            combat.collisions.push(ProjectileCollision {
                tick: Tick(tick),
                projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(target),
                final_position: SimulationVector::new(5.0, 5.0),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            });
            combat.raw_damage_intents.push(RawDamageIntent {
                tick: Tick(tick),
                projectile_id,
                source: RawDamageIntentSource::Primary,
                target,
                base_raw_damage: 10_000,
                multiplier_basis_points: 10_000,
                resolved_raw_damage: 10_000,
                contact_ordinal: 0,
            });
        }
        combat
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
        assert!(plans.iter().all(|plan| {
            plan.assignments
                .iter()
                .map(|assignment| (assignment.x_milli_tiles, assignment.y_milli_tiles))
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
        let skull = b2
            .assignments
            .iter()
            .find(|assignment| assignment.runtime_kind == CoreFixedRoomActorRuntimeKind::ChoirSkull)
            .expect("Skull");
        assert_eq!(skull.anchor_id, "d3");
        assert_eq!((skull.x_milli_tiles, skull.y_milli_tiles), (10_500, 7_500));
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

    #[test]
    fn only_b1_and_b5_instantiate_through_the_immutable_fp_runtime() {
        let content = load_core_development_encounter_rooms(&content_root()).expect("content");
        let plans = compile_core_fixed_room_encounters(&content, 1).expect("plans");
        for index in [0, 3] {
            let (player, allocator) = player_fixture();
            let wave =
                instantiate_immutable_fixed_room_wave(&plans[index], player, allocator, Tick(100))
                    .expect("immutable room wave");
            assert_eq!(wave.starts_at(), Tick(100));
            assert_eq!(
                wave.phase(),
                NormalWavePhase::DormantTelegraph {
                    activates_at: Tick(127)
                }
            );
            assert_eq!(wave.snapshots().len(), plans[index].assignments.len());
        }
        for index in [1, 2] {
            let (player, allocator) = player_fixture();
            assert!(matches!(
                instantiate_immutable_fixed_room_wave(&plans[index], player, allocator, Tick(100),),
                Err(CoreFixedRoomEncounterError::AuthoredRuntimeRequired { .. })
            ));
        }
    }

    #[test]
    fn b1_owner_derives_completion_and_honors_door_warning_quiet_boundaries() {
        let content = load_core_development_encounter_rooms(&content_root()).expect("content");
        let plan = compile_core_fixed_room_encounters(&content, 1)
            .expect("plans")
            .remove(0);
        let (player, allocator) = player_fixture();
        let mut room =
            CoreImmutableFixedRoomSimulation::new(plan, player, allocator).expect("room");

        let mut blocked = room_input(1, 10);
        blocked.crossed_activation_boundary = true;
        blocked.doorway_hurtbox_blocked = true;
        assert_eq!(
            room.step(Tick(10), &blocked)
                .expect("participant lock")
                .lifecycle_events,
            [FixedRoomEvent::ParticipantLocked {
                activation_ordinal: 1,
                participant_count: 1,
            }]
        );
        assert!(room.wave().is_none());

        let warning = room.step(Tick(11), &room_input(1, 11)).expect("warning");
        assert_eq!(
            warning.lifecycle_events,
            [
                FixedRoomEvent::DoorsClosed,
                FixedRoomEvent::BeginGroupWarning { warning_ticks: 27 },
            ]
        );
        assert_eq!(warning.required_hostiles_remaining, 8);
        assert_eq!(room.wave().expect("wave").starts_at(), Tick(11));

        let before_failed_tick = room.wave().expect("wave").tick();
        let mut missing_combat = room_input(1, 12);
        missing_combat.combat_step = None;
        assert!(matches!(
            room.step(Tick(12), &missing_combat),
            Err(CoreFixedRoomEncounterError::MissingCombatStep)
        ));
        assert_eq!(room.wave().expect("rollback").tick(), before_failed_tick);

        for tick in 12..38 {
            room.step(Tick(tick), &room_input(1, tick))
                .expect("warning progression");
        }
        let mut clearing_input = room_input(1, 38);
        clearing_input.combat_step = Some(lethal_step(room.wave().expect("wave"), 38));
        let cleared = room.step(Tick(38), &clearing_input).expect("clear");
        assert_eq!(cleared.required_hostiles_remaining, 0);
        assert_eq!(
            cleared.lifecycle_events,
            [
                FixedRoomEvent::EncounterActivated,
                FixedRoomEvent::CompletionCommitted {
                    activation_ordinal: 1,
                },
                FixedRoomEvent::ClearHostileOutput,
                FixedRoomEvent::BeginQuietPeriod { quiet_ticks: 60 },
            ]
        );
        assert_eq!(room.phase(), FixedRoomPhase::Quiet);
        assert!(room.wave().is_none());

        for tick in 39..98 {
            assert!(
                room.step(Tick(tick), &room_input(1, tick))
                    .expect("quiet")
                    .lifecycle_events
                    .is_empty()
            );
        }
        assert_eq!(
            room.step(Tick(98), &room_input(1, 98))
                .expect("doors open")
                .lifecycle_events,
            [FixedRoomEvent::DoorsOpened]
        );
        assert_eq!(room.phase(), FixedRoomPhase::Cleared);
    }

    #[test]
    fn b5_owner_resets_hostiles_and_reactivates_with_route_disjoint_identities() {
        let content = load_core_development_encounter_rooms(&content_root()).expect("content");
        let plan = compile_core_fixed_room_encounters(&content, 4)
            .expect("plans")
            .remove(3);
        let (player, allocator) = player_fixture();
        let mut room =
            CoreImmutableFixedRoomSimulation::new(plan, player, allocator).expect("room");

        let mut activate = room_input(1, 1);
        activate.crossed_activation_boundary = true;
        room.step(Tick(1), &activate).expect("first activation");
        assert_eq!(
            room.wave()
                .expect("first wave")
                .snapshots()
                .iter()
                .map(|snapshot| snapshot.instance_id.spawn_ordinal)
                .collect::<Vec<_>>(),
            (19..=25).collect::<Vec<_>>()
        );

        for tick in 2..92 {
            room.step(Tick(tick), &room_input(0, tick))
                .expect("empty countdown");
        }
        let reset = room.step(Tick(92), &room_input(0, 92)).expect("reset");
        assert!(reset.lifecycle_events.contains(&FixedRoomEvent::RoomReset));
        assert!(
            reset
                .lifecycle_events
                .contains(&FixedRoomEvent::DiscardUnsecuredDrops)
        );
        assert!(reset.reset_cleared_hostiles.is_some());
        assert_eq!(room.phase(), FixedRoomPhase::Dormant);
        assert!(room.wave().is_none());

        let mut reactivate = room_input(1, 93);
        reactivate.crossed_activation_boundary = true;
        room.step(Tick(93), &reactivate).expect("reactivation");
        assert_eq!(room.activation_ordinal(), 2);
        assert_eq!(
            room.wave()
                .expect("second wave")
                .snapshots()
                .iter()
                .map(|snapshot| snapshot.instance_id.spawn_ordinal)
                .collect::<Vec<_>>(),
            (44..=50).collect::<Vec<_>>()
        );
    }

    #[test]
    fn mixed_and_miniboss_plans_reject_the_immutable_room_owner() {
        let content = load_core_development_encounter_rooms(&content_root()).expect("content");
        let plans = compile_core_fixed_room_encounters(&content, 1).expect("plans");
        for index in [1, 2] {
            let (player, allocator) = player_fixture();
            assert!(matches!(
                CoreImmutableFixedRoomSimulation::new(plans[index].clone(), player, allocator),
                Err(CoreFixedRoomEncounterError::AuthoredRuntimeRequired { .. })
            ));
        }
    }
}
