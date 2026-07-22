//! Authoritative Lantern Halls movement and Realm Gate interaction ownership.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`, `MOV-001`,
//! `TECH-012`, and `TECH-015`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-HUB-001`/`002`), and `Gravebound_Development_Roadmap_v1.md`
//! (`GB-M03-03`). The normal Realm Gate may be prepared only from the current authenticated
//! transport while its selected living character is physically within the authored 1.5-tile
//! interaction range.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Mutex, MutexGuard},
};

use protocol::{
    CharacterLocation, CharacterLocationSnapshot, ENTITY_STATE_ALIVE, EntityKind, EntitySnapshot,
    HALL_INTERACTION_HOLD_TICKS, HALL_INTERACTION_SCHEMA_VERSION, HallInteractionFrameV1,
    HallInteractionIntentV1, HallInteractionResultCodeV1, HallInteractionResultV1, HallStationV1,
    InputFrame, SafeArrival,
};
use sim_core::{
    SceneAccessContext, SceneDisplacement, SceneInteractionAccess, SceneInteractionEvent,
    SceneInteractionSession, SceneObjectGeometry, TilePoint, WorldSceneDefinition,
    WorldScenePlayer,
};
use thiserror::Error;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CorePrivateLifeTransportGeneration,
    CorePrivateLifeTransportLease,
    core_private_gameplay_observation::CorePrivateGameplayObservation,
};

const HALL_ID: &str = "hub.lantern_halls_01";
const REALM_GATE_ID: &str = "station.realm_gate";
const WORLD_FLOW_GATE: &str = "core_world_flow_integration";
const HALL_MOVEMENT_MILLI_TILES_PER_TICK: i32 = 150;
const INPUT_VECTOR_SCALE: i64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CorePrivateHallActorLease {
    account_id: [u8; 16],
    character_id: [u8; 16],
    actor_generation: u64,
}

impl CorePrivateHallActorLease {
    #[must_use]
    pub(crate) const fn account_id(self) -> [u8; 16] {
        self.account_id
    }

    #[must_use]
    pub(crate) const fn character_id(self) -> [u8; 16] {
        self.character_id
    }
}

/// Opaque reservation proving range, actor generation, transport generation, character version,
/// and mutation identity at one fail-closed boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CorePrivateHallRealmGatePermit {
    actor: CorePrivateHallActorLease,
    transport_generation: CorePrivateLifeTransportGeneration,
    character_version: u64,
    mutation_id: [u8; 16],
    input_sequence: u32,
}

impl CorePrivateHallRealmGatePermit {
    #[must_use]
    pub(crate) const fn character_id(self) -> [u8; 16] {
        self.actor.character_id
    }

    #[must_use]
    pub(crate) const fn character_version(self) -> u64 {
        self.character_version
    }

    #[must_use]
    pub(crate) const fn mutation_id(self) -> [u8; 16] {
        self.mutation_id
    }
}

#[derive(Debug)]
struct HallActor {
    lease: CorePrivateHallActorLease,
    character_version: u64,
    transport_generation: Option<CorePrivateLifeTransportGeneration>,
    player: WorldScenePlayer,
    simulation_tick: u64,
    state_version: u64,
    pending_movement: SceneDisplacement,
    velocity_x_milli_tiles_per_second: i32,
    velocity_y_milli_tiles_per_second: i32,
    last_input_sequence: u32,
    last_client_tick: u64,
    interaction: SceneInteractionSession,
    interaction_held: bool,
    interaction_sequence: Option<u32>,
    interaction_station: Option<HallStationV1>,
    interaction_held_ticks: u16,
    last_interaction_sequence: u32,
    transfer: Option<CorePrivateHallRealmGatePermit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CorePrivateHallTick {
    pub(crate) observation: CorePrivateGameplayObservation,
    pub(crate) interaction: Option<HallInteractionResultV1>,
}

#[derive(Debug)]
struct HallState {
    accepting: bool,
    next_generation: BTreeMap<[u8; 16], u64>,
    actors: BTreeMap<[u8; 16], HallActor>,
    committed_gates: BTreeMap<([u8; 16], [u8; 16]), CorePrivateHallRealmGatePermit>,
}

/// Capacity-one-per-account safe-scene authority. The immutable scene comes from the compiled
/// Core content package; clients provide only bounded movement intent.
#[derive(Debug)]
pub(crate) struct CorePrivateHallDirectory {
    scene: WorldSceneDefinition,
    enabled_gates: BTreeSet<String>,
    state: Mutex<HallState>,
}

impl CorePrivateHallDirectory {
    pub(crate) fn load(content_root: &Path) -> Result<Self, CorePrivateHallError> {
        let content = sim_content::load_core_development_world_flow(content_root)
            .map_err(|_| CorePrivateHallError::Content)?;
        let scene = content
            .compile_hall_scene()
            .map_err(|_| CorePrivateHallError::Content)?;
        if scene.id != HALL_ID {
            return Err(CorePrivateHallError::Content);
        }
        Ok(Self {
            scene,
            enabled_gates: BTreeSet::from([WORLD_FLOW_GATE.to_owned()]),
            state: Mutex::new(HallState {
                accepting: true,
                next_generation: BTreeMap::new(),
                actors: BTreeMap::new(),
                committed_gates: BTreeMap::new(),
            }),
        })
    }

