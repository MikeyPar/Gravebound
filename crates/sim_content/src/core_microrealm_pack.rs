//! Cross-document construction boundary for the M03 Core microrealm encounter.
//!
//! The world-flow package owns legal anchors, the encounter-room package owns `pack.bell.01`, and
//! the microrealm lifecycle owns the warning event. This module joins those immutable authorities
//! without teaching any one package about the others' source records.

use content_schema::ContentId;
use sim_core::{
    ArenaAnchor, ArenaGeometry, ArenaGeometryError, CORE_MICROREALM_PACK_WARNING_TICKS, CombatStep,
    CoreMicrorealmError, CoreMicrorealmEvent, CoreMicrorealmInput, CoreMicrorealmPhase,
    CoreMicrorealmSimulation, EnemyLabPlayer, EntityId, EntityIdAllocator,
    NormalWaveClearedHostiles, NormalWaveDefinitions, NormalWaveEnemyKind, NormalWaveEntityIdError,
    NormalWaveError, NormalWaveHandoff, NormalWavePhase, NormalWaveSimulation, NormalWaveSpawn,
    NormalWaveStep, SpawnInstanceId, Tick, TilePoint, TileRectangle, normal_wave_entity_id,
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
    construct_core_microrealm_pack_at_ordinal(
        encounters,
        world_flow,
        warning_started_at,
        event,
        run_ordinal,
        1,
    )
}

