//! Deterministic friendly-damage, enemy-death, and normal-drop scheduling for the M01 laboratory.
//!
//! The GDD `COM-001` through `COM-003` owns damage order and `SIM-010`/`SIM-011` own authority
//! and replay stability. `CONT-FP-004` supplies exact health, armor, hurtboxes, and reward binding;
//! its `250 ms` normal-drop delay compiles with the fairness ceiling to eight ticks. The roadmap
//! places this integration after `GB-M01-03A`–`03C` and the combat intent seam, before full waves.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    BELL_REED_ID, BellReedDefinition, CHAIN_SENTRY_ID, ChainSentryDefinition, CollisionTarget,
    CombatStep, DROWNED_PILGRIM_ID, DamageError, DamageEvent, DamageType, DirectHitParameters,
    DirectHitRequest, DrownedPilgrimDefinition, EnemyHurtbox, EntityId, FriendlyProjectileSource,
    HurtboxError, NORMAL_ENEMY_REWARD_TABLE_ID, ProjectileCollision, RawDamageIntent,
    RawDamageIntentSource, SimulationVector, Tick, resolve_direct_hit,
};

pub const NORMAL_REWARD_DROP_DELAY_TICKS: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FirstPlayableEnemyKind {
    DrownedPilgrim,
    BellReed,
    ChainSentry,
}