    /// Installs an exact durable Hall projection, or reuses the same live actor on reconnect.
    pub(crate) fn install(
        &self,
        authenticated: AuthenticatedAccount,
        snapshot: &CharacterLocationSnapshot,
    ) -> Result<CorePrivateHallActorLease, CorePrivateHallError> {
        validate_hall_snapshot(authenticated, snapshot)?;
        let account_id = authenticated.account_id.as_bytes();
        let mut state = lock(&self.state);
        if !state.accepting {
            return Err(CorePrivateHallError::Retired);
        }
        if let Some(existing) = state.actors.get(&account_id)
            && existing.lease.character_id == snapshot.character_id
            && existing.character_version == snapshot.character_version
        {
            return Ok(existing.lease);
        }
        let generation = *state.next_generation.entry(account_id).or_insert(1);
        let next_generation = generation
            .checked_add(1)
            .ok_or(CorePrivateHallError::GenerationExhausted)?;
        let spawn = hall_arrival_point(&self.scene, &snapshot.location)?;
        let lease = CorePrivateHallActorLease {
            account_id,
            character_id: snapshot.character_id,
            actor_generation: generation,
        };
        state.next_generation.insert(account_id, next_generation);
        state.actors.insert(
            account_id,
            HallActor {
                lease,
                character_version: snapshot.character_version,
                transport_generation: None,
                player: WorldScenePlayer::new(
                    &self.scene,
                    spawn,
                    HALL_MOVEMENT_MILLI_TILES_PER_TICK,
                )
                .map_err(|_| CorePrivateHallError::Content)?,
                simulation_tick: 1,
                state_version: 1,
                pending_movement: SceneDisplacement::new(0, 0),
                velocity_x_milli_tiles_per_second: 0,
                velocity_y_milli_tiles_per_second: 0,
                last_input_sequence: 0,
                last_client_tick: 0,
                interaction: SceneInteractionSession::default(),
                interaction_held: false,
                interaction_sequence: None,
                interaction_station: None,
                interaction_held_ticks: 0,
                last_interaction_sequence: 0,
                transfer: None,
            },
        );
        Ok(lease)
    }

    pub(crate) fn install_stored(
        &self,
        authenticated: AuthenticatedAccount,
        hall: &persistence::StoredPrivateLifeHallV1,
    ) -> Result<CorePrivateHallActorLease, CorePrivateHallError> {
        let arrival = match &hall.arrival {
            persistence::StoredSafeArrival::HallDefault => SafeArrival::HallDefault,
            persistence::StoredSafeArrival::SpawnAnchor(spawn_id) => SafeArrival::SpawnAnchor {
                spawn_id: protocol::WireText::new(spawn_id.clone())
                    .map_err(|_| CorePrivateHallError::InvalidSnapshot)?,
            },
        };
        self.install(
            authenticated,
            &CharacterLocationSnapshot {
                character_id: hall.character.character_id,
                character_version: hall.character.versions.world,
                location: CharacterLocation::Safe {
                    location_id: protocol::WireText::new(HALL_ID)
                        .map_err(|_| CorePrivateHallError::Content)?,
                    arrival,
                },
            },
        )
    }

    /// Pins Hall input and interaction authority to the session directory's winning transport.
    pub(crate) fn attach_transport(
        &self,
        authenticated: AuthenticatedAccount,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
    ) -> Result<(), CorePrivateHallError> {
        let mut state = lock(&self.state);
        let live = exact_actor_mut(&mut state, authenticated, actor)?;
        if transport.account_id() != actor.account_id {
            return Err(CorePrivateHallError::ForeignAuthority);
        }
        live.transport_generation = Some(transport.generation());
        cancel_interaction(live);
        live.pending_movement = SceneDisplacement::new(0, 0);
        Ok(())
    }

    pub(crate) fn retire(
        &self,
        actor: CorePrivateHallActorLease,
    ) -> Result<(), CorePrivateHallError> {
        let mut state = lock(&self.state);
        let live = state
            .actors
            .get(&actor.account_id)
            .ok_or(CorePrivateHallError::ActorUnavailable)?;
        if live.lease != actor {
            return Err(CorePrivateHallError::StaleActor);
        }
        state.actors.remove(&actor.account_id);
        Ok(())
    }

    pub(crate) fn apply_input(
        &self,
        authenticated: AuthenticatedAccount,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
        input: &InputFrame,
    ) -> Result<TilePoint, CorePrivateHallError> {
        input
            .validate()
            .map_err(|_| CorePrivateHallError::InvalidInput)?;
        let mut state = lock(&self.state);
        let live = exact_actor_mut(&mut state, authenticated, actor)?;
        require_transport(live, transport)?;
        if live.transfer.is_some() {
            return Err(CorePrivateHallError::TransferInProgress);
        }
        if input.sequence <= live.last_input_sequence || input.client_tick < live.last_client_tick {
            return Err(CorePrivateHallError::StaleInput);
        }
        if input.held_primary || input.primary_sequence != 0 {
            return Err(CorePrivateHallError::UnsafeAction);
        }
        live.pending_movement = hall_displacement(input.movement_x_milli, input.movement_y_milli)?;
        live.last_input_sequence = input.sequence;
        live.last_client_tick = input.client_tick;
        Ok(live.player.position())
    }

