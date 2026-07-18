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
    AimDirection, CombatAction, CombatError, CombatStep, ConsumableAction, ConsumableError,
    CoreMicrorealmEvent, CoreMicrorealmInput, CoreMicrorealmPhase, CoreMicrorealmSimulation,
    FriendlyProjectileSource, MovementAction, ProjectileCollisionWorld, SceneDisplacement,
    SceneObjectGeometry, SimulationVector, Tick, TilePoint, WorldSceneDefinition, WorldSceneError,
    WorldSceneKind, WorldScenePlayer, normal_wave_projectile_allocator, tile_point_to_simulation,
};
use thiserror::Error;

use crate::{
    CoreCharacterCombat, CoreCharacterCombatEnvelope, CoreCombatFactoryError,
    CorePrivateRouteActorDirectory, CorePrivateRouteActorLease, CorePrivateRouteRuntimeError,
};

const CORE_MICROREALM_SCENE_ID: &str = "world.core_microrealm_01";
const BELL_PORTAL_OBJECT_ID: &str = "portal.dungeon.bell_sepulcher";
const AUTHORITATIVE_TICKS_PER_SECOND: u32 = 30;
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorePrivateMicrorealmStep {
    pub input_sequence: u64,
    pub tick: Tick,
    pub player_position: TilePoint,
    pub phase: CoreMicrorealmPhase,
    pub route: CorePrivateRouteStateV1,
    pub events: Vec<CoreMicrorealmEvent>,
    pub combat: CombatStep,
    pub wave: Option<sim_core::NormalWaveStep>,
    pub pack_clear: Option<CoreMicrorealmPackClearProof>,
    pub player_died: bool,
    pub bell_portal_in_range: bool,
}

#[derive(Debug, Clone)]
pub struct CorePrivateMicrorealmRuntime {
    route_directory: CorePrivateRouteActorDirectory,
    route_lease: CorePrivateRouteActorLease,
    scene: WorldSceneDefinition,
    player: WorldScenePlayer,
    lifecycle: CoreMicrorealmSimulation,
    combat: sim_content::CoreMicrorealmPackCombat,
    combat_envelope: CoreCharacterCombatEnvelope,
    bell_portal_center: TilePoint,
    bell_portal_radius_milli_tiles: i32,
    movement_milli_tiles_per_second: u32,
    tick: Tick,
    last_input_sequence: Option<u64>,
}

