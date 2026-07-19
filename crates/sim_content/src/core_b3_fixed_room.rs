//! Exact Sepulcher Knight owner for the fixed B3 room.

use std::collections::BTreeMap;

use sim_core::{
    AppliedHostileDamage, ArenaGeometry, AttackCastId, CombatStep, CoreEnemyDefinition,
    CoreKnightEvent, CoreKnightSimulation, CoreKnightStep, EnemyHealthActor, EnemyHealthSimulation,
    EnemyHealthSnapshot, EnemyHealthStep, EnemyLabPlayer, EntityId, EntityIdAllocator,
    FixedRoomEvent, FixedRoomInput, FixedRoomPhase, FixedRoomSimulation, HostileDamagePolicy,
    HostileEvent, HostileProjectile, HostileProjectileSimulation, HostileStep,
    NormalRewardDropEvent, NormalWaveHandoff, RewardLifeState, RewardRecallState, RewardTrustState,
    SpawnInstanceId, Tick, apply_hostile_contact_transaction_with_policy, normal_wave_entity_id,
};

use crate::{
    CoreDevelopmentEncounterRooms, CoreFixedRoomActorRuntimeKind, CoreFixedRoomEncounterError,
    CoreFixedRoomEncounterPlan, CoreImmutableFixedRoomInput,
};

const INITIAL_FIXED_ROUTE_ACTOR_COUNT: u16 = 25;
const KNIGHT_INTRODUCTION_TICKS: u64 = 90;