    /// Advances exactly one authoritative 30 Hz Hall tick from the latest bounded input.
    pub(crate) fn advance_tick(
        &self,
        authenticated: AuthenticatedAccount,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
    ) -> Result<CorePrivateHallTick, CorePrivateHallError> {
        let mut state = lock(&self.state);
        let live = exact_actor_mut(&mut state, authenticated, actor)?;
        require_transport(live, transport)?;
        live.simulation_tick = live
            .simulation_tick
            .checked_add(1)
            .ok_or(CorePrivateHallError::TickExhausted)?;
        live.state_version = live
            .state_version
            .checked_add(1)
            .ok_or(CorePrivateHallError::TickExhausted)?;
        let before = live.player.position();
        let after = live
            .player
            .step_movement(&self.scene, live.pending_movement)
            .map_err(|_| CorePrivateHallError::InvalidInput)?;
        live.velocity_x_milli_tiles_per_second = after
            .x_milli_tiles
            .checked_sub(before.x_milli_tiles)
            .and_then(|delta| delta.checked_mul(30))
            .ok_or(CorePrivateHallError::TickExhausted)?;
        live.velocity_y_milli_tiles_per_second = after
            .y_milli_tiles
            .checked_sub(before.y_milli_tiles)
            .and_then(|delta| delta.checked_mul(30))
            .ok_or(CorePrivateHallError::TickExhausted)?;
        let interaction = advance_held_interaction(&self.scene, &self.enabled_gates, live)?;
        Ok(CorePrivateHallTick {
            observation: hall_observation(live)?,
            interaction,
        })
    }