struct StagedMicrorealmFrame {
    player: WorldScenePlayer,
    lifecycle: CoreMicrorealmSimulation,
    combat: sim_content::CoreMicrorealmPackCombat,
    player_position: TilePoint,
    phase: CoreMicrorealmPhase,
    events: Vec<CoreMicrorealmEvent>,
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
        scene: WorldSceneDefinition,
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
            || world_flow.compile_microrealm_scene()? != scene
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
        let movement_milli_tiles_per_second = character_combat.movement_milli_tiles_per_second;
        let maximum_step_milli_tiles =
            i32::try_from(movement_milli_tiles_per_second.div_ceil(AUTHORITATIVE_TICKS_PER_SECOND))
                .map_err(|_| CorePrivateMicrorealmRuntimeError::InvalidComposition)?;
        let player = WorldScenePlayer::new(&scene, scene.player_spawn, maximum_step_milli_tiles)?;
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
        let lifecycle = CoreMicrorealmSimulation::new(scene.player_spawn);
        Ok(Self {
            route_directory,
            route_lease,
            scene,
            player,
            lifecycle,
            combat,
            combat_envelope,
            bell_portal_center,
            bell_portal_radius_milli_tiles,
            movement_milli_tiles_per_second,
            tick: Tick(0),
            last_input_sequence: None,
        })
    }

    #[must_use]
    pub const fn route_lease(&self) -> CorePrivateRouteActorLease {
        self.route_lease
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
        self.player.position()
    }

    #[must_use]
    pub const fn phase(&self) -> CoreMicrorealmPhase {
        self.lifecycle.phase()
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
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

    pub async fn step(
        &mut self,
        input: CorePrivateMicrorealmInput,
    ) -> Result<CorePrivateMicrorealmStep, CorePrivateMicrorealmRuntimeError> {
        if input.input_sequence == 0
            || self
                .last_input_sequence
                .is_some_and(|last| input.input_sequence <= last)
        {
            return Err(CorePrivateMicrorealmRuntimeError::StaleInputSequence);
        }
        if input.ability_2_sequence != 0 {
            return Err(CorePrivateMicrorealmRuntimeError::MovementAbilityUnavailable);
        }
        let tick = self
            .tick
            .checked_next()
            .ok_or(CorePrivateMicrorealmRuntimeError::TickExhausted)?;
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;

        // All fallible simulation work is staged before the shared route CAS. Local state swaps
        // only after the actor commits phase and Bell range under its one lock.
        let frame = self.stage_frame(&input, tick, &route_before)?;
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

        self.player = frame.player;
        self.lifecycle = frame.lifecycle;
        self.combat = frame.combat;
        self.tick = tick;
        self.last_input_sequence = Some(input.input_sequence);
        Ok(CorePrivateMicrorealmStep {
            input_sequence: input.input_sequence,
            tick,
            player_position: frame.player_position,
            phase: frame.phase,
            route,
            events: frame.events,
            combat: frame.combat_step,
            wave: frame.wave_step,
            pack_clear: frame.pack_clear,
            player_died: frame.living_participants == 0,
            bell_portal_in_range,
        })
    }

    fn stage_frame(
        &self,
        input: &CorePrivateMicrorealmInput,
        tick: Tick,
        route_before: &CorePrivateRouteStateV1,
    ) -> Result<StagedMicrorealmFrame, CorePrivateMicrorealmRuntimeError> {
        let mut player = self.player.clone();
        let displacement =
            scene_displacement(input.movement, self.movement_milli_tiles_per_second)?;
        let player_position = player.step_movement(&self.scene, displacement)?;
        let mut combat = self.combat.clone();
        let arena = combat.arena()?;
        let collision_world = ProjectileCollisionWorld::new(&arena, combat.alive_hurtboxes()?)?;
        let player_vector = tile_point_to_simulation(player_position);
        let combat_step = step_player_combat(&mut combat, input, player_vector, &collision_world)?;
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
            player,
            lifecycle,
            combat,
            player_position,
            phase,
            events,
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

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn scene_displacement(
    movement: MovementAction,
    movement_milli_tiles_per_second: u32,
) -> Result<SceneDisplacement, CorePrivateMicrorealmRuntimeError> {
    let step = movement_milli_tiles_per_second as f32 / AUTHORITATIVE_TICKS_PER_SECOND as f32;
    let direction = movement.normalized_vector();
    let x = (direction.x * step).round();
    let y = (direction.y * step).round();
    if !x.is_finite()
        || !y.is_finite()
        || x < i32::MIN as f32
        || x > i32::MAX as f32
        || y < i32::MIN as f32
        || y > i32::MAX as f32
    {
        return Err(CorePrivateMicrorealmRuntimeError::InvalidComposition);
    }
    Ok(SceneDisplacement::new(x as i32, y as i32))
}

fn step_player_combat(
    combat: &mut sim_content::CoreMicrorealmPackCombat,
    input: &CorePrivateMicrorealmInput,
    player_position: SimulationVector,
    collision_world: &ProjectileCollisionWorld,
) -> Result<CombatStep, CorePrivateMicrorealmRuntimeError> {
    let player = combat.player_mut();
    player.target.position = player_position;
    let step = player.combat.step(
        CombatAction {
            aim: input.aim,
            movement: input.movement,
            primary_held: input.primary_held,
            primary_press_sequence: input.primary_sequence,
            ability_1_press_sequence: input.ability_1_sequence,
            ability_2_press_sequence: input.ability_2_sequence,
        },
        player_position,
        collision_world,
    )?;
    player.consumables.step(ConsumableAction::default())?;
    player
        .target
        .additional_direct_damage_reductions_basis_points =
        (step.direct_damage_reduction_basis_points != 0)
            .then_some(step.direct_damage_reduction_basis_points)
            .into_iter()
            .collect();
    Ok(step)
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
    #[error("live Core microrealm input sequence is stale or zero")]
    StaleInputSequence,
    #[error("live Core microrealm route authority no longer matches local state")]
    RouteAuthorityMismatch,
    #[error("live Core microrealm pack-clear proof is invalid or foreign")]
    InvalidClearProof,
    #[error("live Core microrealm run-local tick exhausted")]
    TickExhausted,
    #[error("live Core microrealm combat tick does not match the server-owned frame")]
    CombatTickMismatch,
    #[error("Slipstep movement is not yet composed with the live scene owner")]
    MovementAbilityUnavailable,
    #[error(transparent)]
    Combat(#[from] CombatError),
    #[error(transparent)]
    Consumable(#[from] ConsumableError),
    #[error(transparent)]
    Collision(#[from] sim_core::CollisionError),
    #[error(transparent)]
    Content(#[from] anyhow::Error),
    #[error(transparent)]
    Pack(#[from] sim_content::CoreMicrorealmPackError),
    #[error(transparent)]
    CombatFactory(#[from] CoreCombatFactoryError),
    #[error(transparent)]
    Entity(#[from] sim_core::NormalWaveEntityIdError),
    #[error(transparent)]
    World(#[from] WorldSceneError),
    #[error(transparent)]
    Lifecycle(#[from] sim_core::CoreMicrorealmError),
    #[error(transparent)]
    Route(#[from] CorePrivateRouteRuntimeError),
}

#[cfg(test)]
mod tests {
    use protocol::{ManifestHash, WorldFlowContentRevisionV1};

    use super::*;
    use crate::{
        AccountId, AuthenticatedAccount, AuthenticatedNamespace, CorePrivateRouteActorPosition,
        CorePrivateRouteActorSeed,
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
            scene,
            encounters,
            world,
            crate::combat_factory::core_character_combat_test_fixture(CHARACTER_ID),
        )
        .expect("live runtime")
    }

    #[tokio::test]
    async fn movement_and_lifecycle_commit_with_the_exact_route_projection() {
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
        for sequence in 2..=31 {
            let mut waiting_input = input(sequence);
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
    async fn tick_displacement_damage_and_clear_are_not_ingress_authority() {
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

        let before_tick = runtime.tick();
        let before_position = runtime.player_position();
        let mut unsupported = input(2);
        unsupported.ability_2_sequence = 1;
        assert!(matches!(
            runtime.step(unsupported).await,
            Err(CorePrivateMicrorealmRuntimeError::MovementAbilityUnavailable)
        ));
        assert_eq!(runtime.tick(), before_tick);
        assert_eq!(runtime.player_position(), before_position);

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
    async fn stale_route_or_input_rolls_back_local_movement_and_lifecycle() {
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

        assert!(matches!(
            runtime.step(input(0)).await,
            Err(CorePrivateMicrorealmRuntimeError::StaleInputSequence)
        ));
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
}