#[derive(Debug, Clone, PartialEq)]
pub struct CoreB3ChargeContact {
    pub tick: Tick,
    pub cast_id: AttackCastId,
    pub target: EntityId,
    pub source_position: sim_core::SimulationVector,
    pub application: AppliedHostileDamage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreB3RewardHandoff {
    pub activation_ordinal: u32,
    pub instance_id: SpawnInstanceId,
    pub actor_id: EntityId,
    pub participant_id: EntityId,
    pub death_tick: Tick,
    pub reward_due_tick: Tick,
    pub reward_profile_id: String,
    pub xp_profile_id: String,
    pub active_ticks: u64,
    pub present_ticks: u64,
    pub direct_damage: u64,
    pub reference_health: u64,
    pub longest_inactivity_ticks: u64,
    pub life_state: RewardLifeState,
    pub recall_state: RewardRecallState,
    pub trust_state: RewardTrustState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreB3RewardReceipt {
    Committed,
    Replayed,
}

/// Durable outcome applied to the B3 simulation boundary. Eligibility is authoritative outside
/// simulation; every disposition acknowledges the exact immutable clear without allowing an
/// ineligible participant to receive an item, XP, or Bargain offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreB3RewardDisposition {
    GrantedOffer,
    GrantedNoOffer,
    IneligibleNoOffer,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreB3CombatStep {
    pub tick: Tick,
    pub health: EnemyHealthStep,
    pub knight: Option<CoreKnightStep>,
    pub charge_contacts: Vec<CoreB3ChargeContact>,
    pub hostile_spawn_events: Vec<HostileEvent>,
    pub hostile_step: HostileStep,
    pub reward_drops: Vec<NormalRewardDropEvent>,
    pub cleared_projectiles: Vec<HostileProjectile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreB3FixedRoomStep {
    pub tick: Tick,
    pub phase_after: FixedRoomPhase,
    pub required_hostiles_remaining: u16,
    pub lifecycle_events: Vec<FixedRoomEvent>,
    pub combat: Option<CoreB3CombatStep>,
    pub reward_handoff: Option<CoreB3RewardHandoff>,
    pub reset_cleared_projectiles: Vec<HostileProjectile>,
}

#[derive(Debug, Clone)]
struct CoreB3CombatSimulation {
    arena: ArenaGeometry,
    instance_id: SpawnInstanceId,
    actor_id: EntityId,
    definition: CoreEnemyDefinition,
    warning_started_at: Tick,
    activation_tick: Tick,
    introduction_ends_at: Tick,
    reward_profile_id: String,
    xp_profile_id: String,
    health: EnemyHealthSimulation,
    knight: CoreKnightSimulation,
    hostile: HostileProjectileSimulation,
    player: EnemyLabPlayer,
    damage_policy: HostileDamagePolicy,
    direct_damage: u64,
    present_ticks: u64,
    current_inactivity_ticks: u64,
    longest_inactivity_ticks: u64,
    reward_handoff: Option<CoreB3RewardHandoff>,
}

impl CoreB3CombatSimulation {
    fn new(
        plan: &CoreFixedRoomEncounterPlan,
        definition: CoreEnemyDefinition,
        participant: NormalWaveHandoff,
        warning_started_at: Tick,
        spawn_ordinal: u16,
    ) -> Result<Self, CoreFixedRoomEncounterError> {
        let [assignment] = plan.assignments() else {
            return Err(CoreFixedRoomEncounterError::DefinitionDrift);
        };
        if plan.node_id != "B3"
            || assignment.runtime_kind != CoreFixedRoomActorRuntimeKind::SepulcherKnight
            || assignment.enemy_id.as_str() != "miniboss.sepulcher_knight"
            || definition.parameters().content_id != assignment.enemy_id.as_str()
            || definition.parameters().maximum_health != 1_600
            || definition.parameters().armor != 8
            || definition.parameters().reward_profile_id != "reward.miniboss_t1"
            || definition.parameters().xp_profile_id != "xp.miniboss_t1"
        {
            return Err(CoreFixedRoomEncounterError::DefinitionDrift);
        }
        let instance_id = SpawnInstanceId {
            run_ordinal: assignment.instance_id.run_ordinal,
            spawn_ordinal,
        };
        let actor_id = normal_wave_entity_id(instance_id)?;
        let spawn =
            sim_core::CoreWorldPosition::new(assignment.x_milli_tiles, assignment.y_milli_tiles);
        let activation_tick = add_ticks(warning_started_at, plan.warning_ticks)?;
        let introduction_ends_at = add_ticks(warning_started_at, KNIGHT_INTRODUCTION_TICKS)?;
        let health = EnemyHealthSimulation::new(vec![EnemyHealthActor::core_authored(
            actor_id,
            &definition,
            super::core_fixed_room_encounter::core_position_vector(spawn),
            warning_started_at,
        )?])?;
        let knight = CoreKnightSimulation::new(definition.clone(), actor_id, spawn)?;
        Ok(Self {
            arena: plan.arena().clone(),
            instance_id,
            actor_id,
            reward_profile_id: assignment.reward_profile_id.as_str().to_owned(),
            xp_profile_id: assignment.xp_profile_id.as_str().to_owned(),
            definition,
            warning_started_at,
            activation_tick,
            introduction_ends_at,
            health,
            knight,
            hostile: HostileProjectileSimulation::with_allocator(
                participant.hostile_projectile_ids,
            ),
            player: participant.player,
            damage_policy: HostileDamagePolicy::Standard,
            direct_damage: 0,
            present_ticks: 0,
            current_inactivity_ticks: 0,
            longest_inactivity_ticks: 0,
            reward_handoff: None,
        })
    }

    fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
        self.hostile.set_damage_policy(policy);
    }

    fn snapshot(&self) -> Result<EnemyHealthSnapshot, CoreFixedRoomEncounterError> {
        self.health
            .snapshots()
            .into_iter()
            .next()
            .ok_or(CoreFixedRoomEncounterError::DefinitionDrift)
    }

    fn player(&self) -> &EnemyLabPlayer {
        &self.player
    }

    fn player_mut(&mut self) -> &mut EnemyLabPlayer {
        &mut self.player
    }

    fn alive_hurtboxes(&self) -> Result<Vec<sim_core::EnemyHurtbox>, CoreFixedRoomEncounterError> {
        self.health.alive_hurtboxes().map_err(Into::into)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the single-actor combat transaction keeps exact ordering auditable"
    )]
    fn step(
        &mut self,
        activation_ordinal: u32,
        combat: &CombatStep,
        input: &CoreImmutableFixedRoomInput,
    ) -> Result<CoreB3CombatStep, CoreFixedRoomEncounterError> {
        if input.reward_participation.is_present() {
            self.present_ticks = self
                .present_ticks
                .checked_add(1)
                .ok_or(CoreFixedRoomEncounterError::IdentityOverflow)?;
        }
        if input.reward_participation.is_active() {
            self.current_inactivity_ticks = 0;
        } else {
            self.current_inactivity_ticks = self
                .current_inactivity_ticks
                .checked_add(1)
                .ok_or(CoreFixedRoomEncounterError::IdentityOverflow)?;
            self.longest_inactivity_ticks = self
                .longest_inactivity_ticks
                .max(self.current_inactivity_ticks);
        }
        let health = self.health.apply_combat_step(combat)?;
        for event in &health.damage_events {
            self.direct_damage = self
                .direct_damage
                .checked_add(u64::from(event.damage.health_damage_applied))
                .ok_or(CoreFixedRoomEncounterError::IdentityOverflow)?;
        }
        if let Some(death) = health.death_events.first() {
            let active_ticks = death
                .tick
                .0
                .checked_sub(self.warning_started_at.0)
                .and_then(|ticks| ticks.checked_add(1))
                .ok_or(CoreFixedRoomEncounterError::IdentityOverflow)?;
            self.reward_handoff = Some(CoreB3RewardHandoff {
                activation_ordinal,
                instance_id: self.instance_id,
                actor_id: self.actor_id,
                participant_id: self.player.target.entity_id,
                death_tick: death.tick,
                reward_due_tick: death.reward_due_tick,
                reward_profile_id: self.reward_profile_id.clone(),
                xp_profile_id: self.xp_profile_id.clone(),
                active_ticks,
                present_ticks: self.present_ticks,
                direct_damage: self.direct_damage,
                reference_health: u64::from(self.definition.parameters().maximum_health),
                longest_inactivity_ticks: self.longest_inactivity_ticks,
                life_state: input.reward_life_state,
                recall_state: input.reward_recall_state,
                trust_state: input.reward_trust_state,
            });
        }
        let alive = self.snapshot()?.alive;
        let mut knight_step = None;
        let mut charge_contacts = Vec::new();
        let mut hostile_spawn_events = Vec::new();
        let mut cleared_projectiles = Vec::new();
        let hostile_step;
        if alive {
            let candidates = if input.living_inside > 0 {
                super::core_fixed_room_encounter::player_target_candidates(&self.player)?
            } else {
                Vec::new()
            };
            let step = self.knight.advance(
                &self.arena,
                &candidates,
                combat.tick >= self.introduction_ends_at && combat.tick >= self.activation_tick,
            )?;
            self.health.update_actor_position(
                self.actor_id,
                super::core_fixed_room_encounter::core_position_vector(step.to),
            )?;
            for event in &step.events {
                match event {
                    CoreKnightEvent::ChargeMoved {
                        cast_id, contacts, ..
                    } => {
                        for target in contacts {
                            if *target != self.player.target.entity_id {
                                return Err(CoreFixedRoomEncounterError::DefinitionDrift);
                            }
                            let application = apply_hostile_contact_transaction_with_policy(
                                self.actor_id,
                                34,
                                sim_core::DamageType::Physical,
                                &mut self.player.target,
                                &mut self.player.consumables,
                                &mut self.player.combat,
                                self.damage_policy,
                            )?;
                            charge_contacts.push(CoreB3ChargeContact {
                                tick: combat.tick,
                                cast_id: *cast_id,
                                target: *target,
                                source_position:
                                    super::core_fixed_room_encounter::core_position_vector(step.to),
                                application,
                            });
                        }
                    }
                    CoreKnightEvent::StopRingReleased { .. }
                    | CoreKnightEvent::ShieldFanReleased { .. } => {
                        hostile_spawn_events.extend(self.hostile.spawn_from_core_knight_event(
                            self.actor_id,
                            &self.definition,
                            event,
                        )?);
                    }
                    CoreKnightEvent::TelegraphStarted { .. }
                    | CoreKnightEvent::ChargeStarted { .. }
                    | CoreKnightEvent::TargetlessReset { .. } => {}
                }
            }
            hostile_step = if input.living_inside > 0 {
                self.hostile.step(
                    &self.arena,
                    &mut self.player.target,
                    &mut self.player.consumables,
                    &mut self.player.combat,
                )?
            } else {
                self.hostile
                    .step_players(&self.arena, &mut BTreeMap::new())?
            };
            knight_step = Some(step);
        } else {
            cleared_projectiles = self.hostile.clear_projectiles();
            hostile_step = HostileStep {
                tick: self.hostile.tick(),
                events: Vec::new(),
            };
        }
        let reward_drops = self.health.collect_due_drops(combat.tick)?;
        Ok(CoreB3CombatStep {
            tick: combat.tick,
            health,
            knight: knight_step,
            charge_contacts,
            hostile_spawn_events,
            hostile_step,
            reward_drops,
            cleared_projectiles,
        })
    }
}

/// Owns B3 lifecycle, Knight mechanics, health, player state, and hostile identity atomically.
#[derive(Debug, Clone)]
pub struct CoreB3FixedRoomSimulation {
    plan: CoreFixedRoomEncounterPlan,
    definition: CoreEnemyDefinition,
    authority: FixedRoomSimulation,
    damage_policy: HostileDamagePolicy,
    next_spawn_ordinal: u16,
    participant: Option<NormalWaveHandoff>,
    combat: Option<CoreB3CombatSimulation>,
    pending_reward_handoff: Option<CoreB3RewardHandoff>,
    reward_disposition: Option<CoreB3RewardDisposition>,
}

impl CoreB3FixedRoomSimulation {
    pub fn new(
        plan: CoreFixedRoomEncounterPlan,
        content: &CoreDevelopmentEncounterRooms,
        player: EnemyLabPlayer,
        hostile_projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CoreFixedRoomEncounterError> {
        if plan.node_id != "B3" || plan.assignments().len() != 1 {
            return Err(CoreFixedRoomEncounterError::DefinitionDrift);
        }
        let definition = super::core_fixed_room_encounter::authored_definition(
            content,
            "miniboss.sepulcher_knight",
        )?;
        let authority = plan.new_authority()?;
        let next_spawn_ordinal = plan.first_spawn_ordinal;
        Ok(Self {
            plan,
            definition,
            authority,
            damage_policy: HostileDamagePolicy::Standard,
            next_spawn_ordinal,
            participant: Some(NormalWaveHandoff {
                player,
                hostile_projectile_ids,
            }),
            combat: None,
            pending_reward_handoff: None,
            reward_disposition: None,
        })
    }

    #[must_use]
    pub const fn phase(&self) -> FixedRoomPhase {
        self.authority.phase()
    }

    /// Transfers the one mutable player/projectile allocation only after committed B3 completion
    /// and its one reward handoff. Active Knight state cannot escape into B4.
    pub fn into_handoff(self) -> Result<NormalWaveHandoff, CoreFixedRoomEncounterError> {
        if self.authority.phase() != FixedRoomPhase::Cleared
            || self.combat.is_some()
            || self.pending_reward_handoff.is_none()
            || self.reward_disposition.is_none()
        {
            return Err(CoreFixedRoomEncounterError::RoomHandoffUnavailable);
        }
        self.participant
            .ok_or(CoreFixedRoomEncounterError::MissingParticipantHandoff)
    }

    #[must_use]
    pub const fn pending_reward_handoff(&self) -> Option<&CoreB3RewardHandoff> {
        if self.reward_disposition.is_some() {
            None
        } else {
            self.pending_reward_handoff.as_ref()
        }
    }

    #[must_use]
    pub const fn reward_disposition(&self) -> Option<CoreB3RewardDisposition> {
        self.reward_disposition
    }

    /// Acknowledges only the exact B3 handoff that persistence resolved. The server layer keeps
    /// the durable receipt opaque; this simulation boundary compares the immutable encounter
    /// material and makes exact retry read-only.
    pub fn acknowledge_reward(
        &mut self,
        handoff: &CoreB3RewardHandoff,
        disposition: CoreB3RewardDisposition,
    ) -> Result<CoreB3RewardReceipt, CoreFixedRoomEncounterError> {
        let pending = self
            .pending_reward_handoff
            .as_ref()
            .ok_or(CoreFixedRoomEncounterError::B3RewardUnavailable)?;
        if pending != handoff {
            return Err(CoreFixedRoomEncounterError::B3RewardConflict);
        }
        if let Some(stored) = self.reward_disposition {
            return if stored == disposition {
                Ok(CoreB3RewardReceipt::Replayed)
            } else {
                Err(CoreFixedRoomEncounterError::B3RewardConflict)
            };
        }
        self.reward_disposition = Some(disposition);
        Ok(CoreB3RewardReceipt::Committed)
    }

    #[must_use]
    pub fn snapshot(&self) -> Option<EnemyHealthSnapshot> {
        self.combat
            .as_ref()
            .and_then(|combat| combat.snapshot().ok())
    }

    pub fn player(&self) -> Result<&EnemyLabPlayer, CoreFixedRoomEncounterError> {
        if let Some(combat) = &self.combat {
            return Ok(combat.player());
        }
        self.participant
            .as_ref()
            .map(|handoff| &handoff.player)
            .ok_or(CoreFixedRoomEncounterError::MissingParticipantHandoff)
    }

    pub fn player_mut(&mut self) -> Result<&mut EnemyLabPlayer, CoreFixedRoomEncounterError> {
        if let Some(combat) = &mut self.combat {
            return Ok(combat.player_mut());
        }
        self.participant
            .as_mut()
            .map(|handoff| &mut handoff.player)
            .ok_or(CoreFixedRoomEncounterError::MissingParticipantHandoff)
    }

    pub fn alive_hurtboxes(
        &self,
    ) -> Result<Vec<sim_core::EnemyHurtbox>, CoreFixedRoomEncounterError> {
        if self.authority.phase() != FixedRoomPhase::Active {
            return Ok(Vec::new());
        }
        self.combat
            .as_ref()
            .map_or_else(|| Ok(Vec::new()), CoreB3CombatSimulation::alive_hurtboxes)
    }

    pub fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
        if let Some(combat) = &mut self.combat {
            combat.set_damage_policy(policy);
        }
    }

