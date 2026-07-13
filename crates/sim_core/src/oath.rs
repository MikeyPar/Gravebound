//! Deterministic Grave Arbalist Oath mechanics for `GB-M03-05C`.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{EntityId, SimulationVector, Tick};

pub const LONG_VIGIL_ID: &str = "oath.arbalist.long_vigil";
pub const NAILKEEPER_ID: &str = "oath.arbalist.nailkeeper";
pub const LONG_VIGIL_FOCUSED_ACTIVATION_TICKS: u32 = 11;
pub const LONG_VIGIL_GRAVE_MARK_RANGE_BONUS_MILLI_TILES: u32 = 2_000;
pub const LONG_VIGIL_MARKED_PRIMARY_BONUS_BASIS_POINTS: u32 = 2_000;
pub const LONG_VIGIL_MAX_HEALTH_MULTIPLIER_BASIS_POINTS: u32 = 9_000;
pub const NAILKEEPER_PRIMARY_INTERVAL_MULTIPLIER_BASIS_POINTS: u32 = 10_800;
pub const NAILKEEPER_TRAP_RADIUS_MILLI_TILES: u32 = 1_250;
pub const NAILKEEPER_TRAP_RADIUS_TILES: f32 = 1.25;
pub const NAILKEEPER_ARM_TICKS: u32 = 12;
pub const NAILKEEPER_LIFETIME_TICKS: u32 = 150;
pub const NAILKEEPER_DAMAGE_BASIS_POINTS: u32 = 9_000;
pub const NAILKEEPER_FROSTBIND_TICKS: u32 = 45;
pub const NAILKEEPER_MAXIMUM_ACTIVE_TRAPS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraveArbalistOath {
    LongVigil,
    Nailkeeper,
}