    pub(crate) fn observation(
        &self,
        authenticated: AuthenticatedAccount,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
    ) -> Result<CorePrivateGameplayObservation, CorePrivateHallError> {
        let mut state = lock(&self.state);
        let live = exact_actor_mut(&mut state, authenticated, actor)?;
        require_transport(live, transport)?;
        hall_observation(live)
    }

    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "interaction frames follow the shared validated-frame API used by larger request types"
    )]
    pub(crate) fn handle_interaction(
        &self,
        authenticated: AuthenticatedAccount,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
        frame: &HallInteractionFrameV1,
    ) -> Result<HallInteractionResultV1, CorePrivateHallError> {
        frame
            .validate()
            .map_err(|_| CorePrivateHallError::InvalidInteraction)?;
        let mut state = lock(&self.state);
        let live = exact_actor_mut(&mut state, authenticated, actor)?;
        require_transport(live, transport)?;
        if frame.sequence <= live.last_interaction_sequence {
            return Ok(interaction_result(
                frame.sequence,
                HallInteractionResultCodeV1::StaleSequence,
                None,
                0,
                0,
            ));
        }
        live.last_interaction_sequence = frame.sequence;
        match frame.intent {
            HallInteractionIntentV1::BeginHold => {
                begin_interaction(&self.scene, &self.enabled_gates, live, frame.sequence)
            }
            HallInteractionIntentV1::Release => Ok(release_interaction(live, frame.sequence)),
            HallInteractionIntentV1::ClosePanel => close_panel(live, frame.sequence),
        }
    }

    #[must_use]
    pub(crate) fn panel_authorizes(
        &self,
        authenticated: AuthenticatedAccount,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
        station: HallStationV1,
    ) -> bool {
        let mut state = lock(&self.state);
        exact_actor_mut(&mut state, authenticated, actor)
            .and_then(|live| {
                require_transport(live, transport)?;
                Ok(live.interaction.open_panel_object_id() == Some(station.content_id()))
            })
            .unwrap_or(false)
    }

    pub(crate) fn prepare_realm_gate(
        &self,
        authenticated: AuthenticatedAccount,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
        mutation_id: [u8; 16],
        expected_character_version: u64,
    ) -> Result<CorePrivateHallRealmGatePermit, CorePrivateHallError> {
        if mutation_id == [0; 16] {
            return Err(CorePrivateHallError::InvalidMutation);
        }
        let mut state = lock(&self.state);
        let live = exact_actor_mut(&mut state, authenticated, actor)?;
        require_transport(live, transport)?;
        if live.transfer.is_some() {
            return Err(CorePrivateHallError::TransferInProgress);
        }
        if live.character_version != expected_character_version {
            return Err(CorePrivateHallError::VersionMismatch);
        }
        if live.interaction.open_panel_object_id() != Some(REALM_GATE_ID) {
            return Err(CorePrivateHallError::PanelRequired);
        }
        let interaction = live
            .player
            .nearest_interaction(
                &self.scene,
                SceneAccessContext {
                    enabled_integration_gates: &self.enabled_gates,
                    microrealm_cleared: false,
                },
            )
            .map_err(|_| CorePrivateHallError::Content)?
            .ok_or(CorePrivateHallError::OutOfRange)?;
        if interaction.object_id != REALM_GATE_ID
            || interaction.access != SceneInteractionAccess::Available
        {
            return Err(CorePrivateHallError::OutOfRange);
        }
        let permit = CorePrivateHallRealmGatePermit {
            actor,
            transport_generation: transport.generation(),
            character_version: live.character_version,
            mutation_id,
            input_sequence: live.last_input_sequence,
        };
        live.transfer = Some(permit);
        Ok(permit)
    }

    pub(crate) fn abort_realm_gate(
        &self,
        permit: CorePrivateHallRealmGatePermit,
    ) -> Result<(), CorePrivateHallError> {
        let mut state = lock(&self.state);
        let live = state
            .actors
            .get_mut(&permit.actor.account_id)
            .ok_or(CorePrivateHallError::ActorUnavailable)?;
        if live.lease != permit.actor || live.transfer != Some(permit) {
            return Err(CorePrivateHallError::StaleActor);
        }
        live.transfer = None;
        Ok(())
    }

    pub(crate) fn commit_realm_gate(
        &self,
        permit: CorePrivateHallRealmGatePermit,
    ) -> Result<(), CorePrivateHallError> {
        let mut state = lock(&self.state);
        let live = state
            .actors
            .get(&permit.actor.account_id)
            .ok_or(CorePrivateHallError::ActorUnavailable)?;
        if live.lease != permit.actor || live.transfer != Some(permit) {
            return Err(CorePrivateHallError::StaleActor);
        }
        state.actors.remove(&permit.actor.account_id);
        state
            .committed_gates
            .insert((permit.actor.account_id, permit.mutation_id), permit);
        Ok(())
    }

    /// Returns true only for the exact already-committed Gate identity retained by this process.
    /// This permits response-loss replay after the Hall actor has been retired without opening a
    /// second range-free mutation path.
    pub(crate) fn is_committed_realm_gate(
        &self,
        authenticated: AuthenticatedAccount,
        character_id: [u8; 16],
        character_version: u64,
        mutation_id: [u8; 16],
    ) -> bool {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return false;
        }
        lock(&self.state)
            .committed_gates
            .get(&(authenticated.account_id.as_bytes(), mutation_id))
            .is_some_and(|permit| {
                permit.actor.character_id == character_id
                    && permit.character_version == character_version
            })
    }

    #[cfg(test)]
    pub(crate) fn install_at(
        &self,
        authenticated: AuthenticatedAccount,
        character_id: [u8; 16],
        character_version: u64,
        point: TilePoint,
        transport_generation: CorePrivateLifeTransportGeneration,
    ) -> CorePrivateHallActorLease {
        let account_id = authenticated.account_id.as_bytes();
        let lease = CorePrivateHallActorLease {
            account_id,
            character_id,
            actor_generation: 1,
        };
        lock(&self.state).actors.insert(
            account_id,
            HallActor {
                lease,
                character_version,
                transport_generation: Some(transport_generation),
                player: WorldScenePlayer::new(
                    &self.scene,
                    point,
                    HALL_MOVEMENT_MILLI_TILES_PER_TICK,
                )
                .unwrap(),
                simulation_tick: 1,
                state_version: 1,
                pending_movement: SceneDisplacement::new(0, 0),
                velocity_x_milli_tiles_per_second: 0,
                velocity_y_milli_tiles_per_second: 0,
                last_input_sequence: 0,
                last_client_tick: 0,
                interaction: SceneInteractionSession::default(),
                interaction_held: false,
                interaction_sequence: None,
                interaction_station: None,
                interaction_held_ticks: 0,
                last_interaction_sequence: 0,
                transfer: None,
            },
        );
        lease
    }
}

fn begin_interaction(
    scene: &WorldSceneDefinition,
    enabled_gates: &BTreeSet<String>,
    actor: &mut HallActor,
    sequence: u32,
) -> Result<HallInteractionResultV1, CorePrivateHallError> {
    if actor.transfer.is_some() {
        return Ok(interaction_result(
            sequence,
            HallInteractionResultCodeV1::InvalidState,
            None,
            0,
            0,
        ));
    }
    if let Some(open) = actor.interaction.open_panel_object_id() {
        let station = HallStationV1::from_content_id(open).ok_or(CorePrivateHallError::Content)?;
        return Ok(interaction_result(
            sequence,
            HallInteractionResultCodeV1::PanelAlreadyOpen,
            Some(station),
            0,
            0,
        ));
    }
    if actor.interaction_held {
        return Ok(interaction_result(
            sequence,
            HallInteractionResultCodeV1::InvalidState,
            None,
            0,
            0,
        ));
    }
    let Some(projection) = actor
        .player
        .nearest_interaction(
            scene,
            SceneAccessContext {
                enabled_integration_gates: enabled_gates,
                microrealm_cleared: false,
            },
        )
        .map_err(|_| CorePrivateHallError::Content)?
    else {
        return Ok(interaction_result(
            sequence,
            HallInteractionResultCodeV1::OutOfRange,
            None,
            0,
            0,
        ));
    };
    let station = HallStationV1::from_content_id(&projection.object_id)
        .ok_or(CorePrivateHallError::Content)?;
    if projection.access != SceneInteractionAccess::Available {
        return Ok(interaction_result(
            sequence,
            HallInteractionResultCodeV1::InvalidState,
            None,
            0,
            0,
        ));
    }
    if projection.hold_ticks == 0 {
        let events = actor
            .interaction
            .step(Some(&projection), true, false)
            .map_err(|_| CorePrivateHallError::InvalidInteraction)?;
        if !matches!(events.as_slice(), [SceneInteractionEvent::Opened { .. }]) {
            return Err(CorePrivateHallError::InvalidInteraction);
        }
        actor.state_version = actor
            .state_version
            .checked_add(1)
            .ok_or(CorePrivateHallError::TickExhausted)?;
        return Ok(interaction_result(
            sequence,
            HallInteractionResultCodeV1::Opened,
            Some(station),
            0,
            0,
        ));
    }
    if projection.hold_ticks != HALL_INTERACTION_HOLD_TICKS {
        return Err(CorePrivateHallError::Content);
    }
    actor.interaction_held = true;
    actor.interaction_sequence = Some(sequence);
    actor.interaction_station = Some(station);
    actor.interaction_held_ticks = 0;
    Ok(interaction_result(
        sequence,
        HallInteractionResultCodeV1::Holding,
        Some(station),
        0,
        HALL_INTERACTION_HOLD_TICKS,
    ))
}