impl FirstPlayableEnemyKind {
    #[must_use]
    pub const fn content_id(self) -> &'static str {
        match self {
            Self::DrownedPilgrim => DROWNED_PILGRIM_ID,
            Self::BellReed => BELL_REED_ID,
            Self::ChainSentry => CHAIN_SENTRY_ID,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnemyHealthActor {
    actor_id: EntityId,
    kind: FirstPlayableEnemyKind,
    max_health: u32,
    current_health: u32,
    armor: u32,
    hurtbox_radius_tiles: f32,
    position: SimulationVector,
    reward_table_id: &'static str,
    alive: bool,
    death_tick: Option<Tick>,
    frostbind_expires_tick: Option<Tick>,
}

impl EnemyHealthActor {
    #[must_use]
    pub fn drowned_pilgrim(
        actor_id: EntityId,
        definition: &DrownedPilgrimDefinition,
        position: SimulationVector,
    ) -> Self {
        let parameters = definition.parameters();
        Self::from_exact(
            actor_id,
            FirstPlayableEnemyKind::DrownedPilgrim,
            parameters.health,
            parameters.armor,
            parameters.hurtbox_radius_milli_tiles,
            position,
            parameters.reward_table_id.as_str(),
        )
    }

    #[must_use]
    pub fn bell_reed(
        actor_id: EntityId,
        definition: &BellReedDefinition,
        position: SimulationVector,
    ) -> Self {
        let parameters = definition.parameters();
        Self::from_exact(
            actor_id,
            FirstPlayableEnemyKind::BellReed,
            parameters.health,
            parameters.armor,
            parameters.hurtbox_radius_milli_tiles,
            position,
            parameters.reward_table_id.as_str(),
        )
    }

    #[must_use]
    pub fn chain_sentry(
        actor_id: EntityId,
        definition: &ChainSentryDefinition,
        position: SimulationVector,
    ) -> Self {
        let parameters = definition.parameters();
        Self::from_exact(
            actor_id,
            FirstPlayableEnemyKind::ChainSentry,
            parameters.health,
            parameters.armor,
            parameters.hurtbox_radius_milli_tiles,
            position,
            parameters.reward_table_id.as_str(),
        )
    }

    fn from_exact(
        actor_id: EntityId,
        kind: FirstPlayableEnemyKind,
        max_health: u32,
        armor: u32,
        hurtbox_radius_milli_tiles: u32,
        position: SimulationVector,
        reward_table_id: &str,
    ) -> Self {
        debug_assert_eq!(reward_table_id, NORMAL_ENEMY_REWARD_TABLE_ID);
        Self {
            actor_id,
            kind,
            max_health,
            current_health: max_health,
            armor,
            hurtbox_radius_tiles: milli_to_tiles(hurtbox_radius_milli_tiles),
            position,
            reward_table_id: NORMAL_ENEMY_REWARD_TABLE_ID,
            alive: true,
            death_tick: None,
            frostbind_expires_tick: None,
        }
    }

    #[must_use]
    pub const fn actor_id(&self) -> EntityId {
        self.actor_id
    }

    #[must_use]
    pub const fn kind(&self) -> FirstPlayableEnemyKind {
        self.kind
    }

    #[must_use]
    pub const fn max_health(&self) -> u32 {
        self.max_health
    }

    #[must_use]
    pub const fn current_health(&self) -> u32 {
        self.current_health
    }

    #[must_use]
    pub const fn armor(&self) -> u32 {
        self.armor
    }

    #[must_use]
    pub const fn position(&self) -> SimulationVector {
        self.position
    }

    #[must_use]
    pub const fn alive(&self) -> bool {
        self.alive
    }

    #[must_use]
    pub const fn death_tick(&self) -> Option<Tick> {
        self.death_tick
    }

    #[must_use]
    pub const fn frostbind_expires_tick(&self) -> Option<Tick> {
        self.frostbind_expires_tick
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyHealthSnapshot {
    pub actor_id: EntityId,
    pub kind: FirstPlayableEnemyKind,
    pub max_health: u32,
    pub current_health: u32,
    pub armor: u32,
    pub alive: bool,
    pub death_tick: Option<Tick>,
    pub frostbind_expires_tick: Option<Tick>,
}

#[derive(Debug, Clone, PartialEq)]
struct ScheduledNormalDrop {
    actor_id: EntityId,
    enemy_kind: FirstPlayableEnemyKind,
    reward_table_id: &'static str,
    death_tick: Tick,
    due_tick: Tick,
    position: SimulationVector,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalRewardDropEvent {
    pub actor_id: EntityId,
    pub enemy_kind: FirstPlayableEnemyKind,
    pub reward_table_id: &'static str,
    pub death_tick: Tick,
    pub due_tick: Tick,
    pub position: SimulationVector,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyDamageEvent {
    pub tick: Tick,
    pub intent_index: u32,
    pub projectile_id: EntityId,
    pub contact_ordinal: u32,
    pub intent_source: RawDamageIntentSource,
    pub target: EntityId,
    pub base_raw_damage: u32,
    pub authored_multiplier_basis_points: u32,
    pub resolved_raw_damage: u32,
    pub damage: DamageEvent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnemyDeathEvent {
    pub tick: Tick,
    pub actor_id: EntityId,
    pub enemy_kind: FirstPlayableEnemyKind,
    pub lethal_projectile_id: EntityId,
    pub lethal_contact_ordinal: u32,
    pub position: SimulationVector,
    pub reward_due_tick: Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IgnoredFriendlyIntent {
    pub tick: Tick,
    pub intent_index: u32,
    pub projectile_id: EntityId,
    pub contact_ordinal: u32,
    pub target: EntityId,
    pub reason: IgnoredIntentReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoredIntentReason {
    TargetAlreadyDead,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnemyHealthStep {
    pub tick: Tick,
    pub damage_events: Vec<EnemyDamageEvent>,
    pub death_events: Vec<EnemyDeathEvent>,
    pub ignored_intents: Vec<IgnoredFriendlyIntent>,
    pub frostbind_events: Vec<EnemyFrostbindEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyFrostbindEvent {
    pub tick: Tick,
    pub source_trap_id: EntityId,
    pub target: EntityId,
    pub duration_ticks: u32,
    pub expires_tick: Tick,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnemyHealthSimulation {
    actors: Vec<EnemyHealthActor>,
    latest_tick: Option<Tick>,
    scheduled_drops: Vec<ScheduledNormalDrop>,
}

impl EnemyHealthSimulation {
    pub fn new(mut actors: Vec<EnemyHealthActor>) -> Result<Self, EnemyHealthError> {
        actors.sort_by_key(EnemyHealthActor::actor_id);
        let mut ids = BTreeSet::new();
        for actor in &actors {
            if !actor.position.is_finite() {
                return Err(EnemyHealthError::NonFiniteActorPosition(actor.actor_id));
            }
            if !ids.insert(actor.actor_id) {
                return Err(EnemyHealthError::DuplicateActorId(actor.actor_id));
            }
        }
        Ok(Self {
            actors,
            latest_tick: None,
            scheduled_drops: Vec::new(),
        })
    }

    #[must_use]
    pub const fn latest_tick(&self) -> Option<Tick> {
        self.latest_tick
    }

    #[must_use]
    pub fn snapshots(&self) -> Vec<EnemyHealthSnapshot> {
        self.actors
            .iter()
            .map(|actor| EnemyHealthSnapshot {
                actor_id: actor.actor_id,
                kind: actor.kind,
                max_health: actor.max_health,
                current_health: actor.current_health,
                armor: actor.armor,
                alive: actor.alive,
                death_tick: actor.death_tick,
                frostbind_expires_tick: actor.frostbind_expires_tick,
            })
            .collect()
    }

    pub fn alive_hurtboxes(&self) -> Result<Vec<EnemyHurtbox>, EnemyHealthError> {
        self.actors
            .iter()
            .filter(|actor| actor.alive)
            .map(|actor| {
                EnemyHurtbox::new(actor.actor_id, actor.position, actor.hurtbox_radius_tiles)
                    .map_err(EnemyHealthError::Hurtbox)
            })
            .collect()
    }

    /// Synchronizes an authoritative actor position before collision/death processing.
    pub fn update_actor_position(
        &mut self,
        actor_id: EntityId,
        position: SimulationVector,
    ) -> Result<(), EnemyHealthError> {
        if !position.is_finite() {
            return Err(EnemyHealthError::NonFiniteActorPosition(actor_id));
        }
        let index = self
            .actors
            .binary_search_by_key(&actor_id, EnemyHealthActor::actor_id)
            .map_err(|_| EnemyHealthError::UnknownTarget(actor_id))?;
        self.actors[index].position = position;
        Ok(())
    }

    /// Applies one combat tick transactionally, preserving raw-intent vector order exactly.
    pub fn apply_combat_step(
        &mut self,
        step: &CombatStep,
    ) -> Result<EnemyHealthStep, EnemyHealthError> {
        let mut next = self.clone();
        let result = next.apply_combat_step_inner(step)?;
        *self = next;
        Ok(result)
    }

    fn apply_combat_step_inner(
        &mut self,
        step: &CombatStep,
    ) -> Result<EnemyHealthStep, EnemyHealthError> {
        if self.latest_tick.is_some_and(|latest| step.tick <= latest) {
            return Err(EnemyHealthError::NonMonotonicCombatTick {
                received: step.tick,
                latest: self.latest_tick.expect("checked Some"),
            });
        }
        validate_intent_order_and_provenance(step)?;
        let mut output = EnemyHealthStep {
            tick: step.tick,
            ..EnemyHealthStep::default()
        };
        for (index, intent) in step.raw_damage_intents.iter().copied().enumerate() {
            let intent_index =
                u32::try_from(index).map_err(|_| EnemyHealthError::TooManyIntents)?;
            let actor_index = self
                .actors
                .binary_search_by_key(&intent.target, EnemyHealthActor::actor_id)
                .map_err(|_| EnemyHealthError::UnknownTarget(intent.target))?;
            let actor = &mut self.actors[actor_index];
            if !actor.alive {
                output.ignored_intents.push(IgnoredFriendlyIntent {
                    tick: step.tick,
                    intent_index,
                    projectile_id: intent.projectile_id,
                    contact_ordinal: intent.contact_ordinal,
                    target: intent.target,
                    reason: IgnoredIntentReason::TargetAlreadyDead,
                });
                continue;
            }
            let request = DirectHitRequest::new(DirectHitParameters {
                source: intent.projectile_id,
                target: actor.actor_id,
                collision_confirmed: true,
                target_is_immune: false,
                raw_damage: intent.resolved_raw_damage,
                damage_type: DamageType::Physical,
                attacker_multiplier_basis_points: step.attacker_multiplier_basis_points,
                target_resistance_basis_points: 0,
                direct_damage_reductions_basis_points: Vec::new(),
                armor: actor.armor,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
                current_health: actor.current_health,
                max_health: actor.max_health,
            })?;
            let damage = resolve_direct_hit(&request)?;
            actor.current_health = damage.health_after;
            output.damage_events.push(EnemyDamageEvent {
                tick: step.tick,
                intent_index,
                projectile_id: intent.projectile_id,
                contact_ordinal: intent.contact_ordinal,
                intent_source: intent.source,
                target: intent.target,
                base_raw_damage: intent.base_raw_damage,
                authored_multiplier_basis_points: intent.multiplier_basis_points,
                resolved_raw_damage: intent.resolved_raw_damage,
                damage: damage.clone(),
            });
            apply_nail_trap_frostbind(actor, step, intent, damage.lethal, &mut output)?;
            if damage.lethal {
                actor.alive = false;
                actor.death_tick = Some(step.tick);
                let due_tick = add_ticks(step.tick, NORMAL_REWARD_DROP_DELAY_TICKS)?;
                self.scheduled_drops.push(ScheduledNormalDrop {
                    actor_id: actor.actor_id,
                    enemy_kind: actor.kind,
                    reward_table_id: actor.reward_table_id,
                    death_tick: step.tick,
                    due_tick,
                    position: actor.position,
                });
                output.death_events.push(EnemyDeathEvent {
                    tick: step.tick,
                    actor_id: actor.actor_id,
                    enemy_kind: actor.kind,
                    lethal_projectile_id: intent.projectile_id,
                    lethal_contact_ordinal: intent.contact_ordinal,
                    position: actor.position,
                    reward_due_tick: due_tick,
                });
            }
        }
        self.latest_tick = Some(step.tick);
        self.scheduled_drops
            .sort_by_key(|drop| (drop.due_tick, drop.actor_id));
        Ok(output)
    }

    /// Collects every due normal reward once in `(due_tick, actor_id)` order.
    pub fn collect_due_drops(
        &mut self,
        current_tick: Tick,
    ) -> Result<Vec<NormalRewardDropEvent>, EnemyHealthError> {
        if self.latest_tick.is_some_and(|latest| current_tick < latest) {
            return Err(EnemyHealthError::DropCollectionBeforeLatestTick {
                requested: current_tick,
                latest: self.latest_tick.expect("checked Some"),
            });
        }
        let due_count = self
            .scheduled_drops
            .partition_point(|drop| drop.due_tick <= current_tick);
        let events = self
            .scheduled_drops
            .drain(..due_count)
            .map(|drop| NormalRewardDropEvent {
                actor_id: drop.actor_id,
                enemy_kind: drop.enemy_kind,
                reward_table_id: drop.reward_table_id,
                death_tick: drop.death_tick,
                due_tick: drop.due_tick,
                position: drop.position,
            })
            .collect();
        self.latest_tick = Some(current_tick);
        Ok(events)
    }
}

fn apply_nail_trap_frostbind(
    actor: &mut EnemyHealthActor,
    step: &CombatStep,
    intent: RawDamageIntent,
    lethal: bool,
    output: &mut EnemyHealthStep,
) -> Result<(), EnemyHealthError> {
    if intent.source != RawDamageIntentSource::NailTrap || lethal {
        return Ok(());
    }
    let trigger = step
        .nail_traps
        .triggers
        .iter()
        .find(|trigger| {
            trigger.trap_id == intent.projectile_id && trigger.target_id == intent.target
        })
        .ok_or(EnemyHealthError::InvalidCollisionProvenance {
            projectile_id: intent.projectile_id,
            contact_ordinal: intent.contact_ordinal,
            matches: 0,
        })?;
    let expires_tick = add_ticks(step.tick, trigger.frostbind_ticks)?;
    actor.frostbind_expires_tick = Some(expires_tick);
    output.frostbind_events.push(EnemyFrostbindEvent {
        tick: step.tick,
        source_trap_id: trigger.trap_id,
        target: trigger.target_id,
        duration_ticks: trigger.frostbind_ticks,
        expires_tick,
    });
    Ok(())
}

fn validate_intent_order_and_provenance(step: &CombatStep) -> Result<(), EnemyHealthError> {
    let mut previous = None;
    let mut seen = BTreeSet::new();
    for intent in &step.raw_damage_intents {
        if intent.tick != step.tick {
            return Err(EnemyHealthError::IntentTickMismatch {
                step: step.tick,
                intent: intent.tick,
            });
        }
        let key = (intent.projectile_id, intent.contact_ordinal);
        if previous.is_some_and(|prior| key < prior) {
            return Err(EnemyHealthError::UnstableIntentOrder);
        }
        previous = Some(key);
        if !seen.insert(key) {
            return Err(EnemyHealthError::DuplicateIntentProvenance {
                projectile_id: intent.projectile_id,
                contact_ordinal: intent.contact_ordinal,
            });
        }
        if intent.source == RawDamageIntentSource::NailTrap {
            let matching = step
                .nail_traps
                .triggers
                .iter()
                .filter(|trigger| {
                    trigger.tick == intent.tick
                        && trigger.trap_id == intent.projectile_id
                        && trigger.target_id == intent.target
                        && trigger.snapshot_weapon_raw_damage == intent.base_raw_damage
                        && trigger.raw_damage == intent.resolved_raw_damage
                })
                .count();
            if matching != 1 {
                return Err(EnemyHealthError::InvalidCollisionProvenance {
                    projectile_id: intent.projectile_id,
                    contact_ordinal: intent.contact_ordinal,
                    matches: matching,
                });
            }
            continue;
        }
        let expected_source = match intent.source {
            RawDamageIntentSource::Primary => FriendlyProjectileSource::Primary,
            RawDamageIntentSource::BellDebtRepeat => FriendlyProjectileSource::BellDebtRepeat,
            RawDamageIntentSource::GraveMark => FriendlyProjectileSource::GraveMark,
            RawDamageIntentSource::NailTrap => unreachable!("handled above"),
        };
        let matching: Vec<_> = step
            .collisions
            .iter()
            .filter(|collision| collision_matches(collision, intent, expected_source))
            .collect();
        if matching.len() != 1 {
            return Err(EnemyHealthError::InvalidCollisionProvenance {
                projectile_id: intent.projectile_id,
                contact_ordinal: intent.contact_ordinal,
                matches: matching.len(),
            });
        }
    }
    Ok(())
}

fn collision_matches(
    collision: &ProjectileCollision,
    intent: &RawDamageIntent,
    source: FriendlyProjectileSource,
) -> bool {
    collision.tick == intent.tick
        && collision.projectile_id == intent.projectile_id
        && collision.source == source
        && collision.contact_ordinal == intent.contact_ordinal
        && collision.target == CollisionTarget::Enemy(intent.target)
}

fn add_ticks(tick: Tick, count: u32) -> Result<Tick, EnemyHealthError> {
    tick.0
        .checked_add(u64::from(count))
        .map(Tick)
        .ok_or(EnemyHealthError::TickOverflow)
}

#[allow(clippy::cast_precision_loss)]
fn milli_to_tiles(value: u32) -> f32 {
    value as f32 / 1_000.0
}

#[derive(Debug, Error)]
pub enum EnemyHealthError {
    #[error("duplicate enemy actor ID {0}")]
    DuplicateActorId(EntityId),
    #[error("enemy actor {0} position must be finite")]
    NonFiniteActorPosition(EntityId),
    #[error("combat tick {received} is not later than already committed tick {latest}")]
    NonMonotonicCombatTick { received: Tick, latest: Tick },
    #[error("raw damage intent tick {intent} differs from combat step tick {step}")]
    IntentTickMismatch { step: Tick, intent: Tick },
    #[error("raw damage intents are not in stable projectile/contact order")]
    UnstableIntentOrder,
    #[error(
        "duplicate damage intent provenance for projectile {projectile_id} contact {contact_ordinal}"
    )]
    DuplicateIntentProvenance {
        projectile_id: EntityId,
        contact_ordinal: u32,
    },
    #[error(
        "projectile {projectile_id} contact {contact_ordinal} has {matches} matching collisions, expected one"
    )]
    InvalidCollisionProvenance {
        projectile_id: EntityId,
        contact_ordinal: u32,
        matches: usize,
    },
    #[error("raw damage intent targets unknown enemy {0}")]
    UnknownTarget(EntityId),
    #[error("combat step contains more than u32::MAX intents")]
    TooManyIntents,
    #[error("reward drop tick overflow")]
    TickOverflow,
    #[error("drop collection tick {requested} precedes latest combat tick {latest}")]
    DropCollectionBeforeLatestTick { requested: Tick, latest: Tick },
    #[error(transparent)]
    Damage(#[from] DamageError),
    #[error(transparent)]
    Hurtbox(#[from] HurtboxError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero ID")
    }

    fn simulation() -> EnemyHealthSimulation {
        EnemyHealthSimulation::new(vec![
            EnemyHealthActor::drowned_pilgrim(
                id(100),
                &DrownedPilgrimDefinition::first_playable(),
                SimulationVector::new(8.0, 3.0),
            ),
            EnemyHealthActor::bell_reed(
                id(200),
                &BellReedDefinition::first_playable(),
                SimulationVector::new(16.0, 3.0),
            ),
            EnemyHealthActor::chain_sentry(
                id(300),
                &ChainSentryDefinition::first_playable(),
                SimulationVector::new(16.0, 12.0),
            ),
        ])
        .expect("health simulation")
    }

    #[allow(clippy::too_many_arguments)]
    fn intent(
        tick: u64,
        projectile: u64,
        source: RawDamageIntentSource,
        target: u64,
        base: u32,
        multiplier: u32,
        resolved: u32,
        ordinal: u32,
    ) -> RawDamageIntent {
        RawDamageIntent {
            tick: Tick(tick),
            projectile_id: id(projectile),
            source,
            target: id(target),
            base_raw_damage: base,
            multiplier_basis_points: multiplier,
            resolved_raw_damage: resolved,
            contact_ordinal: ordinal,
        }
    }

    fn collision(intent: &RawDamageIntent) -> ProjectileCollision {
        ProjectileCollision {
            tick: intent.tick,
            projectile_id: intent.projectile_id,
            source: match intent.source {
                RawDamageIntentSource::Primary => FriendlyProjectileSource::Primary,
                RawDamageIntentSource::BellDebtRepeat => FriendlyProjectileSource::BellDebtRepeat,
                RawDamageIntentSource::GraveMark => FriendlyProjectileSource::GraveMark,
                RawDamageIntentSource::NailTrap => {
                    panic!("test projectile collision cannot represent a nail trap")
                }
            },
            target: CollisionTarget::Enemy(intent.target),
            final_position: SimulationVector::new(8.0, 3.0),
            distance_travelled_tiles: 1.0,
            contact_ordinal: intent.contact_ordinal,
            empowered_by_slipstep: false,
            focused_by_stillness: false,
            projectile_continues: false,
        }
    }

    fn step(tick: u64, intents: Vec<RawDamageIntent>) -> CombatStep {
        CombatStep {
            tick: Tick(tick),
            collisions: intents.iter().map(collision).collect(),
            raw_damage_intents: intents,
            ..CombatStep::default()
        }
    }

    #[test]
    fn pine_primary_uses_each_exact_enemy_armor_and_alive_hurtboxes() {
        let mut simulation = simulation();
        let intents = vec![
            intent(1, 1, RawDamageIntentSource::Primary, 100, 20, 10_000, 20, 0),
            intent(1, 2, RawDamageIntentSource::Primary, 200, 20, 10_000, 20, 0),
            intent(1, 3, RawDamageIntentSource::Primary, 300, 20, 10_000, 20, 0),
        ];
        let output = simulation
            .apply_combat_step(&step(1, intents))
            .expect("damage");
        assert_eq!(
            output
                .damage_events
                .iter()
                .map(|event| event.damage.health_damage_applied)
                .collect::<Vec<_>>(),
            vec![20, 18, 15]
        );
        assert_eq!(
            simulation
                .snapshots()
                .iter()
                .map(|snapshot| snapshot.current_health)
                .collect::<Vec<_>>(),
            vec![65, 112, 285]
        );
        assert_eq!(simulation.alive_hurtboxes().expect("hurtboxes").len(), 3);
    }

    #[test]
    fn cinder_attacker_stage_covers_primary_grave_mark_and_nail_trap_intents() {
        let cases = [
            (RawDamageIntentSource::Primary, 20, 24),
            (RawDamageIntentSource::GraveMark, 10, 12),
            (RawDamageIntentSource::NailTrap, 18, 21),
        ];
        for (source, resolved_raw_damage, expected_damage) in cases {
            let mut simulation = simulation();
            let damage_intent = intent(
                1,
                40,
                source,
                100,
                resolved_raw_damage,
                10_000,
                resolved_raw_damage,
                0,
            );
            let mut combat = if source == RawDamageIntentSource::NailTrap {
                CombatStep {
                    tick: Tick(1),
                    raw_damage_intents: vec![damage_intent],
                    nail_traps: crate::NailTrapStep {
                        triggers: vec![crate::NailTrapTrigger {
                            trap_id: id(40),
                            target_id: id(100),
                            tick: Tick(1),
                            position: SimulationVector::new(8.0, 3.0),
                            raw_damage: resolved_raw_damage,
                            snapshot_weapon_raw_damage: resolved_raw_damage,
                            frostbind_ticks: 45,
                        }],
                        ..crate::NailTrapStep::default()
                    },
                    ..CombatStep::default()
                }
            } else {
                step(1, vec![damage_intent])
            };
            combat.attacker_multiplier_basis_points = 11_800;
            let output = simulation.apply_combat_step(&combat).unwrap();
            let event = &output.damage_events[0];
            assert_eq!(event.resolved_raw_damage, resolved_raw_damage);
            assert_eq!(event.damage.attacker_multiplier_basis_points, 11_800);
            assert_eq!(event.damage.health_damage_applied, expected_damage);
        }
    }

    #[test]
    fn nailkeeper_trigger_applies_physical_damage_and_full_normal_frostbind() {
        let mut simulation = simulation();
        let trigger = crate::NailTrapTrigger {
            trap_id: id(40),
            target_id: id(100),
            tick: Tick(1),
            position: SimulationVector::new(8.0, 3.0),
            raw_damage: 18,
            snapshot_weapon_raw_damage: 20,
            frostbind_ticks: 45,
        };
        let combat = CombatStep {
            tick: Tick(1),
            raw_damage_intents: vec![intent(
                1,
                40,
                RawDamageIntentSource::NailTrap,
                100,
                20,
                9_000,
                18,
                0,
            )],
            nail_traps: crate::NailTrapStep {
                triggers: vec![trigger],
                ..crate::NailTrapStep::default()
            },
            ..CombatStep::default()
        };
        let output = simulation.apply_combat_step(&combat).unwrap();
        assert_eq!(output.damage_events[0].damage.health_damage_applied, 18);
        assert_eq!(
            output.frostbind_events,
            vec![EnemyFrostbindEvent {
                tick: Tick(1),
                source_trap_id: id(40),
                target: id(100),
                duration_ticks: 45,
                expires_tick: Tick(46),
            }]
        );
        assert_eq!(
            simulation.snapshots()[0].frostbind_expires_tick,
            Some(Tick(46))
        );
    }

    #[test]
    fn marked_and_focused_resolved_intents_are_not_recomposed() {
        let mut simulation = simulation();
        let intents = vec![
            intent(
                1,
                1,
                RawDamageIntentSource::GraveMark,
                100,
                20,
                18_000,
                36,
                0,
            ),
            intent(1, 2, RawDamageIntentSource::Primary, 200, 20, 11_500, 23, 0),
            intent(1, 3, RawDamageIntentSource::Primary, 300, 20, 10_800, 22, 0),
        ];
        let output = simulation
            .apply_combat_step(&step(1, intents))
            .expect("damage");
        assert_eq!(output.damage_events[0].damage.raw_damage, 36);
        assert_eq!(output.damage_events[1].damage.raw_damage, 23);
        assert_eq!(output.damage_events[2].damage.raw_damage, 22);
        assert_eq!(
            output.damage_events[1].authored_multiplier_basis_points,
            11_500
        );
    }

    #[test]
    fn same_tick_lethal_is_committed_once_and_later_intents_are_ignored() {
        let mut simulation = simulation();
        let intents = vec![
            intent(
                5,
                1,
                RawDamageIntentSource::GraveMark,
                100,
                20,
                18_000,
                36,
                0,
            ),
            intent(
                5,
                2,
                RawDamageIntentSource::GraveMark,
                100,
                20,
                18_000,
                36,
                0,
            ),
            intent(
                5,
                3,
                RawDamageIntentSource::GraveMark,
                100,
                20,
                18_000,
                36,
                0,
            ),
            intent(5, 4, RawDamageIntentSource::Primary, 100, 20, 10_000, 20, 0),
        ];
        let output = simulation
            .apply_combat_step(&step(5, intents))
            .expect("lethal");
        assert_eq!(output.damage_events.len(), 3);
        assert_eq!(output.death_events.len(), 1);
        assert_eq!(output.death_events[0].lethal_projectile_id, id(3));
        assert_eq!(output.ignored_intents.len(), 1);
        assert_eq!(simulation.snapshots()[0].current_health, 0);
        assert_eq!(simulation.alive_hurtboxes().expect("hurtboxes").len(), 2);

        let post_death = intent(6, 5, RawDamageIntentSource::Primary, 100, 20, 10_000, 20, 0);
        let later = simulation
            .apply_combat_step(&step(6, vec![post_death]))
            .expect("post-death intent");
        assert!(later.damage_events.is_empty());
        assert_eq!(later.ignored_intents.len(), 1);
        assert!(later.death_events.is_empty());
    }

    #[test]
    fn normal_drop_appears_exactly_eight_ticks_after_death_once() {
        let mut simulation = simulation();
        simulation
            .update_actor_position(id(100), SimulationVector::new(9.0, 4.0))
            .expect("authoritative actor position");
        let lethal = vec![
            intent(
                10,
                1,
                RawDamageIntentSource::GraveMark,
                100,
                20,
                18_000,
                36,
                0,
            ),
            intent(
                10,
                2,
                RawDamageIntentSource::GraveMark,
                100,
                20,
                18_000,
                36,
                0,
            ),
            intent(
                10,
                3,
                RawDamageIntentSource::GraveMark,
                100,
                20,
                18_000,
                36,
                0,
            ),
        ];
        simulation
            .apply_combat_step(&step(10, lethal))
            .expect("death");
        assert!(
            simulation
                .collect_due_drops(Tick(17))
                .expect("not due")
                .is_empty()
        );
        let drops = simulation.collect_due_drops(Tick(18)).expect("due");
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].actor_id, id(100));
        assert_eq!(drops[0].reward_table_id, NORMAL_ENEMY_REWARD_TABLE_ID);
        assert_eq!(drops[0].position, SimulationVector::new(9.0, 4.0));
        assert!(
            simulation
                .collect_due_drops(Tick(99))
                .expect("once")
                .is_empty()
        );
    }

    #[test]
    fn invalid_provenance_rolls_back_all_health() {
        let mut simulation = simulation();
        let before = simulation.clone();
        let intent = intent(1, 1, RawDamageIntentSource::Primary, 100, 20, 10_000, 20, 0);
        let invalid = CombatStep {
            tick: Tick(1),
            raw_damage_intents: vec![intent],
            ..CombatStep::default()
        };
        assert!(matches!(
            simulation.apply_combat_step(&invalid),
            Err(EnemyHealthError::InvalidCollisionProvenance { .. })
        ));
        assert_eq!(simulation, before);
    }

    #[test]
    fn fixed_health_and_drop_replay_is_identical() {
        fn replay() -> blake3::Hash {
            let mut simulation = simulation();
            let first = vec![
                intent(1, 1, RawDamageIntentSource::Primary, 100, 20, 10_000, 20, 0),
                intent(1, 2, RawDamageIntentSource::Primary, 200, 20, 10_000, 20, 0),
            ];
            let second = vec![
                intent(
                    2,
                    3,
                    RawDamageIntentSource::GraveMark,
                    100,
                    20,
                    18_000,
                    36,
                    0,
                ),
                intent(
                    2,
                    4,
                    RawDamageIntentSource::GraveMark,
                    100,
                    20,
                    18_000,
                    36,
                    0,
                ),
                intent(2, 5, RawDamageIntentSource::Primary, 100, 20, 10_000, 20, 0),
            ];
            let mut hasher = blake3::Hasher::new();
            hasher
                .update(format!("{:?}", simulation.apply_combat_step(&step(1, first))).as_bytes());
            hasher
                .update(format!("{:?}", simulation.apply_combat_step(&step(2, second))).as_bytes());
            hasher.update(format!("{:?}", simulation.collect_due_drops(Tick(10))).as_bytes());
            hasher.update(format!("{:?}", simulation.snapshots()).as_bytes());
            hasher.finalize()
        }
        let first = replay();
        assert_eq!(
            first.to_string(),
            "6542260da7100ff09eeb9dc30996a67f674980530c75517da74ec289a70a47ed"
        );
        assert_eq!(first, replay());
    }
}