impl GraveArbalistOath {
    pub fn from_content_id(value: &str) -> Result<Self, OathMechanicError> {
        match value {
            LONG_VIGIL_ID => Ok(Self::LongVigil),
            NAILKEEPER_ID => Ok(Self::Nailkeeper),
            _ => Err(OathMechanicError::UnknownOath),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedArbalistOathStats {
    pub focused_activation_ticks: u32,
    pub grave_mark_range_milli_tiles: u32,
    pub marked_primary_bonus_basis_points: u32,
    pub maximum_health_multiplier_basis_points: u32,
    pub primary_interval_micros: u32,
}

/// Resolves Oath stats before ordinary tick conversion loses authored interval precision.
pub fn resolve_arbalist_oath_stats(
    oath: GraveArbalistOath,
    base_focused_activation_ticks: u32,
    base_grave_mark_range_milli_tiles: u32,
    base_marked_primary_bonus_basis_points: u32,
    base_primary_interval_micros: u32,
    ordinary_attack_rate_basis_points: u32,
) -> Result<ResolvedArbalistOathStats, OathMechanicError> {
    if base_focused_activation_ticks == 0
        || base_grave_mark_range_milli_tiles == 0
        || base_marked_primary_bonus_basis_points == 0
        || base_primary_interval_micros == 0
        || ordinary_attack_rate_basis_points == 0
    {
        return Err(OathMechanicError::InvalidResolvedStatInput);
    }
    let ordinary_interval = round_half_up_ratio(
        u64::from(base_primary_interval_micros) * 10_000,
        u64::from(ordinary_attack_rate_basis_points),
    )?;
    let (focused, range, marked_bonus, health, interval) = match oath {
        GraveArbalistOath::LongVigil => (
            LONG_VIGIL_FOCUSED_ACTIVATION_TICKS,
            base_grave_mark_range_milli_tiles
                .checked_add(LONG_VIGIL_GRAVE_MARK_RANGE_BONUS_MILLI_TILES)
                .ok_or(OathMechanicError::ArithmeticOverflow)?,
            LONG_VIGIL_MARKED_PRIMARY_BONUS_BASIS_POINTS,
            LONG_VIGIL_MAX_HEALTH_MULTIPLIER_BASIS_POINTS,
            ordinary_interval,
        ),
        GraveArbalistOath::Nailkeeper => (
            base_focused_activation_ticks,
            base_grave_mark_range_milli_tiles,
            base_marked_primary_bonus_basis_points,
            10_000,
            round_half_up_ratio(
                ordinary_interval * u64::from(NAILKEEPER_PRIMARY_INTERVAL_MULTIPLIER_BASIS_POINTS),
                10_000,
            )?,
        ),
    };
    Ok(ResolvedArbalistOathStats {
        focused_activation_ticks: focused,
        grave_mark_range_milli_tiles: range,
        marked_primary_bonus_basis_points: marked_bonus,
        maximum_health_multiplier_basis_points: health,
        primary_interval_micros: u32::try_from(interval)
            .map_err(|_| OathMechanicError::ArithmeticOverflow)?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NailTrapEnemy {
    pub entity_id: EntityId,
    pub position: SimulationVector,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NailTrap {
    id: EntityId,
    position: SimulationVector,
    created_tick: Tick,
    arm_tick: Tick,
    expires_tick: Tick,
    snapshot_weapon_raw_damage: u32,
    armed: bool,
    occupants_requiring_reentry: BTreeSet<EntityId>,
}

impl NailTrap {
    #[must_use]
    pub const fn id(&self) -> EntityId {
        self.id
    }

    #[must_use]
    pub const fn position(&self) -> SimulationVector {
        self.position
    }

    #[must_use]
    pub const fn created_tick(&self) -> Tick {
        self.created_tick
    }

    #[must_use]
    pub const fn arm_tick(&self) -> Tick {
        self.arm_tick
    }

    #[must_use]
    pub const fn expires_tick(&self) -> Tick {
        self.expires_tick
    }

    #[must_use]
    pub const fn armed(&self) -> bool {
        self.armed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NailTrapRemovalReason {
    Overflow,
    Triggered,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NailTrapRemoval {
    pub trap_id: EntityId,
    pub tick: Tick,
    pub reason: NailTrapRemovalReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NailTrapTrigger {
    pub trap_id: EntityId,
    pub target_id: EntityId,
    pub tick: Tick,
    pub raw_damage: u32,
    pub frostbind_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NailTrapStep {
    pub armed: Vec<EntityId>,
    pub triggers: Vec<NailTrapTrigger>,
    pub removals: Vec<NailTrapRemoval>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NailTrapField {
    traps: Vec<NailTrap>,
}

impl NailTrapField {
    #[must_use]
    pub fn traps(&self) -> &[NailTrap] {
        &self.traps
    }

    pub fn spawn(
        &mut self,
        id: EntityId,
        position: SimulationVector,
        created_tick: Tick,
        snapshot_weapon_raw_damage: u32,
    ) -> Result<Option<NailTrapRemoval>, OathMechanicError> {
        if !position.is_finite() || snapshot_weapon_raw_damage == 0 {
            return Err(OathMechanicError::InvalidTrapSpawn);
        }
        if self.traps.iter().any(|trap| trap.id == id) {
            return Err(OathMechanicError::DuplicateTrapId);
        }
        let arm_tick = created_tick
            .0
            .checked_add(u64::from(NAILKEEPER_ARM_TICKS))
            .map(Tick)
            .ok_or(OathMechanicError::ArithmeticOverflow)?;
        let expires_tick = created_tick
            .0
            .checked_add(u64::from(NAILKEEPER_LIFETIME_TICKS))
            .map(Tick)
            .ok_or(OathMechanicError::ArithmeticOverflow)?;
        self.traps.push(NailTrap {
            id,
            position,
            created_tick,
            arm_tick,
            expires_tick,
            snapshot_weapon_raw_damage,
            armed: false,
            occupants_requiring_reentry: BTreeSet::new(),
        });
        self.traps.sort_by_key(|trap| (trap.created_tick, trap.id));
        if self.traps.len() <= NAILKEEPER_MAXIMUM_ACTIVE_TRAPS {
            return Ok(None);
        }
        let removed = self.traps.remove(0);
        Ok(Some(NailTrapRemoval {
            trap_id: removed.id,
            tick: created_tick,
            reason: NailTrapRemovalReason::Overflow,
        }))
    }

    /// Advances traps at `now`. Expiry wins over triggering on the exact expiry tick.
    pub fn step(
        &mut self,
        now: Tick,
        enemies: &[NailTrapEnemy],
    ) -> Result<NailTrapStep, OathMechanicError> {
        if enemies.iter().any(|enemy| !enemy.position.is_finite()) {
            return Err(OathMechanicError::InvalidEnemyPosition);
        }
        let mut ordered_enemies = enemies.to_vec();
        ordered_enemies.sort_by_key(|enemy| enemy.entity_id);
        if ordered_enemies
            .windows(2)
            .any(|pair| pair[0].entity_id == pair[1].entity_id)
        {
            return Err(OathMechanicError::DuplicateEnemyId);
        }
        let mut output = NailTrapStep::default();
        let mut survivors = Vec::with_capacity(self.traps.len());
        for mut trap in self.traps.drain(..) {
            if now.0 >= trap.expires_tick.0 {
                output.removals.push(NailTrapRemoval {
                    trap_id: trap.id,
                    tick: now,
                    reason: NailTrapRemovalReason::Expired,
                });
                continue;
            }
            if !trap.armed && now.0 >= trap.arm_tick.0 {
                trap.armed = true;
                trap.occupants_requiring_reentry = ordered_enemies
                    .iter()
                    .filter(|enemy| inside(&trap, enemy.position))
                    .map(|enemy| enemy.entity_id)
                    .collect();
                output.armed.push(trap.id);
            }
            if trap.armed {
                let trap_position = trap.position;
                trap.occupants_requiring_reentry.retain(|entity_id| {
                    ordered_enemies
                        .iter()
                        .find(|enemy| enemy.entity_id == *entity_id)
                        .is_some_and(|enemy| inside_position(trap_position, enemy.position))
                });
                if let Some(target) = ordered_enemies.iter().find(|enemy| {
                    inside(&trap, enemy.position)
                        && !trap.occupants_requiring_reentry.contains(&enemy.entity_id)
                }) {
                    output.triggers.push(NailTrapTrigger {
                        trap_id: trap.id,
                        target_id: target.entity_id,
                        tick: now,
                        raw_damage: multiply_basis_points(
                            trap.snapshot_weapon_raw_damage,
                            NAILKEEPER_DAMAGE_BASIS_POINTS,
                        )?,
                        frostbind_ticks: NAILKEEPER_FROSTBIND_TICKS,
                    });
                    output.removals.push(NailTrapRemoval {
                        trap_id: trap.id,
                        tick: now,
                        reason: NailTrapRemovalReason::Triggered,
                    });
                    continue;
                }
            }
            survivors.push(trap);
        }
        self.traps = survivors;
        Ok(output)
    }
}

fn inside(trap: &NailTrap, position: SimulationVector) -> bool {
    inside_position(trap.position, position)
}

fn inside_position(trap_position: SimulationVector, position: SimulationVector) -> bool {
    let delta = position - trap_position;
    delta.length_squared() <= NAILKEEPER_TRAP_RADIUS_TILES * NAILKEEPER_TRAP_RADIUS_TILES
}

fn multiply_basis_points(value: u32, basis_points: u32) -> Result<u32, OathMechanicError> {
    let numerator = u64::from(value)
        .checked_mul(u64::from(basis_points))
        .ok_or(OathMechanicError::ArithmeticOverflow)?;
    u32::try_from(round_half_up_ratio(numerator, 10_000)?)
        .map_err(|_| OathMechanicError::ArithmeticOverflow)
}

fn round_half_up_ratio(numerator: u64, denominator: u64) -> Result<u64, OathMechanicError> {
    numerator
        .checked_add(denominator / 2)
        .map(|value| value / denominator)
        .ok_or(OathMechanicError::ArithmeticOverflow)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OathMechanicError {
    #[error("unknown Grave Arbalist Oath")]
    UnknownOath,
    #[error("resolved Oath stat inputs must be positive")]
    InvalidResolvedStatInput,
    #[error("Oath arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("nail trap spawn is invalid")]
    InvalidTrapSpawn,
    #[error("nail trap ID already exists")]
    DuplicateTrapId,
    #[error("nail trap enemy position is invalid")]
    InvalidEnemyPosition,
    #[error("nail trap enemy IDs must be unique")]
    DuplicateEnemyId,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(value: u64) -> EntityId {
        EntityId::new(value).unwrap()
    }

    fn enemy(value: u64, x: f32) -> NailTrapEnemy {
        NailTrapEnemy {
            entity_id: entity(value),
            position: SimulationVector::new(x, 0.0),
        }
    }

    #[test]
    fn approved_oath_stats_and_cadence_order_are_exact() {
        let vigil = resolve_arbalist_oath_stats(
            GraveArbalistOath::LongVigil,
            18,
            11_000,
            1_500,
            454_545,
            8_500,
        )
        .unwrap();
        assert_eq!(vigil.focused_activation_ticks, 11);
        assert_eq!(vigil.grave_mark_range_milli_tiles, 13_000);
        assert_eq!(vigil.marked_primary_bonus_basis_points, 2_000);
        assert_eq!(vigil.maximum_health_multiplier_basis_points, 9_000);
        assert_eq!(vigil.primary_interval_micros, 534_759);

        let nailkeeper = resolve_arbalist_oath_stats(
            GraveArbalistOath::Nailkeeper,
            18,
            11_000,
            1_500,
            454_545,
            8_500,
        )
        .unwrap();
        assert_eq!(nailkeeper.primary_interval_micros, 577_540);
    }

    #[test]
    fn third_trap_removes_oldest_by_tick_then_entity_id() {
        let mut field = NailTrapField::default();
        assert_eq!(
            field
                .spawn(entity(8), SimulationVector::new(0.0, 0.0), Tick(1), 20)
                .unwrap(),
            None
        );
        field
            .spawn(entity(7), SimulationVector::new(0.0, 0.0), Tick(1), 20)
            .unwrap();
        let removed = field
            .spawn(entity(9), SimulationVector::new(0.0, 0.0), Tick(2), 20)
            .unwrap()
            .unwrap();
        assert_eq!(removed.trap_id, entity(7));
        assert_eq!(removed.reason, NailTrapRemovalReason::Overflow);
        assert_eq!(
            field.traps().iter().map(NailTrap::id).collect::<Vec<_>>(),
            vec![entity(8), entity(9)]
        );
    }

    #[test]
    fn occupant_at_arm_must_exit_and_reenter_before_trigger() {
        let mut field = NailTrapField::default();
        field
            .spawn(entity(1), SimulationVector::new(0.0, 0.0), Tick(0), 21)
            .unwrap();
        let armed = field.step(Tick(12), &[enemy(3, 0.0)]).unwrap();
        assert_eq!(armed.armed, vec![entity(1)]);
        assert!(armed.triggers.is_empty());
        assert!(
            field
                .step(Tick(13), &[enemy(3, 2.0)])
                .unwrap()
                .triggers
                .is_empty()
        );
        let triggered = field.step(Tick(14), &[enemy(3, 0.0)]).unwrap();
        assert_eq!(triggered.triggers.len(), 1);
        assert_eq!(triggered.triggers[0].raw_damage, 19);
        assert_eq!(triggered.triggers[0].frostbind_ticks, 45);
        assert!(field.traps().is_empty());
    }

    #[test]
    fn expiry_wins_on_exact_tick_and_target_order_is_stable() {
        let mut expiring = NailTrapField::default();
        expiring
            .spawn(entity(1), SimulationVector::new(0.0, 0.0), Tick(0), 20)
            .unwrap();
        expiring.step(Tick(12), &[]).unwrap();
        let expired = expiring.step(Tick(150), &[enemy(2, 0.0)]).unwrap();
        assert!(expired.triggers.is_empty());
        assert_eq!(expired.removals[0].reason, NailTrapRemovalReason::Expired);

        let mut ordered = NailTrapField::default();
        ordered
            .spawn(entity(4), SimulationVector::new(0.0, 0.0), Tick(0), 20)
            .unwrap();
        ordered.step(Tick(12), &[]).unwrap();
        let result = ordered
            .step(Tick(13), &[enemy(9, 0.0), enemy(7, 0.0)])
            .unwrap();
        assert_eq!(result.triggers[0].target_id, entity(7));
    }
}