fn advance_held_interaction(
    scene: &WorldSceneDefinition,
    enabled_gates: &BTreeSet<String>,
    actor: &mut HallActor,
) -> Result<Option<HallInteractionResultV1>, CorePrivateHallError> {
    if !actor.interaction_held {
        return Ok(None);
    }
    let sequence = actor
        .interaction_sequence
        .ok_or(CorePrivateHallError::InvalidInteraction)?;
    let station = actor
        .interaction_station
        .ok_or(CorePrivateHallError::InvalidInteraction)?;
    let projection = actor
        .player
        .nearest_interaction(
            scene,
            SceneAccessContext {
                enabled_integration_gates: enabled_gates,
                microrealm_cleared: false,
            },
        )
        .map_err(|_| CorePrivateHallError::Content)?;
    let Some(projection) = projection.filter(|projection| {
        projection.object_id == station.content_id()
            && projection.access == SceneInteractionAccess::Available
            && projection.hold_ticks == HALL_INTERACTION_HOLD_TICKS
    }) else {
        let held_ticks = actor.interaction_held_ticks;
        actor
            .interaction
            .step(None, false, false)
            .map_err(|_| CorePrivateHallError::InvalidInteraction)?;
        clear_hold(actor);
        return Ok(Some(interaction_result(
            sequence,
            HallInteractionResultCodeV1::CancelledOutOfRange,
            Some(station),
            held_ticks,
            HALL_INTERACTION_HOLD_TICKS,
        )));
    };
    let events = actor
        .interaction
        .step(Some(&projection), true, false)
        .map_err(|_| CorePrivateHallError::InvalidInteraction)?;
    match events.as_slice() {
        [
            SceneInteractionEvent::Progress {
                held_ticks,
                required_ticks,
                ..
            },
        ] if *required_ticks == HALL_INTERACTION_HOLD_TICKS => {
            actor.interaction_held_ticks = *held_ticks;
            Ok(Some(interaction_result(
                sequence,
                HallInteractionResultCodeV1::Holding,
                Some(station),
                *held_ticks,
                *required_ticks,
            )))
        }
        [SceneInteractionEvent::Opened { .. }] => {
            clear_hold(actor);
            Ok(Some(interaction_result(
                sequence,
                HallInteractionResultCodeV1::Opened,
                Some(station),
                HALL_INTERACTION_HOLD_TICKS,
                HALL_INTERACTION_HOLD_TICKS,
            )))
        }
        _ => Err(CorePrivateHallError::InvalidInteraction),
    }
}

fn release_interaction(actor: &mut HallActor, request_sequence: u32) -> HallInteractionResultV1 {
    if !actor.interaction_held {
        return interaction_result(
            request_sequence,
            HallInteractionResultCodeV1::NoActiveHold,
            None,
            0,
            0,
        );
    }
    let station = actor.interaction_station;
    let held_ticks = actor.interaction_held_ticks;
    let _ = actor.interaction.step(None, false, false);
    clear_hold(actor);
    interaction_result(
        request_sequence,
        HallInteractionResultCodeV1::CancelledReleased,
        station,
        held_ticks,
        HALL_INTERACTION_HOLD_TICKS,
    )
}

fn close_panel(
    actor: &mut HallActor,
    request_sequence: u32,
) -> Result<HallInteractionResultV1, CorePrivateHallError> {
    let Some(station) = actor
        .interaction
        .open_panel_object_id()
        .and_then(HallStationV1::from_content_id)
    else {
        return Ok(interaction_result(
            request_sequence,
            HallInteractionResultCodeV1::NoOpenPanel,
            None,
            0,
            0,
        ));
    };
    let events = actor
        .interaction
        .step(None, false, true)
        .map_err(|_| CorePrivateHallError::InvalidInteraction)?;
    if !matches!(events.as_slice(), [SceneInteractionEvent::Closed { .. }]) {
        return Err(CorePrivateHallError::InvalidInteraction);
    }
    actor.state_version = actor
        .state_version
        .checked_add(1)
        .ok_or(CorePrivateHallError::TickExhausted)?;
    Ok(interaction_result(
        request_sequence,
        HallInteractionResultCodeV1::Closed,
        Some(station),
        0,
        0,
    ))
}

