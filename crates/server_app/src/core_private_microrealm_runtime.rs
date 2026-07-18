//! Live capacity-one movement and lifecycle owner for the ordinary Core microrealm.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`-`003`,
//! `TECH-010`-`023`), `Gravebound_Content_Production_Spec_v1.md` (`CONT-WORLD-001` and
//! `CONT-WORLD-004`), and `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`). This owner keeps
//! client movement and primary-release intent below server-owned collision, pack-clear, phase,
//! and Bell-range authority. Its existence does not enable normal route admission.

use protocol::{
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteSceneV1,
    CorePrivateRouteStateV1,
};
use sim_core::{
    CoreMicrorealmEvent, CoreMicrorealmInput, CoreMicrorealmPhase, CoreMicrorealmSimulation,
    SceneDisplacement, SceneObjectGeometry, Tick, TilePoint, WorldSceneDefinition, WorldSceneError,
    WorldSceneKind, WorldScenePlayer,
};
use thiserror::Error;

use crate::{
    CorePrivateRouteActorDirectory, CorePrivateRouteActorLease, CorePrivateRouteRuntimeError,
};

const CORE_MICROREALM_SCENE_ID: &str = "world.core_microrealm_01";
const BELL_PORTAL_OBJECT_ID: &str = "portal.dungeon.bell_sepulcher";
/// The compiled Grave Arbalist speed is 5,100 milli-tiles/second at the canonical 30 Hz tick.
const GRAVE_ARBALIST_STEP_MILLI_TILES: i32 = 170;

/// Opaque server-owned proof that the live combat owner cleared the microrealm pack on this tick.
/// The ordinary input decoder cannot construct this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreMicrorealmPackClearProof {
    character_id: [u8; 16],
    actor_generation: u64,
    instance_lineage_id: [u8; 16],
    tick: Tick,
}

impl CoreMicrorealmPackClearProof {
    pub fn from_live_combat(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateMicrorealmInput {
    pub input_sequence: u64,
    pub tick: Tick,
    pub displacement: SceneDisplacement,
    pub primary_released: bool,
    pub pack_clear: Option<CoreMicrorealmPackClearProof>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateMicrorealmStep {
    pub input_sequence: u64,
    pub tick: Tick,
    pub player_position: TilePoint,
    pub phase: CoreMicrorealmPhase,
    pub route: CorePrivateRouteStateV1,
    pub events: Vec<CoreMicrorealmEvent>,
    pub bell_portal_in_range: bool,
}

#[derive(Debug, Clone)]
pub struct CorePrivateMicrorealmRuntime {
    route_directory: CorePrivateRouteActorDirectory,
    route_lease: CorePrivateRouteActorLease,
    scene: WorldSceneDefinition,
    player: WorldScenePlayer,
    lifecycle: CoreMicrorealmSimulation,
    bell_portal_center: TilePoint,
    bell_portal_radius_milli_tiles: i32,
    last_input_sequence: Option<u64>,
}

impl CorePrivateMicrorealmRuntime {
    pub fn new(
        route_directory: CorePrivateRouteActorDirectory,
        route_lease: CorePrivateRouteActorLease,
        expected_content_revision: &CorePrivateRouteContentRevisionV1,
        scene: WorldSceneDefinition,
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
        let player =
            WorldScenePlayer::new(&scene, scene.player_spawn, GRAVE_ARBALIST_STEP_MILLI_TILES)?;
        let lifecycle = CoreMicrorealmSimulation::new(scene.player_spawn);
        Ok(Self {
            route_directory,
            route_lease,
            scene,
            player,
            lifecycle,
            bell_portal_center,
            bell_portal_radius_milli_tiles,
            last_input_sequence: None,
        })
    }

    #[must_use]
    pub const fn player_position(&self) -> TilePoint {
        self.player.position()
    }

    #[must_use]
    pub const fn phase(&self) -> CoreMicrorealmPhase {
        self.lifecycle.phase()
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
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        let pack_cleared = match input.pack_clear {
            None => false,
            Some(proof) => {
                Self::validate_clear_proof(proof, input.tick, &route_before)?;
                true
            }
        };

        // Stage all fallible simulation work before touching the shared route actor. The actor's
        // combined command then applies phase and Bell range under one lock; local state is swapped
        // only after that command succeeds.
        let mut staged_player = self.player.clone();
        let player_position = staged_player.step_movement(&self.scene, input.displacement)?;
        let mut staged_lifecycle = self.lifecycle.clone();
        let events = staged_lifecycle.step(
            input.tick,
            CoreMicrorealmInput {
                entrant_position: player_position,
                primary_released: input.primary_released,
                living_participants: 1,
                pack_cleared,
            },
        )?;
        let phase = staged_lifecycle.phase();
        let bell_portal_in_range = phase == CoreMicrorealmPhase::Cleared
            && point_in_circle(
                player_position,
                self.bell_portal_center,
                self.bell_portal_radius_milli_tiles,
            );
        let route = self
            .route_directory
            .apply_microrealm_authority(
                self.route_lease,
                route_before.state_version,
                route_phase(phase),
                bell_portal_in_range,
            )
            .await?;

        self.player = staged_player;
        self.lifecycle = staged_lifecycle;
        self.last_input_sequence = Some(input.input_sequence);
        Ok(CorePrivateMicrorealmStep {
            input_sequence: input.input_sequence,
            tick: input.tick,
            player_position,
            phase,
            route,
            events,
            bell_portal_in_range,
        })
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

    fn scene() -> WorldSceneDefinition {
        sim_content::load_core_development_world_flow(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"),
        )
        .expect("Core world content")
        .compile_microrealm_scene()
        .expect("microrealm scene")
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

    fn input(sequence: u64, tick: u64) -> CorePrivateMicrorealmInput {
        CorePrivateMicrorealmInput {
            input_sequence: sequence,
            tick: Tick(tick),
            displacement: SceneDisplacement::new(0, 0),
            primary_released: false,
            pack_clear: None,
        }
    }

    #[tokio::test]
    async fn movement_and_lifecycle_commit_with_the_exact_route_projection() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 7)
            .expect("actor");
        let mut runtime =
            CorePrivateMicrorealmRuntime::new(directory.clone(), lease, &route_revision(), scene())
                .expect("live runtime");

        let mut release = input(1, 1);
        release.primary_released = true;
        let waiting = runtime.step(release).await.expect("waiting");
        assert_eq!(waiting.phase, CoreMicrorealmPhase::Waiting);
        assert_eq!(
            waiting.route.phase,
            CorePrivateRoutePhaseV1::MicrorealmWaiting
        );

        let active = runtime.step(input(2, 31)).await.expect("active");
        assert_eq!(active.phase, CoreMicrorealmPhase::Active);
        assert_eq!(
            active.route.phase,
            CorePrivateRoutePhaseV1::MicrorealmActive
        );
        assert_eq!(
            active.events,
            vec![CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 }]
        );
    }

    #[tokio::test]
    async fn only_exact_server_clear_proof_opens_the_bell_range() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), seed(), 9)
            .expect("actor");
        let mut runtime =
            CorePrivateMicrorealmRuntime::new(directory.clone(), lease, &route_revision(), scene())
                .expect("live runtime");
        let mut release = input(1, 1);
        release.primary_released = true;
        runtime.step(release).await.expect("waiting");
        runtime.step(input(2, 31)).await.expect("active");

