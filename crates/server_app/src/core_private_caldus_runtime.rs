//! Route-bound owner for the Core Sir Caldus B6 lifecycle.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`SIM-004`,
//! `DNG-006`, `ENC-010`, `TECH-012`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-ROOM-002`, `CONT-BOSS-001`-`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`). Loading, lock, player simulation,
//! encounter simulation, eligibility evidence, and route CAS are staged and committed as one
//! frame. Durable reward persistence remains outside this owner; only its exact frozen result may
//! unlock the stable exit. Normal route admission remains disabled.

use std::collections::BTreeMap;

use protocol::{
    CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1, CorePrivateRouteSceneV1,
    CorePrivateRouteStateV1,
};
use sim_core::{
    BodyCollisionWorld, CoreBossConnectionState, CoreBossEntrantInput, CoreBossLifeState,
    CoreBossLockEvent, CoreBossLockInput, CoreBossLockPhase, CoreBossLockSimulation,
    CoreBossLockStep, CoreBossParticipant, CoreCaldusEncounterSimulation, CoreCaldusEncounterStep,
    CoreCaldusFriendlyInput, CoreCaldusPhase, CoreCaldusState, EnemyLabPlayer, EntityId,
    EntityIdAllocator, PlayerMovementState, ProjectileCollisionWorld, Tick, TilePoint,
    core_caldus_entity_id, simulation_to_tile_point, tile_point_to_simulation,
};
use thiserror::Error;

