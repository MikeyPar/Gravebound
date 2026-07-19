//! Live capacity-one movement and lifecycle owner for the ordinary Core microrealm.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`-`003`,
//! `TECH-010`-`023`), `Gravebound_Content_Production_Spec_v1.md` (`CONT-WORLD-001` and
//! `CONT-WORLD-004`), and `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`). This owner keeps
//! client action state below server-owned ticks, displacement, combat, collision, pack-clear,
//! phase, and Bell-range authority. Its existence does not enable normal route admission.

use protocol::{
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteSceneV1,
    CorePrivateRouteStateV1,
};
use sim_core::{
    AimDirection, ArenaGeometry, CombatError, CombatStep, ConsumableError, CoreMicrorealmEvent,
    CoreMicrorealmInput, CoreMicrorealmPhase, CoreMicrorealmSimulation, FriendlyProjectileSource,
    MovementAction, MovementError, MovementStep, PlayerMovementState, ProjectileCollisionWorld,
    SceneObjectGeometry, Tick, TilePoint, WorldSceneDefinition, WorldSceneKind,
    normal_wave_projectile_allocator, simulation_to_tile_point, tile_point_to_simulation,
};
use thiserror::Error;

use crate::core_private_combat_frame::{core_player_movement_config, step_live_player_combat};
use crate::{
    CoreBellPortalTransition, CoreCharacterCombat, CoreCharacterCombatEnvelope,
    CoreCombatFactoryError, CorePrivatePlayerDamageError, CorePrivatePlayerDamageFactV1,
    CorePrivateRouteActorDirectory, CorePrivateRouteActorLease, CorePrivateRouteRuntimeError,
    normal_wave_player_damage_facts,
};

#[derive(Debug)]
pub(crate) struct CorePrivateMicrorealmDungeonHandoff {
    pub route_directory: CorePrivateRouteActorDirectory,
    pub route_lease: CorePrivateRouteActorLease,
    pub entry_restore_point_id: [u8; 16],
    pub combat_envelope: CoreCharacterCombatEnvelope,
    pub participant: sim_core::NormalWaveHandoff,
    pub next_hostile_spawn_ordinal: u16,
    pub final_tick: Tick,
}

const CORE_MICROREALM_SCENE_ID: &str = "world.core_microrealm_01";
const BELL_PORTAL_OBJECT_ID: &str = "portal.dungeon.bell_sepulcher";
const RUN_ENTITY_ID_STRIDE: u64 = 100_000;
const PLAYER_ENTITY_ID_OFFSET: u64 = 10_000;

/// Opaque server-owned proof that the live combat owner cleared the microrealm pack on this tick.
/// The ordinary input decoder cannot construct this value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoreMicrorealmPackClearProof {
    character_id: [u8; 16],
    actor_generation: u64,
    instance_lineage_id: [u8; 16],
    tick: Tick,
}

