//! Deterministic direct-hit damage resolution for `COM-001` through `COM-003`.
//!
//! All fractional damage is carried as integer units of `1 / 10_000`. The only conversion to
//! whole damage is the explicit post-armor half-up step required by `COM-002`.

use thiserror::Error;

use crate::{BASIS_POINTS_PER_ONE, EntityId};

const ARMOR_MAX_REDUCTION_BASIS_POINTS: u32 = 3_500;
const ORDINARY_RESISTANCE_CAP_BASIS_POINTS: i32 = 2_500;

/// Resistance family selected by an authored attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageType {
    Physical,
    Veil,
}

/// `COM-003` final-damage tuning category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DamageBand {
    Chip,
    Pressure,
    Major,
    Severe,
    Execution,
}

/// Inputs used to construct one validated direct-hit request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectHitParameters {
    pub source: EntityId,
    pub target: EntityId,
    pub collision_confirmed: bool,
    pub target_is_immune: bool,
    pub raw_damage: u32,
    pub damage_type: DamageType,
    pub attacker_multiplier_basis_points: u32,
    pub target_resistance_basis_points: i32,
    pub direct_damage_reductions_basis_points: Vec<u32>,
    pub armor: u32,
    pub current_barrier: u32,
    pub health_damage_cap_basis_points: Option<u32>,
    pub current_health: u32,
    pub max_health: u32,
}

/// A direct hit whose source, target, collision, immunity, and numeric state passed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectHitRequest {
    parameters: DirectHitParameters,
}

impl DirectHitRequest {
    pub fn new(parameters: DirectHitParameters) -> Result<Self, DamageError> {
        if parameters.source == parameters.target {
            return Err(DamageError::SourceEqualsTarget);
        }
        if !parameters.collision_confirmed {
            return Err(DamageError::CollisionNotConfirmed);
        }
        if parameters.target_is_immune {
            return Err(DamageError::TargetImmune);
        }
        if parameters.raw_damage == 0 {
            return Err(DamageError::ZeroRawDamage);
        }
        if parameters.attacker_multiplier_basis_points == 0 {
            return Err(DamageError::ZeroAttackerMultiplier);
        }
        if parameters
            .direct_damage_reductions_basis_points
            .iter()
            .any(|&reduction| reduction > BASIS_POINTS_PER_ONE)
        {
            return Err(DamageError::InvalidDirectDamageReduction);
        }
        if let Some(cap) = parameters.health_damage_cap_basis_points
            && (cap == 0 || cap > BASIS_POINTS_PER_ONE)
        {
            return Err(DamageError::InvalidHealthDamageCap);
        }
        if parameters.max_health == 0 {
            return Err(DamageError::ZeroMaxHealth);
        }
        if parameters.current_health == 0 {
            return Err(DamageError::TargetAlreadyDead);
        }
        if parameters.current_health > parameters.max_health {
            return Err(DamageError::HealthExceedsMaximum);
        }
        Ok(Self { parameters })
    }

    #[must_use]
    pub const fn parameters(&self) -> &DirectHitParameters {
        &self.parameters
    }
}

/// Complete authoritative result and trace intermediates for one direct hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageEvent {
    pub source: EntityId,
    pub target: EntityId,
    pub damage_type: DamageType,
    pub raw_damage: u32,
    pub attacker_multiplier_basis_points: u32,
    pub damage_after_attacker_fixed: u64,
    pub resistance_input_basis_points: i32,
    pub resistance_applied_basis_points: i32,
    pub damage_after_resistance_fixed: u64,
    pub strongest_direct_reduction_basis_points: u32,
    pub damage_after_direct_reduction_fixed: u64,
    pub armor: u32,
    pub armor_max_reduction_fixed: u64,
    pub armor_reduction_fixed: u64,
    pub post_armor_damage_fixed: u64,
    pub post_armor_damage: u32,
    pub barrier_before: u32,
    pub barrier_absorbed: u32,
    pub barrier_after: u32,
    pub damage_after_barrier: u32,
    pub health_damage_cap_basis_points: Option<u32>,
    pub health_damage_cap: Option<u32>,
    pub cap_reduction: u32,
    pub health_damage_after_cap: u32,
    pub health_before: u32,
    pub health_damage_applied: u32,
    pub health_after: u32,
    pub resolved_band: Option<DamageBand>,
    pub lethal: bool,
}