use crate::{
    CaldusInstancePresentation, CoreCharacterCombatEnvelope, CoreDurableCaldusResolution,
    CorePrivateCaldusDefeatHandoff, CorePrivateCaldusRewardCommit,
    CorePrivateCaldusRewardCommitDisposition, CorePrivateCaldusRewardError,
    CorePrivateCaldusStagingHandoff, CorePrivateMicrorealmInput, CorePrivateMicrorealmRuntimeError,
    CorePrivatePlayerDamageError, CorePrivatePlayerDamageFactV1, CorePrivateRouteActorDirectory,
    CorePrivateRouteActorLease, CorePrivateRouteRuntimeError, caldus_player_damage_facts,
    core_private_caldus_reward::CoreCaldusRewardTracker,
    core_private_combat_frame::{core_player_movement_config, step_live_player_combat_with_bodies},
    core_private_gameplay_observation::{
        CorePrivateGameplayObservation, CorePrivateGameplayObservationError,
        CorePrivateProjectileProvenance, boss_snapshot, hostile_projectile_snapshot,
        player_snapshot,
    },
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorePrivateCaldusRuntimeInput {
    pub action: CorePrivateMicrorealmInput,
    pub connection: CoreBossConnectionState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorePrivateCaldusFrame {
    pub input_sequence: u64,
    pub tick: Tick,
    pub player_position: TilePoint,
    pub movement: sim_core::MovementStep,
    pub combat: sim_core::CombatStep,
    pub(crate) observation: CorePrivateGameplayObservation,
    pub lock: CoreBossLockStep,
    pub encounter: Option<CoreCaldusEncounterStep>,
    pub route: CorePrivateRouteStateV1,
    pub boss_entity_id: EntityId,
    pub boss_health: Option<(u32, u32)>,
    pub player_damage: Vec<CorePrivatePlayerDamageFactV1>,
    pub player_died: bool,
}

/// Neutral post-reward frame authority. Combat remains frozen, while Recall, extraction, and
/// terminal precedence continue to receive monotonically increasing authoritative ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CorePrivateCaldusTerminalHeartbeat {
    pub tick: Tick,
    pub player_position: TilePoint,
    pub route: CorePrivateRouteStateV1,
}

#[derive(Debug)]
pub struct CorePrivateCaldusRuntime {
    route_directory: CorePrivateRouteActorDirectory,
    route_lease: CorePrivateRouteActorLease,
    entry_restore_point_id: [u8; 16],
    combat_envelope: CoreCharacterCombatEnvelope,
    arena: sim_core::ArenaGeometry,
    movement: PlayerMovementState,
    players: BTreeMap<EntityId, EnemyLabPlayer>,
    projectile_ids: Option<EntityIdAllocator>,
    lock: CoreBossLockSimulation,
    encounter: Option<CoreCaldusEncounterSimulation>,
    participant: CoreBossParticipant,
    boss_entity_id: EntityId,
    route_phase: CorePrivateRoutePhaseV1,
    tick: Tick,
    reward_tracker: CoreCaldusRewardTracker,
    defeat_handoff: Option<CorePrivateCaldusDefeatHandoff>,
    reward_resolution: Option<CoreDurableCaldusResolution>,
    presentation: Option<CaldusInstancePresentation>,
    projectile_provenance: CorePrivateProjectileProvenance,
}

struct StagedCaldusFrame {
    lock_simulation: CoreBossLockSimulation,
    players: BTreeMap<EntityId, EnemyLabPlayer>,
    movement_state: PlayerMovementState,
    encounter_simulation: Option<CoreCaldusEncounterSimulation>,
    projectile_ids: Option<EntityIdAllocator>,
    route_phase: CorePrivateRoutePhaseV1,
    movement: sim_core::MovementStep,
    combat: sim_core::CombatStep,
    lock: CoreBossLockStep,
    encounter: Option<CoreCaldusEncounterStep>,
    reward_tracker: CoreCaldusRewardTracker,
    projectile_provenance: CorePrivateProjectileProvenance,
}

impl CorePrivateCaldusRuntime {
    pub fn from_staging_handoff(
        mut handoff: CorePrivateCaldusStagingHandoff,
    ) -> Result<Self, CorePrivateCaldusRuntimeError> {
        let route = handoff.route_directory.snapshot(handoff.route_lease)?;
        validate_staging_route(&route, &handoff)?;
        if handoff.arena.id != "arena.boss.caldus_01.combat"
            || handoff.arena.boss_spawn != TilePoint::new(9_000, 9_000)
            || handoff.participant.player.combat.tick() != handoff.tick
            || handoff.participant.player.consumables.tick() != handoff.tick
        {
            return Err(CorePrivateCaldusRuntimeError::InvalidComposition);
        }
        let first_tick = handoff
            .tick
            .checked_next()
            .ok_or(CorePrivateCaldusRuntimeError::TickExhausted)?;
        let run_ordinal = u32::try_from(route.actor_generation)
            .map_err(|_| CorePrivateCaldusRuntimeError::InvalidComposition)?;
        let boss_entity_id = core_caldus_entity_id(run_ordinal)?;
        let player_id = handoff.participant.player.target.entity_id;
        if player_id == boss_entity_id {
            return Err(CorePrivateCaldusRuntimeError::InvalidComposition);
        }
        let spawn = tile_point_to_simulation(handoff.arena.player_spawn);
        handoff.participant.player.target.position = spawn;
        let movement = PlayerMovementState::new_with_config(
            spawn,
            core_player_movement_config(
                handoff.combat_envelope.movement_milli_tiles_per_second(),
                sim_core::PLAYER_COLLISION_RADIUS_MILLI_TILES,
            )?,
            &handoff.arena,
        )?;
        let participant = CoreBossParticipant {
            entity_id: player_id,
            party_slot: 0,
        };
        Ok(Self {
            route_directory: handoff.route_directory,
            route_lease: handoff.route_lease,
            entry_restore_point_id: handoff.entry_restore_point_id,
            combat_envelope: handoff.combat_envelope,
            arena: handoff.arena,
            movement,
            players: BTreeMap::from([(player_id, handoff.participant.player)]),
            projectile_ids: Some(handoff.participant.hostile_projectile_ids),
            lock: CoreBossLockSimulation::new_at_tick(first_tick),
            encounter: None,
            participant,
            boss_entity_id,
            route_phase: CorePrivateRoutePhaseV1::BossStaging,
            tick: handoff.tick,
            reward_tracker: CoreCaldusRewardTracker::new(handoff.last_reward_activity_sequence),
            defeat_handoff: None,
            reward_resolution: None,
            presentation: None,
            projectile_provenance: handoff.projectile_provenance,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn route_lease(&self) -> CorePrivateRouteActorLease {
        self.route_lease
    }

    #[must_use]
    pub const fn boss_entity_id(&self) -> EntityId {
        self.boss_entity_id
    }

    #[must_use]
    pub const fn combat_envelope(&self) -> &CoreCharacterCombatEnvelope {
        &self.combat_envelope
    }

    #[must_use]
    pub const fn arena(&self) -> &sim_core::ArenaGeometry {
        &self.arena
    }

    #[must_use]
    pub fn player(&self) -> &EnemyLabPlayer {
        self.players
            .get(&self.participant.entity_id)
            .expect("constructor installs immutable participant")
    }

    #[must_use]
    pub const fn pending_reward_handoff(&self) -> Option<&CorePrivateCaldusDefeatHandoff> {
        self.defeat_handoff.as_ref()
    }

    #[must_use]
    pub fn presentation(&self) -> Option<&CaldusInstancePresentation> {
        self.presentation.as_ref()
    }

    pub async fn step(
        &mut self,
        input: CorePrivateCaldusRuntimeInput,
    ) -> Result<CorePrivateCaldusFrame, CorePrivateCaldusRuntimeError> {
        self.step_inner(input, None).await
    }

    pub async fn commit_reward_resolution(
        &mut self,
        content: &sim_content::CoreDevelopmentCaldus,
        resolution: CoreDurableCaldusResolution,
    ) -> Result<CorePrivateCaldusRewardCommit, CorePrivateCaldusRuntimeError> {
        if let Some(stored) = &self.reward_resolution {
            if stored != &resolution {
                return Err(CorePrivateCaldusRuntimeError::RewardResolutionConflict);
            }
            let route = self.route_directory.snapshot(self.route_lease)?;
            self.validate_route_authority(&route)?;
            if route.phase != CorePrivateRoutePhaseV1::BossExitReady {
                return Err(CorePrivateCaldusRuntimeError::RewardAuthorityMismatch);
            }
            let exit = self
                .presentation
                .as_ref()
                .and_then(CaldusInstancePresentation::exit)
                .cloned()
                .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
            return Ok(CorePrivateCaldusRewardCommit {
                route,
                exit,
                disposition: CorePrivateCaldusRewardCommitDisposition::Replayed,
            });
        }
        let handoff = self
            .defeat_handoff
            .as_ref()
            .ok_or(CorePrivateCaldusRuntimeError::RewardResolutionUnavailable)?;
        if resolution.handoff() != handoff {
            return Err(CorePrivateCaldusRuntimeError::RewardAuthorityMismatch);
        }
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        if route_before.phase != CorePrivateRoutePhaseV1::BossDefeated
            || route_before.state_version != handoff.route_state_version()
        {
            return Err(CorePrivateCaldusRuntimeError::RewardAuthorityMismatch);
        }
        let mut presentation = CaldusInstancePresentation::new(
            handoff.instance_lineage_id(),
            handoff.lock().attempt_ordinal,
        )?;
        presentation.present_committed_exit(content, resolution.exit())?;
        let route = self
            .route_directory
            .apply_fixed_dungeon_authority(
                self.route_lease,
                route_before.state_version,
                CorePrivateRouteRoomV1::CaldusArenaB6,
                CorePrivateRoutePhaseV1::BossExitReady,
            )
            .await?;
        let exit = presentation
            .exit()
            .cloned()
            .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
        self.route_phase = CorePrivateRoutePhaseV1::BossExitReady;
        self.reward_resolution = Some(resolution);
        self.defeat_handoff = None;
        self.presentation = Some(presentation);
        Ok(CorePrivateCaldusRewardCommit {
            route,
            exit,
            disposition: CorePrivateCaldusRewardCommitDisposition::Committed,
        })
    }

    pub(crate) fn terminal_heartbeat(
        &mut self,
    ) -> Result<CorePrivateCaldusTerminalHeartbeat, CorePrivateCaldusRuntimeError> {
        if self.reward_resolution.is_none()
            || self.route_phase != CorePrivateRoutePhaseV1::BossExitReady
        {
            return Err(CorePrivateCaldusRuntimeError::RewardResolutionUnavailable);
        }
        let route = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route)?;
        if route.phase != CorePrivateRoutePhaseV1::BossExitReady {
            return Err(CorePrivateCaldusRuntimeError::RouteAuthorityMismatch);
        }
        let tick = self
            .tick
            .checked_next()
            .ok_or(CorePrivateCaldusRuntimeError::TickExhausted)?;
        let player_position = simulation_to_tile_point(self.player().target.position)?;
        self.tick = tick;
        Ok(CorePrivateCaldusTerminalHeartbeat {
            tick,
            player_position,
            route,
        })
    }

    #[cfg(test)]
    async fn step_with_test_friendly_inputs(
        &mut self,
        input: CorePrivateCaldusRuntimeInput,
        friendly_inputs: Vec<CoreCaldusFriendlyInput>,
    ) -> Result<CorePrivateCaldusFrame, CorePrivateCaldusRuntimeError> {
        self.step_inner(input, Some(friendly_inputs)).await
    }

    async fn step_inner(
        &mut self,
        input: CorePrivateCaldusRuntimeInput,
        friendly_inputs: Option<Vec<CoreCaldusFriendlyInput>>,
    ) -> Result<CorePrivateCaldusFrame, CorePrivateCaldusRuntimeError> {
        if self.reward_resolution.is_some() {
            return Err(CorePrivateCaldusRuntimeError::ExitReady);
        }
        if self.defeat_handoff.is_some() {
            return Err(CorePrivateCaldusRuntimeError::RewardResolutionRequired);
        }
        let tick = self
            .tick
            .checked_next()
            .ok_or(CorePrivateCaldusRuntimeError::TickExhausted)?;
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        let staged = self.stage_frame(input, tick, friendly_inputs.as_deref())?;
        let player = staged
            .players
            .get(&self.participant.entity_id)
            .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
        let player_died = player.consumables.vitals().current_health() == 0;
        let player_damage = caldus_player_damage_facts(
            tick,
            staged.encounter.as_ref(),
            self.participant.entity_id,
            player_died,
        )?;
        let mut observation = project_caldus_observation(
            tick,
            &route_before,
            input.action.input_sequence,
            self.participant.entity_id,
            self.boss_entity_id,
            &staged,
        )?;
        let route = self
            .route_directory
            .apply_fixed_dungeon_authority(
                self.route_lease,
                route_before.state_version,
                CorePrivateRouteRoomV1::CaldusArenaB6,
                staged.route_phase,
            )
            .await?;
        observation.route_state_version = route.state_version;
        let player_position = simulation_to_tile_point(player.target.position)?;
        let boss_health = staged
            .encounter_simulation
            .as_ref()
            .map(|boss| (boss.current_health(), boss.maximum_health()));
        let defeat_handoff = if route.phase == CorePrivateRoutePhaseV1::BossDefeated {
            let encounter = staged
                .encounter_simulation
                .as_ref()
                .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
            let living = !player_died;
            let contribution = encounter
                .contribution_damage(self.participant)
                .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
            let (active_duration_ticks, eligibility) =
                staged
                    .reward_tracker
                    .finish(self.participant, tick, contribution, living)?;
            Some(CorePrivateCaldusDefeatHandoff {
                route_lease: self.route_lease,
                route_state_version: route.state_version,
                instance_lineage_id: route
                    .instance_lineage_id
                    .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?,
                entry_restore_point_id: self.entry_restore_point_id,
                lock: encounter.participant_lock().clone(),
                active_duration_ticks,
                defeat_tick: tick,
                character_id: self.combat_envelope.character_id(),
                expected_progression_version: self.combat_envelope.progression_version(),
                eligibility: vec![eligibility],
            })
        } else {
            None
        };

        self.lock = staged.lock_simulation;
        self.players = staged.players;
        self.movement = staged.movement_state;
        self.encounter = staged.encounter_simulation;
        self.projectile_ids = staged.projectile_ids;
        self.route_phase = staged.route_phase;
        self.tick = tick;
        self.reward_tracker = staged.reward_tracker;
        self.projectile_provenance = staged.projectile_provenance;
        self.defeat_handoff = defeat_handoff;
        Ok(CorePrivateCaldusFrame {
            input_sequence: input.action.input_sequence,
            tick,
            player_position,
            movement: staged.movement,
            combat: staged.combat,
            observation,
            lock: staged.lock,
            encounter: staged.encounter,
            route,
            boss_entity_id: self.boss_entity_id,
            boss_health,
            player_damage,
            player_died,
        })
    }

    fn stage_frame(
        &self,
        input: CorePrivateCaldusRuntimeInput,
        tick: Tick,
        friendly_inputs: Option<&[CoreCaldusFriendlyInput]>,
    ) -> Result<StagedCaldusFrame, CorePrivateCaldusRuntimeError> {
        let mut lock_simulation = self.lock.clone();
        let mut players = self.players.clone();
        let mut movement_state = self.movement;
        let mut encounter_simulation = self.encounter.clone();
        let mut projectile_ids = self.projectile_ids.clone();
        let mut reward_tracker = self.reward_tracker.clone();
        let mut projectile_provenance = self.projectile_provenance.clone();
        let life = participant_life(&players, self.participant.entity_id)?;
        let lock = lock_simulation.step(&CoreBossLockInput {
            tick,
            entrants: vec![CoreBossEntrantInput {
                participant: self.participant,
                connection: input.connection,
                life,
                inside_boundary: true,
            }],
        })?;
        start_encounter_if_due(
            &lock,
            &mut encounter_simulation,
            &mut projectile_ids,
            &self.arena,
            self.boss_entity_id,
            tick,
        )?;
        let hurtboxes = encounter_simulation
            .as_ref()
            .map(CoreCaldusEncounterSimulation::hurtbox)
            .transpose()?
            .into_iter()
            .flatten()
            .collect();
        let collision_world = ProjectileCollisionWorld::new(&self.arena, hurtboxes)?;
        let bodies = encounter_simulation
            .as_ref()
            .map(CoreCaldusEncounterSimulation::body_collider)
            .transpose()?
            .into_iter()
            .collect();
        let body_world = BodyCollisionWorld::new(&self.arena, bodies)?;
        let player = players
            .get_mut(&self.participant.entity_id)
            .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
        let (combat, mut movement) = step_live_player_combat_with_bodies(
            player,
            &mut movement_state,
            &input.action,
            &self.arena,
            &collision_world,
            &body_world,
        )?;
        if combat.tick != tick {
            return Err(CorePrivateCaldusRuntimeError::CombatTickMismatch);
        }
        let encounter = step_encounter_if_active(
            &lock,
            encounter_simulation.as_mut(),
            self.participant,
            &combat,
            friendly_inputs,
            &mut players,
        )?;
        observe_reward_evidence(
            &mut reward_tracker,
            tick,
            &input,
            &players,
            self.participant,
            &lock.phase,
        )?;
        let resolved_player_position = players
            .get(&self.participant.entity_id)
            .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?
            .target
            .position;
        if resolved_player_position != movement_state.position() {
            movement =
                movement_state.apply_body_separation(resolved_player_position, &self.arena)?;
        }
        recover_empty_reset(
            &lock,
            &mut encounter_simulation,
            &mut projectile_ids,
            &mut reward_tracker,
        );
        let player = players
            .get(&self.participant.entity_id)
            .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
        projectile_provenance.apply_committed_combat(&combat, player.combat.projectiles())?;
        let route_phase =
            projected_route_phase(&lock.phase, encounter_simulation.as_ref(), self.route_phase)?;
        Ok(StagedCaldusFrame {
            lock_simulation,
            players,
            movement_state,
            encounter_simulation,
            projectile_ids,
            route_phase,
            movement,
            combat,
            lock,
            encounter,
            reward_tracker,
            projectile_provenance,
        })
    }

    fn validate_route_authority(
        &self,
        route: &CorePrivateRouteStateV1,
    ) -> Result<(), CorePrivateCaldusRuntimeError> {
        if route.character_id != self.route_lease.character_id()
            || route.actor_generation != self.route_lease.actor_generation()
            || route.character_version != self.combat_envelope.character_state_version()
            || route.scene != CorePrivateRouteSceneV1::BellSepulcher
            || route.room != Some(CorePrivateRouteRoomV1::CaldusArenaB6)
            || route.phase != self.route_phase
        {
            return Err(CorePrivateCaldusRuntimeError::RouteAuthorityMismatch);
        }
        Ok(())
    }
}

fn project_caldus_observation(
    tick: Tick,
    route: &CorePrivateRouteStateV1,
    input_sequence: u64,
    player_id: EntityId,
    boss_entity_id: EntityId,
    staged: &StagedCaldusFrame,
) -> Result<CorePrivateGameplayObservation, CorePrivateCaldusRuntimeError> {
    let player = staged
        .players
        .get(&player_id)
        .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
    let mut entities = vec![player_snapshot(
        player,
        staged.movement.position,
        staged.movement.velocity,
    )?];
    for projectile in player.combat.projectiles() {
        entities.push(
            staged
                .projectile_provenance
                .friendly_snapshot(player_id, projectile)?,
        );
    }
    if let Some(encounter) = &staged.encounter_simulation {
        entities.push(boss_snapshot(
            boss_entity_id,
            encounter.body().simulation_position(),
            encounter.current_health(),
            encounter.maximum_health(),
            encounter.current_health() != 0,
        )?);
        for projectile in encounter.hostile_projectiles() {
            entities.push(hostile_projectile_snapshot(projectile)?);
        }
    }
    CorePrivateGameplayObservation::new(
        tick.0,
        route.actor_generation,
        route.state_version,
        input_sequence,
        entities,
    )
    .map_err(Into::into)
}

fn participant_life(
    players: &BTreeMap<EntityId, EnemyLabPlayer>,
    participant: EntityId,
) -> Result<CoreBossLifeState, CorePrivateCaldusRuntimeError> {
    let health = players
        .get(&participant)
        .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?
        .consumables
        .vitals()
        .current_health();
    Ok(if health == 0 {
        CoreBossLifeState::Dead
    } else {
        CoreBossLifeState::Living
    })
}

fn observe_reward_evidence(
    tracker: &mut CoreCaldusRewardTracker,
    tick: Tick,
    input: &CorePrivateCaldusRuntimeInput,
    players: &BTreeMap<EntityId, EnemyLabPlayer>,
    participant: CoreBossParticipant,
    lock: &CoreBossLockPhase,
) -> Result<(), CorePrivateCaldusRuntimeError> {
    let living = participant_life(players, participant.entity_id)? == CoreBossLifeState::Living;
    tracker.observe(
        tick,
        input,
        living,
        matches!(lock, CoreBossLockPhase::Combat { .. }),
    )?;
    Ok(())
}

fn recover_empty_reset(
    lock: &CoreBossLockStep,
    encounter: &mut Option<CoreCaldusEncounterSimulation>,
    projectile_ids: &mut Option<EntityIdAllocator>,
    reward_tracker: &mut CoreCaldusRewardTracker,
) {
    if lock
        .events
        .iter()
        .any(|event| matches!(event, CoreBossLockEvent::EmptyResetCompleted { .. }))
        && let Some(encounter) = encounter.take()
    {
        *projectile_ids = Some(encounter.into_cleared_projectile_allocator());
        reward_tracker.reset_for_attempt();
    }
}

fn start_encounter_if_due(
    lock_step: &CoreBossLockStep,
    encounter: &mut Option<CoreCaldusEncounterSimulation>,
    projectile_ids: &mut Option<EntityIdAllocator>,
    arena: &sim_core::ArenaGeometry,
    boss_entity_id: EntityId,
    tick: Tick,
) -> Result<(), CorePrivateCaldusRuntimeError> {
    if !lock_step
        .events
        .iter()
        .any(|event| matches!(event, CoreBossLockEvent::CombatStarted { .. }))
    {
        return Ok(());
    }
    let CoreBossLockPhase::Combat { lock } = &lock_step.phase else {
        return Err(CorePrivateCaldusRuntimeError::InvalidComposition);
    };
    let allocator = projectile_ids
        .take()
        .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
    *encounter = Some(CoreCaldusEncounterSimulation::new_at_tick(
        lock.clone(),
        arena.clone(),
        boss_entity_id,
        allocator,
        tick,
    )?);
    Ok(())
}

fn step_encounter_if_active(
    lock: &CoreBossLockStep,
    encounter: Option<&mut CoreCaldusEncounterSimulation>,
    participant: CoreBossParticipant,
    combat: &sim_core::CombatStep,
    friendly_inputs: Option<&[CoreCaldusFriendlyInput]>,
    players: &mut BTreeMap<EntityId, EnemyLabPlayer>,
) -> Result<Option<CoreCaldusEncounterStep>, CorePrivateCaldusRuntimeError> {
    if !matches!(lock.phase, CoreBossLockPhase::Combat { .. }) {
        return Ok(None);
    }
    let generated;
    let friendly_inputs = if let Some(inputs) = friendly_inputs {
        inputs
    } else {
        generated = [CoreCaldusFriendlyInput {
            participant,
            combat: combat.clone(),
        }];
        &generated
    };
    Ok(Some(
        encounter
            .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?
            .step(friendly_inputs, players)?,
    ))
}

fn validate_staging_route(
    route: &CorePrivateRouteStateV1,
    handoff: &CorePrivateCaldusStagingHandoff,
) -> Result<(), CorePrivateCaldusRuntimeError> {
    if route.character_id != handoff.combat_envelope.character_id()
        || route.character_version != handoff.combat_envelope.character_state_version()
        || route.actor_generation != handoff.route_lease.actor_generation()
        || route.content_revision != handoff.content_revision
        || route.scene != CorePrivateRouteSceneV1::BellSepulcher
        || route.room != Some(CorePrivateRouteRoomV1::CaldusArenaB6)
        || route.phase != CorePrivateRoutePhaseV1::BossStaging
        || route.instance_lineage_id.is_none()
    {
        return Err(CorePrivateCaldusRuntimeError::InvalidComposition);
    }
    Ok(())
}

fn projected_route_phase(
    lock: &CoreBossLockPhase,
    encounter: Option<&CoreCaldusEncounterSimulation>,
    previous: CorePrivateRoutePhaseV1,
) -> Result<CorePrivateRoutePhaseV1, CorePrivateCaldusRuntimeError> {
    let phase = match lock {
        CoreBossLockPhase::BossWarning | CoreBossLockPhase::Loading { .. } => {
            CorePrivateRoutePhaseV1::BossStaging
        }
        CoreBossLockPhase::ReadyCountdown { .. } => CorePrivateRoutePhaseV1::BossReadyCountdown,
        CoreBossLockPhase::Introduction { .. } => CorePrivateRoutePhaseV1::BossIntroduction,
        CoreBossLockPhase::ResetPending { .. } => previous,
        CoreBossLockPhase::Combat { .. } => match encounter
            .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?
            .state()
        {
            CoreCaldusState::Active {
                phase: CoreCaldusPhase::Phase1,
                ..
            } => CorePrivateRoutePhaseV1::BossPhaseOne,
            CoreCaldusState::Break {
                entering: CoreCaldusPhase::Phase2,
                ..
            } => CorePrivateRoutePhaseV1::BossBreakToTwo,
            CoreCaldusState::Active {
                phase: CoreCaldusPhase::Phase2,
                ..
            } => CorePrivateRoutePhaseV1::BossPhaseTwo,
            CoreCaldusState::Break {
                entering: CoreCaldusPhase::Phase3,
                ..
            } => CorePrivateRoutePhaseV1::BossBreakToThree,
            CoreCaldusState::Active {
                phase: CoreCaldusPhase::Phase3,
                ..
            } => CorePrivateRoutePhaseV1::BossPhaseThree,
            CoreCaldusState::Break {
                entering: CoreCaldusPhase::Phase1,
                ..
            } => return Err(CorePrivateCaldusRuntimeError::InvalidComposition),
            CoreCaldusState::Defeated => CorePrivateRoutePhaseV1::BossDefeated,
        },
    };
    Ok(phase)
}

#[cfg(test)]
pub(crate) fn core_private_caldus_runtime_test_fixture()
-> (CorePrivateRouteActorDirectory, CorePrivateCaldusRuntime) {
    use std::{num::NonZeroU64, path::Path};

    use protocol::{CorePrivateRouteContentRevisionV1, ManifestHash, WorldFlowContentRevisionV1};

    let hash = |byte: char| ManifestHash::new(byte.to_string().repeat(64)).expect("hash");
    let route_revision = CorePrivateRouteContentRevisionV1 {
        records_blake3: hash('a'),
        assets_blake3: hash('b'),
        localization_blake3: hash('c'),
    };
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(
            crate::AuthenticatedAccount {
                account_id: crate::AccountId::new([0x71; 16]).expect("account"),
                namespace: crate::AuthenticatedNamespace::WipeableTest,
            },
            crate::CorePrivateRouteActorSeed {
                character_id: [0x72; 16],
                character_version: 2,
                content_revision: route_revision.clone(),
                world_flow_revision: WorldFlowContentRevisionV1 {
                    records_blake3: hash('d'),
                    assets_blake3: hash('e'),
                    localization_blake3: hash('f'),
                },
                position: crate::CorePrivateRouteActorPosition {
                    instance_lineage_id: Some([0x73; 16]),
                    scene: CorePrivateRouteSceneV1::BellSepulcher,
                    room: Some(CorePrivateRouteRoomV1::CaldusArenaB6),
                    phase: CorePrivateRoutePhaseV1::BossStaging,
                },
            },
            7,
        )
        .expect("route actor");
    let combat = crate::combat_factory::core_character_combat_test_fixture([0x72; 16]);
    let player_id = EntityId::new(710_000).expect("player");
    let (envelope, player) = combat
        .into_live_player(player_id, sim_core::SimulationVector::new(1.0, 1.0))
        .expect("live player");
    let content_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let encounters =
        sim_content::load_core_development_encounter_rooms(&content_root).expect("encounters");
    let handoff = CorePrivateCaldusStagingHandoff {
        route_directory: directory.clone(),
        route_lease: lease,
        content_revision: route_revision,
        entry_restore_point_id: [0x74; 16],
        combat_envelope: envelope,
        participant: sim_core::NormalWaveHandoff {
            player,
            hostile_projectile_ids: EntityIdAllocator::starting_at(
                NonZeroU64::new(900_000).expect("allocator"),
            ),
        },
        arena: encounters.compile_caldus_arena().expect("B6 arena"),
        tick: Tick(0),
        last_reward_activity_sequence: 1,
        projectile_provenance: CorePrivateProjectileProvenance::default(),
    };
    let runtime = CorePrivateCaldusRuntime::from_staging_handoff(handoff).expect("Caldus runtime");
    (directory, runtime)
}

#[derive(Debug, Error)]
pub enum CorePrivateCaldusRuntimeError {
    #[error("route-bound Sir Caldus composition is invalid")]
    InvalidComposition,
    #[error("route-bound Sir Caldus authority no longer matches local state")]
    RouteAuthorityMismatch,
    #[error("route-bound Sir Caldus tick exhausted")]
    TickExhausted,
    #[error("route-bound player combat tick diverged from the Caldus danger tick")]
    CombatTickMismatch,
    #[error("Sir Caldus is defeated and its frozen reward result must be resolved")]
    RewardResolutionRequired,
    #[error("the stable Sir Caldus exit is already ready")]
    ExitReady,
    #[error("no frozen Sir Caldus defeat is available for reward resolution")]
    RewardResolutionUnavailable,
    #[error("a different durable Sir Caldus reward result was already accepted")]
    RewardResolutionConflict,
    #[error("the durable Sir Caldus reward result does not match live route authority")]
    RewardAuthorityMismatch,
    #[error(transparent)]
    Movement(#[from] sim_core::MovementError),
    #[error(transparent)]
    Collision(#[from] sim_core::CollisionError),
    #[error(transparent)]
    Microrealm(#[from] CorePrivateMicrorealmRuntimeError),
    #[error(transparent)]
    BossLock(#[from] sim_core::CoreBossLockError),
    #[error(transparent)]
    Encounter(#[from] sim_core::CoreCaldusEncounterError),
    #[error(transparent)]
    Reward(#[from] CorePrivateCaldusRewardError),
    #[error(transparent)]
    Presentation(#[from] crate::CaldusInstancePresentationError),
    #[error(transparent)]
    Route(#[from] CorePrivateRouteRuntimeError),
    #[error(transparent)]
    PlayerDamage(#[from] CorePrivatePlayerDamageError),
    #[error(transparent)]
    GameplayObservation(#[from] CorePrivateGameplayObservationError),
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, path::Path};

    use persistence::{StoredCaldusVictoryExit, StoredCaldusVictoryOwner};
    use protocol::{CorePrivateRouteContentRevisionV1, ManifestHash, WorldFlowContentRevisionV1};
    use sim_core::{
        AimDirection, CollisionTarget, CombatStep, CoreCaldusVictoryIdentities,
        FriendlyProjectileSource, MovementAction, ProjectileCollision, RawDamageIntent,
        RawDamageIntentSource, SimulationVector,
    };

    use super::*;
    use crate::{
        AccountId, AuthenticatedAccount, AuthenticatedNamespace, CorePrivateRouteActorPosition,
        CorePrivateRouteActorSeed,
    };

    const ACCOUNT_ID: [u8; 16] = [0x51; 16];
    const CHARACTER_ID: [u8; 16] = [0x52; 16];
    const LINEAGE_ID: [u8; 16] = [0x53; 16];

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).expect("hash")
    }

    fn route_revision() -> CorePrivateRouteContentRevisionV1 {
        CorePrivateRouteContentRevisionV1 {
            records_blake3: hash('a'),
            assets_blake3: hash('b'),
            localization_blake3: hash('c'),
        }
    }

    fn world_revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: hash('d'),
            assets_blake3: hash('e'),
            localization_blake3: hash('f'),
        }
    }

    fn fixture() -> (
        CorePrivateRouteActorDirectory,
        CorePrivateRouteActorLease,
        CorePrivateCaldusRuntime,
    ) {
        let directory = CorePrivateRouteActorDirectory::new();
        let authenticated = AuthenticatedAccount {
            account_id: AccountId::new(ACCOUNT_ID).expect("account"),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        let lease = directory
            .register_actor(
                authenticated,
                CorePrivateRouteActorSeed {
                    character_id: CHARACTER_ID,
                    character_version: 2,
                    content_revision: route_revision(),
                    world_flow_revision: world_revision(),
                    position: CorePrivateRouteActorPosition {
                        instance_lineage_id: Some(LINEAGE_ID),
                        scene: CorePrivateRouteSceneV1::BellSepulcher,
                        room: Some(CorePrivateRouteRoomV1::CaldusArenaB6),
                        phase: CorePrivateRoutePhaseV1::BossStaging,
                    },
                },
                7,
            )
            .expect("route actor");
        let combat = crate::combat_factory::core_character_combat_test_fixture(CHARACTER_ID);
        let player_id = EntityId::new(710_000).expect("player");
        let (envelope, player) = combat
            .into_live_player(player_id, SimulationVector::new(1.0, 1.0))
            .expect("live player");
        let content_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let encounters =
            sim_content::load_core_development_encounter_rooms(&content_root).expect("encounters");
        let handoff = CorePrivateCaldusStagingHandoff {
            route_directory: directory.clone(),
            route_lease: lease,
            content_revision: route_revision(),
            entry_restore_point_id: [0x54; 16],
            combat_envelope: envelope,
            participant: sim_core::NormalWaveHandoff {
                player,
                hostile_projectile_ids: EntityIdAllocator::starting_at(
                    NonZeroU64::new(900_000).expect("allocator"),
                ),
            },
            arena: encounters.compile_caldus_arena().expect("B6 arena"),
            tick: Tick(0),
            last_reward_activity_sequence: 0,
            projectile_provenance: CorePrivateProjectileProvenance::default(),
        };
        let runtime =
            CorePrivateCaldusRuntime::from_staging_handoff(handoff).expect("Caldus runtime");
        (directory, lease, runtime)
    }

    fn input(sequence: u64) -> CorePrivateCaldusRuntimeInput {
        CorePrivateCaldusRuntimeInput {
            action: CorePrivateMicrorealmInput {
                input_sequence: sequence,
                movement: MovementAction::default(),
                aim: AimDirection::east(),
                primary_held: false,
                primary_sequence: 0,
                ability_1_sequence: 0,
                ability_2_sequence: 0,
                reward_session_active: true,
                reward_trust_valid: true,
                reward_activity_sequence: sequence,
            },
            connection: CoreBossConnectionState::ConnectedLoaded,
        }
    }

    fn scripted_damage(
        runtime: &CorePrivateCaldusRuntime,
        tick: Tick,
        raw_damage: u32,
    ) -> Vec<CoreCaldusFriendlyInput> {
        let projectile_id = EntityId::new(960_000 + tick.0).expect("script projectile");
        let boss_position = runtime
            .encounter
            .as_ref()
            .expect("active encounter")
            .body()
            .simulation_position();
        vec![CoreCaldusFriendlyInput {
            participant: runtime.participant,
            combat: CombatStep {
                tick,
                collisions: vec![ProjectileCollision {
                    tick,
                    projectile_id,
                    source: FriendlyProjectileSource::Primary,
                    target: CollisionTarget::Enemy(runtime.boss_entity_id),
                    final_position: boss_position,
                    distance_travelled_tiles: 1.0,
                    contact_ordinal: 0,
                    empowered_by_slipstep: false,
                    focused_by_stillness: false,
                    projectile_continues: false,
                }],
                raw_damage_intents: vec![RawDamageIntent {
                    tick,
                    projectile_id,
                    source: RawDamageIntentSource::Primary,
                    target: runtime.boss_entity_id,
                    base_raw_damage: raw_damage,
                    multiplier_basis_points: 10_000,
                    resolved_raw_damage: raw_damage,
                    contact_ordinal: 0,
                }],
                ..CombatStep::default()
            },
        }]
    }

    fn relocate_for_scripted_charge(runtime: &mut CorePrivateCaldusRuntime) {
        let charge_target = SimulationVector::new(14.0, 9.0);
        runtime
            .players
            .get_mut(&runtime.participant.entity_id)
            .expect("player")
            .target
            .position = charge_target;
        runtime.movement = PlayerMovementState::new_with_config(
            charge_target,
            runtime.movement.config(),
            &runtime.arena,
        )
        .expect("phase-two charge target");
    }

    fn scripted_fight_damage(
        runtime: &CorePrivateCaldusRuntime,
        sequence: u64,
    ) -> Vec<CoreCaldusFriendlyInput> {
        match sequence {
            300 => scripted_damage(runtime, Tick(sequence), 2_500),
            550 => scripted_damage(runtime, Tick(sequence), 2_400),
            950 => scripted_damage(runtime, Tick(sequence), 3_000),
            _ => Vec::new(),
        }
    }

    async fn drive_to_defeat() -> (
        CorePrivateRouteActorDirectory,
        CorePrivateRouteActorLease,
        CorePrivateCaldusRuntime,
    ) {
        let (directory, lease, mut runtime) = fixture();
        for sequence in 1..=226 {
            runtime.step(input(sequence)).await.expect("boss entry");
        }
        runtime
            .encounter
            .as_mut()
            .expect("active encounter")
            .set_damage_policy(sim_core::HostileDamagePolicy::DebugInvulnerable);
        for sequence in 227..=950 {
            if sequence == 420 {
                relocate_for_scripted_charge(&mut runtime);
            }
            let scripted = scripted_fight_damage(&runtime, sequence);
            let frame = runtime
                .step_with_test_friendly_inputs(input(sequence), scripted)
                .await
                .unwrap_or_else(|error| panic!("defeat tick {sequence}: {error}"));
            if frame.route.phase == CorePrivateRoutePhaseV1::BossDefeated {
                return (directory, lease, runtime);
            }
        }
        panic!("scripted fight did not defeat Sir Caldus");
    }

    fn stored_exit(
        handoff: &CorePrivateCaldusDefeatHandoff,
        replayed: bool,
        request_hash_byte: u8,
    ) -> StoredCaldusVictoryExit {
        let identities =
            CoreCaldusVictoryIdentities::derive(handoff.instance_lineage_id(), handoff.lock())
                .expect("victory identities");
        let participant = handoff.lock().participants[0];
        StoredCaldusVictoryExit {
            replayed,
            encounter_id: identities.encounter_id.bytes(),
            instance_lineage_id: handoff.instance_lineage_id(),
            attempt_ordinal: handoff.lock().attempt_ordinal,
            exit_instance_id: identities.exit_instance_id.bytes(),
            canonical_request_hash: [request_hash_byte; 32],
            owners: vec![StoredCaldusVictoryOwner {
                party_slot: participant.party_slot,
                participant_entity_id: participant.entity_id.get(),
                account_id: handoff.route_lease().account_id(),
                character_id: handoff.character_id(),
                reward_request_id: identities
                    .reward_for(participant)
                    .expect("participant reward identity")
                    .bytes(),
                reward_result_hash: [0x91; 32],
                progression_payload_hash: [0x92; 32],
            }],
        }
    }

    async fn scripted_full_fight_trace()
    -> (blake3::Hash, Vec<CorePrivateRoutePhaseV1>, bool, bool, u32) {
        let (directory, _, mut runtime) = fixture();
        let mut trace = blake3::Hasher::new();
        let mut phases = Vec::new();
        for sequence in 1..=226 {
            let frame = runtime.step(input(sequence)).await.expect("boss entry");
            if phases.last() != Some(&frame.route.phase) {
                phases.push(frame.route.phase);
            }
        }
        runtime
            .encounter
            .as_mut()
            .expect("active encounter")
            .set_damage_policy(sim_core::HostileDamagePolicy::DebugInvulnerable);
        let mut separated = false;
        let mut charged = false;
        let mut minimum_charge_distance_milli_tiles = u32::MAX;
        let mut defeated = false;
        for sequence in 227..=950 {
            if sequence == 420 {
                relocate_for_scripted_charge(&mut runtime);
            }
            let scripted = scripted_fight_damage(&runtime, sequence);
            let frame = runtime
                .step_with_test_friendly_inputs(input(sequence), scripted)
                .await
                .unwrap_or_else(|error| panic!("full-fight tick {sequence}: {error}"));
            if phases.last() != Some(&frame.route.phase) {
                phases.push(frame.route.phase);
            }
            if let Some(encounter) = &frame.encounter {
                separated |= !encounter.player_separations.is_empty();
                let moved = encounter.body_events.iter().any(|event| {
                    matches!(event, sim_core::CoreCaldusBodyEvent::ChargeMoved { .. })
                });
                charged |= moved;
                if moved {
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "arena distances are finite and tightly bounded"
                    )]
                    let distance = ((runtime.player().target.position
                        - runtime
                            .encounter
                            .as_ref()
                            .expect("encounter")
                            .body()
                            .simulation_position())
                    .length()
                        * 1_000.0)
                        .round() as u32;
                    minimum_charge_distance_milli_tiles =
                        minimum_charge_distance_milli_tiles.min(distance);
                }
                trace.update(format!("{encounter:?}\n").as_bytes());
            }
            trace.update(
                format!(
                    "{:?}|{:?}|{:?}|{:?}\n",
                    frame.tick, frame.route.phase, frame.boss_health, frame.player_position
                )
                .as_bytes(),
            );
            if frame.route.phase == CorePrivateRoutePhaseV1::BossDefeated {
                assert_eq!(frame.boss_health, Some((0, 7_200)));
                assert!(
                    runtime
                        .encounter
                        .as_ref()
                        .expect("defeated encounter")
                        .hostile_projectiles()
                        .is_empty()
                );
                defeated = true;
                break;
            }
        }
        assert!(defeated);
        drop(runtime);
        directory.begin_shutdown();
        assert!(
            directory
                .finish_shutdown()
                .await
                .expect("shutdown")
                .zero_residue
        );
        (
            trace.finalize(),
            phases,
            separated,
            charged,
            minimum_charge_distance_milli_tiles,
        )
    }

    #[tokio::test]
    async fn inherited_tick_runs_exact_countdown_intro_and_same_tick_phase_one() {
        let (directory, _, mut runtime) = fixture();
        assert_eq!(
            runtime.player().target.position,
            tile_point_to_simulation(runtime.arena().player_spawn)
        );
        let countdown = runtime.step(input(1)).await.expect("countdown");
        assert_eq!(countdown.tick, Tick(1));
        assert_eq!(countdown.combat.tick, Tick(1));
        assert_eq!(
            countdown.route.phase,
            CorePrivateRoutePhaseV1::BossReadyCountdown
        );
        for sequence in 2..=150 {
            let frame = runtime
                .step(input(sequence))
                .await
                .expect("countdown frame");
            assert_eq!(
                frame.route.phase,
                CorePrivateRoutePhaseV1::BossReadyCountdown
            );
        }
        let introduction = runtime.step(input(151)).await.expect("introduction");
        assert_eq!(
            introduction.route.phase,
            CorePrivateRoutePhaseV1::BossIntroduction
        );
        for sequence in 152..=225 {
            let frame = runtime.step(input(sequence)).await.expect("intro frame");
            assert_eq!(frame.route.phase, CorePrivateRoutePhaseV1::BossIntroduction);
        }
        let combat = runtime.step(input(226)).await.expect("combat");
        assert_eq!(combat.tick, Tick(226));
        assert_eq!(combat.combat.tick, Tick(226));
        assert_eq!(
            combat.encounter.as_ref().expect("encounter").tick,
            Tick(226)
        );
        assert_eq!(combat.route.phase, CorePrivateRoutePhaseV1::BossPhaseOne);
        assert_eq!(combat.boss_health, Some((7_200, 7_200)));

        let near_body = SimulationVector::new(7.8, 9.0);
        runtime
            .players
            .get_mut(&runtime.participant.entity_id)
            .expect("player")
            .target
            .position = near_body;
        runtime.movement = PlayerMovementState::new_with_config(
            near_body,
            runtime.movement.config(),
            &runtime.arena,
        )
        .expect("near-body movement");
        for sequence in 227..=230 {
            let mut body_input = input(sequence);
            body_input.action.movement = MovementAction::new(1, 0);
            runtime
                .step(body_input)
                .await
                .expect("body collision frame");
        }
        assert!((runtime.player().target.position.x - 8.0).abs() < 1.0e-5);

        drop(runtime);
        directory.begin_shutdown();
        assert!(
            directory
                .finish_shutdown()
                .await
                .expect("shutdown")
                .zero_residue
        );
    }

    #[tokio::test]
    async fn complete_route_bound_fight_is_byte_identical_through_defeat() {
        let first = scripted_full_fight_trace().await;
        let second = scripted_full_fight_trace().await;
        assert_eq!(first, second);
        assert!(
            first.2,
            "charge moved: {}, minimum distance: {}",
            first.3, first.4
        );
        assert_eq!(first.4, 1_000);
        assert_eq!(
            first.1,
            [
                CorePrivateRoutePhaseV1::BossReadyCountdown,
                CorePrivateRoutePhaseV1::BossIntroduction,
                CorePrivateRoutePhaseV1::BossPhaseOne,
                CorePrivateRoutePhaseV1::BossBreakToTwo,
                CorePrivateRoutePhaseV1::BossPhaseTwo,
                CorePrivateRoutePhaseV1::BossBreakToThree,
                CorePrivateRoutePhaseV1::BossPhaseThree,
                CorePrivateRoutePhaseV1::BossDefeated,
            ]
        );
    }

    #[tokio::test]
    async fn defeat_freezes_evidence_and_only_exact_durable_result_unlocks_exit() {
        let (directory, lease, mut runtime) = drive_to_defeat().await;
        let frozen = runtime
            .pending_reward_handoff()
            .expect("frozen defeat")
            .clone();
        let frozen_route = directory.snapshot(lease).expect("defeated route");
        assert_eq!(frozen.route_state_version(), frozen_route.state_version);
        assert_eq!(frozen.entry_restore_point_id(), [0x54; 16]);
        assert_eq!(frozen.defeat_tick(), Tick(950));
        assert_eq!(frozen.active_duration_ticks(), 725);
        assert_eq!(frozen.lock().maximum_health, 7_200);
        assert_eq!(frozen.eligibility().len(), 1);
        assert_eq!(frozen.eligibility()[0].presence_ticks, 725);
        assert_eq!(frozen.eligibility()[0].direct_damage, 7_200);

        assert!(matches!(
            runtime.step(input(951)).await,
            Err(CorePrivateCaldusRuntimeError::RewardResolutionRequired)
        ));
        assert_eq!(runtime.tick(), Tick(950));
        assert_eq!(
            directory.snapshot(lease).expect("still defeated"),
            frozen_route
        );

        let fresh = CoreDurableCaldusResolution::from_stored_for_test(
            frozen.clone(),
            stored_exit(&frozen, false, 0xa1),
        )
        .expect("fresh durable result");
        let replayed = CoreDurableCaldusResolution::from_stored_for_test(
            frozen.clone(),
            stored_exit(&frozen, true, 0xa1),
        )
        .expect("replayed durable result");
        assert_eq!(fresh, replayed);
        let changed = CoreDurableCaldusResolution::from_stored_for_test(
            frozen.clone(),
            stored_exit(&frozen, false, 0xa2),
        )
        .expect("changed durable material");
        let content_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let content = sim_content::load_core_development_caldus(&content_root).expect("content");

        let committed = runtime
            .commit_reward_resolution(&content, fresh)
            .await
            .expect("commit reward result");
        assert_eq!(
            committed.disposition,
            CorePrivateCaldusRewardCommitDisposition::Committed
        );
        assert_eq!(
            committed.route.phase,
            CorePrivateRoutePhaseV1::BossExitReady
        );
        assert_eq!(
            committed.route.state_version,
            frozen_route.state_version + 1
        );
        assert_eq!(
            runtime
                .presentation()
                .and_then(CaldusInstancePresentation::exit),
            Some(&committed.exit)
        );
        assert!(runtime.pending_reward_handoff().is_none());

        let replay = runtime
            .commit_reward_resolution(&content, replayed)
            .await
            .expect("exact replay");
        assert_eq!(
            replay.disposition,
            CorePrivateCaldusRewardCommitDisposition::Replayed
        );
        assert_eq!(replay.route.state_version, committed.route.state_version);
        assert_eq!(replay.exit, committed.exit);
        assert!(matches!(
            runtime.commit_reward_resolution(&content, changed).await,
            Err(CorePrivateCaldusRuntimeError::RewardResolutionConflict)
        ));
        assert!(matches!(
            runtime.step(input(951)).await,
            Err(CorePrivateCaldusRuntimeError::ExitReady)
        ));
        let first_heartbeat = runtime
            .terminal_heartbeat()
            .expect("first exit-ready heartbeat");
        let second_heartbeat = runtime
            .terminal_heartbeat()
            .expect("second exit-ready heartbeat");
        assert_eq!(first_heartbeat.tick, Tick(951));
        assert_eq!(second_heartbeat.tick, Tick(952));
        assert_eq!(first_heartbeat.route, committed.route);
        assert_eq!(second_heartbeat.route, committed.route);
        assert_eq!(
            first_heartbeat.player_position,
            second_heartbeat.player_position
        );

        drop(runtime);
        directory.begin_shutdown();
        assert!(
            directory
                .finish_shutdown()
                .await
                .expect("shutdown")
                .zero_residue
        );
    }

    #[tokio::test]
    async fn foreign_route_advance_rejects_frame_without_advancing_local_tick() {
        let (directory, lease, mut runtime) = fixture();
        let route = directory.snapshot(lease).expect("route");
        directory
            .apply_fixed_dungeon_authority(
                lease,
                route.state_version,
                CorePrivateRouteRoomV1::CaldusArenaB6,
                CorePrivateRoutePhaseV1::BossReadyCountdown,
            )
            .await
            .expect("foreign advance");
        assert!(matches!(
            runtime.step(input(1)).await,
            Err(CorePrivateCaldusRuntimeError::RouteAuthorityMismatch)
        ));
        assert_eq!(runtime.tick(), Tick(0));
        assert_eq!(runtime.player().combat.tick(), Tick(0));

        drop(runtime);
        directory.begin_shutdown();
        assert!(
            directory
                .finish_shutdown()
                .await
                .expect("shutdown")
                .zero_residue
        );
    }
}
