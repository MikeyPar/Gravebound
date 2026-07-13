use thiserror::Error;

use crate::{MILLI_TILES_PER_TILE, TICKS_PER_SECOND};

/// Exact fixed-point inputs compiled from an immutable weapon content record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponDefinitionParameters {
    pub content_id: String,
    pub raw_damage: u32,
    pub attack_interval_ticks: u32,
    pub range_milli_tiles: u32,
    pub projectile_speed_milli_tiles_per_second: u32,
    pub projectile_radius_milli_tiles: u32,
    pub projectile_count: u32,
    pub projectile_directions_millionths: Vec<(i32, i32)>,
    pub max_projectiles_per_target: u32,
    pub pierce: u32,
    pub stops_on_first_enemy: bool,
}

/// Simulation-owned immutable resolved weapon values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponDefinition {
    parameters: WeaponDefinitionParameters,
    projectile_lifetime_ticks: u32,
}

impl WeaponDefinition {
    pub fn new(parameters: WeaponDefinitionParameters) -> Result<Self, WeaponDefinitionError> {
        if parameters.content_id.trim().is_empty() {
            return Err(WeaponDefinitionError::EmptyContentId);
        }
        if parameters.raw_damage == 0 {
            return Err(WeaponDefinitionError::ZeroDamage);
        }
        if parameters.attack_interval_ticks == 0 {
            return Err(WeaponDefinitionError::ZeroAttackInterval);
        }
        if parameters.range_milli_tiles == 0 {
            return Err(WeaponDefinitionError::ZeroRange);
        }
        if parameters.projectile_speed_milli_tiles_per_second == 0 {
            return Err(WeaponDefinitionError::ZeroProjectileSpeed);
        }
        if parameters.projectile_radius_milli_tiles == 0 {
            return Err(WeaponDefinitionError::ZeroProjectileRadius);
        }
        if parameters.projectile_count == 0 {
            return Err(WeaponDefinitionError::ZeroProjectileCount);
        }
        if parameters.projectile_directions_millionths.len()
            != usize::try_from(parameters.projectile_count)
                .map_err(|_| WeaponDefinitionError::ProjectileCountOverflow)?
            || parameters
                .projectile_directions_millionths
                .iter()
                .any(|direction| direction.0 == 0 && direction.1 == 0)
        {
            return Err(WeaponDefinitionError::InvalidProjectileDirections);
        }
        if parameters.max_projectiles_per_target == 0
            || parameters.max_projectiles_per_target > parameters.projectile_count
        {
            return Err(WeaponDefinitionError::InvalidPerTargetCap);
        }
        let scaled_range = u64::from(parameters.range_milli_tiles)
            .checked_mul(u64::from(TICKS_PER_SECOND))
            .ok_or(WeaponDefinitionError::LifetimeOverflow)?;
        let lifetime = scaled_range.div_ceil(u64::from(
            parameters.projectile_speed_milli_tiles_per_second,
        ));
        let projectile_lifetime_ticks =
            u32::try_from(lifetime).map_err(|_| WeaponDefinitionError::LifetimeOverflow)?;
        if projectile_lifetime_ticks == 0 {
            return Err(WeaponDefinitionError::ZeroProjectileLifetime);
        }
        Ok(Self {
            parameters,
            projectile_lifetime_ticks,
        })
    }

    #[must_use]
    pub fn content_id(&self) -> &str {
        &self.parameters.content_id
    }

    #[must_use]
    pub const fn raw_damage(&self) -> u32 {
        self.parameters.raw_damage
    }

    #[must_use]
    pub const fn attack_interval_ticks(&self) -> u32 {
        self.parameters.attack_interval_ticks
    }

    #[must_use]
    pub const fn range_milli_tiles(&self) -> u32 {
        self.parameters.range_milli_tiles
    }

    #[must_use]
    pub const fn projectile_speed_milli_tiles_per_second(&self) -> u32 {
        self.parameters.projectile_speed_milli_tiles_per_second
    }

    #[must_use]
    pub const fn projectile_radius_milli_tiles(&self) -> u32 {
        self.parameters.projectile_radius_milli_tiles
    }

    #[must_use]
    pub const fn projectile_count(&self) -> u32 {
        self.parameters.projectile_count
    }

    #[must_use]
    pub fn projectile_directions_millionths(&self) -> &[(i32, i32)] {
        &self.parameters.projectile_directions_millionths
    }