impl CoreMicrorealmPackClearProof {
    pub(crate) fn from_live_combat(
        character_id: [u8; 16],
        actor_generation: u64,
        instance_lineage_id: [u8; 16],
        tick: Tick,
    ) -> Result<Self, CorePrivateMicrorealmRuntimeError> {
        if character_id == [0; 16]
            || actor_generation == 0
            || instance_lineage_id == [0; 16]
            || tick.0 == 0
        {
            return Err(CorePrivateMicrorealmRuntimeError::InvalidClearProof);
        }
        Ok(Self {
            character_id,
            actor_generation,
            instance_lineage_id,
            tick,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorePrivateMicrorealmInput {
    pub input_sequence: u64,
    pub movement: MovementAction,
    pub aim: AimDirection,
    pub primary_held: bool,
    pub primary_sequence: u32,
    pub ability_1_sequence: u32,
    pub ability_2_sequence: u32,
    /// Authenticated session/activity evidence carried by the sole server driver. These fields
    /// are never decoded from gameplay frames and exist only for reward-presence accounting.
    pub reward_session_active: bool,
    pub reward_trust_valid: bool,
    pub reward_activity_sequence: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorePrivateMicrorealmStep {
    pub input_sequence: u64,
    pub tick: Tick,
    pub player_position: TilePoint,
    pub phase: CoreMicrorealmPhase,
    pub route: CorePrivateRouteStateV1,
    pub events: Vec<CoreMicrorealmEvent>,
    pub movement: MovementStep,
    pub combat: CombatStep,
    pub wave: Option<sim_core::NormalWaveStep>,
    pub player_damage: Vec<CorePrivatePlayerDamageFactV1>,
    pub pack_clear: Option<CoreMicrorealmPackClearProof>,
    pub player_died: bool,
    pub bell_portal_in_range: bool,
}

#[derive(Debug, Clone)]
pub struct CorePrivateMicrorealmRuntime {
    route_directory: CorePrivateRouteActorDirectory,
    route_lease: CorePrivateRouteActorLease,
    movement: PlayerMovementState,
    player_position: TilePoint,
    lifecycle: CoreMicrorealmSimulation,
    combat: sim_content::CoreMicrorealmPackCombat,
    combat_envelope: CoreCharacterCombatEnvelope,
    bell_portal_center: TilePoint,
    bell_portal_radius_milli_tiles: i32,
    tick: Tick,
}

struct StagedMicrorealmFrame {
    movement: PlayerMovementState,
    lifecycle: CoreMicrorealmSimulation,
    combat: sim_content::CoreMicrorealmPackCombat,
    player_position: TilePoint,
    phase: CoreMicrorealmPhase,
    events: Vec<CoreMicrorealmEvent>,
    movement_step: MovementStep,
    combat_step: CombatStep,
    wave_step: Option<sim_core::NormalWaveStep>,
    pack_clear: Option<CoreMicrorealmPackClearProof>,
    living_participants: u16,
}

impl CorePrivateMicrorealmRuntime {
    pub fn new(
        route_directory: CorePrivateRouteActorDirectory,
        route_lease: CorePrivateRouteActorLease,
        expected_content_revision: &CorePrivateRouteContentRevisionV1,
        scene: &WorldSceneDefinition,
        encounters: sim_content::CoreDevelopmentEncounterRooms,
        world_flow: sim_content::CoreDevelopmentWorldFlow,
        character_combat: CoreCharacterCombat,
    ) -> Result<Self, CorePrivateMicrorealmRuntimeError> {
        let route = route_directory.snapshot(route_lease)?;
        if route.content_revision != *expected_content_revision
            || route.character_id != route_lease.character_id()
            || route.actor_generation != route_lease.actor_generation()
            || route.scene != CorePrivateRouteSceneV1::CoreMicrorealm
            || route.room.is_some()
            || route.phase != CorePrivateRoutePhaseV1::MicrorealmDormant
            || route
                .instance_lineage_id
                .is_none_or(|lineage| lineage == [0; 16])
            || scene.id != CORE_MICROREALM_SCENE_ID
            || scene.kind != WorldSceneKind::PrivateDanger
            || scene.capacity != Some(1)
            || world_flow.compile_microrealm_scene()? != *scene
            || character_combat.character_id != route.character_id
        {
            return Err(CorePrivateMicrorealmRuntimeError::InvalidComposition);
        }
        let (bell_portal_center, bell_portal_radius_milli_tiles) = scene
            .objects
            .iter()
            .find_map(|object| (object.id == BELL_PORTAL_OBJECT_ID).then_some(object.geometry))
            .and_then(|geometry| match geometry {
                SceneObjectGeometry::Circle {
                    center,
                    radius_milli_tiles,
                } => Some((center, radius_milli_tiles)),
                _ => None,
            })
            .filter(|(_, radius)| *radius > 0)
            .ok_or(CorePrivateMicrorealmRuntimeError::InvalidComposition)?;
        let movement_config = core_player_movement_config(
            character_combat.movement_milli_tiles_per_second,
            scene.player_radius_milli_tiles,
        )?;
        let run_ordinal = u32::try_from(route.actor_generation)
            .map_err(|_| CorePrivateMicrorealmRuntimeError::InvalidComposition)?;
        let player_entity_id = run_player_entity_id(run_ordinal)?;
        let (combat_envelope, live_player) = character_combat.into_live_player(
            player_entity_id,
            tile_point_to_simulation(scene.player_spawn),
        )?;
        let combat = sim_content::CoreMicrorealmPackCombat::new(
            encounters,
            world_flow,
            run_ordinal,
            live_player,
            normal_wave_projectile_allocator(run_ordinal)?,
        )?;
        let arena = combat.arena()?;
        if arena.width_milli_tiles != scene.width_milli_tiles
            || arena.height_milli_tiles != scene.height_milli_tiles
            || arena.shell_thickness_milli_tiles != scene.shell_thickness_milli_tiles
            || arena.player_spawn != scene.player_spawn
            || !scene.solid_rectangles.is_empty()
        {
            return Err(CorePrivateMicrorealmRuntimeError::InvalidComposition);
        }
        let movement = PlayerMovementState::new_with_config(
            tile_point_to_simulation(scene.player_spawn),
            movement_config,
            &arena,
        )?;
        let lifecycle = CoreMicrorealmSimulation::new(scene.player_spawn);
        Ok(Self {
            route_directory,
            route_lease,
            movement,
            player_position: scene.player_spawn,
            lifecycle,
            combat,
            combat_envelope,
            bell_portal_center,
            bell_portal_radius_milli_tiles,
            tick: Tick(0),
        })
    }

    #[must_use]
    pub const fn route_lease(&self) -> CorePrivateRouteActorLease {
        self.route_lease
    }

    #[must_use]
    pub(crate) fn route_directory(&self) -> CorePrivateRouteActorDirectory {
        self.route_directory.clone()
    }

    #[must_use]
    pub const fn account_id(&self) -> [u8; 16] {
        self.route_lease.account_id()
    }

    #[must_use]
    pub const fn character_id(&self) -> [u8; 16] {
        self.route_lease.character_id()
    }

    #[must_use]
    pub const fn player_position(&self) -> TilePoint {
        self.player_position
    }

    #[must_use]
    pub const fn phase(&self) -> CoreMicrorealmPhase {
        self.lifecycle.phase()
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Reports the exact live Bell-transfer boundary from owned simulation state. The driver uses
    /// this only while pausing between frames; a client observation cannot author readiness.
    #[must_use]
    pub(crate) fn bell_transfer_ready(&self) -> bool {
        self.lifecycle.phase() == CoreMicrorealmPhase::Cleared
            && point_in_circle(
                self.player_position,
                self.bell_portal_center,
                self.bell_portal_radius_milli_tiles,
            )
    }

    /// Consumes a quiet/cleared microrealm owner and rejoins its one mutable combat allocation for
    /// the next room or terminal owner. Active packs cannot transfer.
    pub fn into_character_combat(
        self,
    ) -> Result<CoreCharacterCombat, CorePrivateMicrorealmRuntimeError> {
        let participant = self.combat.into_handoff()?;
        self.combat_envelope
            .rejoin(participant.player)
            .map_err(Into::into)
    }

    /// Consumes the paused microrealm only after the durable Bell transition has converged the
    /// exact route generation on B0. It preserves the player, hostile-projectile allocator,
    /// next hostile spawn ordinal, combat envelope, and server tick for the fixed dungeon owner.
    pub(crate) fn into_fixed_dungeon_handoff(
        self,
        transition: &CoreBellPortalTransition,
    ) -> Result<CorePrivateMicrorealmDungeonHandoff, CorePrivateMicrorealmRuntimeError> {
        if !self.bell_transfer_ready() {
            return Err(CorePrivateMicrorealmRuntimeError::RouteAuthorityMismatch);
        }
        let route = self.route_directory.snapshot(self.route_lease)?;
        let binding = &transition.binding;
        if binding.account_id != self.account_id()
            || binding.character_id != self.character_id()
            || binding.character_version != self.combat_envelope.character_state_version()
            || binding.instance_lineage_id != route.instance_lineage_id.unwrap_or([0; 16])
            || transition.destination_character_version != route.character_version
            || route.scene != CorePrivateRouteSceneV1::BellSepulcher
            || route.room != Some(protocol::CorePrivateRouteRoomV1::BellVestibuleB0)
            || route.phase != CorePrivateRoutePhaseV1::DungeonVestibule
            || route.actor_generation != self.route_lease.actor_generation()
        {
            return Err(CorePrivateMicrorealmRuntimeError::RouteAuthorityMismatch);
        }
        let next_hostile_spawn_ordinal = self.combat.next_spawn_ordinal();
        let participant = self.combat.into_handoff()?;
        let mut combat_envelope = self.combat_envelope;
        combat_envelope.rebase_character_state_version(
            binding.character_version,
            transition.destination_character_version,
        )?;
        Ok(CorePrivateMicrorealmDungeonHandoff {
            route_directory: self.route_directory,
            route_lease: self.route_lease,
            entry_restore_point_id: binding.entry_restore_point_id,
            combat_envelope,
            participant,
            next_hostile_spawn_ordinal,
            final_tick: self.tick,
        })
    }

    /// Advances one server-owned simulation frame with the driver's retained action state. The
    /// sequence is an acknowledgement value; transport ingress validates/coalesces it and the
    /// independent scheduler may intentionally reuse it across many frames.
    pub async fn step(
        &mut self,
        input: CorePrivateMicrorealmInput,
    ) -> Result<CorePrivateMicrorealmStep, CorePrivateMicrorealmRuntimeError> {
        let tick = self
            .tick
            .checked_next()
            .ok_or(CorePrivateMicrorealmRuntimeError::TickExhausted)?;
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;

        // All fallible simulation work is staged before the shared route CAS. Local state swaps
        // only after the actor commits phase and Bell range under its one lock.
        let frame = self.stage_frame(&input, tick, &route_before)?;
        let player_died = frame.living_participants == 0;
        let player = frame.combat.player().target.entity_id;
        let player_damage = match frame.wave_step.as_ref() {
            Some(wave) => normal_wave_player_damage_facts(wave, player, player_died)?,
            None if player_died => {
                return Err(CorePrivatePlayerDamageError::LethalityMismatch.into());
            }
            None => Vec::new(),
        };
        let bell_portal_in_range = frame.phase == CoreMicrorealmPhase::Cleared
            && point_in_circle(
                frame.player_position,
                self.bell_portal_center,
                self.bell_portal_radius_milli_tiles,
            );
        let route = self
            .route_directory
            .apply_microrealm_authority(
                self.route_lease,
                route_before.state_version,
                route_phase(frame.phase),
                bell_portal_in_range,
            )
            .await?;

        self.movement = frame.movement;
        self.player_position = frame.player_position;
        self.lifecycle = frame.lifecycle;
        self.combat = frame.combat;
        self.tick = tick;
        Ok(CorePrivateMicrorealmStep {
            input_sequence: input.input_sequence,
            tick,
            player_position: frame.player_position,
            phase: frame.phase,
            route,
            events: frame.events,
            movement: frame.movement_step,
            combat: frame.combat_step,
            wave: frame.wave_step,
            player_damage,
            pack_clear: frame.pack_clear,
            player_died,
            bell_portal_in_range,
        })
    }

    fn stage_frame(
        &self,
        input: &CorePrivateMicrorealmInput,
        tick: Tick,
        route_before: &CorePrivateRouteStateV1,
    ) -> Result<StagedMicrorealmFrame, CorePrivateMicrorealmRuntimeError> {
        let mut movement = self.movement;
        let mut combat = self.combat.clone();
        let arena = combat.arena()?;
        let collision_world = ProjectileCollisionWorld::new(&arena, combat.alive_hurtboxes()?)?;
        let (combat_step, movement_step) =
            step_player_combat(&mut combat, &mut movement, input, &arena, &collision_world)?;
        let player_position = simulation_to_tile_point(movement_step.position)?;
        if combat_step.tick != tick {
            return Err(CorePrivateMicrorealmRuntimeError::CombatTickMismatch);
        }
        let primary_released = combat_step.shots.iter().any(|shot| {
            shot.projectile.source() == FriendlyProjectileSource::Primary && shot.tick == tick
        });
        let mut wave_step = combat
            .wave()
            .is_some()
            .then(|| combat.step(&combat_step))
            .transpose()?;
        let pack_cleared = wave_step.as_ref().is_some_and(|step| {
            matches!(step.phase_after, sim_core::NormalWavePhase::Cleared { cleared_at } if cleared_at == tick)
        });
        let living_participants =
            u16::from(combat.player().consumables.vitals().current_health() != 0);
        let mut lifecycle = self.lifecycle.clone();
        let events = lifecycle.step(
            tick,
            CoreMicrorealmInput {
                entrant_position: player_position,
                primary_released,
                living_participants,
                pack_cleared,
            },
        )?;
        for event in events.iter().copied() {
            combat.apply_lifecycle_event(tick, event)?;
            if matches!(event, CoreMicrorealmEvent::BeginPackWarning { .. }) {
                wave_step = Some(combat.step(&combat_step)?);
            }
        }
        let phase = lifecycle.phase();
        let pack_clear = Self::pack_clear_proof(&events, tick, route_before)?;
        Ok(StagedMicrorealmFrame {
            movement,
            lifecycle,
            combat,
            player_position,
            phase,
            events,
            movement_step,
            combat_step,
            wave_step,
            pack_clear,
            living_participants,
        })
    }

    fn pack_clear_proof(
        events: &[CoreMicrorealmEvent],
        tick: Tick,
        route: &CorePrivateRouteStateV1,
    ) -> Result<Option<CoreMicrorealmPackClearProof>, CorePrivateMicrorealmRuntimeError> {
        let proof = events
            .contains(&CoreMicrorealmEvent::Cleared)
            .then(|| {
                CoreMicrorealmPackClearProof::from_live_combat(
                    route.character_id,
                    route.actor_generation,
                    route
                        .instance_lineage_id
                        .ok_or(CorePrivateMicrorealmRuntimeError::InvalidComposition)?,
                    tick,
                )
            })
            .transpose()?;
        if let Some(proof) = proof {
            Self::validate_clear_proof(proof, tick, route)?;
        }
        Ok(proof)
    }

    fn validate_route_authority(
        &self,
        route: &CorePrivateRouteStateV1,
    ) -> Result<(), CorePrivateMicrorealmRuntimeError> {
        if route.character_id != self.route_lease.character_id()
            || route.actor_generation != self.route_lease.actor_generation()
            || route.scene != CorePrivateRouteSceneV1::CoreMicrorealm
            || route.room.is_some()
            || route.phase != route_phase(self.lifecycle.phase())
        {
            return Err(CorePrivateMicrorealmRuntimeError::RouteAuthorityMismatch);
        }
        Ok(())
    }

    fn validate_clear_proof(
        proof: CoreMicrorealmPackClearProof,
        tick: Tick,
        route: &CorePrivateRouteStateV1,
    ) -> Result<(), CorePrivateMicrorealmRuntimeError> {
        if proof.tick != tick
            || proof.character_id != route.character_id
            || proof.actor_generation != route.actor_generation
            || Some(proof.instance_lineage_id) != route.instance_lineage_id
        {
            return Err(CorePrivateMicrorealmRuntimeError::InvalidClearProof);
        }
        Ok(())
    }
}

fn run_player_entity_id(
    run_ordinal: u32,
) -> Result<sim_core::EntityId, CorePrivateMicrorealmRuntimeError> {
    let zero_based = run_ordinal
        .checked_sub(1)
        .ok_or(CorePrivateMicrorealmRuntimeError::InvalidComposition)?;
    let value = u64::from(zero_based)
        .checked_mul(RUN_ENTITY_ID_STRIDE)
        .and_then(|base| base.checked_add(PLAYER_ENTITY_ID_OFFSET))
        .and_then(sim_core::EntityId::new)
        .ok_or(CorePrivateMicrorealmRuntimeError::InvalidComposition)?;
    Ok(value)
}

fn step_player_combat(
    combat: &mut sim_content::CoreMicrorealmPackCombat,
    movement: &mut PlayerMovementState,
    input: &CorePrivateMicrorealmInput,
    arena: &ArenaGeometry,
    collision_world: &ProjectileCollisionWorld,
) -> Result<(CombatStep, MovementStep), CorePrivateMicrorealmRuntimeError> {
    let player = combat.player_mut();
    step_live_player_combat(player, movement, input, arena, collision_world)
}

fn route_phase(phase: CoreMicrorealmPhase) -> CorePrivateRoutePhaseV1 {
    match phase {
        CoreMicrorealmPhase::Dormant => CorePrivateRoutePhaseV1::MicrorealmDormant,
        CoreMicrorealmPhase::Waiting => CorePrivateRoutePhaseV1::MicrorealmWaiting,
        CoreMicrorealmPhase::Active => CorePrivateRoutePhaseV1::MicrorealmActive,
        CoreMicrorealmPhase::Cleared => CorePrivateRoutePhaseV1::MicrorealmCleared,
    }
}

fn point_in_circle(point: TilePoint, center: TilePoint, radius_milli_tiles: i32) -> bool {
    let dx = i128::from(point.x_milli_tiles) - i128::from(center.x_milli_tiles);
    let dy = i128::from(point.y_milli_tiles) - i128::from(center.y_milli_tiles);
    let radius = i128::from(radius_milli_tiles);
    dx * dx + dy * dy <= radius * radius
}

#[derive(Debug, Error)]
pub enum CorePrivateMicrorealmRuntimeError {
    #[error("live Core microrealm composition is invalid")]
    InvalidComposition,
    #[error("live Core microrealm route authority no longer matches local state")]
    RouteAuthorityMismatch,
    #[error("live Core microrealm pack-clear proof is invalid or foreign")]
    InvalidClearProof,
    #[error("live Core microrealm run-local tick exhausted")]
    TickExhausted,
    #[error("live Core microrealm combat tick does not match the server-owned frame")]
    CombatTickMismatch,
    #[error(transparent)]
    Combat(#[from] CombatError),
    #[error(transparent)]
    Consumable(#[from] ConsumableError),
    #[error(transparent)]
    Collision(#[from] sim_core::CollisionError),
    #[error(transparent)]
    Movement(#[from] MovementError),
    #[error(transparent)]
    Content(#[from] anyhow::Error),
    #[error(transparent)]
    Pack(#[from] sim_content::CoreMicrorealmPackError),
    #[error(transparent)]
    CombatFactory(#[from] CoreCombatFactoryError),
    #[error(transparent)]
    Entity(#[from] sim_core::NormalWaveEntityIdError),
    #[error(transparent)]
    Lifecycle(#[from] sim_core::CoreMicrorealmError),
    #[error(transparent)]
    Route(#[from] CorePrivateRouteRuntimeError),
    #[error(transparent)]
    PlayerDamage(#[from] CorePrivatePlayerDamageError),
}

#[cfg(test)]
pub(crate) fn core_bell_ready_runtime_test_fixture(
    mut runtime: CorePrivateMicrorealmRuntime,
) -> CorePrivateMicrorealmRuntime {
    let arena = runtime.combat.arena().expect("test fixture arena");
    let collision_world =
        ProjectileCollisionWorld::new(&arena, Vec::new()).expect("test fixture collision world");
    for sequence in 1..=32 {
        let (combat, _) = step_live_player_combat(
            runtime.combat.player_mut(),
            &mut runtime.movement,
            &CorePrivateMicrorealmInput {
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
            &arena,
            &collision_world,
        )
        .expect("test fixture combat tick");
        assert_eq!(combat.tick, Tick(sequence));
    }
    let ordinary = CoreMicrorealmInput {
        entrant_position: runtime.player_position,
        primary_released: false,
        living_participants: 1,
        pack_cleared: false,
    };
    runtime
        .lifecycle
        .step(
            Tick(1),
            CoreMicrorealmInput {
                primary_released: true,
                ..ordinary
            },
        )
        .expect("test fixture trigger");
    runtime
        .lifecycle
        .step(Tick(31), ordinary)
        .expect("test fixture activation");
    runtime
        .lifecycle
        .step(
            Tick(32),
            CoreMicrorealmInput {
                pack_cleared: true,
                ..ordinary
            },
        )
        .expect("test fixture clear");
    runtime.player_position = runtime.bell_portal_center;
    runtime.tick = Tick(32);
    assert!(runtime.bell_transfer_ready());
    runtime
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{CorePrivateRouteRoomV1, ManifestHash, WorldFlowContentRevisionV1};

    use super::*;
    use crate::{
        AccountId, AuthenticatedAccount, AuthenticatedNamespace, CoreBellPortalAuthority,
        CoreBellPortalBinding, CorePrivateMicrorealmDriver, CorePrivateMicrorealmDriverOutcome,
        CorePrivateRouteActorPosition, CorePrivateRouteActorSeed,
    };

    const ACCOUNT_ID: [u8; 16] = [0x11; 16];
    const CHARACTER_ID: [u8; 16] = [0x22; 16];
    const LINEAGE_ID: [u8; 16] = [0x33; 16];

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).expect("valid hash")
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

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new(ACCOUNT_ID).expect("account"),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn content() -> (
        WorldSceneDefinition,
        sim_content::CoreDevelopmentEncounterRooms,
        sim_content::CoreDevelopmentWorldFlow,
    ) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let world = sim_content::load_core_development_world_flow(&root).expect("Core world");
        let scene = world.compile_microrealm_scene().expect("microrealm scene");
        let encounters =
            sim_content::load_core_development_encounter_rooms(&root).expect("Core encounters");
        (scene, encounters, world)
    }

    fn seed() -> CorePrivateRouteActorSeed {
        CorePrivateRouteActorSeed {
            character_id: CHARACTER_ID,
            character_version: 2,
            content_revision: route_revision(),
            world_flow_revision: world_revision(),
            position: CorePrivateRouteActorPosition {
                instance_lineage_id: Some(LINEAGE_ID),
                scene: CorePrivateRouteSceneV1::CoreMicrorealm,
                room: None,
                phase: CorePrivateRoutePhaseV1::MicrorealmDormant,
            },
        }
    }

    fn input(sequence: u64) -> CorePrivateMicrorealmInput {
        CorePrivateMicrorealmInput {
            input_sequence: sequence,
            movement: MovementAction::default(),
            aim: AimDirection::east(),
            primary_held: false,
            primary_sequence: 0,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
            reward_session_active: true,
            reward_trust_valid: true,
            reward_activity_sequence: sequence.max(1),
        }
    }

    fn runtime(
        directory: &CorePrivateRouteActorDirectory,
        lease: CorePrivateRouteActorLease,
    ) -> CorePrivateMicrorealmRuntime {
        let (scene, encounters, world) = content();
        CorePrivateMicrorealmRuntime::new(
            directory.clone(),
            lease,
            &route_revision(),
            &scene,
            encounters,
            world,
            crate::combat_factory::core_character_combat_test_fixture(CHARACTER_ID),
        )
        .expect("live runtime")
    }

    fn bell_ready_runtime(
        directory: &CorePrivateRouteActorDirectory,
        lease: CorePrivateRouteActorLease,
    ) -> CorePrivateMicrorealmRuntime {
        core_bell_ready_runtime_test_fixture(runtime(directory, lease))
    }

    async fn commit_bell_transition(
        directory: &CorePrivateRouteActorDirectory,
        lease: CorePrivateRouteActorLease,
    ) -> CoreBellPortalTransition {
        for advance in [
            crate::CorePrivateRouteActorAdvance::MicrorealmWaiting,
            crate::CorePrivateRouteActorAdvance::MicrorealmActive,
            crate::CorePrivateRouteActorAdvance::MicrorealmCleared,
        ] {
            directory
                .advance(lease, advance)
                .await
                .expect("route phase");
        }
        directory
            .set_bell_portal_in_range(lease, true)
            .await
            .expect("Bell range");
        let binding = CoreBellPortalBinding {
            account_id: ACCOUNT_ID,
            character_id: CHARACTER_ID,
            mutation_id: [0x44; 16],
            instance_lineage_id: LINEAGE_ID,
            entry_restore_point_id: [0x55; 16],
            character_version: 2,
            content_revision: world_revision(),
        };
        let permit = directory
            .prepare_bell_portal(binding.clone())
            .await
            .expect("Bell permit");
        let transition = CoreBellPortalTransition {
            binding,
            transfer_id: [0x66; 16],
            destination_character_version: 3,
        };
        directory
            .commit_bell_portal(permit, transition.clone())
            .await
            .expect("Bell commit");
        transition
    }

    #[tokio::test]
    async fn retained_input_advances_many_ticks_and_commits_the_exact_route_projection() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 7)
            .expect("actor");
        let mut runtime = runtime(&directory, lease);

        let mut release = input(1);
        release.primary_held = true;
        release.primary_sequence = 1;
        let waiting = runtime.step(release).await.expect("waiting");
        assert_eq!(waiting.tick, Tick(1));
        assert_eq!(waiting.combat.shots.len(), 1);
        assert_eq!(waiting.phase, CoreMicrorealmPhase::Waiting);
        assert_eq!(
            waiting.route.phase,
            CorePrivateRoutePhaseV1::MicrorealmWaiting
        );

        let mut active = None;
        for _ in 2..=31 {
            let mut waiting_input = input(1);
            waiting_input.primary_sequence = 1;
            active = Some(runtime.step(waiting_input).await.expect("wait tick"));
        }
        let active = active.expect("active step");
        assert_eq!(active.tick, Tick(31));
        assert_eq!(active.phase, CoreMicrorealmPhase::Active);
        assert_eq!(
            active.route.phase,
            CorePrivateRoutePhaseV1::MicrorealmActive
        );
        assert_eq!(
            active.events,
            vec![CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 }]
        );
        let wave = active.wave.expect("warning wave step");
        assert_eq!(wave.tick, Tick(31));
        assert_eq!(runtime.combat.wave().expect("pack").snapshots().len(), 8);
        assert!(
            runtime
                .combat
                .wave()
                .expect("pack")
                .snapshots()
                .iter()
                .all(|enemy| enemy.health.alive)
        );
    }

    #[tokio::test]
    async fn tick_displacement_slipstep_damage_and_clear_are_not_ingress_authority() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 9)
            .expect("actor");
        let mut runtime = runtime(&directory, lease);
        let start = runtime.player_position();
        let mut movement = input(1);
        movement.movement = MovementAction::new(1, 0);
        let moved = runtime.step(movement).await.expect("server tick");
        assert_eq!(moved.tick, Tick(1));
        assert!(moved.player_position.x_milli_tiles > start.x_milli_tiles);
        assert!(moved.pack_clear.is_none());
        assert!(moved.wave.is_none());

        let before_position = runtime.player_position();
        let mut slipstep = input(2);
        slipstep.movement = MovementAction::new(1, 0);
        slipstep.ability_2_sequence = 1;
        let slipped = runtime.step(slipstep).await.expect("Slipstep frame");
        assert_eq!(slipped.tick, Tick(2));
        assert!(slipped.player_position.x_milli_tiles > before_position.x_milli_tiles);
        assert!(slipped.combat.slipstep_inputs.iter().any(|event| {
            event.result == sim_core::SlipstepInputResult::Began && event.press_sequence == 1
        }));
        assert!(
            slipped
                .combat
                .slipstep_transitions
                .iter()
                .any(|transition| {
                    matches!(
                        transition.kind,
                        sim_core::SlipstepTransitionKind::Travelled
                            | sim_core::SlipstepTransitionKind::Completed
                    )
                })
        );
        assert!(slipped.combat.direct_damage_reduction_basis_points > 0);

        let foreign =
            CoreMicrorealmPackClearProof::from_live_combat(CHARACTER_ID, 8, LINEAGE_ID, Tick(2))
                .expect("structured proof");
        assert!(matches!(
            CorePrivateMicrorealmRuntime::validate_clear_proof(
                foreign,
                Tick(2),
                &directory.snapshot(lease).expect("route"),
            ),
            Err(CorePrivateMicrorealmRuntimeError::InvalidClearProof)
        ));
    }

    #[tokio::test]
    async fn slipstep_stops_at_the_compiled_world_shell_in_the_same_combat_frame() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 8)
            .expect("actor");
        let mut runtime = runtime(&directory, lease);
        let arena = runtime.combat.arena().expect("arena");
        let start = sim_core::SimulationVector::new(1.4, 20.0);
        runtime.movement =
            PlayerMovementState::new_with_config(start, runtime.movement.config(), &arena)
                .expect("near-shell movement");
        runtime.player_position = simulation_to_tile_point(start).expect("projection");
        runtime.combat.player_mut().target.position = start;

        let mut slipstep = input(1);
        slipstep.movement = MovementAction::new(-1, 0);
        slipstep.ability_2_sequence = 1;
        let stopped = runtime.step(slipstep).await.expect("collision frame");

        assert!(stopped.movement.collided);
        assert_eq!(stopped.player_position.x_milli_tiles, 1_300);
        assert!(
            stopped
                .combat
                .slipstep_transitions
                .iter()
                .any(|transition| {
                    transition.kind == sim_core::SlipstepTransitionKind::Collided
                        && transition.solid.is_some()
                })
        );
        assert_eq!(
            runtime.combat.player().target.position,
            runtime.movement.position()
        );
    }

    #[tokio::test]
    async fn combined_actor_command_rejects_stale_version_without_partial_phase_change() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 10)
            .expect("actor");
        let before = directory.snapshot(lease).expect("before");
        assert!(matches!(
            directory
                .apply_microrealm_authority(
                    lease,
                    before.state_version + 1,
                    CorePrivateRoutePhaseV1::MicrorealmWaiting,
                    false,
                )
                .await,
            Err(CorePrivateRouteRuntimeError::StaleRouteState)
        ));
        assert_eq!(directory.snapshot(lease).expect("after"), before);
    }

    #[tokio::test]
    async fn stale_route_rolls_back_local_movement_and_lifecycle() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 11)
            .expect("actor");
        let mut runtime = runtime(&directory, lease);
        let start = runtime.player_position();
        directory
            .advance(
                lease,
                crate::CorePrivateRouteActorAdvance::MicrorealmWaiting,
            )
            .await
            .expect("foreign server caller advances actor");

        let mut moved = input(1);
        moved.movement = MovementAction::new(1, 0);
        assert!(matches!(
            runtime.step(moved).await,
            Err(CorePrivateMicrorealmRuntimeError::RouteAuthorityMismatch)
        ));
        assert_eq!(runtime.player_position(), start);
        assert_eq!(runtime.phase(), CoreMicrorealmPhase::Dormant);
    }

    #[tokio::test]
    async fn dormant_owner_rejoins_the_exact_character_combat_without_a_clone() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 6)
            .expect("actor");
        let combat = runtime(&directory, lease)
            .into_character_combat()
            .expect("quiet handoff");
        assert_eq!(combat.character_id, CHARACTER_ID);
        assert_eq!(combat.state.tick(), Tick(0));
        assert_eq!(combat.consumables.vitals().current_health(), 120);
    }

    #[tokio::test(start_paused = true)]
    #[allow(
        clippy::too_many_lines,
        reason = "one contiguous ownership trace proves cancellation, relocation, ticks, input, and shutdown"
    )]
    async fn dropped_conversion_ack_still_converts_inside_the_exact_live_task() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 12)
            .expect("actor");
        let runtime = bell_ready_runtime(&directory, lease);
        let driver = CorePrivateMicrorealmDriver::spawn_without_terminal_owner(runtime);
        let handle = driver.handle();
        let state_reader = handle.observe();
        tokio::task::yield_now().await;
        handle
            .submit_latest_input(crate::CorePrivateMicrorealmRetainedInput {
                input_sequence: 41,
                movement: MovementAction::new(1, 0),
                aim: AimDirection::east(),
                primary_held: true,
                primary_sequence: 9,
            })
            .expect("Bell-held intent");
        let prepared = driver.prepare_handoff().await.expect("prepare handoff");
        let transition = commit_bell_transition(&directory, lease).await;
        let (_, encounters, _) = content();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let caldus = Arc::new(sim_content::load_core_development_caldus(&root).unwrap());
        let conversion = prepared
            .commit_into_fixed_dungeon(transition, route_revision(), encounters, caldus)
            .expect("conversion decision");
        drop(conversion);
        let mut state_reader = state_reader;
        let pending_state = state_reader
            .changed()
            .await
            .expect("fixed-dungeon observation");
        assert!(matches!(
            pending_state,
            crate::CorePrivateMicrorealmDriverState::BellResolutionPending { .. }
        ));
        let published_state = state_reader
            .changed()
            .await
            .expect("fixed-dungeon observation");
        let crate::CorePrivateMicrorealmDriverState::FixedDungeonReady { ready } = published_state
        else {
            panic!("same observer must publish fixed-dungeon ownership");
        };

