//! Route-bound owner for the Core Sir Caldus B6 lifecycle.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`SIM-004`,
//! `DNG-006`, `ENC-010`, `TECH-012`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-ROOM-002`, `CONT-BOSS-001`-`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`). Loading, lock, player simulation,
//! encounter simulation, and route CAS are staged and committed as one frame. Reward and exit
//! authority remain outside this owner and normal route admission remains disabled.

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
    CoreCharacterCombatEnvelope, CorePrivateCaldusStagingHandoff, CorePrivateMicrorealmInput,
    CorePrivateMicrorealmRuntimeError, CorePrivateRouteActorDirectory, CorePrivateRouteActorLease,
    CorePrivateRouteRuntimeError,
    core_private_combat_frame::{core_player_movement_config, step_live_player_combat_with_bodies},
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
    pub lock: CoreBossLockStep,
    pub encounter: Option<CoreCaldusEncounterStep>,
    pub route: CorePrivateRouteStateV1,
    pub boss_entity_id: EntityId,
    pub boss_health: Option<(u32, u32)>,
    pub player_died: bool,
}

#[derive(Debug)]
pub struct CorePrivateCaldusRuntime {
    route_directory: CorePrivateRouteActorDirectory,
    route_lease: CorePrivateRouteActorLease,
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

    pub async fn step(
        &mut self,
        input: CorePrivateCaldusRuntimeInput,
    ) -> Result<CorePrivateCaldusFrame, CorePrivateCaldusRuntimeError> {
        let tick = self
            .tick
            .checked_next()
            .ok_or(CorePrivateCaldusRuntimeError::TickExhausted)?;
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        let staged = self.stage_frame(input, tick)?;
        let route = self
            .route_directory
            .apply_fixed_dungeon_authority(
                self.route_lease,
                route_before.state_version,
                CorePrivateRouteRoomV1::CaldusArenaB6,
                staged.route_phase,
            )
            .await?;
        let player = staged
            .players
            .get(&self.participant.entity_id)
            .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?;
        let player_position = simulation_to_tile_point(player.target.position)?;
        let player_died = player.consumables.vitals().current_health() == 0;
        let boss_health = staged
            .encounter_simulation
            .as_ref()
            .map(|boss| (boss.current_health(), boss.maximum_health()));

        self.lock = staged.lock_simulation;
        self.players = staged.players;
        self.movement = staged.movement_state;
        self.encounter = staged.encounter_simulation;
        self.projectile_ids = staged.projectile_ids;
        self.route_phase = staged.route_phase;
        self.tick = tick;
        Ok(CorePrivateCaldusFrame {
            input_sequence: input.action.input_sequence,
            tick,
            player_position,
            movement: staged.movement,
            combat: staged.combat,
            lock: staged.lock,
            encounter: staged.encounter,
            route,
            boss_entity_id: self.boss_entity_id,
            boss_health,
            player_died,
        })
    }

    fn stage_frame(
        &self,
        input: CorePrivateCaldusRuntimeInput,
        tick: Tick,
    ) -> Result<StagedCaldusFrame, CorePrivateCaldusRuntimeError> {
        let mut lock_simulation = self.lock.clone();
        let mut players = self.players.clone();
        let mut movement_state = self.movement;
        let mut encounter_simulation = self.encounter.clone();
        let mut projectile_ids = self.projectile_ids.clone();
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
            &mut players,
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
        if lock
            .events
            .iter()
            .any(|event| matches!(event, CoreBossLockEvent::EmptyResetCompleted { .. }))
            && let Some(encounter) = encounter_simulation.take()
        {
            projectile_ids = Some(encounter.into_cleared_projectile_allocator());
        }
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
    players: &mut BTreeMap<EntityId, EnemyLabPlayer>,
) -> Result<Option<CoreCaldusEncounterStep>, CorePrivateCaldusRuntimeError> {
    if !matches!(lock.phase, CoreBossLockPhase::Combat { .. }) {
        return Ok(None);
    }
    Ok(Some(
        encounter
            .ok_or(CorePrivateCaldusRuntimeError::InvalidComposition)?
            .step(
                &[CoreCaldusFriendlyInput {
                    participant,
                    combat: combat.clone(),
                }],
                players,
            )?,
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
    Route(#[from] CorePrivateRouteRuntimeError),
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, path::Path};

    use protocol::{CorePrivateRouteContentRevisionV1, ManifestHash, WorldFlowContentRevisionV1};
    use sim_core::{AimDirection, MovementAction, SimulationVector};

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
            combat_envelope: envelope,
            participant: sim_core::NormalWaveHandoff {
                player,
                hostile_projectile_ids: EntityIdAllocator::starting_at(
                    NonZeroU64::new(900_000).expect("allocator"),
                ),
            },
            arena: encounters.compile_caldus_arena().expect("B6 arena"),
            tick: Tick(0),
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