/// Resolves a validated request in the exact `COM-002` order.
pub fn resolve_direct_hit(request: &DirectHitRequest) -> Result<DamageEvent, DamageError> {
    let input = request.parameters();
    let scale = u64::from(BASIS_POINTS_PER_ONE);

    let damage_after_attacker_fixed = u64::from(input.raw_damage)
        .checked_mul(u64::from(input.attacker_multiplier_basis_points))
        .ok_or(DamageError::ArithmeticOverflow)?;

    let resistance_applied_basis_points = input.target_resistance_basis_points.clamp(
        -ORDINARY_RESISTANCE_CAP_BASIS_POINTS,
        ORDINARY_RESISTANCE_CAP_BASIS_POINTS,
    );
    let resistance_factor =
        u32::try_from(i64::from(BASIS_POINTS_PER_ONE) - i64::from(resistance_applied_basis_points))
            .map_err(|_| DamageError::ArithmeticOverflow)?;
    let damage_after_resistance_fixed =
        multiply_fixed(damage_after_attacker_fixed, resistance_factor)?;

    let strongest_direct_reduction_basis_points = input
        .direct_damage_reductions_basis_points
        .iter()
        .copied()
        .max()
        .unwrap_or(0);
    let reduction_factor = BASIS_POINTS_PER_ONE - strongest_direct_reduction_basis_points;
    let damage_after_direct_reduction_fixed =
        multiply_fixed(damage_after_resistance_fixed, reduction_factor)?;

    let armor_max_reduction_fixed = multiply_fixed(
        damage_after_direct_reduction_fixed,
        ARMOR_MAX_REDUCTION_BASIS_POINTS,
    )?;
    let armor_fixed = u64::from(input.armor)
        .checked_mul(scale)
        .ok_or(DamageError::ArithmeticOverflow)?;
    let armor_reduction_fixed = armor_fixed.min(armor_max_reduction_fixed);
    let post_armor_damage_fixed = damage_after_direct_reduction_fixed
        .checked_sub(armor_reduction_fixed)
        .ok_or(DamageError::ArithmeticOverflow)?;
    let post_armor_damage = round_positive_damage_half_up(post_armor_damage_fixed)?;

    let barrier_absorbed = input.current_barrier.min(post_armor_damage);
    let barrier_after = input.current_barrier - barrier_absorbed;
    let damage_after_barrier = post_armor_damage - barrier_absorbed;

    let health_damage_cap = input
        .health_damage_cap_basis_points
        .map(|cap| {
            u64::from(input.max_health)
                .checked_mul(u64::from(cap))
                .map(|scaled| scaled / scale)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or(DamageError::ArithmeticOverflow)
        })
        .transpose()?;
    let health_damage_after_cap =
        health_damage_cap.map_or(damage_after_barrier, |cap| damage_after_barrier.min(cap));
    let cap_reduction = damage_after_barrier - health_damage_after_cap;

    let health_damage_applied = input.current_health.min(health_damage_after_cap);
    let health_after = input.current_health - health_damage_applied;
    let resolved_band = classify_damage_band(health_damage_after_cap, input.max_health);

    Ok(DamageEvent {
        source: input.source,
        target: input.target,
        damage_type: input.damage_type,
        raw_damage: input.raw_damage,
        attacker_multiplier_basis_points: input.attacker_multiplier_basis_points,
        damage_after_attacker_fixed,
        resistance_input_basis_points: input.target_resistance_basis_points,
        resistance_applied_basis_points,
        damage_after_resistance_fixed,
        strongest_direct_reduction_basis_points,
        damage_after_direct_reduction_fixed,
        armor: input.armor,
        armor_max_reduction_fixed,
        armor_reduction_fixed,
        post_armor_damage_fixed,
        post_armor_damage,
        barrier_before: input.current_barrier,
        barrier_absorbed,
        barrier_after,
        damage_after_barrier,
        health_damage_cap_basis_points: input.health_damage_cap_basis_points,
        health_damage_cap,
        cap_reduction,
        health_damage_after_cap,
        health_before: input.current_health,
        health_damage_applied,
        health_after,
        resolved_band,
        lethal: health_after == 0,
    })
}