    pub fn step(
        &mut self,
        tick: Tick,
        input: &CoreImmutableFixedRoomInput,
    ) -> Result<CoreB3FixedRoomStep, CoreFixedRoomEncounterError> {
        let mut staged = self.clone();
        let step = staged.step_inner(tick, input)?;
        *self = staged;
        Ok(step)
    }

    fn step_inner(
        &mut self,
        tick: Tick,
        input: &CoreImmutableFixedRoomInput,
    ) -> Result<CoreB3FixedRoomStep, CoreFixedRoomEncounterError> {
        let mut combat_step = if let Some(combat) = &mut self.combat {
            Some(combat.step(
                self.authority.activation_ordinal(),
                &super::core_fixed_room_encounter::combat_input(tick, input)?,
                input,
            )?)
        } else {
            None
        };
        let required_hostiles_remaining = self.combat.as_ref().map_or(Ok(1), |combat| {
            combat.snapshot().map(|snapshot| u16::from(snapshot.alive))
        })?;
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
        let mut reward_handoff = None;
        let mut reset_cleared_projectiles = Vec::new();
        for event in lifecycle_events.iter().copied() {
            match event {
                FixedRoomEvent::BeginGroupWarning { .. } => {
                    let participant = self
                        .participant
                        .take()
                        .ok_or(CoreFixedRoomEncounterError::MissingParticipantHandoff)?;
                    let mut combat = CoreB3CombatSimulation::new(
                        &self.plan,
                        self.definition.clone(),
                        participant,
                        tick,
                        self.next_spawn_ordinal,
                    )?;
                    combat.set_damage_policy(self.damage_policy);
                    combat_step = Some(combat.step(
                        self.authority.activation_ordinal(),
                        &super::core_fixed_room_encounter::combat_input(tick, input)?,
                        input,
                    )?);
                    self.next_spawn_ordinal = self
                        .next_spawn_ordinal
                        .checked_add(INITIAL_FIXED_ROUTE_ACTOR_COUNT)
                        .ok_or(CoreFixedRoomEncounterError::IdentityOverflow)?;
                    self.combat = Some(combat);
                }
                FixedRoomEvent::CompletionCommitted { .. } => {
                    let mut combat = self
                        .combat
                        .take()
                        .ok_or(CoreFixedRoomEncounterError::MissingB3Combat)?;
                    let cleared = combat.hostile.clear_projectiles();
                    if let Some(step) = &mut combat_step {
                        step.cleared_projectiles.extend(cleared);
                    }
                    let handoff = combat
                        .reward_handoff
                        .take()
                        .ok_or(CoreFixedRoomEncounterError::B3RewardUnavailable)?;
                    self.pending_reward_handoff = Some(handoff.clone());
                    self.reward_disposition = None;
                    reward_handoff = Some(handoff);
                    self.participant = Some(NormalWaveHandoff {
                        player: combat.player,
                        hostile_projectile_ids: combat.hostile.into_allocator(),
                    });
                }
                FixedRoomEvent::RoomReset => {
                    self.pending_reward_handoff = None;
                    self.reward_disposition = None;
                    if let Some(mut combat) = self.combat.take() {
                        reset_cleared_projectiles = combat.hostile.clear_projectiles();
                        self.participant = Some(NormalWaveHandoff {
                            player: combat.player,
                            hostile_projectile_ids: combat.hostile.into_allocator(),
                        });
                    } else if self.participant.is_none() {
                        return Err(CoreFixedRoomEncounterError::MissingParticipantHandoff);
                    }
                }
                _ => {}
            }
        }
        Ok(CoreB3FixedRoomStep {
            tick,
            phase_after: self.authority.phase(),
            required_hostiles_remaining,
            lifecycle_events,
            combat: combat_step,
            reward_handoff,
            reset_cleared_projectiles,
        })
    }
}

fn add_ticks(tick: Tick, amount: u64) -> Result<Tick, CoreFixedRoomEncounterError> {
    tick.0
        .checked_add(amount)
        .map(Tick)
        .ok_or(CoreFixedRoomEncounterError::IdentityOverflow)
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, path::Path};