fn construct_core_microrealm_pack_at_ordinal(
    encounters: &CoreDevelopmentEncounterRooms,
    world_flow: &CoreDevelopmentWorldFlow,
    warning_started_at: Tick,
    event: CoreMicrorealmEvent,
    run_ordinal: u32,
    first_spawn_ordinal: u16,
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
            let offset =
                u16::try_from(index).map_err(|_| CoreMicrorealmPackError::DefinitionDrift)?;
            let spawn_ordinal = first_spawn_ordinal
                .checked_add(offset)
                .ok_or(CoreMicrorealmPackError::DefinitionDrift)?;
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

#[derive(Debug, Clone)]
pub struct CoreMicrorealmEncounterInput {
    pub entrant_position: TilePoint,
    pub primary_released: bool,
    pub living_participants: u16,
    pub combat_step: Option<CombatStep>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreMicrorealmEncounterStep {
    pub tick: Tick,
    pub phase_after: CoreMicrorealmPhase,
    pub lifecycle_events: Vec<CoreMicrorealmEvent>,
    pub wave_step: Option<NormalWaveStep>,
    pub reset_cleared_hostiles: Option<NormalWaveClearedHostiles>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreMicrorealmPackCombatTransition {
    WarningPrepared,
    Reset {
        cleared_hostiles: NormalWaveClearedHostiles,
    },
    Cleared,
}

#[derive(Debug, Clone)]
enum CoreMicrorealmPackCombatState {
    Handoff(Box<NormalWaveHandoff>),
    Wave(Box<NormalWaveSimulation>),
}

/// Lifecycle-free capacity-one combat owner for the exact Core microrealm pack.
///
/// The server supplies already-authoritative lifecycle events. This component never evaluates
/// entrant movement, participant presence, reset deadlines, or portal state; it owns only the
/// participant/projectile handoff or the one wave created from that handoff.
#[derive(Debug, Clone)]
pub struct CoreMicrorealmPackCombat {
    encounters: CoreDevelopmentEncounterRooms,
    world_flow: CoreDevelopmentWorldFlow,
    run_ordinal: u32,
    next_spawn_ordinal: u16,
    state: CoreMicrorealmPackCombatState,
}

impl CoreMicrorealmPackCombat {
    pub fn new(
        encounters: CoreDevelopmentEncounterRooms,
        world_flow: CoreDevelopmentWorldFlow,
        run_ordinal: u32,
        player: EnemyLabPlayer,
        hostile_projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CoreMicrorealmPackError> {
        if run_ordinal == 0 {
            return Err(CoreMicrorealmPackError::EntityId(
                NormalWaveEntityIdError::ZeroRunOrdinal,
            ));
        }
        Ok(Self {
            encounters,
            world_flow,
            run_ordinal,
            next_spawn_ordinal: 1,
            state: CoreMicrorealmPackCombatState::Handoff(Box::new(NormalWaveHandoff {
                player,
                hostile_projectile_ids,
            })),
        })
    }

    #[must_use]
    pub const fn wave(&self) -> Option<&NormalWaveSimulation> {
        match &self.state {
            CoreMicrorealmPackCombatState::Handoff(_) => None,
            CoreMicrorealmPackCombatState::Wave(wave) => Some(wave),
        }
    }

    #[must_use]
    pub fn player(&self) -> &EnemyLabPlayer {
        match &self.state {
            CoreMicrorealmPackCombatState::Handoff(handoff) => &handoff.player,
            CoreMicrorealmPackCombatState::Wave(wave) => wave.player(),
        }
    }

    pub fn player_mut(&mut self) -> &mut EnemyLabPlayer {
        match &mut self.state {
            CoreMicrorealmPackCombatState::Handoff(handoff) => &mut handoff.player,
            CoreMicrorealmPackCombatState::Wave(wave) => wave.player_mut(),
        }
    }

    pub fn alive_hurtboxes(&self) -> Result<Vec<sim_core::EnemyHurtbox>, CoreMicrorealmPackError> {
        match &self.state {
            CoreMicrorealmPackCombatState::Handoff(_) => Ok(Vec::new()),
            CoreMicrorealmPackCombatState::Wave(wave) => wave.alive_hurtboxes().map_err(Into::into),
        }
    }

    /// Returns the exact compiled collision arena used by this pack. The server combat owner uses
    /// the same geometry for friendly projectile collision before feeding the resulting step back
    /// into this component.
    pub fn arena(&self) -> Result<ArenaGeometry, CoreMicrorealmPackError> {
        microrealm_combat_arena(&self.world_flow)
    }

    /// Consumes a quiet or cleared component and returns the one mutable participant allocation.
    /// Active combat cannot be transferred to another scene.
    pub fn into_handoff(self) -> Result<NormalWaveHandoff, CoreMicrorealmPackError> {
        match self.state {
            CoreMicrorealmPackCombatState::Handoff(handoff) => Ok(*handoff),
            CoreMicrorealmPackCombatState::Wave(_) => {
                Err(CoreMicrorealmPackError::HandoffBeforeClear)
            }
        }
    }

    /// Advances the owned wave with an existing server-generated combat step. State changes only
    /// after the complete wave step succeeds.
    pub fn step(
        &mut self,
        combat_step: &CombatStep,
    ) -> Result<NormalWaveStep, CoreMicrorealmPackError> {
        let mut staged = self.clone();
        let step = match &mut staged.state {
            CoreMicrorealmPackCombatState::Handoff(_) => {
                return Err(CoreMicrorealmPackError::MissingWave);
            }
            CoreMicrorealmPackCombatState::Wave(wave) => wave.step(combat_step)?,
        };
        *self = staged;
        Ok(step)
    }

    /// Applies one event emitted by the authoritative lifecycle. Construction, ordinal advance,
    /// reset cleanup, and clear handoff are staged on a clone and commit together.
    pub fn apply_lifecycle_event(
        &mut self,
        tick: Tick,
        event: CoreMicrorealmEvent,
    ) -> Result<CoreMicrorealmPackCombatTransition, CoreMicrorealmPackError> {
        let mut staged = self.clone();
        let transition = staged.apply_lifecycle_event_inner(tick, event)?;
        *self = staged;
        Ok(transition)
    }

    fn apply_lifecycle_event_inner(
        &mut self,
        tick: Tick,
        event: CoreMicrorealmEvent,
    ) -> Result<CoreMicrorealmPackCombatTransition, CoreMicrorealmPackError> {
        match event {
            CoreMicrorealmEvent::BeginPackWarning { .. } => {
                let CoreMicrorealmPackCombatState::Handoff(participant) = &self.state else {
                    return Err(CoreMicrorealmPackError::MissingParticipantHandoff);
                };
                let plan = construct_core_microrealm_pack_at_ordinal(
                    &self.encounters,
                    &self.world_flow,
                    tick,
                    event,
                    self.run_ordinal,
                    self.next_spawn_ordinal,
                )?;
                let wave = instantiate_core_microrealm_pack(
                    &plan,
                    &self.world_flow,
                    participant.player.clone(),
                    participant.hostile_projectile_ids.clone(),
                )?;
                let next_spawn_ordinal = self
                    .next_spawn_ordinal
                    .checked_add(8)
                    .ok_or(CoreMicrorealmPackError::DefinitionDrift)?;
                self.state = CoreMicrorealmPackCombatState::Wave(Box::new(wave));
                self.next_spawn_ordinal = next_spawn_ordinal;
                Ok(CoreMicrorealmPackCombatTransition::WarningPrepared)
            }
            CoreMicrorealmEvent::ResetPack => {
                let CoreMicrorealmPackCombatState::Wave(wave) = &self.state else {
                    return Err(CoreMicrorealmPackError::MissingWave);
                };
                let reset = wave.as_ref().clone().into_reset_handoff()?;
                self.state = CoreMicrorealmPackCombatState::Handoff(Box::new(reset.participant));
                Ok(CoreMicrorealmPackCombatTransition::Reset {
                    cleared_hostiles: reset.cleared_hostiles,
                })
            }
            CoreMicrorealmEvent::Cleared => {
                let CoreMicrorealmPackCombatState::Wave(wave) = &self.state else {
                    return Err(CoreMicrorealmPackError::MissingWave);
                };
                let participant = wave.as_ref().clone().into_handoff()?;
                self.state = CoreMicrorealmPackCombatState::Handoff(Box::new(participant));
                Ok(CoreMicrorealmPackCombatTransition::Cleared)
            }
        }
    }
}

/// Capacity-one owner joining the `03C` lifecycle to the exact `03D` pack runtime.
#[derive(Debug, Clone)]
pub struct CoreMicrorealmEncounterSimulation {
    lifecycle: CoreMicrorealmSimulation,
    combat: CoreMicrorealmPackCombat,
}

impl CoreMicrorealmEncounterSimulation {
    pub fn new(
        encounters: CoreDevelopmentEncounterRooms,
        world_flow: CoreDevelopmentWorldFlow,
        run_ordinal: u32,
        player: EnemyLabPlayer,
        hostile_projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CoreMicrorealmPackError> {
        let spawn = world_flow.world().player_spawn;
        Ok(Self {
            lifecycle: CoreMicrorealmSimulation::new(TilePoint::new(spawn.x, spawn.y)),
            combat: CoreMicrorealmPackCombat::new(
                encounters,
                world_flow,
                run_ordinal,
                player,
                hostile_projectile_ids,
            )?,
        })
    }

    #[must_use]
    pub const fn phase(&self) -> CoreMicrorealmPhase {
        self.lifecycle.phase()
    }

    #[must_use]
    pub const fn wave(&self) -> Option<&NormalWaveSimulation> {
        self.combat.wave()
    }

    #[must_use]
    pub const fn bell_portal_available(&self) -> bool {
        self.lifecycle.bell_portal_available()
    }

    pub fn step(
        &mut self,
        tick: Tick,
        input: &CoreMicrorealmEncounterInput,
    ) -> Result<CoreMicrorealmEncounterStep, CoreMicrorealmPackError> {
        let mut staged = self.clone();
        let step = staged.step_inner(tick, input)?;
        *self = staged;
        Ok(step)
    }

    fn step_inner(
        &mut self,
        tick: Tick,
        input: &CoreMicrorealmEncounterInput,
    ) -> Result<CoreMicrorealmEncounterStep, CoreMicrorealmPackError> {
        let mut wave_step = None;
        let mut wave_cleared = false;
        if self.combat.wave().is_some() {
            let combat_step = match input.combat_step.as_ref() {
                Some(step) => step.clone(),
                None if input.living_participants == 0 => CombatStep {
                    tick,
                    ..CombatStep::default()
                },
                None => return Err(CoreMicrorealmPackError::MissingCombatStep),
            };
            let step = self.combat.step(&combat_step)?;
            wave_cleared = matches!(step.phase_after, NormalWavePhase::Cleared { .. });
            wave_step = Some(step);
        }

        let lifecycle_events = self.lifecycle.step(
            tick,
            CoreMicrorealmInput {
                entrant_position: input.entrant_position,
                primary_released: input.primary_released,
                living_participants: input.living_participants,
                pack_cleared: wave_cleared,
            },
        )?;
        let mut reset_cleared_hostiles = None;
        for event in lifecycle_events.iter().copied() {
            match event {
                CoreMicrorealmEvent::BeginPackWarning { .. } => {
                    self.combat.apply_lifecycle_event(tick, event)?;
                    let initial_combat = input.combat_step.clone().unwrap_or(CombatStep {
                        tick,
                        ..CombatStep::default()
                    });
                    wave_step = Some(self.combat.step(&initial_combat)?);
                }
                CoreMicrorealmEvent::ResetPack => {
                    let CoreMicrorealmPackCombatTransition::Reset { cleared_hostiles } =
                        self.combat.apply_lifecycle_event(tick, event)?
                    else {
                        return Err(CoreMicrorealmPackError::DefinitionDrift);
                    };
                    reset_cleared_hostiles = Some(cleared_hostiles);
                }
                CoreMicrorealmEvent::Cleared => {
                    self.combat.apply_lifecycle_event(tick, event)?;
                }
            }
        }
        Ok(CoreMicrorealmEncounterStep {
            tick,
            phase_after: self.lifecycle.phase(),
            lifecycle_events,
            wave_step,
            reset_cleared_hostiles,
        })
    }
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
    let width = i32::try_from(world.width_tiles)
        .ok()
        .and_then(|tiles| tiles.checked_mul(1_000))
        .ok_or(CoreMicrorealmPackError::DefinitionDrift)?;
    let height = i32::try_from(world.height_tiles)
        .ok()
        .and_then(|tiles| tiles.checked_mul(1_000))
        .ok_or(CoreMicrorealmPackError::DefinitionDrift)?;
    let shell = i32::try_from(world.solid_shell_tiles)
        .ok()
        .and_then(|tiles| tiles.checked_mul(1_000))
        .ok_or(CoreMicrorealmPackError::DefinitionDrift)?;
    let interior_height = height
        .checked_sub(
            shell
                .checked_mul(2)
                .ok_or(CoreMicrorealmPackError::DefinitionDrift)?,
        )
        .ok_or(CoreMicrorealmPackError::DefinitionDrift)?;
    ArenaGeometry {
        id: world.header.id.to_string(),
        width_milli_tiles: width,
        height_milli_tiles: height,
        shell_thickness_milli_tiles: shell,
        player_spawn: TilePoint::new(world.player_spawn.x, world.player_spawn.y),
        // `ArenaGeometry` carries a compatibility boss point; the Core microrealm has no boss.
        boss_spawn: TilePoint::new(
            world.bell_portal_area.center.x,
            world.bell_portal_area.center.y,
        ),
        // `WorldSceneDefinition` treats the authored shell as interior blocked terrain. Materialize
        // that same shell as non-overlapping arena solids so avatar, projectile, and Slipstep
        // collision share one geometry interpretation.
        pillars: vec![
            TileRectangle::new(0, 0, width, shell),
            TileRectangle::new(0, height - shell, width, shell),
            TileRectangle::new(0, shell, shell, interior_height),
            TileRectangle::new(width - shell, shell, shell, interior_height),
        ],
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
    #[error("an active Core microrealm participant requires an authoritative combat step")]
    MissingCombatStep,
    #[error("Core microrealm pack warning has no participant handoff")]
    MissingParticipantHandoff,
    #[error("Core microrealm lifecycle requested cleanup without an owned wave")]
    MissingWave,
    #[error("Core microrealm combat cannot hand off while its wave remains active")]
    HandoffBeforeClear,
    #[error(transparent)]
    EntityId(#[from] NormalWaveEntityIdError),
    #[error(transparent)]
    Arena(#[from] ArenaGeometryError),
    #[error(transparent)]
    Wave(#[from] NormalWaveError),
    #[error(transparent)]
    Lifecycle(#[from] CoreMicrorealmError),
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, path::Path};

    use super::*;
    use crate::{load_core_development_encounter_rooms, load_core_development_world_flow};
    use sim_core::{
        CollisionTarget, CombatStep, FriendlyProjectileSource, HostileTargetState, NormalWavePhase,
        PlayerVitals, ProjectileCollision, RawDamageIntent, RawDamageIntentSource,
        RedTonicSimulation, SimulationVector, TonicBelt,
    };

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn combat_inputs() -> (
        CoreDevelopmentEncounterRooms,
        CoreDevelopmentWorldFlow,
        EnemyLabPlayer,
        EntityIdAllocator,
    ) {
        let root = content_root();
        let encounters = load_core_development_encounter_rooms(&root).expect("encounters");
        let world = load_core_development_world_flow(&root).expect("world");
        let (source, _) = crate::load_and_validate(&root).expect("FP source");
        let fixture = crate::first_playable_authority_combat_test(&source).expect("FP fixture");
        let definitions = fixture.definitions;
        let player = EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: EntityId::new(900).expect("player ID"),
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
        (
            encounters,
            world,
            player,
            EntityIdAllocator::starting_at(NonZeroU64::new(20_000).expect("projectile ID")),
        )
    }

    fn combat_fixture() -> CoreMicrorealmPackCombat {
        let (encounters, world, player, projectile_ids) = combat_inputs();
        CoreMicrorealmPackCombat::new(encounters, world, 1, player, projectile_ids).expect("combat")
    }

    fn runtime_fixture() -> CoreMicrorealmEncounterSimulation {
        let (encounters, world, player, projectile_ids) = combat_inputs();
        CoreMicrorealmEncounterSimulation::new(encounters, world, 1, player, projectile_ids)
            .expect("runtime")
    }

    fn lethal_step(combat: &CoreMicrorealmPackCombat, tick: Tick) -> CombatStep {
        let targets = combat
            .wave()
            .expect("wave")
            .snapshots()
            .into_iter()
            .map(|snapshot| snapshot.entity_id)
            .collect::<Vec<_>>();
        let mut lethal = CombatStep {
            tick,
            ..CombatStep::default()
        };
        for (index, target) in targets.into_iter().enumerate() {
            let projectile_id = EntityId::new(50_000 + u64::try_from(index).expect("index"))
                .expect("projectile ID");
            lethal.collisions.push(ProjectileCollision {
                tick,
                projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(target),
                final_position: SimulationVector::new(8.5, 8.5),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            });
            lethal.raw_damage_intents.push(RawDamageIntent {
                tick,
                projectile_id,
                source: RawDamageIntentSource::Primary,
                target,
                base_raw_damage: 10_000,
                multiplier_basis_points: 10_000,
                resolved_raw_damage: 10_000,
                contact_ordinal: 0,
            });
        }
        lethal
    }

    #[test]
    fn combat_component_warning_prepares_exact_wave_without_lifecycle_authority() {
        let mut combat = combat_fixture();
        let player_id = combat.player().target.entity_id;
        assert!(combat.wave().is_none());
        assert_eq!(
            combat
                .apply_lifecycle_event(
                    Tick(32),
                    CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 },
                )
                .expect("warning"),
            CoreMicrorealmPackCombatTransition::WarningPrepared
        );
        let wave = combat.wave().expect("prepared wave");
        assert_eq!(wave.player().target.entity_id, player_id);
        assert_eq!(
            wave.snapshots()
                .iter()
                .map(|snapshot| snapshot.instance_id.spawn_ordinal)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<_>>()
        );
        assert_eq!(
            wave.phase(),
            NormalWavePhase::DormantTelegraph {
                activates_at: Tick(59)
            }
        );
    }

    #[test]
    fn combat_component_reset_returns_cleanup_and_preserves_participant() {
        let mut combat = combat_fixture();
        let player_id = combat.player().target.entity_id;
        combat
            .apply_lifecycle_event(
                Tick(32),
                CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 },
            )
            .expect("warning");
        combat
            .step(&CombatStep {
                tick: Tick(32),
                ..CombatStep::default()
            })
            .expect("warning step");
        let CoreMicrorealmPackCombatTransition::Reset { cleared_hostiles } = combat
            .apply_lifecycle_event(Tick(33), CoreMicrorealmEvent::ResetPack)
            .expect("reset")
        else {
            panic!("unexpected transition");
        };
        assert!(cleared_hostiles.projectiles.is_empty());
        assert!(cleared_hostiles.lanes.is_empty());
        assert!(combat.wave().is_none());
        assert_eq!(combat.player().target.entity_id, player_id);
    }

    #[test]
    fn combat_component_reset_never_reuses_spawn_ordinals() {
        let mut combat = combat_fixture();
        combat
            .apply_lifecycle_event(
                Tick(32),
                CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 },
            )
            .expect("first warning");
        combat
            .apply_lifecycle_event(Tick(33), CoreMicrorealmEvent::ResetPack)
            .expect("reset");
        combat
            .apply_lifecycle_event(
                Tick(64),
                CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 },
            )
            .expect("second warning");
        assert_eq!(
            combat
                .wave()
                .expect("second wave")
                .snapshots()
                .iter()
                .map(|snapshot| snapshot.instance_id.spawn_ordinal)
                .collect::<Vec<_>>(),
            (9..=16).collect::<Vec<_>>()
        );
    }

    #[test]
    fn combat_component_clear_validates_and_restores_terminal_handoff() {
        let mut combat = combat_fixture();
        let player_id = combat.player().target.entity_id;
        combat
            .apply_lifecycle_event(
                Tick(32),
                CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 },
            )
            .expect("warning");
        for tick in 32..59 {
            combat
                .step(&CombatStep {
                    tick: Tick(tick),
                    ..CombatStep::default()
                })
                .expect("warning progression");
        }
        let lethal = lethal_step(&combat, Tick(59));
        let cleared = combat.step(&lethal).expect("authoritative clear");
        assert!(matches!(
            cleared.phase_after,
            NormalWavePhase::Cleared { .. }
        ));
        assert_eq!(
            combat
                .apply_lifecycle_event(Tick(59), CoreMicrorealmEvent::Cleared)
                .expect("clear handoff"),
            CoreMicrorealmPackCombatTransition::Cleared
        );
        assert!(combat.wave().is_none());
        assert_eq!(combat.player().target.entity_id, player_id);
        assert!(combat.alive_hurtboxes().expect("hurtboxes").is_empty());
    }

    #[test]
    fn combat_component_handoff_is_available_only_without_an_active_wave() {
        let quiet = combat_fixture();
        let player_id = quiet.player().target.entity_id;
        assert_eq!(
            quiet
                .into_handoff()
                .expect("quiet handoff")
                .player
                .target
                .entity_id,
            player_id
        );

        let mut active = combat_fixture();
        active
            .apply_lifecycle_event(
                Tick(32),
                CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 },
            )
            .expect("warning");
        assert!(matches!(
            active.into_handoff(),
            Err(CoreMicrorealmPackError::HandoffBeforeClear)
        ));
    }

    #[test]
    fn combat_arena_materializes_the_exact_compiled_world_shell() {
        let combat = combat_fixture();
        let arena = combat.arena().expect("combat arena");
        assert_eq!(arena.pillars.len(), 4);

        let root = content_root();
        let scene = load_core_development_world_flow(&root)
            .expect("world")
            .compile_microrealm_scene()
            .expect("scene");
        let mut movement = sim_core::PlayerMovementState::at_arena_spawn(&arena).expect("player");
        let collision =
            sim_core::ProjectileCollisionWorld::new(&arena, Vec::new()).expect("collision world");
        let stopped = movement
            .apply_forced_displacement(SimulationVector::new(-20.0, 0.0), &collision, &arena)
            .expect("forced movement");
        let projected = sim_core::simulation_to_tile_point(stopped.position).expect("projection");
        assert_eq!(projected.x_milli_tiles, 1_300);
        assert!(stopped.solid.is_some());
        assert!(scene.can_occupy(projected));
        assert!(!scene.can_occupy(TilePoint::new(1_299, projected.y_milli_tiles)));
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

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the reset, rollback, and retrigger boundaries remain one readable lifecycle trace"
    )]
    fn encounter_owner_resets_atomically_and_never_reuses_spawn_ordinals() {
        let mut runtime = runtime_fixture();
        let spawn = TilePoint::new(8_500, 40_500);
        runtime
            .step(
                Tick(1),
                &CoreMicrorealmEncounterInput {
                    entrant_position: spawn,
                    primary_released: true,
                    living_participants: 1,
                    combat_step: None,
                },
            )
            .expect("trigger");
        let warning = runtime
            .step(
                Tick(31),
                &CoreMicrorealmEncounterInput {
                    entrant_position: spawn,
                    primary_released: false,
                    living_participants: 1,
                    combat_step: Some(CombatStep {
                        tick: Tick(31),
                        ..CombatStep::default()
                    }),
                },
            )
            .expect("warning");
        assert_eq!(
            warning.lifecycle_events,
            [CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 }]
        );
        assert_eq!(
            runtime
                .wave()
                .expect("wave")
                .snapshots()
                .iter()
                .map(|snapshot| snapshot.instance_id.spawn_ordinal)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<_>>()
        );

        let before_failed_tick = runtime.wave().expect("wave").tick();
        assert!(matches!(
            runtime.step(
                Tick(32),
                &CoreMicrorealmEncounterInput {
                    entrant_position: spawn,
                    primary_released: false,
                    living_participants: 1,
                    combat_step: None,
                },
            ),
            Err(CoreMicrorealmPackError::MissingCombatStep)
        ));
        assert_eq!(
            runtime.wave().expect("rollback wave").tick(),
            before_failed_tick
        );

        for tick in 32..182 {
            let step = runtime
                .step(
                    Tick(tick),
                    &CoreMicrorealmEncounterInput {
                        entrant_position: spawn,
                        primary_released: false,
                        living_participants: 0,
                        combat_step: None,
                    },
                )
                .expect("empty tick");
            assert!(step.reset_cleared_hostiles.is_none());
        }
        let reset = runtime
            .step(
                Tick(182),
                &CoreMicrorealmEncounterInput {
                    entrant_position: spawn,
                    primary_released: false,
                    living_participants: 0,
                    combat_step: None,
                },
            )
            .expect("reset");
        assert_eq!(reset.lifecycle_events, [CoreMicrorealmEvent::ResetPack]);
        assert!(reset.reset_cleared_hostiles.is_some());
        assert_eq!(runtime.phase(), CoreMicrorealmPhase::Dormant);
        assert!(runtime.wave().is_none());

        runtime
            .step(
                Tick(183),
                &CoreMicrorealmEncounterInput {
                    entrant_position: spawn,
                    primary_released: true,
                    living_participants: 1,
                    combat_step: None,
                },
            )
            .expect("retrigger");
        runtime
            .step(
                Tick(213),
                &CoreMicrorealmEncounterInput {
                    entrant_position: spawn,
                    primary_released: false,
                    living_participants: 1,
                    combat_step: Some(CombatStep {
                        tick: Tick(213),
                        ..CombatStep::default()
                    }),
                },
            )
            .expect("second warning");
        assert_eq!(
            runtime
                .wave()
                .expect("second wave")
                .snapshots()
                .iter()
                .map(|snapshot| snapshot.instance_id.spawn_ordinal)
                .collect::<Vec<_>>(),
            (9..=16).collect::<Vec<_>>()
        );
    }

    #[test]
    fn only_authoritative_wave_defeat_opens_the_terminal_bell_portal() {
        let mut runtime = runtime_fixture();
        let spawn = TilePoint::new(8_500, 40_500);
        runtime
            .step(
                Tick(1),
                &CoreMicrorealmEncounterInput {
                    entrant_position: spawn,
                    primary_released: true,
                    living_participants: 1,
                    combat_step: None,
                },
            )
            .expect("trigger");
        runtime
            .step(
                Tick(31),
                &CoreMicrorealmEncounterInput {
                    entrant_position: spawn,
                    primary_released: false,
                    living_participants: 1,
                    combat_step: Some(CombatStep {
                        tick: Tick(31),
                        ..CombatStep::default()
                    }),
                },
            )
            .expect("warning");
        for tick in 32..58 {
            runtime
                .step(
                    Tick(tick),
                    &CoreMicrorealmEncounterInput {
                        entrant_position: spawn,
                        primary_released: false,
                        living_participants: 1,
                        combat_step: Some(CombatStep {
                            tick: Tick(tick),
                            ..CombatStep::default()
                        }),
                    },
                )
                .expect("warning progression");
        }
        assert!(!runtime.bell_portal_available());
        let targets = runtime
            .wave()
            .expect("wave")
            .snapshots()
            .into_iter()
            .map(|snapshot| snapshot.entity_id)
            .collect::<Vec<_>>();
        let mut lethal = CombatStep {
            tick: Tick(58),
            ..CombatStep::default()
        };
        for (index, target) in targets.into_iter().enumerate() {
            let projectile_id = EntityId::new(50_000 + u64::try_from(index).expect("index"))
                .expect("projectile ID");
            lethal.collisions.push(ProjectileCollision {
                tick: Tick(58),
                projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(target),
                final_position: SimulationVector::new(8.5, 8.5),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            });
            lethal.raw_damage_intents.push(RawDamageIntent {
                tick: Tick(58),
                projectile_id,
                source: RawDamageIntentSource::Primary,
                target,
                base_raw_damage: 10_000,
                multiplier_basis_points: 10_000,
                resolved_raw_damage: 10_000,
                contact_ordinal: 0,
            });
        }
        let cleared = runtime
            .step(
                Tick(58),
                &CoreMicrorealmEncounterInput {
                    entrant_position: spawn,
                    primary_released: false,
                    living_participants: 1,
                    combat_step: Some(lethal),
                },
            )
            .expect("authoritative clear");
        assert_eq!(cleared.lifecycle_events, [CoreMicrorealmEvent::Cleared]);
        assert_eq!(cleared.phase_after, CoreMicrorealmPhase::Cleared);
        assert!(runtime.bell_portal_available());
        assert!(runtime.wave().is_none());
    }
}
