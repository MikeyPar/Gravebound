//! Deterministic production item arithmetic for `GB-M03-04B`.
//!
//! Content supplies template scalars and rarity. This module performs integer-only formula
//! resolution and owns no content lookup, random stream, persistence, or inventory mutation.

use thiserror::Error;

const BASIS_POINTS: u128 = 10_000;
const CENTI_UNITS_PER_ONE: u128 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipmentRarity {
    Worn,
    Forged,
    Oathed,
    Relic,
    Sainted,
    BlackUnique,
}

impl EquipmentRarity {
    #[must_use]
    pub const fn base_multiplier_basis_points(self) -> u16 {
        match self {
            Self::Worn => 9_500,
            Self::Forged => 10_000,
            Self::Oathed => 10_150,
            Self::Relic | Self::BlackUnique => 10_300,
            Self::Sainted => 10_450,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrossbowPowerRequest {
    pub item_level: u8,
    pub template_damage_scalar_basis_points: u16,
    pub rarity: EquipmentRarity,
    /// Additive same-family weapon-W affixes; Core supplies zero.
    pub weapon_w_affix_basis_points: u16,
}

/// Resolves displayed Crossbow `W` with one final round-half-up operation.
pub fn resolve_crossbow_weapon_power(
    request: CrossbowPowerRequest,
) -> Result<u32, ProductionItemMathError> {
    if !(1..=20).contains(&request.item_level)
        || request.template_damage_scalar_basis_points == 0
        || request.weapon_w_affix_basis_points > 10_000
    {
        return Err(ProductionItemMathError::InvalidWeaponRequest);
    }
    // `15.00 + 0.95 * (L - 1)`, held in exact centi-W units.
    let raw_centi = 1_500_u128 + 95 * u128::from(request.item_level - 1);
    let affix_multiplier = BASIS_POINTS + u128::from(request.weapon_w_affix_basis_points);
    let numerator = raw_centi
        .checked_mul(u128::from(request.template_damage_scalar_basis_points))
        .and_then(|value| {
            value.checked_mul(u128::from(request.rarity.base_multiplier_basis_points()))
        })
        .and_then(|value| value.checked_mul(affix_multiplier))
        .ok_or(ProductionItemMathError::ArithmeticOverflow)?;
    let denominator = CENTI_UNITS_PER_ONE * BASIS_POINTS * BASIS_POINTS * BASIS_POINTS;
    u32::try_from(round_half_up_ratio(numerator, denominator)?)
        .map_err(|_| ProductionItemMathError::ArithmeticOverflow)
}

fn round_half_up_ratio(
    numerator: u128,
    denominator: u128,
) -> Result<u128, ProductionItemMathError> {
    if denominator == 0 {
        return Err(ProductionItemMathError::ArithmeticOverflow);
    }
    numerator
        .checked_add(denominator / 2)
        .map(|value| value / denominator)
        .ok_or(ProductionItemMathError::ArithmeticOverflow)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProductionItemMathError {
    #[error("production weapon request is outside the authored item contract")]
    InvalidWeaponRequest,
    #[error("production item arithmetic overflowed")]
    ArithmeticOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_pine_crossbow_levels_one_and_ten_are_exact() {
        let resolve = |item_level| {
            resolve_crossbow_weapon_power(CrossbowPowerRequest {
                item_level,
                template_damage_scalar_basis_points: 10_000,
                rarity: EquipmentRarity::Forged,
                weapon_w_affix_basis_points: 0,
            })
            .unwrap()
        };
        assert_eq!(resolve(1), 15);
        assert_eq!(resolve(10), 24);
    }

    #[test]
    fn gdd_grave_repeater_example_rounds_half_up_to_twenty() {
        assert_eq!(
            resolve_crossbow_weapon_power(CrossbowPowerRequest {
                item_level: 8,
                template_damage_scalar_basis_points: 8_400,
                rarity: EquipmentRarity::Relic,
                weapon_w_affix_basis_points: 600,
            }),
            Ok(20)
        );
    }

    #[test]
    fn all_rarity_multipliers_and_invalid_boundaries_are_explicit() {
        assert_eq!(EquipmentRarity::Worn.base_multiplier_basis_points(), 9_500);
        assert_eq!(
            EquipmentRarity::Forged.base_multiplier_basis_points(),
            10_000
        );
        assert_eq!(
            EquipmentRarity::Oathed.base_multiplier_basis_points(),
            10_150
        );
        assert_eq!(
            EquipmentRarity::Relic.base_multiplier_basis_points(),
            10_300
        );
        assert_eq!(
            EquipmentRarity::Sainted.base_multiplier_basis_points(),
            10_450
        );
        assert_eq!(
            EquipmentRarity::BlackUnique.base_multiplier_basis_points(),
            10_300
        );
        assert_eq!(
            resolve_crossbow_weapon_power(CrossbowPowerRequest {
                item_level: 0,
                template_damage_scalar_basis_points: 10_000,
                rarity: EquipmentRarity::Forged,
                weapon_w_affix_basis_points: 0,
            }),
            Err(ProductionItemMathError::InvalidWeaponRequest)
        );
    }
}