        let mut foreign = input(3, 32);
        foreign.pack_clear = Some(
            CoreMicrorealmPackClearProof::from_live_combat(CHARACTER_ID, 8, LINEAGE_ID, Tick(32))
                .expect("structured foreign proof"),
        );
        assert!(matches!(
            runtime.step(foreign).await,
            Err(CorePrivateMicrorealmRuntimeError::InvalidClearProof)
        ));
        assert_eq!(runtime.phase(), CoreMicrorealmPhase::Active);

        let mut clear = input(3, 32);
        clear.displacement = SceneDisplacement::new(0, -170);
        clear.pack_clear = Some(
            CoreMicrorealmPackClearProof::from_live_combat(CHARACTER_ID, 9, LINEAGE_ID, Tick(32))
                .expect("exact proof"),
        );
        let cleared = runtime.step(clear).await.expect("cleared");
        assert_eq!(cleared.phase, CoreMicrorealmPhase::Cleared);
        assert_eq!(cleared.events, vec![CoreMicrorealmEvent::Cleared]);
        assert!(!cleared.bell_portal_in_range);

        let mut sequence = 4;
        let mut tick = 33;
        for _ in 0..188 {
            let mut movement = input(sequence, tick);
            movement.displacement = SceneDisplacement::new(170, 0);
            runtime.step(movement).await.expect("eastward route step");
            sequence += 1;
            tick += 1;
        }
        let mut arrived = None;
        for _ in 0..187 {
            let mut movement = input(sequence, tick);
            movement.displacement = SceneDisplacement::new(0, -170);
            arrived = Some(runtime.step(movement).await.expect("northward route step"));
            sequence += 1;
            tick += 1;
        }
        let arrived = arrived.expect("at least one northward step");
        assert_eq!(arrived.phase, CoreMicrorealmPhase::Cleared);
        assert!(arrived.bell_portal_in_range);
        assert!(point_in_circle(
            arrived.player_position,
            TilePoint::new(40_500, 8_500),
            3_000
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
        let mut runtime =
            CorePrivateMicrorealmRuntime::new(directory.clone(), lease, &route_revision(), scene())
                .expect("live runtime");
        let start = runtime.player_position();
        directory
            .advance(
                lease,
                crate::CorePrivateRouteActorAdvance::MicrorealmWaiting,
            )
            .await
            .expect("foreign server caller advances actor");

        let mut moved = input(1, 1);
        moved.displacement = SceneDisplacement::new(170, 0);
        assert!(matches!(
            runtime.step(moved).await,
            Err(CorePrivateMicrorealmRuntimeError::RouteAuthorityMismatch)
        ));
        assert_eq!(runtime.player_position(), start);
        assert_eq!(runtime.phase(), CoreMicrorealmPhase::Dormant);

        assert!(matches!(
            runtime.step(input(0, 2)).await,
            Err(CorePrivateMicrorealmRuntimeError::StaleInputSequence)
        ));
    }
}
