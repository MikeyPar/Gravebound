//! Complete renderer-independent Mire Leech full-combat fixture.

use sim_core::{
    AppliedHostileDamage, ArenaGeometry, AttackCastId, CombatStep, CoreEnemyDefinition,
    CoreMireEvent, CoreMireSimulation, CoreMireStep, EnemyHealthActor, EnemyHealthSimulation,
    EnemyHealthSnapshot, EnemyHealthStep, EnemyLabPlayer, EntityId, HostileDamagePolicy, Tick,
    apply_hostile_contact_transaction_with_policy,
};
use thiserror::Error;

use crate::CoreDevelopmentEncounterRooms;

const SPAWN_WARNING_TICKS: u64 = 27;

#[derive(Debug, Clone, PartialEq)]
pub struct CoreMireContact {
    pub tick: Tick,
    pub cast_id: AttackCastId,
    pub target: EntityId,
    pub application: AppliedHostileDamage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreMireRewardHandoff {
    pub actor_id: EntityId,
    pub participant_id: EntityId,
    pub death_tick: Tick,
    pub reward_due_tick: Tick,
    pub reward_profile_id: String,
    pub xp_profile_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreMireFixtureStep {
    pub tick: Tick,
    pub health: EnemyHealthStep,
    pub mire: Option<CoreMireStep>,
    pub contacts: Vec<CoreMireContact>,
    pub reward_handoff: Option<CoreMireRewardHandoff>,
}

#[derive(Debug, Clone)]
pub struct CoreMireFixtureSimulation {
    definition: CoreEnemyDefinition,
    arena: ArenaGeometry,
    actor_id: EntityId,
    spawn: sim_core::CoreWorldPosition,
    player: EnemyLabPlayer,
    health: EnemyHealthSimulation,
    mire: CoreMireSimulation,
    damage_policy: HostileDamagePolicy,
    tick: Tick,
    active_at: Tick,
    defeated: bool,
}

impl CoreMireFixtureSimulation {
    pub fn new(
        content: &CoreDevelopmentEncounterRooms,
        actor_id: EntityId,
        mut player: EnemyLabPlayer,
    ) -> Result<Self, CoreMireFixtureError> {
        let definition =
            super::core_fixed_room_encounter::authored_definition(content, "enemy.mire_leech")?;
        if definition.parameters().maximum_health != 70
            || definition.parameters().armor != 0
            || definition.parameters().reward_profile_id != "reward.normal_outer"
            || definition.parameters().xp_profile_id != "xp.normal_t1"
        {
            return Err(CoreMireFixtureError::DefinitionDrift);
        }
        let room = content
            .compile_room_definitions()?
            .into_iter()
            .find(|room| room.id == "room.bell.cross_01")
            .ok_or(CoreMireFixtureError::DefinitionDrift)?
            .rotated(0)?;
        let arena = super::core_fixed_room_encounter::combat_arena(&room)?;
        let spawn = sim_core::CoreWorldPosition::new(3_000, 3_000);
        player.target.position = sim_core::SimulationVector::new(5.5, 3.0);
        let health = build_health(actor_id, &definition, spawn, Tick(0))?;
        let mire = CoreMireSimulation::new(definition.clone(), actor_id, spawn)?;
        Ok(Self {
            definition,
            arena,
            actor_id,
            spawn,
            player,
            health,
            mire,
            damage_policy: HostileDamagePolicy::Standard,
            tick: Tick(0),
            active_at: Tick(SPAWN_WARNING_TICKS),
            defeated: false,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }
    #[must_use]
    pub fn snapshot(&self) -> EnemyHealthSnapshot {
        self.health.snapshots()[0]
    }

    pub fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
    }

    pub fn step(
        &mut self,
        combat: &CombatStep,
    ) -> Result<CoreMireFixtureStep, CoreMireFixtureError> {
        let mut staged = self.clone();
        let step = staged.step_inner(combat)?;
        *self = staged;
        Ok(step)
    }

    pub fn reset(&mut self) -> Result<(), CoreMireFixtureError> {
        self.health = build_health(self.actor_id, &self.definition, self.spawn, self.tick)?;
        self.mire.reset()?;
        self.active_at = add_ticks(self.tick, SPAWN_WARNING_TICKS)?;
        self.defeated = false;
        Ok(())
    }

    fn step_inner(
        &mut self,
        combat: &CombatStep,
    ) -> Result<CoreMireFixtureStep, CoreMireFixtureError> {
        if combat.tick != self.tick {
            return Err(CoreMireFixtureError::TickMismatch);
        }
        if self.defeated && (!combat.collisions.is_empty() || !combat.raw_damage_intents.is_empty())
        {
            return Err(CoreMireFixtureError::DamageAfterDefeat);
        }
        let health = if self.defeated {
            EnemyHealthStep {
                tick: self.tick,
                ..EnemyHealthStep::default()
            }
        } else {
            self.health.apply_combat_step(combat)?
        };
        let mut reward_handoff = None;
        if let Some(death) = health.death_events.first() {
            self.defeated = true;
            reward_handoff = Some(CoreMireRewardHandoff {
                actor_id: self.actor_id,
                participant_id: self.player.target.entity_id,
                death_tick: death.tick,
                reward_due_tick: death.reward_due_tick,
                reward_profile_id: self.definition.parameters().reward_profile_id.clone(),
                xp_profile_id: self.definition.parameters().xp_profile_id.clone(),
            });
        }
        let mut mire_step = None;
        let mut contacts = Vec::new();
        if !self.defeated {
            let candidates =
                super::core_fixed_room_encounter::player_target_candidates(&self.player)?;
            let step = self
                .mire
                .advance(&self.arena, &candidates, self.tick >= self.active_at)?;
            self.health.update_actor_position(
                self.actor_id,
                super::core_fixed_room_encounter::core_position_vector(
                    step.events
                        .iter()
                        .rev()
                        .find_map(|event| match event {
                            CoreMireEvent::Movement { to, .. } => Some(*to),
                            CoreMireEvent::TargetlessReset {
                                restored_position, ..
                            } => Some(*restored_position),
                            CoreMireEvent::ChargeContact { .. } => None,
                        })
                        .unwrap_or(self.mire.position()),
                ),
            )?;
            for event in &step.events {
                if let CoreMireEvent::ChargeContact {
                    cast_id, target, ..
                } = event
                {
                    if *target != self.player.target.entity_id {
                        return Err(CoreMireFixtureError::DefinitionDrift);
                    }
                    let application = apply_hostile_contact_transaction_with_policy(
                        self.actor_id,
                        12,
                        sim_core::DamageType::Physical,
                        &mut self.player.target,
                        &mut self.player.consumables,
                        &mut self.player.combat,
                        self.damage_policy,
                    )?;
                    contacts.push(CoreMireContact {
                        tick: self.tick,
                        cast_id: *cast_id,
                        target: *target,
                        application,
                    });
                }
            }
            mire_step = Some(step);
        }
        let output = CoreMireFixtureStep {
            tick: self.tick,
            health,
            mire: mire_step,
            contacts,
            reward_handoff,
        };
        self.tick = self
            .tick
            .checked_next()
            .ok_or(CoreMireFixtureError::TickOverflow)?;
        Ok(output)
    }
}

fn build_health(
    actor_id: EntityId,
    definition: &CoreEnemyDefinition,
    spawn: sim_core::CoreWorldPosition,
    tick: Tick,
) -> Result<EnemyHealthSimulation, CoreMireFixtureError> {
    Ok(EnemyHealthSimulation::new(vec![
        EnemyHealthActor::core_authored(
            actor_id,
            definition,
            super::core_fixed_room_encounter::core_position_vector(spawn),
            tick,
        )?,
    ])?)
}
fn add_ticks(tick: Tick, amount: u64) -> Result<Tick, CoreMireFixtureError> {
    tick.0
        .checked_add(amount)
        .map(Tick)
        .ok_or(CoreMireFixtureError::TickOverflow)
}

#[derive(Debug, Error)]
pub enum CoreMireFixtureError {
    #[error("Mire Leech fixture content drifted")]
    DefinitionDrift,
    #[error("Mire Leech fixture tick diverged")]
    TickMismatch,
    #[error("Mire Leech fixture received damage after defeat")]
    DamageAfterDefeat,
    #[error("Mire Leech fixture tick overflowed")]
    TickOverflow,
    #[error(transparent)]
    Content(#[from] anyhow::Error),
    #[error(transparent)]
    Room(#[from] sim_core::DungeonRoomError),
    #[error(transparent)]
    FixedRoom(#[from] crate::CoreFixedRoomEncounterError),
    #[error(transparent)]
    Mire(#[from] sim_core::CoreMireError),
    #[error(transparent)]
    Health(#[from] sim_core::EnemyHealthError),
    #[error(transparent)]
    Hostile(#[from] sim_core::HostileError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_core_development_encounter_rooms;
    use sim_core::{
        CollisionTarget, FriendlyProjectileSource, HostileTargetState, PlayerVitals,
        ProjectileCollision, RawDamageIntent, RawDamageIntentSource, RedTonicSimulation,
        SimulationVector, TonicBelt,
    };
    use std::path::Path;

    fn fixture() -> CoreMireFixtureSimulation {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let content = load_core_development_encounter_rooms(&root).expect("Core content");
        let (source, _) = crate::load_and_validate(&root).expect("FP source");
        let combat = crate::first_playable_authority_combat_test(&source).expect("combat fixture");
        let definitions = combat.definitions;
        let player = EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: EntityId::new(900).expect("player"),
                position: SimulationVector::new(5.5, 3.0),
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
        let mut fixture =
            CoreMireFixtureSimulation::new(&content, EntityId::new(71_000).expect("Mire"), player)
                .expect("fixture");
        fixture.set_damage_policy(HostileDamagePolicy::DebugInvulnerable);
        fixture
    }
    fn empty(t: u64) -> CombatStep {
        CombatStep {
            tick: Tick(t),
            ..CombatStep::default()
        }
    }
    fn lethal(id: EntityId, t: u64) -> CombatStep {
        let p = EntityId::new(99_700).expect("projectile");
        CombatStep {
            tick: Tick(t),
            collisions: vec![ProjectileCollision {
                tick: Tick(t),
                projectile_id: p,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(id),
                final_position: SimulationVector::new(3.0, 3.0),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            }],
            raw_damage_intents: vec![RawDamageIntent {
                tick: Tick(t),
                projectile_id: p,
                source: RawDamageIntentSource::Primary,
                target: id,
                base_raw_damage: 10_000,
                multiplier_basis_points: 10_000,
                resolved_raw_damage: 10_000,
                contact_ordinal: 0,
            }],
            ..CombatStep::default()
        }
    }

    fn trace() -> Vec<(u64, String)> {
        let mut f = fixture();
        let id = f.snapshot().actor_id;
        let mut out = Vec::new();
        for t in 0..=260 {
            let combat = if t == 260 { lethal(id, t) } else { empty(t) };
            let step = f.step(&combat).expect("step");
            if let Some(m) = step.mire {
                for event in m.attack_events {
                    let name = match event {
                        sim_core::CoreNormalAttackEvent::TelegraphStarted { .. } => "telegraph",
                        sim_core::CoreNormalAttackEvent::Released { .. } => "charge_start",
                        sim_core::CoreNormalAttackEvent::MireRetreatStarted { .. } => {
                            "retreat_start"
                        }
                        sim_core::CoreNormalAttackEvent::RotorRecoveryStarted { .. } => "invalid",
                    };
                    out.push((t, name.to_owned()));
                }
                for event in m.events {
                    let name = match event {
                        CoreMireEvent::Movement {
                            phase: sim_core::CoreMireMovementPhase::Approach,
                            ..
                        } => "approach",
                        CoreMireEvent::Movement {
                            phase: sim_core::CoreMireMovementPhase::Charge,
                            ..
                        } => "charge_move",
                        CoreMireEvent::Movement {
                            phase: sim_core::CoreMireMovementPhase::Retreat,
                            ..
                        } => "retreat_move",
                        CoreMireEvent::ChargeContact { .. } => "contact",
                        CoreMireEvent::TargetlessReset { .. } => "reset",
                    };
                    out.push((t, name.to_owned()));
                }
            }
            if t == 260 {
                let r = step.reward_handoff.expect("reward");
                assert_eq!(r.reward_profile_id, "reward.normal_outer");
                assert_eq!(r.xp_profile_id, "xp.normal_t1");
            } else {
                assert!(step.reward_handoff.is_none());
            }
        }
        out
    }

    #[test]
    fn sustained_mire_trace_replays_exact_charge_contact_and_retreat_boundaries() {
        let a = trace();
        assert_eq!(a, trace());
        for (t, n) in [
            (27, "telegraph"),
            (39, "charge_start"),
            (54, "retreat_start"),
            (125, "telegraph"),
            (134, "charge_start"),
        ] {
            assert!(
                a.iter().any(|e| e.0 == t && e.1 == n),
                "missing {t} {n}: {a:?}"
            );
        }
        assert_eq!(
            a.iter()
                .filter(|e| (39..=53).contains(&e.0) && e.1 == "charge_move")
                .count(),
            15
        );
        assert_eq!(
            a.iter()
                .filter(|e| (54..=98).contains(&e.0) && e.1 == "retreat_move")
                .count(),
            45
        );
        assert_eq!(
            a.iter()
                .filter(|e| (39..=53).contains(&e.0) && e.1 == "contact")
                .count(),
            1
        );
    }

    #[test]
    fn reset_during_charge_restores_spawn_health_and_first_warning() {
        let mut f = fixture();
        for t in 0..=45 {
            f.step(&empty(t)).expect("step");
        }
        assert_ne!(f.mire.position(), f.spawn);
        f.reset().expect("reset");
        assert_eq!(f.mire.position(), f.spawn);
        assert_eq!(f.snapshot().current_health, 70);
        for t in 46..73 {
            let s = f.step(&empty(t)).expect("warning wait");
            assert!(s.reward_handoff.is_none());
        }
        let s = f.step(&empty(73)).expect("warning");
        assert!(
            s.mire
                .expect("Mire")
                .attack_events
                .iter()
                .any(|event| matches!(
                    event,
                    sim_core::CoreNormalAttackEvent::TelegraphStarted {
                        first_use: true,
                        ..
                    }
                ))
        );
    }
}