fn cancel_interaction(actor: &mut HallActor) {
    let _ = actor.interaction.step(None, false, true);
    clear_hold(actor);
}

fn clear_hold(actor: &mut HallActor) {
    actor.interaction_held = false;
    actor.interaction_sequence = None;
    actor.interaction_station = None;
    actor.interaction_held_ticks = 0;
}

const fn interaction_result(
    request_sequence: u32,
    code: HallInteractionResultCodeV1,
    station: Option<HallStationV1>,
    held_ticks: u16,
    required_ticks: u16,
) -> HallInteractionResultV1 {
    HallInteractionResultV1 {
        schema_version: HALL_INTERACTION_SCHEMA_VERSION,
        request_sequence,
        code,
        station,
        held_ticks,
        required_ticks,
    }
}

fn hall_observation(
    actor: &HallActor,
) -> Result<CorePrivateGameplayObservation, CorePrivateHallError> {
    let entity_id = hall_player_entity_id(actor.lease.character_id);
    let position = actor.player.position();
    CorePrivateGameplayObservation::new(
        actor.simulation_tick,
        actor.lease.actor_generation,
        actor.state_version,
        u64::from(actor.last_input_sequence),
        vec![EntitySnapshot {
            entity_id,
            kind: EntityKind::Player,
            x_milli_tiles: position.x_milli_tiles,
            y_milli_tiles: position.y_milli_tiles,
            velocity_x_milli_tiles_per_second: actor.velocity_x_milli_tiles_per_second,
            velocity_y_milli_tiles_per_second: actor.velocity_y_milli_tiles_per_second,
            source_entity_id: 0,
            source_input_sequence: 0,
            source_projectile_ordinal: 0,
            current_health: 1,
            maximum_health: 1,
            state_flags: ENTITY_STATE_ALIVE,
        }],
    )
    .map_err(|_| CorePrivateHallError::Observation)
}

fn hall_player_entity_id(character_id: [u8; 16]) -> u64 {
    let mut material = [0_u8; 8];
    material.copy_from_slice(&character_id[..8]);
    u64::from_le_bytes(material).max(1)
}

fn exact_actor_mut(
    state: &mut HallState,
    authenticated: AuthenticatedAccount,
    actor: CorePrivateHallActorLease,
) -> Result<&mut HallActor, CorePrivateHallError> {
    if authenticated.namespace != AuthenticatedNamespace::WipeableTest
        || authenticated.account_id.as_bytes() != actor.account_id
    {
        return Err(CorePrivateHallError::ForeignAuthority);
    }
    let live = state
        .actors
        .get_mut(&actor.account_id)
        .ok_or(CorePrivateHallError::ActorUnavailable)?;
    if live.lease != actor {
        return Err(CorePrivateHallError::StaleActor);
    }
    Ok(live)
}

fn require_transport(
    actor: &HallActor,
    transport: CorePrivateLifeTransportLease,
) -> Result<(), CorePrivateHallError> {
    if transport.account_id() != actor.lease.account_id
        || actor.transport_generation != Some(transport.generation())
    {
        return Err(CorePrivateHallError::StaleTransport);
    }
    Ok(())
}

fn validate_hall_snapshot(
    authenticated: AuthenticatedAccount,
    snapshot: &CharacterLocationSnapshot,
) -> Result<(), CorePrivateHallError> {
    snapshot
        .validate()
        .map_err(|_| CorePrivateHallError::InvalidSnapshot)?;
    if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
        return Err(CorePrivateHallError::ForeignAuthority);
    }
    match &snapshot.location {
        CharacterLocation::Safe { location_id, .. } if location_id.as_str() == HALL_ID => Ok(()),
        _ => Err(CorePrivateHallError::InvalidSnapshot),
    }
}

fn hall_arrival_point(
    scene: &WorldSceneDefinition,
    location: &CharacterLocation,
) -> Result<TilePoint, CorePrivateHallError> {
    let CharacterLocation::Safe { arrival, .. } = location else {
        return Err(CorePrivateHallError::InvalidSnapshot);
    };
    match arrival {
        SafeArrival::HallDefault => Ok(scene.player_spawn),
        SafeArrival::SpawnAnchor { spawn_id } => scene
            .objects
            .iter()
            .find(|object| object.id == spawn_id.as_str())
            .and_then(|object| match object.geometry {
                SceneObjectGeometry::Point(point) => Some(point),
                _ => None,
            })
            .ok_or(CorePrivateHallError::InvalidSnapshot),
    }
}