        assert_eq!(
            ready.node,
            sim_content::CoreFixedDungeonNode::BellVestibuleB0
        );
        assert_eq!(ready.route_lease, lease);
        assert_eq!(ready.final_microrealm_tick, Tick(32));
        assert!(matches!(
            state_reader.latest(),
            crate::CorePrivateMicrorealmDriverState::FixedDungeonReady { ready: installed }
                if installed == ready
        ));
        assert!(matches!(
            handle.prepare_handoff().await,
            Err(crate::CorePrivateMicrorealmDriverError::HandoffNotReady)
        ));
        assert!(crate::active_core_microrealm_driver_tasks() >= 1);

        let entered = handle
            .advance_fixed_dungeon()
            .await
            .expect("server-selected B0 to B1 transition");
        assert_eq!(
            entered.transition.to,
            sim_content::CoreFixedDungeonNode::BellCrossB1
        );
        assert!(matches!(
            state_reader.changed().await.expect("B1 boundary observation"),
            crate::CorePrivateMicrorealmDriverState::FixedDungeonReady { ready: entered }
                if entered.node == sim_content::CoreFixedDungeonNode::BellCrossB1
        ));
        let neutral = handle.latest_retained_input();
        assert_eq!(neutral.input_sequence, 41);
        assert_eq!(neutral.primary_sequence, 9);
        assert_eq!(neutral.movement, MovementAction::default());
        assert!(!neutral.primary_held);

