//! Route-bound owner for the exact M03 Bell Sepulcher B0-B6 combat lifecycle.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`DNG-003`-`006`,
//! `COM-001`-`006`, `BRG-001`-`002`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-ROOM-007`, `CONT-BOSS-001`-`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`). This component does not enable normal
//! admission, commit rewards, resolve Bargains, or create the B6 exit.

use protocol::{
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1,
    CorePrivateRouteSceneV1, CorePrivateRouteStateV1,
};
use sim_core::{
    FixedRoomPhase, PlayerMovementState, ProjectileCollisionWorld, Tick, TilePoint,
    simulation_to_tile_point,
};
use thiserror::Error;

use crate::{
    CoreBellPortalTransition, CoreCharacterCombatEnvelope, CoreDurableB3Resolution,
    CoreDurableBargainRestResolution, CorePrivateMicrorealmInput, CorePrivateMicrorealmRuntime,
    CorePrivateMicrorealmRuntimeError, CorePrivatePlayerDamageError, CorePrivatePlayerDamageFactV1,
    CorePrivateRouteActorDirectory, CorePrivateRouteActorLease, CorePrivateRouteRuntimeError,
    core_private_combat_frame::{
        CorePrivateConsumableAvailability, consumable_availability, core_player_movement_config,
        step_live_player_combat,
    },
    core_private_gameplay_observation::{
        CorePrivateGameplayObservation, CorePrivateGameplayObservationError,
        CorePrivateProjectileProvenance, combat_actor_binding, enemy_snapshot,
        hostile_projectile_snapshot, normal_wave_telegraphs, player_snapshot,
    },
    core_private_microrealm_runtime::CorePrivateMicrorealmDungeonHandoff,
    fixed_room_player_damage_facts,
};

