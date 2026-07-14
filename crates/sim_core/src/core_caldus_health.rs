//! Real scaled Sir Caldus health, hurtbox, armor, and friendly-damage authority.
//!
//! `ENC-010`, `CONT-BOSS-001`/`002`, `GB-M03-03`, and approved `SPEC-CONFLICT-022`
//! establish the exact values and ordering. Collision provenance is shared with every other
//! authoritative friendly projectile through `friendly_intent`.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::{
    CombatStep, CoreBossParticipant, CoreBossParticipantLock, CoreCaldusState, DamageError,
    DamageEvent, DamageType, DirectHitParameters, DirectHitRequest, EnemyHurtbox, EntityId,
    HurtboxError, RawDamageIntentSource, SimulationVector, Tick, resolve_direct_hit,
};

pub const CALDUS_ARMOR: u32 = 10;
pub const CALDUS_HURTBOX_RADIUS_TILES: f32 = 0.62;
pub const CALDUS_BREAK_DAMAGE_BASIS_POINTS: u32 = 12_500;

#[derive(Debug, Clone, PartialEq)]
pub struct CoreCaldusFriendlyInput {
    pub participant: CoreBossParticipant,
    pub combat: CombatStep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusDamageEvent {
    pub tick: Tick,
    pub participant: CoreBossParticipant,
    pub projectile_id: EntityId,
    pub contact_ordinal: u32,
    pub source: RawDamageIntentSource,
    pub base_raw_damage: u32,
    pub authored_multiplier_basis_points: u32,
    pub break_multiplier_basis_points: u32,
    pub resolved_raw_damage: u32,
    pub damage: DamageEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCaldusDefeat {
    pub tick: Tick,
    pub entity_id: EntityId,
    pub participant: CoreBossParticipant,
    pub lethal_projectile_id: EntityId,
    pub lethal_contact_ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusHealthStep {
    pub tick: Tick,
    pub damage: Vec<CoreCaldusDamageEvent>,
    pub defeat: Option<CoreCaldusDefeat>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusHealthSimulation {
    lock: CoreBossParticipantLock,
    entity_id: EntityId,
    current_health: u32,
    tick: Tick,
    defeated: bool,
    contribution_damage: BTreeMap<EntityId, u64>,
}

impl CoreCaldusHealthSimulation {
    pub fn new(
        lock: CoreBossParticipantLock,
        entity_id: EntityId,
    ) -> Result<Self, CoreCaldusHealthError> {
        if lock.participants.is_empty() || lock.maximum_health == 0 {
            return Err(CoreCaldusHealthError::InvalidParticipantLock);
        }
        if lock
            .participants
            .iter()
            .any(|participant| participant.entity_id == entity_id)
        {
            return Err(CoreCaldusHealthError::BossMatchesParticipant);
        }
        let contribution_damage = lock
            .participants
            .iter()
            .map(|participant| (participant.entity_id, 0))
            .collect();
        Ok(Self {
            current_health: lock.maximum_health,
            lock,
            entity_id,
            tick: Tick(0),
            defeated: false,
            contribution_damage,
        })
    }

    #[must_use]
    pub const fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    #[must_use]
    pub const fn current_health(&self) -> u32 {
        self.current_health
    }

    #[must_use]
    pub const fn maximum_health(&self) -> u32 {
        self.lock.maximum_health
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn defeated(&self) -> bool {
        self.defeated
    }

    #[must_use]
    pub fn contribution_damage(&self, participant: CoreBossParticipant) -> Option<u64> {
        self.contribution_damage
            .get(&participant.entity_id)
            .copied()
    }

    pub fn hurtbox(
        &self,
        position: SimulationVector,
    ) -> Result<Option<EnemyHurtbox>, CoreCaldusHealthError> {
        if self.defeated {
            return Ok(None);
        }
        EnemyHurtbox::new(self.entity_id, position, CALDUS_HURTBOX_RADIUS_TILES)
            .map(Some)
            .map_err(CoreCaldusHealthError::Hurtbox)
    }

    pub fn apply_friendly_damage(
        &mut self,
        state: &CoreCaldusState,
        inputs: &[CoreCaldusFriendlyInput],
    ) -> Result<CoreCaldusHealthStep, CoreCaldusHealthError> {
        let mut staged = self.clone();
        let step = staged.apply_friendly_damage_inner(state, inputs)?;
        *self = staged;
        Ok(step)
    }

    fn apply_friendly_damage_inner(
        &mut self,
        state: &CoreCaldusState,
        inputs: &[CoreCaldusFriendlyInput],
    ) -> Result<CoreCaldusHealthStep, CoreCaldusHealthError> {
        validate_inputs(&self.lock, self.entity_id, self.tick, inputs)?;
        if self.defeated
            && inputs
                .iter()
                .any(|input| !input.combat.raw_damage_intents.is_empty())
        {
            return Err(CoreCaldusHealthError::DamageAfterDefeat);
        }
        let break_multiplier_basis_points = if matches!(state, CoreCaldusState::Break { .. }) {
            CALDUS_BREAK_DAMAGE_BASIS_POINTS
        } else {
            10_000
        };
        let mut damage = Vec::new();
        let mut defeat = None;
        for input in inputs {
            for intent in &input.combat.raw_damage_intents {
                if self.current_health == 0 {
                    break;
                }
                let resolved_raw_damage =
                    multiply_half_up(intent.resolved_raw_damage, break_multiplier_basis_points)?;
                let request = DirectHitRequest::new(DirectHitParameters {
                    source: intent.projectile_id,
                    target: self.entity_id,
                    collision_confirmed: true,
                    target_is_immune: false,
                    raw_damage: resolved_raw_damage,
                    damage_type: DamageType::Physical,
                    attacker_multiplier_basis_points: input.combat.attacker_multiplier_basis_points,
                    target_resistance_basis_points: 0,
                    direct_damage_reductions_basis_points: Vec::new(),
                    armor: CALDUS_ARMOR,
                    current_barrier: 0,
                    health_damage_cap_basis_points: None,
                    current_health: self.current_health,
                    max_health: self.lock.maximum_health,
                })?;
                let resolved = resolve_direct_hit(&request)?;
                self.current_health = resolved.health_after;
                let contribution = self
                    .contribution_damage
                    .get_mut(&input.participant.entity_id)
                    .ok_or(CoreCaldusHealthError::ParticipantOutsideLock)?;
                *contribution = contribution
                    .checked_add(u64::from(resolved.health_damage_applied))
                    .ok_or(CoreCaldusHealthError::ArithmeticOverflow)?;
                damage.push(CoreCaldusDamageEvent {
                    tick: self.tick,
                    participant: input.participant,
                    projectile_id: intent.projectile_id,
                    contact_ordinal: intent.contact_ordinal,
                    source: intent.source,
                    base_raw_damage: intent.base_raw_damage,
                    authored_multiplier_basis_points: intent.multiplier_basis_points,
                    break_multiplier_basis_points,
                    resolved_raw_damage,
                    damage: resolved.clone(),
                });
                if resolved.lethal {
                    self.defeated = true;
                    defeat = Some(CoreCaldusDefeat {
                        tick: self.tick,
                        entity_id: self.entity_id,
                        participant: input.participant,
                        lethal_projectile_id: intent.projectile_id,
                        lethal_contact_ordinal: intent.contact_ordinal,
                    });
                }
            }
        }
        let tick = self.tick;
        self.tick = Tick(
            self.tick
                .0
                .checked_add(1)
                .ok_or(CoreCaldusHealthError::ArithmeticOverflow)?,
        );
        Ok(CoreCaldusHealthStep {
            tick,
            damage,
            defeat,
        })
    }
}

fn validate_inputs(
    lock: &CoreBossParticipantLock,
    boss: EntityId,
    tick: Tick,
    inputs: &[CoreCaldusFriendlyInput],
) -> Result<(), CoreCaldusHealthError> {
    let mut previous = None;
    let mut seen = BTreeSet::new();
    for input in inputs {
        if !lock.participants.contains(&input.participant) {
            return Err(CoreCaldusHealthError::ParticipantOutsideLock);
        }
        let key = (input.participant.party_slot, input.participant.entity_id);
        if previous.is_some_and(|prior| key <= prior) || !seen.insert(input.participant.entity_id) {
            return Err(CoreCaldusHealthError::UnstableParticipantOrder);
        }
        previous = Some(key);
        if input.combat.tick != tick {
            return Err(CoreCaldusHealthError::CombatTickMismatch);
        }
        crate::friendly_intent::validate_friendly_intents(&input.combat, boss).map_err(
            |error| match error {
                crate::friendly_intent::FriendlyIntentError::InvalidProvenance => {
                    CoreCaldusHealthError::InvalidFriendlyIntent
                }
                crate::friendly_intent::FriendlyIntentError::UnstableOrder => {
                    CoreCaldusHealthError::InvalidFriendlyIntentOrder
                }
            },
        )?;
    }
    Ok(())
}

fn multiply_half_up(value: u32, basis_points: u32) -> Result<u32, CoreCaldusHealthError> {
    let scaled = u64::from(value)
        .checked_mul(u64::from(basis_points))
        .and_then(|value| value.checked_add(5_000))
        .ok_or(CoreCaldusHealthError::ArithmeticOverflow)?;
    u32::try_from(scaled / 10_000).map_err(|_| CoreCaldusHealthError::ArithmeticOverflow)
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreCaldusHealthError {
    #[error("Caldus health requires a valid nonempty scaled participant lock")]
    InvalidParticipantLock,
    #[error("Caldus entity identity collides with a participant")]
    BossMatchesParticipant,
    #[error("friendly damage participant is outside the immutable lock")]
    ParticipantOutsideLock,
    #[error("friendly damage participants are not uniquely sorted by immutable slot and entity")]
    UnstableParticipantOrder,
    #[error("friendly combat step does not match the Caldus health tick")]
    CombatTickMismatch,
    #[error("friendly damage intent has invalid collision provenance")]
    InvalidFriendlyIntent,
    #[error("friendly damage intent order is unstable or duplicated")]
    InvalidFriendlyIntentOrder,
    #[error("friendly damage was submitted after Caldus defeat")]
    DamageAfterDefeat,
    #[error("Caldus health arithmetic overflowed")]
    ArithmeticOverflow,
    #[error(transparent)]
    Damage(#[from] DamageError),
    #[error(transparent)]
    Hurtbox(#[from] HurtboxError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CollisionTarget, FriendlyProjectileSource, ProjectileCollision, RawDamageIntent};

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("entity")
    }

    fn participant(entity: u64, slot: u8) -> CoreBossParticipant {
        CoreBossParticipant {
            entity_id: id(entity),
            party_slot: slot,
        }
    }

    fn lock() -> CoreBossParticipantLock {
        CoreBossParticipantLock {
            attempt_ordinal: 1,
            participants: vec![participant(10, 0), participant(20, 1)],
            maximum_health: 12_384,
        }
    }

    fn damage_input(
        tick: u64,
        participant: CoreBossParticipant,
        projectile: u64,
        raw_damage: u32,
    ) -> CoreCaldusFriendlyInput {
        let projectile_id = id(projectile);
        CoreCaldusFriendlyInput {
            participant,
            combat: CombatStep {
                tick: Tick(tick),
                collisions: vec![ProjectileCollision {
                    tick: Tick(tick),
                    projectile_id,
                    source: FriendlyProjectileSource::Primary,
                    target: CollisionTarget::Enemy(id(99)),
                    final_position: SimulationVector::new(9.0, 9.0),
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
                    target: id(99),
                    base_raw_damage: raw_damage,
                    multiplier_basis_points: 10_000,
                    resolved_raw_damage: raw_damage,
                    contact_ordinal: 0,
                }],
                ..CombatStep::default()
            },
        }
    }

    #[test]
    fn scaled_health_armor_hurtbox_break_damage_and_contribution_are_real() {
        let mut health = CoreCaldusHealthSimulation::new(lock(), id(99)).expect("health");
        let hurtbox = health
            .hurtbox(SimulationVector::new(9.0, 9.0))
            .expect("hurtbox")
            .expect("alive");
        assert!((hurtbox.radius_tiles() - CALDUS_HURTBOX_RADIUS_TILES).abs() < f32::EPSILON);
        let active = health
            .apply_friendly_damage(
                &CoreCaldusState::Active {
                    phase: crate::CoreCaldusPhase::Phase1,
                    phase_tick: 0,
                    loop_tick: 0,
                    loop_length: 234,
                },
                &[damage_input(0, participant(10, 0), 100, 100)],
            )
            .expect("active damage");
        assert_eq!(active.damage[0].damage.armor, CALDUS_ARMOR);
        let active_applied = active.damage[0].damage.health_damage_applied;
        let break_step = health
            .apply_friendly_damage(
                &CoreCaldusState::Break {
                    entering: crate::CoreCaldusPhase::Phase2,
                    ends_at: Tick(120),
                },
                &[damage_input(1, participant(10, 0), 101, 100)],
            )
            .expect("break damage");
        assert_eq!(
            break_step.damage[0].break_multiplier_basis_points,
            CALDUS_BREAK_DAMAGE_BASIS_POINTS
        );
        assert!(break_step.damage[0].damage.health_damage_applied > active_applied);
        assert_eq!(
            health.contribution_damage(participant(10, 0)),
            Some(u64::from(
                active_applied + break_step.damage[0].damage.health_damage_applied
            ))
        );
    }

    #[test]
    fn lethal_damage_is_single_terminal_and_invalid_provenance_rolls_back() {
        let mut health = CoreCaldusHealthSimulation::new(lock(), id(99)).expect("health");
        let before = health.clone();
        let mut invalid = damage_input(0, participant(10, 0), 100, 10);
        invalid.combat.collisions.clear();
        assert_eq!(
            health
                .apply_friendly_damage(
                    &CoreCaldusState::Active {
                        phase: crate::CoreCaldusPhase::Phase1,
                        phase_tick: 0,
                        loop_tick: 0,
                        loop_length: 234,
                    },
                    &[invalid],
                )
                .expect_err("provenance"),
            CoreCaldusHealthError::InvalidFriendlyIntent
        );
        assert_eq!(health, before);
        let lethal = health
            .apply_friendly_damage(
                &CoreCaldusState::Active {
                    phase: crate::CoreCaldusPhase::Phase1,
                    phase_tick: 0,
                    loop_tick: 0,
                    loop_length: 234,
                },
                &[damage_input(0, participant(10, 0), 101, 20_000)],
            )
            .expect("lethal");
        assert!(lethal.defeat.is_some());
        assert!(health.defeated());
        assert_eq!(health.current_health(), 0);
        assert!(
            health
                .hurtbox(SimulationVector::new(9.0, 9.0))
                .expect("hurtbox")
                .is_none()
        );
    }
}
