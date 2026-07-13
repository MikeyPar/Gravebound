//! Typed, renderer-independent Veil Bargain loadouts and fixed-point stat composition.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::MAX_ACTIVE_BARGAINS;

pub const BASIS_POINTS_PER_ONE: u32 = 10_000;
pub const MAXIMUM_OUTGOING_DAMAGE_BASIS_POINTS: u32 = 15_000;
pub const MINIMUM_MAXIMUM_HEALTH_BASIS_POINTS: u32 = 7_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoreBargainKind {
    CinderHunger,
    BellDebt,
    LanternAsh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CinderHungerDefinition {
    pub outgoing_direct_damage_multiplier_basis_points: u32,
    pub maximum_health_multiplier_basis_points: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // These flags preserve the validated authored contract.
pub struct BellDebtDefinition {
    pub accepted_primary_emissions_per_repeat: u8,
    pub repeat_delay_ticks: u32,
    pub repeat_damage_multiplier_basis_points: u32,
    pub primary_attack_rate_multiplier_basis_points: u32,
    pub counts_legal_misses: bool,
    pub generated_repeats_advance_counter: bool,
    pub snapshots_aim_and_resolved_behavior: bool,
    pub uses_live_origin_at_repeat: bool,
    pub repeat_is_recursive: bool,
    pub repeat_spends_cooldown_or_resource: bool,
    pub counter_persists_reconnect_and_room_change: bool,
    pub counter_resets_on_acquisition_purge_death_retirement_or_safe_transfer: bool,
    pub cancel_pending_repeat_when_dead_transferred_or_primary_illegal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanternAshDefinition {
    pub potion_healing_multiplier_basis_points: u32,
    pub active_belt_slot_count: u8,
    pub active_belt_index: u8,
    pub inactive_slot_remains_stored_visible_locked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreBargainDefinition {
    CinderHunger(CinderHungerDefinition),
    BellDebt(BellDebtDefinition),
    LanternAsh(LanternAshDefinition),
}

impl CoreBargainDefinition {
    #[must_use]
    pub const fn kind(self) -> CoreBargainKind {
        match self {
            Self::CinderHunger(_) => CoreBargainKind::CinderHunger,
            Self::BellDebt(_) => CoreBargainKind::BellDebt,
            Self::LanternAsh(_) => CoreBargainKind::LanternAsh,
        }
    }

    fn validate(self) -> Result<(), CoreBargainError> {
        match self {
            Self::CinderHunger(definition) => {
                if !(BASIS_POINTS_PER_ONE..=MAXIMUM_OUTGOING_DAMAGE_BASIS_POINTS)
                    .contains(&definition.outgoing_direct_damage_multiplier_basis_points)
                    || !(MINIMUM_MAXIMUM_HEALTH_BASIS_POINTS..=BASIS_POINTS_PER_ONE)
                        .contains(&definition.maximum_health_multiplier_basis_points)
                {
                    return Err(CoreBargainError::InvalidCinderHunger);
                }
            }
            Self::BellDebt(definition) => {
                if definition.accepted_primary_emissions_per_repeat == 0
                    || definition.repeat_delay_ticks == 0
                    || !(1..BASIS_POINTS_PER_ONE)
                        .contains(&definition.repeat_damage_multiplier_basis_points)
                    || !(1..=BASIS_POINTS_PER_ONE)
                        .contains(&definition.primary_attack_rate_multiplier_basis_points)
                    || !definition.counts_legal_misses
                    || definition.generated_repeats_advance_counter
                    || !definition.snapshots_aim_and_resolved_behavior
                    || !definition.uses_live_origin_at_repeat
                    || definition.repeat_is_recursive
                    || definition.repeat_spends_cooldown_or_resource
                    || !definition.counter_persists_reconnect_and_room_change
                    || !definition
                        .counter_resets_on_acquisition_purge_death_retirement_or_safe_transfer
                    || !definition.cancel_pending_repeat_when_dead_transferred_or_primary_illegal
                {
                    return Err(CoreBargainError::InvalidBellDebt);
                }
            }
            Self::LanternAsh(definition) => {
                if definition.potion_healing_multiplier_basis_points < BASIS_POINTS_PER_ONE
                    || definition.active_belt_slot_count != 1
                    || definition.active_belt_index != 0
                    || !definition.inactive_slot_remains_stored_visible_locked
                {
                    return Err(CoreBargainError::InvalidLanternAsh);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBargainLoadout {
    definitions: Vec<CoreBargainDefinition>,
}

impl CoreBargainLoadout {
    pub fn new(definitions: Vec<CoreBargainDefinition>) -> Result<Self, CoreBargainError> {
        if definitions.len() > MAX_ACTIVE_BARGAINS {
            return Err(CoreBargainError::TooManyActiveBargains);
        }
        let mut kinds = BTreeSet::new();
        for definition in &definitions {
            definition.validate()?;
            if !kinds.insert(definition.kind()) {
                return Err(CoreBargainError::DuplicateBargain);
            }
        }
        Ok(Self { definitions })
    }

    #[must_use]
    pub fn definitions(&self) -> &[CoreBargainDefinition] {
        &self.definitions
    }

    #[must_use]
    pub fn bell_debt(&self) -> Option<BellDebtDefinition> {
        self.definitions
            .iter()
            .find_map(|definition| match definition {
                CoreBargainDefinition::BellDebt(value) => Some(*value),
                _ => None,
            })
    }

    #[must_use]
    pub fn lantern_ash(&self) -> Option<LanternAshDefinition> {
        self.definitions
            .iter()
            .find_map(|definition| match definition {
                CoreBargainDefinition::LanternAsh(value) => Some(*value),
                _ => None,
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedCoreBargainModifiers {
    pub ordinary_attack_rate_basis_points: u32,
    pub outgoing_direct_damage_basis_points: u32,
    pub maximum_health_multiplier_basis_points: u32,
    pub potion_healing_multiplier_basis_points: u32,
    pub active_belt_slots: u8,
}

#[must_use]
pub fn resolve_core_bargain_modifiers(
    loadout: &CoreBargainLoadout,
) -> ResolvedCoreBargainModifiers {
    let mut result = ResolvedCoreBargainModifiers {
        ordinary_attack_rate_basis_points: BASIS_POINTS_PER_ONE,
        outgoing_direct_damage_basis_points: BASIS_POINTS_PER_ONE,
        maximum_health_multiplier_basis_points: BASIS_POINTS_PER_ONE,
        potion_healing_multiplier_basis_points: BASIS_POINTS_PER_ONE,
        active_belt_slots: 2,
    };
    for definition in loadout.definitions() {
        match definition {
            CoreBargainDefinition::CinderHunger(definition) => {
                result.outgoing_direct_damage_basis_points =
                    definition.outgoing_direct_damage_multiplier_basis_points;
                result.maximum_health_multiplier_basis_points =
                    definition.maximum_health_multiplier_basis_points;
            }
            CoreBargainDefinition::BellDebt(definition) => {
                result.ordinary_attack_rate_basis_points =
                    definition.primary_attack_rate_multiplier_basis_points;
            }
            CoreBargainDefinition::LanternAsh(definition) => {
                result.potion_healing_multiplier_basis_points =
                    definition.potion_healing_multiplier_basis_points;
                result.active_belt_slots = definition.active_belt_slot_count;
            }
        }
    }
    result
}

pub fn compose_maximum_health_multiplier(
    oath_multiplier_basis_points: u32,
    bargain_multiplier_basis_points: u32,
) -> Result<u32, CoreBargainError> {
    if oath_multiplier_basis_points == 0 || bargain_multiplier_basis_points == 0 {
        return Err(CoreBargainError::InvalidMaximumHealthMultiplier);
    }
    let product = u64::from(oath_multiplier_basis_points)
        .checked_mul(u64::from(bargain_multiplier_basis_points))
        .ok_or(CoreBargainError::ArithmeticOverflow)?;
    let rounded = product
        .checked_add(u64::from(BASIS_POINTS_PER_ONE / 2))
        .ok_or(CoreBargainError::ArithmeticOverflow)?
        / u64::from(BASIS_POINTS_PER_ONE);
    Ok(u32::try_from(rounded)
        .map_err(|_| CoreBargainError::ArithmeticOverflow)?
        .max(MINIMUM_MAXIMUM_HEALTH_BASIS_POINTS))
}

pub fn resolve_primary_interval_micros(
    base_interval_micros: u32,
    attack_rate_basis_points: u32,
) -> Result<u32, CoreBargainError> {
    if base_interval_micros == 0 || attack_rate_basis_points == 0 {
        return Err(CoreBargainError::InvalidPrimaryInterval);
    }
    let numerator = u64::from(base_interval_micros)
        .checked_mul(u64::from(BASIS_POINTS_PER_ONE))
        .ok_or(CoreBargainError::ArithmeticOverflow)?;
    let rounded = numerator
        .checked_add(u64::from(attack_rate_basis_points / 2))
        .ok_or(CoreBargainError::ArithmeticOverflow)?
        / u64::from(attack_rate_basis_points);
    u32::try_from(rounded).map_err(|_| CoreBargainError::ArithmeticOverflow)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreBargainError {
    #[error("more than three active Core Bargains were supplied")]
    TooManyActiveBargains,
    #[error("a Core Bargain appears more than once")]
    DuplicateBargain,
    #[error("Cinder Hunger has invalid resolved modifiers")]
    InvalidCinderHunger,
    #[error("Bell Debt has invalid deterministic-repeat semantics")]
    InvalidBellDebt,
    #[error("Lantern Ash has invalid belt or healing semantics")]
    InvalidLanternAsh,
    #[error("a maximum-health multiplier must be positive")]
    InvalidMaximumHealthMultiplier,
    #[error("base primary interval and attack rate must be positive")]
    InvalidPrimaryInterval,
    #[error("Core Bargain fixed-point arithmetic overflowed")]
    ArithmeticOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cinder() -> CoreBargainDefinition {
        CoreBargainDefinition::CinderHunger(CinderHungerDefinition {
            outgoing_direct_damage_multiplier_basis_points: 11_800,
            maximum_health_multiplier_basis_points: 8_800,
        })
    }

    fn bell() -> CoreBargainDefinition {
        CoreBargainDefinition::BellDebt(BellDebtDefinition {
            accepted_primary_emissions_per_repeat: 5,
            repeat_delay_ticks: 9,
            repeat_damage_multiplier_basis_points: 5_000,
            primary_attack_rate_multiplier_basis_points: 8_500,
            counts_legal_misses: true,
            generated_repeats_advance_counter: false,
            snapshots_aim_and_resolved_behavior: true,
            uses_live_origin_at_repeat: true,
            repeat_is_recursive: false,
            repeat_spends_cooldown_or_resource: false,
            counter_persists_reconnect_and_room_change: true,
            counter_resets_on_acquisition_purge_death_retirement_or_safe_transfer: true,
            cancel_pending_repeat_when_dead_transferred_or_primary_illegal: true,
        })
    }

    fn lantern() -> CoreBargainDefinition {
        CoreBargainDefinition::LanternAsh(LanternAshDefinition {
            potion_healing_multiplier_basis_points: 14_000,
            active_belt_slot_count: 1,
            active_belt_index: 0,
            inactive_slot_remains_stored_visible_locked: true,
        })
    }

    #[test]
    fn every_ordered_zero_to_three_combination_resolves_without_an_oath() {
        let definitions = [cinder(), bell(), lantern()];
        for mask in 0_u8..8 {
            let active = definitions
                .iter()
                .enumerate()
                .filter_map(|(index, definition)| (mask & (1 << index) != 0).then_some(*definition))
                .collect();
            let loadout = CoreBargainLoadout::new(active).unwrap();
            let resolved = resolve_core_bargain_modifiers(&loadout);
            assert!(resolved.outgoing_direct_damage_basis_points <= 15_000);
            assert!(
                compose_maximum_health_multiplier(
                    BASIS_POINTS_PER_ONE,
                    resolved.maximum_health_multiplier_basis_points,
                )
                .unwrap()
                    >= MINIMUM_MAXIMUM_HEALTH_BASIS_POINTS
            );
        }
    }

    #[test]
    fn duplicate_invalid_and_overfull_loadouts_fail_closed() {
        assert_eq!(
            CoreBargainLoadout::new(vec![cinder(), cinder()]),
            Err(CoreBargainError::DuplicateBargain)
        );
        assert_eq!(
            CoreBargainLoadout::new(vec![cinder(), bell(), lantern(), cinder()]),
            Err(CoreBargainError::TooManyActiveBargains)
        );
        let CoreBargainDefinition::BellDebt(mut invalid) = bell() else {
            unreachable!()
        };
        invalid.generated_repeats_advance_counter = true;
        assert_eq!(
            CoreBargainLoadout::new(vec![CoreBargainDefinition::BellDebt(invalid)]),
            Err(CoreBargainError::InvalidBellDebt)
        );
    }

    #[test]
    fn health_composition_rounds_once_and_enforces_the_global_floor() {
        assert_eq!(compose_maximum_health_multiplier(9_000, 8_800), Ok(7_920));
        assert_eq!(compose_maximum_health_multiplier(7_000, 7_000), Ok(7_000));
        assert_eq!(resolve_primary_interval_micros(454_545, 8_500), Ok(534_759));
    }
}