#[derive(Debug, Clone, PartialEq)]
pub struct CorePrivateFixedDungeonRoomFrame {
    pub tick: Tick,
    pub route: CorePrivateRouteStateV1,
    pub step: sim_content::CoreFixedDungeonRoomStep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateFixedDungeonAdvance {
    pub route: CorePrivateRouteStateV1,
    pub transition: sim_content::CoreFixedDungeonAdvance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateFixedDungeonRestCommit {
    pub route: CorePrivateRouteStateV1,
    pub receipt: sim_content::CoreFixedDungeonRestReceipt,
    pub resolution: sim_content::CoreFixedDungeonRestResolution,
    pub source_receipt_id: [u8; 16],
    pub offer_id: Option<[u8; 16]>,
    pub oath_bargain_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateFixedDungeonB3RewardCommit {
    pub route: CorePrivateRouteStateV1,
    pub receipt: sim_content::CoreB3RewardReceipt,
    pub disposition: sim_content::CoreB3RewardDisposition,
    pub reward_event_id: [u8; 16],
    pub reward_result_hash: Option<[u8; 32]>,
    pub progression_payload_hash: [u8; 32],
    pub bargain_offer_id: Option<[u8; 16]>,
    pub has_no_offer_resolution: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorePrivateFixedDungeonLiveRoomFrame {
    pub input_sequence: u64,
    pub tick: Tick,
    pub player_position: TilePoint,
    pub movement: sim_core::MovementStep,
    pub combat: sim_core::CombatStep,
    pub(crate) observation: CorePrivateGameplayObservation,
    pub route: CorePrivateRouteStateV1,
    pub step: sim_content::CoreFixedDungeonRoomStep,
    pub player_damage: Vec<CorePrivatePlayerDamageFactV1>,
    pub player_died: bool,
}

/// Consuming B5-to-B6 transfer. The exact player, combat envelope, projectile allocator, route
/// lease, and inherited danger tick cross together; no caller can rebuild or partially clone the
/// boss participant.
#[derive(Debug)]
pub struct CorePrivateCaldusStagingHandoff {
    pub(crate) route_directory: CorePrivateRouteActorDirectory,
    pub(crate) route_lease: CorePrivateRouteActorLease,
    pub(crate) content_revision: CorePrivateRouteContentRevisionV1,
    pub(crate) entry_restore_point_id: [u8; 16],
    pub(crate) combat_envelope: CoreCharacterCombatEnvelope,
    pub(crate) participant: sim_core::NormalWaveHandoff,
    pub(crate) arena: sim_core::ArenaGeometry,
    pub(crate) tick: Tick,
    pub(crate) last_reward_activity_sequence: u64,
    pub(crate) projectile_provenance: CorePrivateProjectileProvenance,
}

impl CorePrivateCaldusStagingHandoff {
    #[must_use]
    pub const fn route_lease(&self) -> CorePrivateRouteActorLease {
        self.route_lease
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn content_revision(&self) -> &CorePrivateRouteContentRevisionV1 {
        &self.content_revision
    }

    #[must_use]
    pub const fn entry_restore_point_id(&self) -> [u8; 16] {
        self.entry_restore_point_id
    }

    #[must_use]
    pub const fn combat_envelope(&self) -> &CoreCharacterCombatEnvelope {
        &self.combat_envelope
    }

    pub fn route_snapshot(&self) -> Result<CorePrivateRouteStateV1, CorePrivateRouteRuntimeError> {
        self.route_directory.snapshot(self.route_lease)
    }

    #[must_use]
    pub fn player(&self) -> &sim_core::EnemyLabPlayer {
        &self.participant.player
    }

    #[must_use]
    pub const fn arena(&self) -> &sim_core::ArenaGeometry {
        &self.arena
    }
}

#[derive(Debug)]
pub struct CorePrivateFixedDungeonRuntime {
    route_directory: CorePrivateRouteActorDirectory,
    route_lease: CorePrivateRouteActorLease,
    content_revision: CorePrivateRouteContentRevisionV1,
    entry_restore_point_id: [u8; 16],
    combat_envelope: CoreCharacterCombatEnvelope,
    combat: sim_content::CoreFixedDungeonCombat,
    movement: Option<PlayerMovementState>,
    tick: Tick,
    last_reward_activity_sequence: u64,
    projectile_provenance: CorePrivateProjectileProvenance,
}

impl CorePrivateFixedDungeonRuntime {
    pub(crate) fn consumable_availability(&self) -> [CorePrivateConsumableAvailability; 2] {
        consumable_availability(
            &self
                .combat
                .player()
                .expect("validated fixed dungeon retains its player")
                .consumables,
        )
    }
    pub fn from_committed_bell(
        microrealm: CorePrivateMicrorealmRuntime,
        transition: &CoreBellPortalTransition,
        expected_content_revision: &CorePrivateRouteContentRevisionV1,
        encounters: sim_content::CoreDevelopmentEncounterRooms,
    ) -> Result<Self, CorePrivateFixedDungeonRuntimeError> {
        let handoff = microrealm.into_fixed_dungeon_handoff(transition)?;
        Self::from_handoff(handoff, expected_content_revision, encounters)
    }

    fn from_handoff(
        handoff: CorePrivateMicrorealmDungeonHandoff,
        expected_content_revision: &CorePrivateRouteContentRevisionV1,
        encounters: sim_content::CoreDevelopmentEncounterRooms,
    ) -> Result<Self, CorePrivateFixedDungeonRuntimeError> {
        let route = handoff.route_directory.snapshot(handoff.route_lease)?;
        if route.content_revision != *expected_content_revision
            || route.character_id != handoff.combat_envelope.character_id()
            || route.character_version != handoff.combat_envelope.character_state_version()
            || route.actor_generation != handoff.route_lease.actor_generation()
            || route.scene != CorePrivateRouteSceneV1::BellSepulcher
            || route.room != Some(CorePrivateRouteRoomV1::BellVestibuleB0)
            || route.phase != CorePrivateRoutePhaseV1::DungeonVestibule
            || route.instance_lineage_id.is_none()
        {
            return Err(CorePrivateFixedDungeonRuntimeError::InvalidComposition);
        }
        let run_ordinal = u32::try_from(route.actor_generation)
            .map_err(|_| CorePrivateFixedDungeonRuntimeError::InvalidComposition)?;
        let combat = sim_content::CoreFixedDungeonCombat::from_handoff_at(
            encounters,
            run_ordinal,
            handoff.next_hostile_spawn_ordinal,
            handoff.participant,
        )?;
        Ok(Self {
            route_directory: handoff.route_directory,
            route_lease: handoff.route_lease,
            content_revision: expected_content_revision.clone(),
            entry_restore_point_id: handoff.entry_restore_point_id,
            combat_envelope: handoff.combat_envelope,
            combat,
            movement: None,
            tick: handoff.final_tick,
            last_reward_activity_sequence: 0,
            projectile_provenance: handoff.projectile_provenance,
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
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn node(&self) -> sim_content::CoreFixedDungeonNode {
        self.combat.node()
    }

    pub fn route_snapshot(
        &self,
    ) -> Result<CorePrivateRouteStateV1, CorePrivateFixedDungeonRuntimeError> {
        let route = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route)?;
        Ok(route)
    }

    #[must_use]
    pub fn room_phase(&self) -> Option<FixedRoomPhase> {
        self.combat.room_phase()
    }

    #[must_use]
    pub fn pending_b3_reward_handoff(&self) -> Option<&sim_content::CoreB3RewardHandoff> {
        self.combat.pending_b3_reward_handoff()
    }

    pub fn into_caldus_staging_handoff(
        self,
    ) -> Result<CorePrivateCaldusStagingHandoff, CorePrivateFixedDungeonRuntimeError> {
        let route = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route)?;
        if self.combat.node() != sim_content::CoreFixedDungeonNode::CaldusArenaB6
            || self.movement.is_some()
            || route.phase != CorePrivateRoutePhaseV1::BossStaging
        {
            return Err(CorePrivateFixedDungeonRuntimeError::InvalidComposition);
        }
        let arena = self
            .combat
            .arena()
            .cloned()
            .ok_or(CorePrivateFixedDungeonRuntimeError::InvalidComposition)?;
        let participant = self.combat.into_boss_handoff()?;
        Ok(CorePrivateCaldusStagingHandoff {
            route_directory: self.route_directory,
            route_lease: self.route_lease,
            content_revision: self.content_revision,
            entry_restore_point_id: self.entry_restore_point_id,
            combat_envelope: self.combat_envelope,
            participant,
            arena,
            tick: self.tick,
            last_reward_activity_sequence: self.last_reward_activity_sequence,
            projectile_provenance: self.projectile_provenance,
        })
    }

    pub async fn advance(
        &mut self,
    ) -> Result<CorePrivateFixedDungeonAdvance, CorePrivateFixedDungeonRuntimeError> {
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        let mut staged = self.combat.clone();
        let transition = staged.advance()?;
        let movement = movement_for_combat(&staged, &self.combat_envelope)?;
        let (room, phase) = route_position(transition.to, staged.room_phase())?;
        let route = self
            .route_directory
            .apply_fixed_dungeon_authority(
                self.route_lease,
                route_before.state_version,
                room,
                phase,
            )
            .await?;
        self.combat = staged;
        self.movement = movement;
        Ok(CorePrivateFixedDungeonAdvance { route, transition })
    }

    /// Applies only an opaque result produced from committed Bargain persistence. The receipt is
    /// bound to this account, character, and dangerous-instance lineage; the route actor CAS makes
    /// the first local application atomic with B4 authority. Exact retries are read-only replays.
    pub async fn resolve_rest(
        &mut self,
        durable: &CoreDurableBargainRestResolution,
    ) -> Result<CorePrivateFixedDungeonRestCommit, CorePrivateFixedDungeonRuntimeError> {
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        if self.combat.node() != sim_content::CoreFixedDungeonNode::BellRestB4
            || durable.account_id() != self.account_id()
            || durable.character_id() != self.character_id()
            || route_before.instance_lineage_id != Some(durable.instance_lineage_id())
        {
            return Err(CorePrivateFixedDungeonRuntimeError::BargainAuthorityMismatch);
        }
        let mut staged = self.combat.clone();
        let receipt = staged.resolve_rest(durable.resolution())?;
        let route = if receipt == sim_content::CoreFixedDungeonRestReceipt::Committed {
            self.route_directory
                .apply_fixed_dungeon_authority(
                    self.route_lease,
                    route_before.state_version,
                    CorePrivateRouteRoomV1::BellRestB4,
                    CorePrivateRoutePhaseV1::Rest,
                )
                .await?
        } else {
            route_before
        };
        self.combat = staged;
        Ok(CorePrivateFixedDungeonRestCommit {
            route,
            receipt,
            resolution: durable.resolution(),
            source_receipt_id: durable.source_receipt_id(),
            offer_id: durable.offer_id(),
            oath_bargain_version: durable.oath_bargain_version(),
        })
    }

    /// Applies only the opaque result returned after both the item reward and progression/
    /// milestone terminal are durable. The exact simulation handoff remains the comparison key;
    /// first application shares the route CAS, while exact retry is read-only.
    pub async fn commit_b3_reward(
        &mut self,
        durable: &CoreDurableB3Resolution,
    ) -> Result<CorePrivateFixedDungeonB3RewardCommit, CorePrivateFixedDungeonRuntimeError> {
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        if self.combat.node() != sim_content::CoreFixedDungeonNode::BellKnightB3
            || durable.account_id() != self.account_id()
            || durable.character_id() != self.character_id()
            || route_before.instance_lineage_id != Some(durable.instance_lineage_id())
            || self.tick.0 < durable.handoff().reward_due_tick.0
        {
            return Err(CorePrivateFixedDungeonRuntimeError::B3RewardAuthorityMismatch);
        }
        let mut staged = self.combat.clone();
        let mut staged_envelope = self.combat_envelope.clone();
        let disposition = durable.disposition();
        let receipt = staged.acknowledge_b3_reward(durable.handoff(), disposition)?;
        if let Some(projection) = &durable.progression().projection {
            staged_envelope.reconcile_progression_version(
                projection.character_id,
                projection.progression_version,
            )?;
        }
        let route = if receipt == sim_content::CoreB3RewardReceipt::Committed {
            let (room, phase) = route_position(staged.node(), staged.room_phase())?;
            self.route_directory
                .apply_fixed_dungeon_authority(
                    self.route_lease,
                    route_before.state_version,
                    room,
                    phase,
                )
                .await?
        } else {
            route_before
        };
        self.combat = staged;
        self.combat_envelope = staged_envelope;
        Ok(CorePrivateFixedDungeonB3RewardCommit {
            route,
            receipt,
            disposition,
            reward_event_id: durable.reward_event_id(),
            reward_result_hash: durable.reward_result_hash(),
            progression_payload_hash: durable.progression_payload_hash(),
            bargain_offer_id: durable.bargain_offer_id(),
            has_no_offer_resolution: durable.has_no_offer_resolution(),
        })
    }

    /// Generates one complete room frame from retained player intent. Movement, player attacks,
    /// hostile room simulation, lifecycle, route CAS, and local state commit share one staged
    /// transaction; client input cannot author combat results or room authority.
    #[expect(
        clippy::too_many_lines,
        reason = "the fixed-room transaction keeps movement, combat, route, and presentation ordering auditable"
    )]
    pub async fn step_live_room(
        &mut self,
        input: CorePrivateMicrorealmInput,
    ) -> Result<CorePrivateFixedDungeonLiveRoomFrame, CorePrivateFixedDungeonRuntimeError> {
        let tick = self
            .tick
            .checked_next()
            .ok_or(CorePrivateFixedDungeonRuntimeError::TickExhausted)?;
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        if input.consumable_inventory_version != 0 {
            self.combat_envelope
                .reconcile_inventory_version(input.consumable_inventory_version)?;
        }
        let mut staged_combat = self.combat.clone();
        let mut staged_projectile_provenance = self.projectile_provenance.clone();
        let mut staged_movement = self
            .movement
            .ok_or(CorePrivateFixedDungeonRuntimeError::RoomMovementUnavailable)?;
        let arena = staged_combat
            .arena()
            .cloned()
            .ok_or(CorePrivateFixedDungeonRuntimeError::RoomMovementUnavailable)?;
        let collision_world =
            ProjectileCollisionWorld::new(&arena, staged_combat.alive_hurtboxes()?)?;
        let (combat_step, movement_step) = step_live_player_combat(
            staged_combat.player_mut()?,
            &mut staged_movement,
            &input,
            &arena,
            &collision_world,
        )?;
        if combat_step.tick != tick {
            return Err(CorePrivateFixedDungeonRuntimeError::CombatTickMismatch);
        }
        let living_inside = u16::from(
            staged_combat
                .player()?
                .consumables
                .vitals()
                .current_health()
                != 0,
        );
        let (reward_participation, reward_trust_state) =
            fixed_room_reward_authority(&input, living_inside, self.last_reward_activity_sequence)?;
        let room_input = sim_content::CoreImmutableFixedRoomInput {
            crossed_activation_boundary: matches!(
                staged_combat.room_phase(),
                Some(FixedRoomPhase::Dormant)
            ),
            living_inside,
            living_party_outside: 0,
            doorway_hurtbox_blocked: false,
            reward_life_state: if living_inside > 0 {
                sim_core::RewardLifeState::Living
            } else {
                sim_core::RewardLifeState::Dead
            },
            // A completed Recall retires this sole danger task before another room frame can run.
            reward_recall_state: sim_core::RewardRecallState::Eligible,
            reward_trust_state,
            reward_participation,
            combat_step: Some(combat_step.clone()),
        };
        let step = staged_combat.step_room(tick, &room_input)?;
        staged_projectile_provenance
            .apply_committed_combat(&combat_step, staged_combat.player()?.combat.projectiles())?;
        let player = staged_combat.player()?;
        let player_position = simulation_to_tile_point(player.target.position)?;
        let player_died = player.consumables.vitals().current_health() == 0;
        let player_damage =
            fixed_room_player_damage_facts(&step, player.target.entity_id, player_died)?;
        let (room, phase) = route_position(staged_combat.node(), Some(step.phase_after()))?;
        let mut observation = project_fixed_observation(
            tick,
            &route_before,
            input.input_sequence,
            &staged_combat,
            &step,
            movement_step,
            &staged_projectile_provenance,
        )?;
        let route = self
            .route_directory
            .apply_fixed_dungeon_authority(
                self.route_lease,
                route_before.state_version,
                room,
                phase,
            )
            .await?;
        observation.route_state_version = route.state_version;
        self.combat = staged_combat;
        self.projectile_provenance = staged_projectile_provenance;
        self.movement = Some(staged_movement);
        self.tick = tick;
        self.last_reward_activity_sequence = input.reward_activity_sequence;
        Ok(CorePrivateFixedDungeonLiveRoomFrame {
            input_sequence: input.input_sequence,
            tick,
            player_position,
            movement: movement_step,
            combat: combat_step,
            observation,
            route,
            step,
            player_damage,
            player_died,
        })
    }

    pub async fn step_room(
        &mut self,
        input: &sim_content::CoreImmutableFixedRoomInput,
    ) -> Result<CorePrivateFixedDungeonRoomFrame, CorePrivateFixedDungeonRuntimeError> {
        let tick = self
            .tick
            .checked_next()
            .ok_or(CorePrivateFixedDungeonRuntimeError::TickExhausted)?;
        if input
            .combat_step
            .as_ref()
            .is_some_and(|combat| combat.tick != tick)
        {
            return Err(CorePrivateFixedDungeonRuntimeError::CombatTickMismatch);
        }
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        let mut staged = self.combat.clone();
        let step = staged.step_room(tick, input)?;
        let (room, phase) = route_position(staged.node(), Some(step.phase_after()))?;
        let route = self
            .route_directory
            .apply_fixed_dungeon_authority(
                self.route_lease,
                route_before.state_version,
                room,
                phase,
            )
            .await?;
        self.combat = staged;
        self.tick = tick;
        Ok(CorePrivateFixedDungeonRoomFrame { tick, route, step })
    }

    fn validate_route_authority(
        &self,
        route: &CorePrivateRouteStateV1,
    ) -> Result<(), CorePrivateFixedDungeonRuntimeError> {
        let (room, phase) = route_position(self.combat.node(), self.combat.room_phase())?;
        if route.character_id != self.combat_envelope.character_id()
            || route.character_version != self.combat_envelope.character_state_version()
            || route.content_revision != self.content_revision
            || route.actor_generation != self.route_lease.actor_generation()
            || route.scene != CorePrivateRouteSceneV1::BellSepulcher
            || route.room != Some(room)
            || route.phase != phase
            || route.instance_lineage_id.is_none()
        {
            return Err(CorePrivateFixedDungeonRuntimeError::RouteAuthorityMismatch);
        }
        Ok(())
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "all fixed-room entity and telegraph authority is projected from one staged frame"
)]
fn project_fixed_observation(
    tick: Tick,
    route: &CorePrivateRouteStateV1,
    input_sequence: u64,
    combat: &sim_content::CoreFixedDungeonCombat,
    step: &sim_content::CoreFixedDungeonRoomStep,
    movement: sim_core::MovementStep,
    projectile_provenance: &CorePrivateProjectileProvenance,
) -> Result<CorePrivateGameplayObservation, CorePrivateFixedDungeonRuntimeError> {
    let player = combat.player()?;
    let player_id = player.target.entity_id;
    let mut actors = vec![combat_actor_binding(
        player_id,
        protocol::CoreCombatActorKindV1::Player,
        protocol::GRAVE_ARBALIST_CLASS_ID,
    )?];
    let mut entities = vec![player_snapshot(
        player,
        movement.position,
        movement.velocity,
    )?];
    for projectile in player.combat.projectiles() {
        entities.push(projectile_provenance.friendly_snapshot(player_id, projectile)?);
    }
    let presentation = combat.presentation()?;
    for enemy in presentation.enemies {
        actors.push(combat_actor_binding(
            enemy.entity_id,
            protocol::CoreCombatActorKindV1::Enemy,
            enemy.content_id,
        )?);
        entities.push(enemy_snapshot(
            enemy.entity_id,
            enemy.position,
            enemy.current_health,
            enemy.maximum_health,
            true,
        )?);
    }
    for projectile in &presentation.hostile_projectiles {
        entities.push(hostile_projectile_snapshot(projectile)?);
    }
    let normal_step = match step {
        sim_content::CoreFixedDungeonRoomStep::B1(step)
        | sim_content::CoreFixedDungeonRoomStep::B5(step) => step.wave_step.as_ref(),
        sim_content::CoreFixedDungeonRoomStep::B2(step) => {
            step.combat.as_ref().map(|combat| &combat.immutable_wave)
        }
        sim_content::CoreFixedDungeonRoomStep::B3(_) => None,
    };
    let mut telegraphs = normal_wave_telegraphs(normal_step, &entities)?;
    match step {
        sim_content::CoreFixedDungeonRoomStep::B2(step) => {
            for actor in step
                .combat
                .iter()
                .flat_map(|combat| &combat.authored_actors)
            {
                for event in &actor.attack_events {
                    let sim_core::CoreNormalAttackEvent::TelegraphStarted {
                        lock,
                        fan_offsets_milli_degrees,
                        ..
                    } = event
                    else {
                        continue;
                    };
                    let origin = lock.origin();
                    let target = lock.target().map_or(origin, |target| target.position);
                    telegraphs.push(protocol::CoreCombatTelegraphV1 {
                        source_entity_id: actor.entity_id.get(),
                        cast_id: lock.cast_id().get(),
                        pattern_id: protocol::WireText::new(lock.pattern_id().to_owned())
                            .map_err(|_| CorePrivateFixedDungeonRuntimeError::InvalidComposition)?,
                        damage_type: fixed_telegraph_damage_type(lock.pattern_id())?,
                        starts_at_tick: lock.telegraph_started_at().0,
                        resolves_at_tick: lock.resolves_at().0,
                        origin_x_milli_tiles: origin.x_milli_tiles,
                        origin_y_milli_tiles: origin.y_milli_tiles,
                        target_x_milli_tiles: target.x_milli_tiles,
                        target_y_milli_tiles: target.y_milli_tiles,
                        shape: normal_attack_telegraph_shape(
                            lock.pattern_id(),
                            fan_offsets_milli_degrees.as_deref(),
                        )?,
                    });
                }
            }
        }
        sim_content::CoreFixedDungeonRoomStep::B3(step) => {
            for event in step
                .combat
                .iter()
                .flat_map(|combat| combat.knight.iter())
                .flat_map(|knight| &knight.events)
            {
                let sim_core::CoreKnightEvent::TelegraphStarted { lock, .. } = event else {
                    continue;
                };
                let origin = lock.origin();
                let target = lock.target().position;
                telegraphs.push(protocol::CoreCombatTelegraphV1 {
                    source_entity_id: step
                        .combat
                        .as_ref()
                        .map(|combat| combat.actor_id.get())
                        .ok_or(CorePrivateFixedDungeonRuntimeError::InvalidComposition)?,
                    cast_id: lock.cast_id().get(),
                    pattern_id: protocol::WireText::new(lock.pattern_id().to_owned())
                        .map_err(|_| CorePrivateFixedDungeonRuntimeError::InvalidComposition)?,
                    damage_type: protocol::CoreCombatDamageTypeV1::Physical,
                    starts_at_tick: lock.telegraph_started_at().0,
                    resolves_at_tick: lock.resolves_at().0,
                    origin_x_milli_tiles: origin.x_milli_tiles,
                    origin_y_milli_tiles: origin.y_milli_tiles,
                    target_x_milli_tiles: target.x_milli_tiles,
                    target_y_milli_tiles: target.y_milli_tiles,
                    shape: knight_telegraph_shape(lock.pattern_id())?,
                });
            }
        }
        sim_content::CoreFixedDungeonRoomStep::B1(_)
        | sim_content::CoreFixedDungeonRoomStep::B5(_) => {}
    }
    CorePrivateGameplayObservation::new(
        tick.0,
        route.actor_generation,
        route.state_version,
        input_sequence,
        entities,
    )?
    .with_presentation(actors, telegraphs)
    .map_err(Into::into)
}

fn fixed_telegraph_damage_type(
    pattern_id: &str,
) -> Result<protocol::CoreCombatDamageTypeV1, CorePrivateFixedDungeonRuntimeError> {
    match pattern_id {
        "pattern.enemy.bell_acolyte.alternating_fan" | "pattern.enemy.choir_skull.rotor" => {
            Ok(protocol::CoreCombatDamageTypeV1::Veil)
        }
        _ => Err(CorePrivateFixedDungeonRuntimeError::InvalidComposition),
    }
}

fn normal_attack_telegraph_shape(
    pattern_id: &str,
    fan_offsets_milli_degrees: Option<&[i32]>,
) -> Result<protocol::CoreCombatTelegraphShapeV1, CorePrivateFixedDungeonRuntimeError> {
    match pattern_id {
        "pattern.enemy.bell_acolyte.alternating_fan" => {
            Ok(protocol::CoreCombatTelegraphShapeV1::Fan {
                ray_count: 5,
                ray_offsets_milli_degrees: fan_offsets(
                    fan_offsets_milli_degrees
                        .ok_or(CorePrivateFixedDungeonRuntimeError::InvalidComposition)?,
                )?,
                extent_milli_tiles: 9_000,
                ray_width_milli_tiles: 220,
            })
        }
        "pattern.enemy.choir_skull.rotor" => Ok(protocol::CoreCombatTelegraphShapeV1::Rotor {
            arm_count: 2,
            clockwise_milli_degrees_per_second: 35_000,
            extent_milli_tiles: 7_000,
            arm_width_milli_tiles: 240,
        }),
        _ => Err(CorePrivateFixedDungeonRuntimeError::InvalidComposition),
    }
}

fn knight_telegraph_shape(
    pattern_id: &str,
) -> Result<protocol::CoreCombatTelegraphShapeV1, CorePrivateFixedDungeonRuntimeError> {
    match pattern_id {
        "miniboss.sepulcher_knight.charge_lane" => {
            Ok(protocol::CoreCombatTelegraphShapeV1::AimedLane {
                extent_milli_tiles: 5_000,
                width_milli_tiles: 1_000,
            })
        }
        "miniboss.sepulcher_knight.shield_fan" => Ok(protocol::CoreCombatTelegraphShapeV1::Fan {
            ray_count: 5,
            ray_offsets_milli_degrees: [-25_000, -12_500, 0, 12_500, 25_000, 0, 0, 0],
            extent_milli_tiles: 8_000,
            ray_width_milli_tiles: 240,
        }),
        _ => Err(CorePrivateFixedDungeonRuntimeError::InvalidComposition),
    }
}

fn fan_offsets(offsets: &[i32]) -> Result<[i32; 8], CorePrivateFixedDungeonRuntimeError> {
    if offsets.len() > 8 {
        return Err(CorePrivateFixedDungeonRuntimeError::InvalidComposition);
    }
    let mut result = [0; 8];
    result[..offsets.len()].copy_from_slice(offsets);
    Ok(result)
}

fn movement_for_combat(
    combat: &sim_content::CoreFixedDungeonCombat,
    envelope: &CoreCharacterCombatEnvelope,
) -> Result<Option<PlayerMovementState>, CorePrivateFixedDungeonRuntimeError> {
    if combat.node() == sim_content::CoreFixedDungeonNode::CaldusArenaB6 {
        return Ok(None);
    }
    let Some(arena) = combat.arena() else {
        return Ok(None);
    };
    let config = core_player_movement_config(
        envelope.movement_milli_tiles_per_second(),
        sim_core::PLAYER_COLLISION_RADIUS_MILLI_TILES,
    )?;
    let movement =
        PlayerMovementState::new_with_config(combat.player()?.target.position, config, arena)?;
    Ok(Some(movement))
}

fn fixed_room_reward_authority(
    input: &CorePrivateMicrorealmInput,
    living_inside: u16,
    last_activity_sequence: u64,
) -> Result<
    (
        sim_content::CoreRewardParticipation,
        sim_core::RewardTrustState,
    ),
    CorePrivateFixedDungeonRuntimeError,
> {
    if input.reward_activity_sequence < last_activity_sequence {
        return Err(CorePrivateFixedDungeonRuntimeError::RewardActivitySequenceRegressed);
    }
    let present = living_inside > 0 && input.reward_session_active;
    let active = present
        && (input.reward_activity_sequence > last_activity_sequence
            || input.movement != sim_core::MovementAction::default()
            || input.primary_held);
    let participation = if active {
        sim_content::CoreRewardParticipation::PresentActive
    } else if present {
        sim_content::CoreRewardParticipation::PresentInactive
    } else {
        sim_content::CoreRewardParticipation::Absent
    };
    let trust = if input.reward_session_active && input.reward_trust_valid {
        sim_core::RewardTrustState::Valid
    } else {
        sim_core::RewardTrustState::InvalidSession
    };
    Ok((participation, trust))
}

fn route_position(
    node: sim_content::CoreFixedDungeonNode,
    room_phase: Option<FixedRoomPhase>,
) -> Result<(CorePrivateRouteRoomV1, CorePrivateRoutePhaseV1), CorePrivateFixedDungeonRuntimeError>
{
    use sim_content::CoreFixedDungeonNode as Node;
    match node {
        Node::BellVestibuleB0 => Ok((
            CorePrivateRouteRoomV1::BellVestibuleB0,
            CorePrivateRoutePhaseV1::DungeonVestibule,
        )),
        Node::BellCrossB1 => combat_route_position(CorePrivateRouteRoomV1::BellCrossB1, room_phase),
        Node::BellNaveB2 => combat_route_position(CorePrivateRouteRoomV1::BellNaveB2, room_phase),
        Node::BellKnightB3 => {
            combat_route_position(CorePrivateRouteRoomV1::BellKnightB3, room_phase)
        }
        Node::BellRestB4 => Ok((
            CorePrivateRouteRoomV1::BellRestB4,
            CorePrivateRoutePhaseV1::Rest,
        )),
        Node::BellBridgeB5 => {
            combat_route_position(CorePrivateRouteRoomV1::BellBridgeB5, room_phase)
        }
        Node::CaldusArenaB6 => Ok((
            CorePrivateRouteRoomV1::CaldusArenaB6,
            CorePrivateRoutePhaseV1::BossStaging,
        )),
    }
}

fn combat_route_position(
    room: CorePrivateRouteRoomV1,
    phase: Option<FixedRoomPhase>,
) -> Result<(CorePrivateRouteRoomV1, CorePrivateRoutePhaseV1), CorePrivateFixedDungeonRuntimeError>
{
    let phase = match phase.ok_or(CorePrivateFixedDungeonRuntimeError::InvalidComposition)? {
        FixedRoomPhase::Dormant => CorePrivateRoutePhaseV1::RoomDormant,
        FixedRoomPhase::AwaitingDoorSafety => CorePrivateRoutePhaseV1::RoomAwaitingDoorSafety,
        FixedRoomPhase::SpawnWarning => CorePrivateRoutePhaseV1::RoomSpawnWarning,
        FixedRoomPhase::Active => CorePrivateRoutePhaseV1::RoomActive,
        FixedRoomPhase::Quiet => CorePrivateRoutePhaseV1::RoomQuiet,
        FixedRoomPhase::Cleared => CorePrivateRoutePhaseV1::RoomCleared,
    };
    Ok((room, phase))
}

#[derive(Debug, Error)]
pub enum CorePrivateFixedDungeonRuntimeError {
    #[error("live Core fixed-dungeon composition is invalid")]
    InvalidComposition,
    #[error("live Core fixed-dungeon route authority no longer matches local state")]
    RouteAuthorityMismatch,
    #[error("durable Bargain result does not belong to this B4 route authority")]
    BargainAuthorityMismatch,
    #[error("durable B3 reward result does not belong to this Sepulcher Knight route authority")]
    B3RewardAuthorityMismatch,
    #[error("live Core fixed-dungeon run-local tick exhausted")]
    TickExhausted,
    #[error("live Core fixed-dungeon combat tick does not match the server-owned frame")]
    CombatTickMismatch,
    #[error("live Core fixed-dungeon reward activity sequence regressed")]
    RewardActivitySequenceRegressed,
    #[error("live Core fixed-dungeon room movement is unavailable")]
    RoomMovementUnavailable,
    #[error(transparent)]
    Movement(#[from] sim_core::MovementError),
    #[error(transparent)]
    Collision(#[from] sim_core::CollisionError),
    #[error(transparent)]
    Microrealm(#[from] CorePrivateMicrorealmRuntimeError),
    #[error(transparent)]
    CombatFactory(#[from] crate::CoreCombatFactoryError),
    #[error(transparent)]
    Dungeon(#[from] sim_content::CoreFixedDungeonError),
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

    use protocol::{ManifestHash, WorldFlowContentRevisionV1};
    use sim_core::{
        AimDirection, CollisionTarget, CombatStep, EntityId, EntityIdAllocator,
        FriendlyProjectileSource, MovementAction, ProjectileCollision, RawDamageIntent,
        RawDamageIntentSource, SimulationVector,
    };

    use super::*;
    use crate::{
        AccountId, AuthenticatedAccount, AuthenticatedNamespace, CorePrivateRouteActorSeed,
    };

    const ACCOUNT_ID: [u8; 16] = [0x31; 16];
    const CHARACTER_ID: [u8; 16] = [0x32; 16];
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

    fn fixture() -> (
        CorePrivateRouteActorDirectory,
        CorePrivateRouteActorLease,
        CorePrivateFixedDungeonRuntime,
    ) {
        fixture_at(Tick(32))
    }

    fn fixture_at(
        final_tick: Tick,
    ) -> (
        CorePrivateRouteActorDirectory,
        CorePrivateRouteActorLease,
        CorePrivateFixedDungeonRuntime,
    ) {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(
                authenticated(),
                CorePrivateRouteActorSeed {
                    character_id: CHARACTER_ID,
                    character_version: 3,
                    content_revision: route_revision(),
                    world_flow_revision: world_revision(),
                    position: crate::CorePrivateRouteActorPosition {
                        instance_lineage_id: Some(LINEAGE_ID),
                        scene: CorePrivateRouteSceneV1::BellSepulcher,
                        room: Some(CorePrivateRouteRoomV1::BellVestibuleB0),
                        phase: CorePrivateRoutePhaseV1::DungeonVestibule,
                    },
                },
                7,
            )
            .expect("route actor");
        let combat = crate::combat_factory::core_character_combat_test_fixture(CHARACTER_ID);
        let player_id = EntityId::new(710_000).expect("player ID");
        let (mut envelope, player) = combat
            .into_live_player(player_id, SimulationVector::new(8.5, 40.5))
            .expect("live player");
        envelope
            .rebase_character_state_version(2, 3)
            .expect("Bell version rebase");
        let handoff = CorePrivateMicrorealmDungeonHandoff {
            route_directory: directory.clone(),
            route_lease: lease,
            entry_restore_point_id: [0x74; 16],
            combat_envelope: envelope,
            participant: sim_core::NormalWaveHandoff {
                player,
                hostile_projectile_ids: EntityIdAllocator::starting_at(
                    NonZeroU64::new(900_000).expect("projectile allocator"),
                ),
            },
            next_hostile_spawn_ordinal: 9,
            final_tick,
            projectile_provenance: CorePrivateProjectileProvenance::default(),
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let encounters =
            sim_content::load_core_development_encounter_rooms(&root).expect("Core encounters");
        let runtime =
            CorePrivateFixedDungeonRuntime::from_handoff(handoff, &route_revision(), encounters)
                .expect("fixed dungeon runtime");
        (directory, lease, runtime)
    }

    fn live_input(sequence: u64) -> CorePrivateMicrorealmInput {
        CorePrivateMicrorealmInput {
            input_sequence: sequence,
            movement: MovementAction::default(),
            aim: AimDirection::east(),
            primary_held: false,
            primary_sequence: 0,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
            consumable_slot_one_sequence: 0,
            consumable_slot_two_sequence: 0,
            consumable_inventory_version: 0,
            reward_session_active: true,
            reward_trust_valid: true,
            reward_activity_sequence: sequence.max(1),
        }
    }

    fn room_input(tick: Tick, crossed: bool) -> sim_content::CoreImmutableFixedRoomInput {
        sim_content::CoreImmutableFixedRoomInput {
            crossed_activation_boundary: crossed,
            living_inside: 1,
            living_party_outside: 0,
            doorway_hurtbox_blocked: false,
            reward_life_state: sim_core::RewardLifeState::Living,
            reward_recall_state: sim_core::RewardRecallState::Eligible,
            reward_trust_state: sim_core::RewardTrustState::Valid,
            reward_participation: sim_content::CoreRewardParticipation::PresentActive,
            combat_step: Some(CombatStep {
                tick,
                ..CombatStep::default()
            }),
        }
    }

    fn lethal_step(tick: Tick, targets: &[EntityId]) -> CombatStep {
        let mut combat = CombatStep {
            tick,
            ..CombatStep::default()
        };
        for (index, target) in targets.iter().copied().enumerate() {
            let projectile_id = EntityId::new(60_000 + u64::try_from(index).unwrap()).unwrap();
            combat.collisions.push(ProjectileCollision {
                tick,
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
        combat
    }

    async fn clear_current_room(
        runtime: &mut CorePrivateFixedDungeonRuntime,
    ) -> Option<sim_content::CoreB3RewardHandoff> {
        let crossed = runtime.tick().checked_next().unwrap();
        runtime
            .step_room(&room_input(crossed, true))
            .await
            .expect("participant lock");
        let warning = runtime.tick().checked_next().unwrap();
        runtime
            .step_room(&room_input(warning, false))
            .await
            .expect("warning");
        let delay = match runtime.node() {
            sim_content::CoreFixedDungeonNode::BellNaveB2 => 57,
            sim_content::CoreFixedDungeonNode::BellKnightB3 => 90,
            sim_content::CoreFixedDungeonNode::BellCrossB1
            | sim_content::CoreFixedDungeonNode::BellBridgeB5 => 27,
            _ => panic!("not a combat room"),
        };
        for _ in 1..delay {
            let tick = runtime.tick().checked_next().unwrap();
            runtime
                .step_room(&room_input(tick, false))
                .await
                .expect("warning progression");
        }
        let tick = runtime.tick().checked_next().unwrap();
        let targets = runtime.combat.hostile_entity_ids();
        let mut input = room_input(tick, false);
        input.combat_step = Some(lethal_step(tick, &targets));
        let clear = runtime.step_room(&input).await.expect("clear room");
        assert_eq!(clear.step.phase_after(), FixedRoomPhase::Quiet);
        let reward = match clear.step {
            sim_content::CoreFixedDungeonRoomStep::B3(step) => step.reward_handoff,
            sim_content::CoreFixedDungeonRoomStep::B1(_)
            | sim_content::CoreFixedDungeonRoomStep::B2(_)
            | sim_content::CoreFixedDungeonRoomStep::B5(_) => None,
        };
        for _ in 1..=60 {
            let tick = runtime.tick().checked_next().unwrap();
            runtime
                .step_room(&room_input(tick, false))
                .await
                .expect("quiet progression");
        }
        assert_eq!(runtime.room_phase(), Some(FixedRoomPhase::Cleared));
        reward
    }

    fn no_offer(lineage: [u8; 16]) -> CoreDurableBargainRestResolution {
        CoreDurableBargainRestResolution::from_no_offer_milestone(
            authenticated(),
            &persistence::StoredBargainMilestoneResult {
                account_id: ACCOUNT_ID,
                character_id: CHARACTER_ID,
                source_reward_event_id: [0x44; 16],
                payload_hash: [0x45; 32],
                result_code: 2,
                pre_oath_bargain_version: 1,
                post_oath_bargain_version: 1,
                pre_earned_bargain_slots: 0,
                post_earned_bargain_slots: 0,
                offer_id: None,
                ash_mutation_id: Some([0x44; 16]),
                milestone_id: persistence::CORE_BARGAIN_MILESTONE_ID.into(),
                source_content_id: persistence::CORE_BARGAIN_SOURCE_ID.into(),
                source_layout_id: persistence::CORE_BARGAIN_LAYOUT_ID.into(),
                instance_lineage_id: lineage,
                entry_restore_point_id: [0x46; 16],
                result_payload: vec![1],
            },
        )
        .expect("no-offer authority")
    }

    #[tokio::test]
    async fn carried_tick_and_route_cas_enter_b1_then_commit_one_multiphase_frame() {
        let (directory, _, mut runtime) = fixture();
        assert_eq!(runtime.tick(), Tick(32));
        assert_eq!(
            runtime.node(),
            sim_content::CoreFixedDungeonNode::BellVestibuleB0
        );

        let entered = runtime.advance().await.expect("enter B1");
        assert_eq!(
            entered.route.room,
            Some(CorePrivateRouteRoomV1::BellCrossB1)
        );
        assert_eq!(entered.route.phase, CorePrivateRoutePhaseV1::RoomDormant);
        assert_eq!(runtime.tick(), Tick(32));

        let frame = runtime
            .step_room(&room_input(Tick(33), true))
            .await
            .expect("participant lock and warning");
        assert_eq!(frame.tick, Tick(33));
        assert_eq!(frame.route.phase, CorePrivateRoutePhaseV1::RoomSpawnWarning);
        assert_eq!(runtime.tick(), Tick(33));

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
    async fn retained_intent_generates_the_first_authoritative_b1_frame() {
        let (directory, _, mut runtime) = fixture_at(Tick(0));
        runtime.advance().await.expect("enter B1");

        let frame = runtime
            .step_live_room(live_input(7))
            .await
            .expect("live room frame");

        assert_eq!(frame.input_sequence, 7);
        assert_eq!(frame.tick, Tick(1));
        assert_eq!(frame.combat.tick, Tick(1));
        assert_eq!(
            frame.player_position,
            simulation_to_tile_point(runtime.combat.player().unwrap().target.position).unwrap()
        );
        assert_eq!(frame.route.phase, CorePrivateRoutePhaseV1::RoomSpawnWarning);
        assert_eq!(frame.step.phase_after(), FixedRoomPhase::SpawnWarning);
        assert!(!frame.player_died);
        assert_eq!(runtime.tick(), Tick(1));

        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn stale_route_rejects_a_frame_without_advancing_local_tick_or_phase() {
        let (directory, lease, mut runtime) = fixture();
        runtime.advance().await.expect("enter B1");
        directory
            .advance(
                lease,
                crate::CorePrivateRouteActorAdvance::RoomAwaitingDoorSafety,
            )
            .await
            .expect("competing route writer");

        assert!(matches!(
            runtime.step_room(&room_input(Tick(33), false)).await,
            Err(CorePrivateFixedDungeonRuntimeError::RouteAuthorityMismatch)
        ));
        assert_eq!(runtime.tick(), Tick(32));
        assert_eq!(runtime.room_phase(), Some(FixedRoomPhase::Dormant));

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
    async fn durable_b4_resolution_is_lineage_bound_replay_safe_and_advances_to_b5() {
        let (directory, lease, mut runtime) = fixture_at(Tick(0));
        runtime.advance().await.expect("enter B1");
        let _ = clear_current_room(&mut runtime).await;
        runtime.advance().await.expect("enter B2");
        let _ = clear_current_room(&mut runtime).await;
        runtime.advance().await.expect("enter B3");
        let reward = clear_current_room(&mut runtime)
            .await
            .expect("B3 reward handoff");
        assert!(runtime.advance().await.is_err());
        let foreign_reward: CoreDurableB3Resolution =
            crate::CoreDurableB3RewardCommit::test_fixture(
                authenticated(),
                CHARACTER_ID,
                [0xFE; 16],
                reward.clone(),
            )
            .into();
        assert!(matches!(
            runtime.commit_b3_reward(&foreign_reward).await,
            Err(CorePrivateFixedDungeonRuntimeError::B3RewardAuthorityMismatch)
        ));
        let durable_reward: CoreDurableB3Resolution =
            crate::CoreDurableB3RewardCommit::test_fixture(
                authenticated(),
                CHARACTER_ID,
                LINEAGE_ID,
                reward,
            )
            .into();
        let committed = runtime
            .commit_b3_reward(&durable_reward)
            .await
            .expect("durable B3 reward");
        assert_eq!(
            committed.receipt,
            sim_content::CoreB3RewardReceipt::Committed
        );
        assert_eq!(
            committed.disposition,
            sim_content::CoreB3RewardDisposition::GrantedOffer
        );
        assert!(runtime.pending_b3_reward_handoff().is_none());
        assert_eq!(runtime.combat_envelope.progression_version(), 2);
        let replay = runtime
            .commit_b3_reward(&durable_reward)
            .await
            .expect("B3 reward replay");
        assert_eq!(replay.receipt, sim_content::CoreB3RewardReceipt::Replayed);
        runtime.advance().await.expect("enter B4");

        let before = directory.snapshot(runtime.route_lease()).unwrap();
        assert!(matches!(
            runtime.resolve_rest(&no_offer([0xFF; 16])).await,
            Err(CorePrivateFixedDungeonRuntimeError::BargainAuthorityMismatch)
        ));
        assert_eq!(directory.snapshot(runtime.route_lease()).unwrap(), before);

        let durable = no_offer(LINEAGE_ID);
        let committed = runtime.resolve_rest(&durable).await.expect("commit B4");
        assert_eq!(
            committed.receipt,
            sim_content::CoreFixedDungeonRestReceipt::Committed
        );
        assert_eq!(committed.route.state_version, before.state_version);
        let replayed = runtime.resolve_rest(&durable).await.expect("replay B4");
        assert_eq!(
            replayed.receipt,
            sim_content::CoreFixedDungeonRestReceipt::Replayed
        );
        assert_eq!(replayed.route.state_version, committed.route.state_version);
        let entered = runtime.advance().await.expect("enter B5");
        assert_eq!(
            entered.transition.rest_resolution,
            Some(sim_content::CoreFixedDungeonRestResolution::NoOffer)
        );
        assert_eq!(
            entered.route.room,
            Some(CorePrivateRouteRoomV1::BellBridgeB5)
        );

        let _ = clear_current_room(&mut runtime).await;
        let inherited_tick = runtime.tick();
        let player_entity_id = runtime.combat.player().expect("B5 player").target.entity_id;
        let entered = runtime.advance().await.expect("enter B6 staging");
        assert_eq!(entered.route.phase, CorePrivateRoutePhaseV1::BossStaging);
        assert_eq!(
            entered.route.room,
            Some(CorePrivateRouteRoomV1::CaldusArenaB6)
        );

        let handoff = runtime
            .into_caldus_staging_handoff()
            .expect("consume exact B6 staging handoff");
        assert_eq!(handoff.route_lease(), lease);
        assert_eq!(handoff.tick(), inherited_tick);
        assert_eq!(handoff.entry_restore_point_id(), [0x74; 16]);
        assert_eq!(handoff.combat_envelope().character_id(), CHARACTER_ID);
        assert_eq!(handoff.combat_envelope().progression_version(), 2);
        assert_eq!(handoff.player().target.entity_id, player_entity_id);
        let route = handoff.route_snapshot().expect("handoff route snapshot");
        assert_eq!(route, entered.route);

        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }
}