        tokio::time::advance(std::time::Duration::from_millis(34)).await;
        let running = state_reader
            .changed()
            .await
            .expect("first authoritative fixed-room frame");
        assert!(
            matches!(
                &running,
                crate::CorePrivateMicrorealmDriverState::FixedDungeonRunning {
                    committed_frames: 1,
                    frame,
                } if frame.tick == Tick(33)
                    && frame.input_sequence == 41
                    && frame.route.room == Some(CorePrivateRouteRoomV1::BellCrossB1)
            ),
            "unexpected first fixed-room observation: {running:?}"
        );
        assert!(matches!(
            handle.advance_fixed_dungeon().await,
            Err(crate::CorePrivateMicrorealmDriverError::FixedDungeonAdvance(message))
                if message.contains("cannot advance")
        ));
        handle
            .submit_latest_input(crate::CorePrivateMicrorealmRetainedInput {
                input_sequence: 42,
                movement: MovementAction::default(),
                aim: AimDirection::east(),
                primary_held: false,
                primary_sequence: 9,
            })
            .expect("early boundary interaction remains non-terminal");
        tokio::time::advance(std::time::Duration::from_millis(34)).await;
        let continued = state_reader
            .changed()
            .await
            .expect("driver continues after early advance");
        assert!(matches!(
            continued,
            crate::CorePrivateMicrorealmDriverState::FixedDungeonRunning {
                committed_frames: 2,
                ref frame,
            } if frame.tick == Tick(34) && frame.input_sequence == 42
        ));
        let report = driver.shutdown().await.expect("joined shutdown");
        assert_eq!(report.outcome, CorePrivateMicrorealmDriverOutcome::Shutdown);
        assert_eq!(report.final_tick, Tick(34));
        assert_eq!(report.committed_frames, 2);
        assert!(report.task_joined);
        assert!(!report.driver_task_live_after_join);
    }

    #[tokio::test]
    async fn committed_bell_handoff_rebases_version_and_preserves_every_run_local_identity() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 13)
            .expect("actor");
        let mut runtime = runtime(&directory, lease);
        let spawn = runtime.player_position;
        let ordinary = CoreMicrorealmInput {
            entrant_position: spawn,
            primary_released: false,
            living_participants: 1,
            pack_cleared: false,
        };
        runtime
            .lifecycle
            .step(
                Tick(1),
                CoreMicrorealmInput {
                    primary_released: true,
                    ..ordinary
                },
            )
            .expect("trigger");
        runtime
            .lifecycle
            .step(Tick(31), ordinary)
            .expect("activate");
        runtime
            .lifecycle
            .step(
                Tick(32),
                CoreMicrorealmInput {
                    pack_cleared: true,
                    ..ordinary
                },
            )
            .expect("clear");
        runtime.player_position = runtime.bell_portal_center;
        runtime.tick = Tick(32);
        let quiet_participant = runtime
            .combat
            .clone()
            .into_handoff()
            .expect("quiet combat handoff");
        let player_id = quiet_participant.player.target.entity_id;
        let projectile_peek = quiet_participant.hostile_projectile_ids.peek();
        for advance in [
            crate::CorePrivateRouteActorAdvance::MicrorealmWaiting,
            crate::CorePrivateRouteActorAdvance::MicrorealmActive,
            crate::CorePrivateRouteActorAdvance::MicrorealmCleared,
        ] {
            directory
                .advance(lease, advance)
                .await
                .expect("route phase");
        }
        directory
            .set_bell_portal_in_range(lease, true)
            .await
            .expect("Bell range");
        let binding = CoreBellPortalBinding {
            account_id: ACCOUNT_ID,
            character_id: CHARACTER_ID,
            mutation_id: [0x44; 16],
            instance_lineage_id: LINEAGE_ID,
            entry_restore_point_id: [0x55; 16],
            character_version: 2,
            content_revision: world_revision(),
        };
        let permit = directory
            .prepare_bell_portal(binding.clone())
            .await
            .expect("Bell permit");
        let transition = CoreBellPortalTransition {
            binding,
            transfer_id: [0x66; 16],
            destination_character_version: 3,
        };
        directory
            .commit_bell_portal(permit, transition.clone())
            .await
            .expect("Bell commit");

        let handoff = runtime
            .into_fixed_dungeon_handoff(&transition)
            .expect("fixed-dungeon handoff");

        assert_eq!(handoff.final_tick, Tick(32));
        assert_eq!(handoff.combat_envelope.character_state_version(), 3);
        assert_eq!(handoff.next_hostile_spawn_ordinal, 1);
        assert_eq!(handoff.participant.player.target.entity_id, player_id);
        assert_eq!(
            handoff.participant.hostile_projectile_ids.peek(),
            projectile_peek
        );
    }
}