    #[must_use]
    pub const fn max_projectiles_per_target(&self) -> u32 {
        self.parameters.max_projectiles_per_target
    }

    #[must_use]
    pub const fn pierce(&self) -> u32 {
        self.parameters.pierce
    }

    #[must_use]
    pub const fn stops_on_first_enemy(&self) -> bool {
        self.parameters.stops_on_first_enemy
    }

    #[must_use]
    pub const fn projectile_lifetime_ticks(&self) -> u32 {
        self.projectile_lifetime_ticks
    }

    #[must_use]
    pub fn range_tiles(&self) -> f32 {
        milli_tiles_to_tiles(self.parameters.range_milli_tiles)
    }

    #[must_use]
    pub fn projectile_speed_tiles_per_second(&self) -> f32 {
        milli_tiles_to_tiles(self.parameters.projectile_speed_milli_tiles_per_second)
    }

    #[must_use]
    pub fn projectile_radius_tiles(&self) -> f32 {
        milli_tiles_to_tiles(self.parameters.projectile_radius_milli_tiles)
    }

    pub fn with_attack_interval_ticks(
        &self,
        attack_interval_ticks: u32,
    ) -> Result<Self, WeaponDefinitionError> {
        let mut parameters = self.parameters.clone();
        parameters.attack_interval_ticks = attack_interval_ticks;
        Self::new(parameters)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WeaponDefinitionError {
    #[error("weapon content ID must not be empty")]
    EmptyContentId,
    #[error("weapon damage must be positive")]
    ZeroDamage,
    #[error("weapon attack interval must be at least one tick")]
    ZeroAttackInterval,
    #[error("weapon range must be positive")]
    ZeroRange,
    #[error("weapon projectile speed must be positive")]
    ZeroProjectileSpeed,
    #[error("weapon projectile radius must be positive")]
    ZeroProjectileRadius,
    #[error("weapon projectile count must be positive")]
    ZeroProjectileCount,
    #[error("weapon projectile count cannot be represented by this platform")]
    ProjectileCountOverflow,
    #[error("weapon projectile directions must be nonzero and match projectile count")]
    InvalidProjectileDirections,
    #[error("weapon per-target projectile cap must be within 1..=projectile count")]
    InvalidPerTargetCap,
    #[error("weapon projectile lifetime must be at least one tick")]
    ZeroProjectileLifetime,
    #[error("weapon projectile lifetime arithmetic overflowed")]
    LifetimeOverflow,
}

#[allow(clippy::cast_precision_loss)]
fn milli_tiles_to_tiles(value: u32) -> f32 {
    value as f32 / MILLI_TILES_PER_TILE as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pine_parameters() -> WeaponDefinitionParameters {
        WeaponDefinitionParameters {
            content_id: "item.prototype.weapon.pine_crossbow".to_owned(),
            raw_damage: 20,
            attack_interval_ticks: 14,
            range_milli_tiles: 9_500,
            projectile_speed_milli_tiles_per_second: 12_000,
            projectile_radius_milli_tiles: 100,
            projectile_count: 1,
            projectile_directions_millionths: vec![(1_000_000, 0)],
            max_projectiles_per_target: 1,
            pierce: 0,
            stops_on_first_enemy: true,
        }
    }

    #[test]
    fn pine_crossbow_values_resolve_without_float_rounding() {
        let weapon = WeaponDefinition::new(pine_parameters()).expect("weapon");
        assert_eq!(weapon.attack_interval_ticks(), 14);
        assert_eq!(weapon.projectile_lifetime_ticks(), 24);
        assert!((weapon.range_tiles() - 9.5).abs() < f32::EPSILON);
        assert!((weapon.projectile_speed_tiles_per_second() - 12.0).abs() < f32::EPSILON);
        assert!((weapon.projectile_radius_tiles() - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn malformed_weapon_values_fail_closed() {
        let mut parameters = pine_parameters();
        parameters.attack_interval_ticks = 0;
        assert_eq!(
            WeaponDefinition::new(parameters),
            Err(WeaponDefinitionError::ZeroAttackInterval)
        );
        let mut parameters = pine_parameters();
        parameters.projectile_speed_milli_tiles_per_second = 0;
        assert_eq!(
            WeaponDefinition::new(parameters),
            Err(WeaponDefinitionError::ZeroProjectileSpeed)
        );
    }
}