/// Classifies resolved health damage. Zero damage has no band; positive sub-1% damage is Chip at
/// runtime, while strict authored validation rejects it through [`validate_damage_band`].
#[must_use]
pub fn classify_damage_band(final_damage: u32, max_health: u32) -> Option<DamageBand> {
    if final_damage == 0 || max_health == 0 {
        return None;
    }
    let damage = u128::from(final_damage) * u128::from(BASIS_POINTS_PER_ONE);
    let maximum = u128::from(max_health);
    Some(if damage <= maximum * 800 {
        DamageBand::Chip
    } else if damage <= maximum * 1_800 {
        DamageBand::Pressure
    } else if damage <= maximum * 3_500 {
        DamageBand::Major
    } else if damage <= maximum * 6_000 {
        DamageBand::Severe
    } else {
        DamageBand::Execution
    })
}

/// Validates an authored hostile damage band against a reference target and execution policy.
pub fn validate_damage_band(
    declared: DamageBand,
    final_damage: u32,
    max_health: u32,
    execution_allowed: bool,
) -> Result<(), DamageBandError> {
    if max_health == 0 {
        return Err(DamageBandError::ZeroMaxHealth);
    }
    if final_damage == 0 {
        return Err(DamageBandError::ZeroDamage);
    }
    if u128::from(final_damage) * u128::from(BASIS_POINTS_PER_ONE) < u128::from(max_health) * 100 {
        return Err(DamageBandError::BelowChipMinimum);
    }
    let resolved =
        classify_damage_band(final_damage, max_health).ok_or(DamageBandError::ZeroDamage)?;
    if resolved != declared {
        return Err(DamageBandError::DeclaredBandMismatch { declared, resolved });
    }
    if resolved == DamageBand::Execution && !execution_allowed {
        return Err(DamageBandError::ExecutionForbidden);
    }
    Ok(())
}

fn multiply_fixed(value: u64, multiplier_basis_points: u32) -> Result<u64, DamageError> {
    value
        .checked_mul(u64::from(multiplier_basis_points))
        .map(|product| product / u64::from(BASIS_POINTS_PER_ONE))
        .ok_or(DamageError::ArithmeticOverflow)
}