    use super::*;
    use crate::{compile_core_fixed_room_encounters, load_core_development_encounter_rooms};
    use sim_core::{
        CollisionTarget, CoreKnightSimulation, CoreTargetCandidate, CoreWorldPosition,
        FriendlyProjectileSource, HostileTargetState, PlayerVitals, ProjectileCollision,
        RawDamageIntent, RawDamageIntentSource, RedTonicSimulation, SimulationVector, TonicBelt,
    };

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn fixture() -> (
        CoreDevelopmentEncounterRooms,
        CoreFixedRoomEncounterPlan,
        EnemyLabPlayer,
        EntityIdAllocator,
    ) {
        let root = content_root();
        let content = load_core_development_encounter_rooms(&root).expect("Core content");
        let plan = compile_core_fixed_room_encounters(&content, 41)
            .expect("fixed plans")
            .remove(2);
        let (source, _) = crate::load_and_validate(&root).expect("FP source");
        let combat = crate::first_playable_authority_combat_test(&source).expect("combat fixture");
        let definitions = combat.definitions;
        let player = EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: EntityId::new(900).expect("player ID"),
                position: SimulationVector::new(3.0, 7.5),
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
            content,
            plan,
            player,
            EntityIdAllocator::starting_at(NonZeroU64::new(80_000).expect("projectile allocator")),
        )
    }

    fn input(tick: u64, living_inside: u16) -> CoreImmutableFixedRoomInput {
        CoreImmutableFixedRoomInput {
            crossed_activation_boundary: tick == 0,
            living_inside,
            living_party_outside: u16::from(living_inside == 0),
            doorway_hurtbox_blocked: false,
            reward_life_state: if living_inside > 0 {
                RewardLifeState::Living
            } else {
                RewardLifeState::Dead
            },
            reward_recall_state: RewardRecallState::Eligible,
            reward_trust_state: RewardTrustState::Valid,
            reward_participation: if living_inside > 0 {
                crate::CoreRewardParticipation::PresentActive
            } else {
                crate::CoreRewardParticipation::Absent
            },
            combat_step: (living_inside > 0).then_some(CombatStep {
                tick: Tick(tick),
                ..CombatStep::default()
            }),
        }
    }

    fn lethal(actor_id: EntityId, tick: u64) -> CombatStep {
        let projectile_id = EntityId::new(99_000).expect("friendly projectile");
        CombatStep {
            tick: Tick(tick),
            collisions: vec![ProjectileCollision {
                tick: Tick(tick),
                projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(actor_id),
                final_position: SimulationVector::new(10.0, 7.5),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            }],
            raw_damage_intents: vec![RawDamageIntent {
                tick: Tick(tick),
                projectile_id,
                source: RawDamageIntentSource::Primary,
                target: actor_id,
                base_raw_damage: 10_000,
                multiplier_basis_points: 10_000,
                resolved_raw_damage: 10_000,
                contact_ordinal: 0,
            }],
            ..CombatStep::default()
        }
    }

    fn sustained_trace() -> Vec<(u64, &'static str, u64)> {
        let (content, plan, player, allocator) = fixture();
        let mut room =
            CoreB3FixedRoomSimulation::new(plan, &content, player, allocator).expect("B3 room");
        room.set_damage_policy(HostileDamagePolicy::DebugInvulnerable);
        let mut trace = Vec::new();
        let mut actor_id = None;
        for tick in 0..=1_050 {
            let mut room_input = input(tick, 1);
            if tick == 1_050 {
                room_input.combat_step = Some(lethal(actor_id.expect("spawned Knight"), tick));
            }
            let step = room.step(Tick(tick), &room_input).expect("B3 step");
            actor_id = room
                .snapshot()
                .map(|snapshot| snapshot.actor_id)
                .or(actor_id);
            if let Some(combat) = step.combat {
                if let Some(knight) = combat.knight {
                    for event in knight.events {
                        let kind = match event {
                            CoreKnightEvent::TelegraphStarted { .. } => "telegraph",
                            CoreKnightEvent::ChargeStarted { .. } => "charge_start",
                            CoreKnightEvent::ChargeMoved { .. } => "charge_move",
                            CoreKnightEvent::StopRingReleased { .. } => "ring",
                            CoreKnightEvent::ShieldFanReleased { .. } => "fan",
                            CoreKnightEvent::TargetlessReset { .. } => "reset",
                        };
                        trace.push((tick, kind, 0));
                    }
                }
                for event in combat.hostile_spawn_events {
                    if let HostileEvent::Spawned { projectile, .. } = event {
                        trace.push((tick, projectile.pattern_id(), projectile.id().get()));
                    }
                }
            }
            if tick == 1_050 {
                let reward = step.reward_handoff.expect("one reward handoff");
                assert_eq!(reward.reward_profile_id, "reward.miniboss_t1");
                assert_eq!(reward.xp_profile_id, "xp.miniboss_t1");
                assert_eq!(reward.death_tick, Tick(1_050));
                assert_eq!(step.phase_after, FixedRoomPhase::Quiet);
            }
        }
        trace
    }

    #[test]
    fn knight_intro_schedule_hostiles_and_35_second_clear_replay_exactly() {
        let first = sustained_trace();
        let second = sustained_trace();
        assert_eq!(first, second);
        assert!(first.iter().all(|(tick, kind, _)| {
            *tick >= 90 || !matches!(*kind, "charge_start" | "ring" | "fan")
        }));
        assert!(
            first
                .iter()
                .any(|(tick, kind, _)| *tick == 90 && *kind == "telegraph")
        );
        assert!(
            first
                .iter()
                .any(|(tick, kind, _)| *tick == 117 && *kind == "charge_start")
        );
        assert_eq!(
            first
                .iter()
                .filter(|(tick, kind, _)| (117..=133).contains(tick) && *kind == "charge_move")
                .count(),
            17
        );
        assert!(
            first
                .iter()
                .any(|(tick, kind, _)| *tick == 134 && *kind == "ring")
        );
        assert!(
            first
                .iter()
                .any(|(tick, kind, _)| *tick == 168 && *kind == "fan")
        );
        assert!(first.iter().any(|(tick, kind, _)| {
            *tick == 134 && *kind == "miniboss.sepulcher_knight.stop_ring"
        }));
        assert_eq!(
            first
                .iter()
                .filter(|(tick, kind, _)| {
                    *tick == 134 && *kind == "miniboss.sepulcher_knight.stop_ring"
                })
                .count(),
            8
        );
        assert!(first.iter().any(|(tick, kind, _)| {
            *tick == 168 && *kind == "miniboss.sepulcher_knight.shield_fan"
        }));
        assert_eq!(
            first
                .iter()
                .filter(|(tick, kind, _)| {
                    *tick == 168 && *kind == "miniboss.sepulcher_knight.shield_fan"
                })
                .count(),
            5
        );
    }

    #[test]
    fn b3_handoff_requires_doors_open_and_preserves_participant_identity() {
        let (content, plan, player, allocator) = fixture();
        let player_entity_id = player.target.entity_id;
        let initial_projectile_id = allocator.peek();
        let mut room =
            CoreB3FixedRoomSimulation::new(plan, &content, player, allocator).expect("B3 room");
        room.set_damage_policy(HostileDamagePolicy::DebugInvulnerable);
        assert!(matches!(
            room.clone().into_handoff(),
            Err(CoreFixedRoomEncounterError::RoomHandoffUnavailable)
        ));

        let mut actor_id = None;
        let mut reward_handoff = None;
        for tick in 0..=1_050 {
            let mut room_input = input(tick, 1);
            if (100..130).contains(&tick) {
                room_input.reward_participation = crate::CoreRewardParticipation::Absent;
                room_input.reward_trust_state = RewardTrustState::InvalidSession;
            }
            if tick == 1_050 {
                room_input.combat_step = Some(lethal(actor_id.expect("spawned Knight"), tick));
            }
            let step = room.step(Tick(tick), &room_input).expect("B3 clear trace");
            actor_id = room
                .snapshot()
                .map(|snapshot| snapshot.actor_id)
                .or(actor_id);
            if tick == 1_050 {
                reward_handoff = step.reward_handoff;
                assert!(reward_handoff.is_some());
            }
        }
        assert_eq!(room.phase(), FixedRoomPhase::Quiet);
        assert!(matches!(
            room.clone().into_handoff(),
            Err(CoreFixedRoomEncounterError::RoomHandoffUnavailable)
        ));
        let reward_handoff = reward_handoff.expect("B3 reward handoff");
        assert_eq!(
            reward_handoff.reward_due_tick.0,
            reward_handoff.death_tick.0 + 8
        );
        assert_eq!(reward_handoff.active_ticks, 1_051);
        assert_eq!(reward_handoff.present_ticks, 1_021);
        assert_eq!(reward_handoff.longest_inactivity_ticks, 30);
        assert_eq!(reward_handoff.life_state, RewardLifeState::Living);
        assert_eq!(reward_handoff.recall_state, RewardRecallState::Eligible);
        assert_eq!(reward_handoff.trust_state, RewardTrustState::Valid);
        let mut changed = reward_handoff.clone();
        changed.reward_due_tick = Tick(changed.reward_due_tick.0 + 1);
        assert!(matches!(
            room.acknowledge_reward(&changed, CoreB3RewardDisposition::GrantedOffer),
            Err(CoreFixedRoomEncounterError::B3RewardConflict)
        ));
        assert_eq!(
            room.acknowledge_reward(&reward_handoff, CoreB3RewardDisposition::GrantedOffer,)
                .expect("durable reward acknowledgement"),
            CoreB3RewardReceipt::Committed
        );
        assert!(room.pending_reward_handoff().is_none());
        assert_eq!(
            room.acknowledge_reward(&reward_handoff, CoreB3RewardDisposition::GrantedOffer,)
                .expect("exact acknowledgement replay"),
            CoreB3RewardReceipt::Replayed
        );
        assert!(matches!(
            room.acknowledge_reward(&reward_handoff, CoreB3RewardDisposition::IneligibleNoOffer,),
            Err(CoreFixedRoomEncounterError::B3RewardConflict)
        ));

        for tick in 1_051..1_110 {
            assert!(
                room.step(Tick(tick), &input(tick, 1))
                    .expect("B3 quiet period")
                    .lifecycle_events
                    .is_empty()
            );
        }
        assert_eq!(
            room.step(Tick(1_110), &input(1_110, 1))
                .expect("B3 doors open")
                .lifecycle_events,
            [FixedRoomEvent::DoorsOpened]
        );
        let handoff = room.into_handoff().expect("completed B3 handoff");
        assert_eq!(handoff.player.target.entity_id, player_entity_id);
        assert!(handoff.hostile_projectile_ids.peek() >= initial_projectile_id);
    }

    #[test]
    fn empty_reset_is_rewardless_and_retry_advances_full_route_stride() {
        let (content, plan, player, allocator) = fixture();
        let mut room =
            CoreB3FixedRoomSimulation::new(plan, &content, player, allocator).expect("B3 room");
        room.step(Tick(0), &input(0, 1)).expect("activate");
        let first_actor = room.snapshot().expect("first Knight").actor_id;
        for tick in 1..91 {
            let step = room.step(Tick(tick), &input(tick, 0)).expect("empty tick");
            assert!(step.reward_handoff.is_none());
        }
        let reset = room.step(Tick(91), &input(91, 0)).expect("reset boundary");
        assert!(reset.lifecycle_events.contains(&FixedRoomEvent::RoomReset));
        assert!(reset.reward_handoff.is_none());
        let mut retry = input(92, 1);
        retry.crossed_activation_boundary = true;
        room.step(Tick(92), &retry).expect("retry");
        let second_actor = room.snapshot().expect("second Knight").actor_id;
        assert_eq!(second_actor.get() - first_actor.get(), 25);
    }

    #[test]
    fn charge_sweeps_once_truncates_at_shell_and_ring_uses_realized_endpoint() {
        let (content, plan, _, _) = fixture();
        let definition = super::super::core_fixed_room_encounter::authored_definition(
            &content,
            "miniboss.sepulcher_knight",
        )
        .expect("Knight definition");
        let actor_id = plan.assignments()[0].entity_id;
        let spawn = CoreWorldPosition::new(
            plan.assignments()[0].x_milli_tiles,
            plan.assignments()[0].y_milli_tiles,
        );
        let player_id = EntityId::new(901).expect("player");
        let candidate = CoreTargetCandidate {
            entity_id: player_id,
            position: CoreWorldPosition::new(18_000, 7_500),
            living: true,
            damageable: true,
        };
        let mut knight = CoreKnightSimulation::new(definition, actor_id, spawn).expect("Knight");
        let mut charge_started = None;
        let mut contacts = 0;
        let mut ring = None;
        for _ in 0..120 {
            let step = knight
                .advance(plan.arena(), &[candidate], true)
                .expect("Knight step");
            for event in step.events {
                match event {
                    CoreKnightEvent::ChargeStarted { tick, lock } => {
                        charge_started = Some((tick, lock));
                    }
                    CoreKnightEvent::ChargeMoved {
                        contacts: segment_contacts,
                        ..
                    } => contacts += segment_contacts.len(),
                    CoreKnightEvent::StopRingReleased {
                        tick,
                        lock,
                        origin,
                        emitted_indices,
                        omitted_indices,
                    } => {
                        ring = Some((tick, lock, origin, emitted_indices, omitted_indices));
                        break;
                    }
                    CoreKnightEvent::TelegraphStarted { .. }
                    | CoreKnightEvent::ShieldFanReleased { .. }
                    | CoreKnightEvent::TargetlessReset { .. } => {}
                }
            }
            if ring.is_some() {
                break;
            }
        }
        let (charge_tick, charge_lock) = charge_started.expect("charge start");
        let (ring_tick, ring_lock, ring_origin, emitted, omitted) = ring.expect("stop ring");
        assert_eq!(ring_tick.0 - charge_tick.0, 17);
        assert_eq!(ring_lock, charge_lock);
        assert_eq!(contacts, 1);
        assert_eq!(ring_origin, knight.acquire_home());
        assert!(ring_origin.x_milli_tiles < 18_500);
        assert!(ring_origin.x_milli_tiles > 18_400);
        assert_eq!(omitted, [0, 1]);
        assert_eq!(emitted, [2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