fn hall_displacement(x: i16, y: i16) -> Result<SceneDisplacement, CorePrivateHallError> {
    let x = i64::from(x);
    let y = i64::from(y);
    let magnitude_squared = x
        .checked_mul(x)
        .and_then(|value| y.checked_mul(y).and_then(|other| value.checked_add(other)))
        .ok_or(CorePrivateHallError::InvalidInput)?;
    if magnitude_squared == 0 {
        return Ok(SceneDisplacement::new(0, 0));
    }
    let magnitude = u64::try_from(magnitude_squared)
        .map_err(|_| CorePrivateHallError::InvalidInput)?
        .isqrt()
        .max(1);
    let scale = if magnitude > INPUT_VECTOR_SCALE as u64 {
        i64::try_from(magnitude).map_err(|_| CorePrivateHallError::InvalidInput)?
    } else {
        INPUT_VECTOR_SCALE
    };
    let step = i64::from(HALL_MOVEMENT_MILLI_TILES_PER_TICK);
    let dx = i32::try_from(x * step / scale).map_err(|_| CorePrivateHallError::InvalidInput)?;
    let dy = i32::try_from(y * step / scale).map_err(|_| CorePrivateHallError::InvalidInput)?;
    Ok(SceneDisplacement::new(dx, dy))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CorePrivateHallError {
    #[error("Hall content is invalid")]
    Content,
    #[error("Hall directory is retired")]
    Retired,
    #[error("Hall actor generation is exhausted")]
    GenerationExhausted,
    #[error("Hall projection is invalid")]
    InvalidSnapshot,
    #[error("Hall authority is foreign")]
    ForeignAuthority,
    #[error("Hall actor is unavailable")]
    ActorUnavailable,
    #[error("Hall actor generation is stale")]
    StaleActor,
    #[error("Hall transport generation is stale")]
    StaleTransport,
    #[error("Hall input is invalid")]
    InvalidInput,
    #[error("Hall input is stale")]
    StaleInput,
    #[error("Hall authoritative tick is exhausted")]
    TickExhausted,
    #[error("Hall interaction frame or state is invalid")]
    InvalidInteraction,
    #[error("Hall observation could not be projected")]
    Observation,
    #[error("combat actions are forbidden in Lantern Halls")]
    UnsafeAction,
    #[error("Realm Gate interaction is out of range")]
    OutOfRange,
    #[error("Realm Gate panel authority is required")]
    PanelRequired,
    #[error("Realm Gate character version is stale")]
    VersionMismatch,
    #[error("Realm Gate mutation is invalid")]
    InvalidMutation,
    #[error("Realm Gate transfer is already in progress")]
    TransferInProgress,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AccountId;

    const ACCOUNT_ID: [u8; 16] = [1; 16];
    const CHARACTER_ID: [u8; 16] = [2; 16];

    fn content_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("content")
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new(ACCOUNT_ID).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn transport(generation: u64) -> CorePrivateLifeTransportLease {
        CorePrivateLifeTransportLease::test_only(ACCOUNT_ID, generation)
    }

    fn interaction(sequence: u32, intent: HallInteractionIntentV1) -> HallInteractionFrameV1 {
        HallInteractionFrameV1 {
            schema_version: HALL_INTERACTION_SCHEMA_VERSION,
            sequence,
            intent,
        }
    }

    #[test]
    fn realm_gate_requires_exact_range_and_reserves_the_actor() {
        let hall = CorePrivateHallDirectory::load(&content_root()).unwrap();
        let actor = hall.install_at(
            authenticated(),
            CHARACTER_ID,
            7,
            TilePoint::new(32_000, 4_500),
            transport(3).generation(),
        );
        assert_eq!(
            hall.handle_interaction(
                authenticated(),
                actor,
                transport(3),
                &interaction(1, HallInteractionIntentV1::BeginHold),
            )
            .unwrap()
            .code,
            HallInteractionResultCodeV1::Opened
        );
        let permit = hall
            .prepare_realm_gate(authenticated(), actor, transport(3), [9; 16], 7)
            .unwrap();
        assert_eq!(permit.character_id(), CHARACTER_ID);
        assert_eq!(permit.character_version(), 7);
        assert_eq!(permit.mutation_id(), [9; 16]);
        assert_eq!(
            hall.prepare_realm_gate(authenticated(), actor, transport(3), [8; 16], 7),
            Err(CorePrivateHallError::TransferInProgress)
        );
        hall.abort_realm_gate(permit).unwrap();

        let actor = hall.install_at(
            authenticated(),
            CHARACTER_ID,
            7,
            TilePoint::new(32_000, 4_501),
            transport(4).generation(),
        );
        assert_eq!(
            hall.prepare_realm_gate(authenticated(), actor, transport(4), [7; 16], 7),
            Err(CorePrivateHallError::PanelRequired)
        );
    }

    #[test]
    fn hall_input_is_generation_bound_bounded_and_noncombat() {
        let hall = CorePrivateHallDirectory::load(&content_root()).unwrap();
        let actor = hall.install_at(
            authenticated(),
            CHARACTER_ID,
            7,
            TilePoint::new(32_000, 42_000),
            transport(3).generation(),
        );
        let input = InputFrame {
            sequence: 1,
            client_tick: 1,
            movement_x_milli: 1_000,
            movement_y_milli: -1_000,
            aim_x_milli: 1,
            aim_y_milli: 0,
            held_primary: false,
            primary_sequence: 0,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        };
        let before = TilePoint::new(32_000, 42_000);
        assert_eq!(
            hall.apply_input(authenticated(), actor, transport(3), &input)
                .unwrap(),
            before
        );
        let tick = hall
            .advance_tick(authenticated(), actor, transport(3))
            .unwrap();
        let after = &tick.observation.entities[0];
        let after = TilePoint::new(after.x_milli_tiles, after.y_milli_tiles);
        let dx = i64::from(after.x_milli_tiles - before.x_milli_tiles);
        let dy = i64::from(after.y_milli_tiles - before.y_milli_tiles);
        assert!(dx * dx + dy * dy <= i64::from(HALL_MOVEMENT_MILLI_TILES_PER_TICK).pow(2));
        assert_eq!(
            hall.apply_input(authenticated(), actor, transport(3), &input),
            Err(CorePrivateHallError::StaleInput)
        );
        assert_eq!(
            hall.apply_input(
                authenticated(),
                actor,
                transport(2),
                &InputFrame {
                    sequence: 2,
                    ..input
                }
            ),
            Err(CorePrivateHallError::StaleTransport)
        );
        let combat = InputFrame {
            sequence: 2,
            client_tick: 2,
            held_primary: true,
            primary_sequence: 1,
            ..input
        };
        assert_eq!(
            hall.apply_input(authenticated(), actor, transport(3), &combat),
            Err(CorePrivateHallError::UnsafeAction)
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn exact_holds_cancel_on_release_range_loss_and_transport_replacement() {
        let hall = CorePrivateHallDirectory::load(&content_root()).unwrap();
        let actor = hall.install_at(
            authenticated(),
            CHARACTER_ID,
            7,
            TilePoint::new(10_000, 11_500),
            transport(3).generation(),
        );
        let started = hall
            .handle_interaction(
                authenticated(),
                actor,
                transport(3),
                &interaction(1, HallInteractionIntentV1::BeginHold),
            )
            .unwrap();
        assert_eq!(started.code, HallInteractionResultCodeV1::Holding);
        for held_tick in 1..HALL_INTERACTION_HOLD_TICKS {
            let progress = hall
                .advance_tick(authenticated(), actor, transport(3))
                .unwrap()
                .interaction
                .unwrap();
            assert_eq!(progress.code, HallInteractionResultCodeV1::Holding);
            assert_eq!(progress.held_ticks, held_tick);
        }
        let opened = hall
            .advance_tick(authenticated(), actor, transport(3))
            .unwrap()
            .interaction
            .unwrap();
        assert_eq!(opened.code, HallInteractionResultCodeV1::Opened);
        assert!(hall.panel_authorizes(
            authenticated(),
            actor,
            transport(3),
            HallStationV1::MemorialWall
        ));

        hall.handle_interaction(
            authenticated(),
            actor,
            transport(3),
            &interaction(2, HallInteractionIntentV1::ClosePanel),
        )
        .unwrap();
        hall.handle_interaction(
            authenticated(),
            actor,
            transport(3),
            &interaction(3, HallInteractionIntentV1::BeginHold),
        )
        .unwrap();
        let released = hall
            .handle_interaction(
                authenticated(),
                actor,
                transport(3),
                &interaction(4, HallInteractionIntentV1::Release),
            )
            .unwrap();
        assert_eq!(
            released.code,
            HallInteractionResultCodeV1::CancelledReleased
        );

        hall.handle_interaction(
            authenticated(),
            actor,
            transport(3),
            &interaction(5, HallInteractionIntentV1::BeginHold),
        )
        .unwrap();
        hall.apply_input(
            authenticated(),
            actor,
            transport(3),
            &InputFrame {
                sequence: 1,
                client_tick: 1,
                movement_x_milli: 0,
                movement_y_milli: 1_000,
                aim_x_milli: 1,
                aim_y_milli: 0,
                held_primary: false,
                primary_sequence: 0,
                ability_1_sequence: 0,
                ability_2_sequence: 0,
            },
        )
        .unwrap();
        let mut range_cancelled = false;
        for _ in 0..4 {
            let tick = hall
                .advance_tick(authenticated(), actor, transport(3))
                .unwrap();
            if let Some(cancelled) = tick.interaction {
                assert_eq!(
                    cancelled.code,
                    HallInteractionResultCodeV1::CancelledOutOfRange
                );
                range_cancelled = true;
                break;
            }
        }
        assert!(range_cancelled);

        hall.install_at(
            authenticated(),
            CHARACTER_ID,
            7,
            TilePoint::new(10_000, 11_500),
            transport(3).generation(),
        );
        hall.handle_interaction(
            authenticated(),
            actor,
            transport(3),
            &interaction(1, HallInteractionIntentV1::BeginHold),
        )
        .unwrap();
        hall.attach_transport(authenticated(), actor, transport(4))
            .unwrap();
        assert_eq!(
            hall.handle_interaction(
                authenticated(),
                actor,
                transport(4),
                &interaction(2, HallInteractionIntentV1::Release),
            )
            .unwrap()
            .code,
            HallInteractionResultCodeV1::NoActiveHold
        );
    }
}