fn round_positive_damage_half_up(value_fixed: u64) -> Result<u32, DamageError> {
    if value_fixed == 0 {
        return Ok(0);
    }
    let rounded = value_fixed
        .checked_add(u64::from(BASIS_POINTS_PER_ONE / 2))
        .ok_or(DamageError::ArithmeticOverflow)?
        / u64::from(BASIS_POINTS_PER_ONE);
    u32::try_from(rounded.max(1)).map_err(|_| DamageError::ArithmeticOverflow)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DamageError {
    #[error("direct-hit source and target must differ")]
    SourceEqualsTarget,
    #[error("direct hit requires an authoritative collision")]
    CollisionNotConfirmed,
    #[error("immune targets do not enter direct-hit damage resolution")]
    TargetImmune,
    #[error("direct-hit raw damage must be positive")]
    ZeroRawDamage,
    #[error("attacker multiplier must be positive")]
    ZeroAttackerMultiplier,
    #[error("direct-damage reductions must be at most 10000 basis points")]
    InvalidDirectDamageReduction,
    #[error("health-damage cap must be between 1 and 10000 basis points")]
    InvalidHealthDamageCap,
    #[error("target maximum health must be positive")]
    ZeroMaxHealth,
    #[error("a dead target cannot receive a later direct hit")]
    TargetAlreadyDead,
    #[error("target health exceeds maximum health")]
    HealthExceedsMaximum,
    #[error("damage fixed-point arithmetic overflowed")]
    ArithmeticOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DamageBandError {
    #[error("damage-band reference maximum health must be positive")]
    ZeroMaxHealth,
    #[error("zero final damage has no damage band")]
    ZeroDamage,
    #[error("positive final damage below 1% max health is below the Chip band")]
    BelowChipMinimum,
    #[error("declared damage band {declared:?} does not match resolved band {resolved:?}")]
    DeclaredBandMismatch {
        declared: DamageBand,
        resolved: DamageBand,
    },
    #[error("Execution damage is forbidden for this content stage")]
    ExecutionForbidden,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero test ID")
    }

    fn baseline() -> DirectHitParameters {
        DirectHitParameters {
            source: id(1),
            target: id(2),
            collision_confirmed: true,
            target_is_immune: false,
            raw_damage: 22,
            damage_type: DamageType::Physical,
            attacker_multiplier_basis_points: 10_000,
            target_resistance_basis_points: 0,
            direct_damage_reductions_basis_points: Vec::new(),
            armor: 2,
            current_barrier: 0,
            health_damage_cap_basis_points: None,
            current_health: 120,
            max_health: 120,
        }
    }

    fn resolve(parameters: DirectHitParameters) -> DamageEvent {
        let request = DirectHitRequest::new(parameters).expect("valid request");
        resolve_direct_hit(&request).expect("resolved damage")
    }

    #[test]
    fn baseline_physical_hit_records_every_stage() {
        let event = resolve(baseline());
        assert_eq!(event.damage_after_attacker_fixed, 220_000);
        assert_eq!(event.damage_after_resistance_fixed, 220_000);
        assert_eq!(event.damage_after_direct_reduction_fixed, 220_000);
        assert_eq!(event.armor_max_reduction_fixed, 77_000);
        assert_eq!(event.armor_reduction_fixed, 20_000);
        assert_eq!(event.post_armor_damage_fixed, 200_000);
        assert_eq!(event.post_armor_damage, 20);
        assert_eq!(event.barrier_absorbed, 0);
        assert_eq!(event.health_damage_after_cap, 20);
        assert_eq!(event.health_damage_applied, 20);
        assert_eq!(event.health_after, 100);
        assert_eq!(event.resolved_band, Some(DamageBand::Pressure));
        assert!(!event.lethal);
    }

    #[test]
    fn ordered_multipliers_strongest_reduction_and_armor_cap_are_exact() {
        let mut parameters = baseline();
        parameters.raw_damage = 100;
        parameters.attacker_multiplier_basis_points = 15_000;
        parameters.target_resistance_basis_points = -2_500;
        parameters.direct_damage_reductions_basis_points = vec![1_000, 4_000, 2_500];
        parameters.armor = 50;
        parameters.current_health = 200;
        parameters.max_health = 200;
        let event = resolve(parameters);
        assert_eq!(event.damage_after_attacker_fixed, 1_500_000);
        assert_eq!(event.damage_after_resistance_fixed, 1_875_000);
        assert_eq!(event.strongest_direct_reduction_basis_points, 4_000);
        assert_eq!(event.damage_after_direct_reduction_fixed, 1_125_000);
        assert_eq!(event.armor_max_reduction_fixed, 393_750);
        assert_eq!(event.armor_reduction_fixed, 393_750);
        assert_eq!(event.post_armor_damage_fixed, 731_250);
        assert_eq!(event.post_armor_damage, 73);
    }

    #[test]
    fn resistance_clamps_symmetrically_to_twenty_five_percent() {
        let mut vulnerable = baseline();
        vulnerable.raw_damage = 100;
        vulnerable.armor = 0;
        vulnerable.target_resistance_basis_points = -20_000;
        let vulnerable = resolve(vulnerable);
        assert_eq!(vulnerable.resistance_applied_basis_points, -2_500);
        assert_eq!(vulnerable.post_armor_damage, 125);

        let mut resistant = baseline();
        resistant.raw_damage = 100;
        resistant.armor = 0;
        resistant.target_resistance_basis_points = 20_000;
        let resistant = resolve(resistant);
        assert_eq!(resistant.resistance_applied_basis_points, 2_500);
        assert_eq!(resistant.post_armor_damage, 75);
    }

    #[test]
    fn post_armor_rounds_half_up_once_and_positive_hits_have_minimum_one() {
        let mut half = baseline();
        half.raw_damage = 1;
        half.attacker_multiplier_basis_points = 5_000;
        half.armor = 0;
        assert_eq!(resolve(half).post_armor_damage, 1);

        let mut tiny = baseline();
        tiny.raw_damage = 1;
        tiny.attacker_multiplier_basis_points = 1;
        tiny.armor = 0;
        assert_eq!(resolve(tiny).post_armor_damage, 1);

        let mut fully_reduced = baseline();
        fully_reduced
            .direct_damage_reductions_basis_points
            .push(10_000);
        assert_eq!(resolve(fully_reduced).post_armor_damage, 0);
    }

    #[test]
    fn armor_cannot_remove_more_than_thirty_five_percent() {
        let mut parameters = baseline();
        parameters.raw_damage = 10;
        parameters.armor = u32::MAX;
        let event = resolve(parameters);
        assert_eq!(event.armor_max_reduction_fixed, 35_000);
        assert_eq!(event.armor_reduction_fixed, 35_000);
        assert_eq!(event.post_armor_damage_fixed, 65_000);
        assert_eq!(event.post_armor_damage, 7);
    }

    #[test]
    fn barrier_precedes_health_cap_and_can_fully_absorb() {
        let mut partial = baseline();
        partial.raw_damage = 100;
        partial.armor = 0;
        partial.current_barrier = 10;
        partial.current_health = 200;
        partial.max_health = 200;
        partial.health_damage_cap_basis_points = Some(3_500);
        let partial = resolve(partial);
        assert_eq!(partial.barrier_absorbed, 10);
        assert_eq!(partial.barrier_after, 0);
        assert_eq!(partial.damage_after_barrier, 90);
        assert_eq!(partial.health_damage_cap, Some(70));
        assert_eq!(partial.cap_reduction, 20);
        assert_eq!(partial.health_damage_after_cap, 70);

        let mut full = baseline();
        full.current_barrier = 100;
        let full = resolve(full);
        assert_eq!(full.barrier_absorbed, 20);
        assert_eq!(full.barrier_after, 80);
        assert_eq!(full.health_damage_applied, 0);
        assert_eq!(full.health_after, 120);
        assert_eq!(full.resolved_band, None);
    }

    #[test]
    fn health_cap_floors_safely_and_health_application_saturates_at_current() {
        let mut parameters = baseline();
        parameters.raw_damage = 100;
        parameters.armor = 0;
        parameters.health_damage_cap_basis_points = Some(3_500);
        parameters.current_health = 10;
        let event = resolve(parameters);
        assert_eq!(event.health_damage_cap, Some(42));
        assert_eq!(event.cap_reduction, 58);
        assert_eq!(event.health_damage_after_cap, 42);
        assert_eq!(event.health_damage_applied, 10);
        assert_eq!(event.health_after, 0);
        assert!(event.lethal);
        assert_eq!(event.resolved_band, Some(DamageBand::Major));
    }

    #[test]
    fn damage_band_boundaries_and_standard_execution_policy_are_exact() {
        let cases = [
            (100, DamageBand::Chip),
            (800, DamageBand::Chip),
            (801, DamageBand::Pressure),
            (1_800, DamageBand::Pressure),
            (1_801, DamageBand::Major),
            (3_500, DamageBand::Major),
            (3_501, DamageBand::Severe),
            (6_000, DamageBand::Severe),
            (6_001, DamageBand::Execution),
        ];
        for (damage, band) in cases {
            assert_eq!(classify_damage_band(damage, 10_000), Some(band));
            let execution_allowed = band == DamageBand::Execution;
            validate_damage_band(band, damage, 10_000, execution_allowed)
                .expect("matching legal band");
        }
        assert_eq!(classify_damage_band(0, 10_000), None);
        assert_eq!(
            validate_damage_band(DamageBand::Chip, 99, 10_000, false),
            Err(DamageBandError::BelowChipMinimum)
        );
        assert_eq!(
            validate_damage_band(DamageBand::Pressure, 800, 10_000, false),
            Err(DamageBandError::DeclaredBandMismatch {
                declared: DamageBand::Pressure,
                resolved: DamageBand::Chip,
            })
        );
        assert_eq!(
            validate_damage_band(DamageBand::Execution, 6_001, 10_000, false),
            Err(DamageBandError::ExecutionForbidden)
        );
    }

    #[test]
    fn bell_proctor_fan_declared_chip_conflicts_with_fp_reference_health() {
        let boss = crate::BellProctorDefinition::first_playable();
        let fan = &boss.parameters().fan;
        assert_eq!(fan.raw_damage, 12);
        assert_eq!(fan.damage_band, DamageBand::Chip);
        assert_eq!(
            classify_damage_band(fan.raw_damage, 128),
            Some(DamageBand::Pressure)
        );
        assert!(matches!(
            validate_damage_band(fan.damage_band, fan.raw_damage, 128, false),
            Err(DamageBandError::DeclaredBandMismatch {
                declared: DamageBand::Chip,
                resolved: DamageBand::Pressure,
            })
        ));
    }

    #[test]
    fn invalid_requests_fail_before_mutation() {
        type InvalidMutation = (DamageError, Box<dyn Fn(&mut DirectHitParameters)>);
        let mutations: Vec<InvalidMutation> = vec![
            (
                DamageError::SourceEqualsTarget,
                Box::new(|p| p.target = p.source),
            ),
            (
                DamageError::CollisionNotConfirmed,
                Box::new(|p| p.collision_confirmed = false),
            ),
            (
                DamageError::TargetImmune,
                Box::new(|p| p.target_is_immune = true),
            ),
            (DamageError::ZeroRawDamage, Box::new(|p| p.raw_damage = 0)),
            (
                DamageError::ZeroAttackerMultiplier,
                Box::new(|p| p.attacker_multiplier_basis_points = 0),
            ),
            (
                DamageError::InvalidDirectDamageReduction,
                Box::new(|p| p.direct_damage_reductions_basis_points = vec![10_001]),
            ),
            (
                DamageError::InvalidHealthDamageCap,
                Box::new(|p| p.health_damage_cap_basis_points = Some(0)),
            ),
            (DamageError::ZeroMaxHealth, Box::new(|p| p.max_health = 0)),
            (
                DamageError::TargetAlreadyDead,
                Box::new(|p| p.current_health = 0),
            ),
            (
                DamageError::HealthExceedsMaximum,
                Box::new(|p| p.current_health = p.max_health + 1),
            ),
        ];
        for (expected, mutate) in mutations {
            let mut parameters = baseline();
            mutate(&mut parameters);
            assert_eq!(DirectHitRequest::new(parameters), Err(expected));
        }
    }

    #[test]
    fn checked_fixed_point_math_reports_overflow() {
        let mut parameters = baseline();
        parameters.raw_damage = u32::MAX;
        parameters.attacker_multiplier_basis_points = u32::MAX;
        parameters.target_resistance_basis_points = -2_500;
        let request = DirectHitRequest::new(parameters).expect("structurally valid");
        assert_eq!(
            resolve_direct_hit(&request),
            Err(DamageError::ArithmeticOverflow)
        );
    }
}
