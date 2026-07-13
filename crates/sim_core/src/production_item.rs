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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmorBaseRequest {
    pub item_level: u8,
    pub rarity: EquipmentRarity,
    pub raw_health_base_hundredths: u16,
    pub raw_health_per_level_hundredths: u16,
    pub raw_armor_base_hundredths: u16,
    pub raw_armor_per_level_hundredths: u16,
    pub raw_resistance_base_basis_points: u16,
    pub raw_resistance_per_level_basis_points: u16,
    pub barrier_raw_base_health_hundredths: Option<u16>,
    pub barrier_raw_health_per_level_hundredths: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedArmorBase {
    pub maximum_health: u32,
    pub armor: u32,
    /// Resistance rounded to the nearest 0.1 percentage point (10 basis points).
    pub resistance_basis_points: u16,
    pub direct_hit_barrier_health: Option<u32>,
}

/// Resolves the authored armor-family formulas and rarity scaling using integer arithmetic only.
pub fn resolve_armor_base(
    request: ArmorBaseRequest,
) -> Result<ResolvedArmorBase, ProductionItemMathError> {
    if !(1..=20).contains(&request.item_level)
        || request.barrier_raw_base_health_hundredths.is_some()
            != request.barrier_raw_health_per_level_hundredths.is_some()
    {
        return Err(ProductionItemMathError::InvalidArmorRequest);
    }
    let level = u128::from(request.item_level);
    let rarity = u128::from(request.rarity.base_multiplier_basis_points());
    let scaled_whole = |base: u16, per_level: u16| {
        let raw_hundredths =
            u128::from(base).checked_add(u128::from(per_level).checked_mul(level)?)?;
        round_half_up_ratio(
            raw_hundredths.checked_mul(rarity)?,
            CENTI_UNITS_PER_ONE * BASIS_POINTS,
        )
        .ok()
    };
    let maximum_health = scaled_whole(
        request.raw_health_base_hundredths,
        request.raw_health_per_level_hundredths,
    )
    .ok_or(ProductionItemMathError::ArithmeticOverflow)?;
    let armor = scaled_whole(
        request.raw_armor_base_hundredths,
        request.raw_armor_per_level_hundredths,
    )
    .ok_or(ProductionItemMathError::ArithmeticOverflow)?;
    let raw_resistance = u128::from(request.raw_resistance_base_basis_points)
        .checked_add(
            u128::from(request.raw_resistance_per_level_basis_points)
                .checked_mul(level)
                .ok_or(ProductionItemMathError::ArithmeticOverflow)?,
        )
        .ok_or(ProductionItemMathError::ArithmeticOverflow)?;
    let resistance_tenths = round_half_up_ratio(
        raw_resistance
            .checked_mul(rarity)
            .ok_or(ProductionItemMathError::ArithmeticOverflow)?,
        BASIS_POINTS * 10,
    )?;
    let direct_hit_barrier_health = request
        .barrier_raw_base_health_hundredths
        .zip(request.barrier_raw_health_per_level_hundredths)
        .map(|(base, per_level)| {
            scaled_whole(base, per_level).ok_or(ProductionItemMathError::ArithmeticOverflow)
        })
        .transpose()?;
    Ok(ResolvedArmorBase {
        maximum_health: u32::try_from(maximum_health)
            .map_err(|_| ProductionItemMathError::ArithmeticOverflow)?,
        armor: u32::try_from(armor).map_err(|_| ProductionItemMathError::ArithmeticOverflow)?,
        resistance_basis_points: u16::try_from(resistance_tenths * 10)
            .map_err(|_| ProductionItemMathError::ArithmeticOverflow)?,
        direct_hit_barrier_health: direct_hit_barrier_health
            .map(u32::try_from)
            .transpose()
            .map_err(|_| ProductionItemMathError::ArithmeticOverflow)?,
    })
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
    #[error("production armor request is outside the authored item contract")]
    InvalidArmorRequest,
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

    #[test]
    fn forged_armor_formulas_round_exactly_at_core_boundaries() {
        let resolve = |level| {
            resolve_armor_base(ArmorBaseRequest {
                item_level: level,
                rarity: EquipmentRarity::Forged,
                raw_health_base_hundredths: 600,
                raw_health_per_level_hundredths: 80,
                raw_armor_base_hundredths: 50,
                raw_armor_per_level_hundredths: 8,
                raw_resistance_base_basis_points: 400,
                raw_resistance_per_level_basis_points: 30,
                barrier_raw_base_health_hundredths: None,
                barrier_raw_health_per_level_hundredths: None,
            })
            .unwrap()
        };
        assert_eq!(
            resolve(1),
            ResolvedArmorBase {
                maximum_health: 7,
                armor: 1,
                resistance_basis_points: 430,
                direct_hit_barrier_health: None,
            }
        );
        assert_eq!(resolve(6).maximum_health, 11);
        assert_eq!(resolve(10).resistance_basis_points, 700);
    }

    #[test]
    fn bellguard_barrier_and_malformed_pair_are_explicit() {
        let request = ArmorBaseRequest {
            item_level: 10,
            rarity: EquipmentRarity::Forged,
            raw_health_base_hundredths: 800,
            raw_health_per_level_hundredths: 100,
            raw_armor_base_hundredths: 100,
            raw_armor_per_level_hundredths: 22,
            raw_resistance_base_basis_points: 0,
            raw_resistance_per_level_basis_points: 0,
            barrier_raw_base_health_hundredths: Some(500),
            barrier_raw_health_per_level_hundredths: Some(100),
        };
        let resolved = resolve_armor_base(request).unwrap();
        assert_eq!(resolved.maximum_health, 18);
        assert_eq!(resolved.armor, 3);
        assert_eq!(resolved.direct_hit_barrier_health, Some(15));
        assert_eq!(
            resolve_armor_base(ArmorBaseRequest {
                barrier_raw_health_per_level_hundredths: None,
                ..request
            }),
            Err(ProductionItemMathError::InvalidArmorRequest)
        );
    }
}
