use thiserror::Error;

use crate::{MILLI_TILES_PER_TILE, TICKS_PER_SECOND};

pub const BASIS_POINTS_PER_ONE: u32 = 10_000;

/// Exact fixed-point inputs compiled from `ability.arbalist.slipstep`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlipstepDefinitionParameters {
    pub content_id: String,
    pub cooldown_ticks: u32,
    pub global_cooldown_ticks: u32,
    pub input_buffer_ticks: u32,
    pub travel_milli_tiles: u32,
    pub travel_ticks: u32,
    pub direct_damage_reduction_basis_points: u32,
    pub empowered_window_ticks: u32,
    pub projectile_speed_bonus_basis_points: u32,
    pub pierce_bonus: u32,
    pub exhaustion_ticks: u32,
}

/// Simulation-owned resolved Slipstep values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlipstepDefinition {
    parameters: SlipstepDefinitionParameters,
}

/// Exact fixed-point inputs compiled from `ability.arbalist.stillness`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StillnessDefinitionParameters {
    pub content_id: String,
    pub activation_ticks: u32,
    pub movement_threshold_basis_points: u32,
    pub projectile_speed_bonus_basis_points: u32,
    pub primary_damage_bonus_basis_points: u32,
    pub break_on_damage: bool,
    pub break_on_slipstep: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StillnessDefinition {
    parameters: StillnessDefinitionParameters,
}

impl StillnessDefinition {
    pub fn new(parameters: StillnessDefinitionParameters) -> Result<Self, AbilityDefinitionError> {
        if parameters.content_id.trim().is_empty() {
            return Err(AbilityDefinitionError::EmptyContentId);
        }
        if parameters.activation_ticks == 0 {
            return Err(AbilityDefinitionError::ZeroStillnessActivation);
        }
        if parameters.movement_threshold_basis_points == 0
            || parameters.movement_threshold_basis_points >= BASIS_POINTS_PER_ONE
        {
            return Err(AbilityDefinitionError::InvalidMovementThreshold);
        }
        if parameters.projectile_speed_bonus_basis_points == 0 {
            return Err(AbilityDefinitionError::ZeroProjectileSpeedBonus);
        }
        if parameters.primary_damage_bonus_basis_points == 0 {
            return Err(AbilityDefinitionError::ZeroPrimaryDamageBonus);
        }
        if !parameters.break_on_damage || !parameters.break_on_slipstep {
            return Err(AbilityDefinitionError::MissingStillnessBreakRule);
        }
        Ok(Self { parameters })
    }

    #[must_use]
    pub fn content_id(&self) -> &str {
        &self.parameters.content_id
    }
    #[must_use]
    pub const fn activation_ticks(&self) -> u32 {
        self.parameters.activation_ticks
    }
    #[must_use]
    pub const fn movement_threshold_basis_points(&self) -> u32 {
        self.parameters.movement_threshold_basis_points
    }
    #[must_use]
    pub const fn projectile_speed_bonus_basis_points(&self) -> u32 {
        self.parameters.projectile_speed_bonus_basis_points
    }
    #[must_use]
    pub const fn primary_damage_bonus_basis_points(&self) -> u32 {
        self.parameters.primary_damage_bonus_basis_points
    }
    #[must_use]
    pub const fn break_on_damage(&self) -> bool {
        self.parameters.break_on_damage
    }
    #[must_use]
    pub const fn break_on_slipstep(&self) -> bool {
        self.parameters.break_on_slipstep
    }

    pub fn focused_primary_raw_damage(&self, raw_damage: u32) -> Result<u32, IntentMathError> {
        let multiplier = BASIS_POINTS_PER_ONE
            .checked_add(self.parameters.primary_damage_bonus_basis_points)
            .ok_or(IntentMathError::Overflow)?;
        multiply_basis_points(raw_damage, multiplier)
    }

    pub fn with_activation_ticks(
        &self,
        activation_ticks: u32,
    ) -> Result<Self, AbilityDefinitionError> {
        let mut parameters = self.parameters.clone();
        parameters.activation_ticks = activation_ticks;
        Self::new(parameters)
    }
}

impl SlipstepDefinition {
    pub fn new(parameters: SlipstepDefinitionParameters) -> Result<Self, AbilityDefinitionError> {
        if parameters.content_id.trim().is_empty() {
            return Err(AbilityDefinitionError::EmptyContentId);
        }
        for (value, error) in [
            (
                parameters.cooldown_ticks,
                AbilityDefinitionError::ZeroCooldown,
            ),
            (
                parameters.global_cooldown_ticks,
                AbilityDefinitionError::ZeroGlobalCooldown,
            ),
            (
                parameters.input_buffer_ticks,
                AbilityDefinitionError::ZeroInputBuffer,
            ),
            (
                parameters.travel_milli_tiles,
                AbilityDefinitionError::ZeroTravelDistance,
            ),
            (
                parameters.travel_ticks,
                AbilityDefinitionError::ZeroTravelDuration,
            ),
            (
                parameters.empowered_window_ticks,
                AbilityDefinitionError::ZeroEmpoweredWindow,
            ),
            (
                parameters.projectile_speed_bonus_basis_points,
                AbilityDefinitionError::ZeroProjectileSpeedBonus,
            ),
            (
                parameters.pierce_bonus,
                AbilityDefinitionError::ZeroPierceBonus,
            ),
            (
                parameters.exhaustion_ticks,
                AbilityDefinitionError::ZeroExhaustionDuration,
            ),
        ] {
            if value == 0 {
                return Err(error);
            }
        }
        if parameters.direct_damage_reduction_basis_points == 0
            || parameters.direct_damage_reduction_basis_points >= BASIS_POINTS_PER_ONE
        {
            return Err(AbilityDefinitionError::InvalidDirectDamageReduction);
        }
        Ok(Self { parameters })
    }

    #[must_use]
    pub fn content_id(&self) -> &str {
        &self.parameters.content_id
    }
    #[must_use]
    pub const fn cooldown_ticks(&self) -> u32 {
        self.parameters.cooldown_ticks
    }
    #[must_use]
    pub const fn global_cooldown_ticks(&self) -> u32 {
        self.parameters.global_cooldown_ticks
    }
    #[must_use]
    pub const fn input_buffer_ticks(&self) -> u32 {
        self.parameters.input_buffer_ticks
    }
    #[must_use]
    pub const fn travel_ticks(&self) -> u32 {
        self.parameters.travel_ticks
    }
    #[must_use]
    pub const fn direct_damage_reduction_basis_points(&self) -> u32 {
        self.parameters.direct_damage_reduction_basis_points
    }
    #[must_use]
    pub const fn empowered_window_ticks(&self) -> u32 {
        self.parameters.empowered_window_ticks
    }
    #[must_use]
    pub const fn projectile_speed_bonus_basis_points(&self) -> u32 {
        self.parameters.projectile_speed_bonus_basis_points
    }
    #[must_use]
    pub const fn pierce_bonus(&self) -> u32 {
        self.parameters.pierce_bonus
    }
    #[must_use]
    pub const fn exhaustion_ticks(&self) -> u32 {
        self.parameters.exhaustion_ticks
    }
    #[must_use]
    pub fn travel_tiles(&self) -> f32 {
        milli_tiles_to_tiles(self.parameters.travel_milli_tiles)
    }
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // Authored tick counts are validated u32 gameplay data.
    pub fn travel_per_tick_tiles(&self) -> f32 {
        self.travel_tiles() / self.parameters.travel_ticks as f32
    }

    pub fn with_equipment_overrides(
        &self,
        travel_milli_tiles: Option<u32>,
        travel_ticks: Option<u32>,
        direct_damage_reduction_basis_points: Option<u32>,
        cooldown_ticks: Option<u32>,
    ) -> Result<Self, AbilityDefinitionError> {
        let mut parameters = self.parameters.clone();
        if let Some(value) = travel_milli_tiles {
            parameters.travel_milli_tiles = value;
        }
        if let Some(value) = travel_ticks {
            parameters.travel_ticks = value;
        }
        if let Some(value) = direct_damage_reduction_basis_points {
            parameters.direct_damage_reduction_basis_points = value;
        }
        if let Some(value) = cooldown_ticks {
            parameters.cooldown_ticks = value;
        }
        Self::new(parameters)
    }
}

/// Exact fixed-point inputs compiled from `ability.arbalist.grave_mark` and shared defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraveMarkDefinitionParameters {
    pub content_id: String,
    pub cooldown_ticks: u32,
    pub global_cooldown_ticks: u32,
    pub input_buffer_ticks: u32,
    pub projectile_speed_milli_tiles_per_second: u32,
    pub range_milli_tiles: u32,
    pub projectile_radius_milli_tiles: u32,
    pub weapon_damage_multiplier_basis_points: u32,
    pub duration_ticks: u32,
    pub marked_primary_bonus_basis_points: u32,
    pub maximum_marked_targets: u32,
    pub consumes_on_solid: bool,
}

/// Simulation-owned resolved Grave Mark values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraveMarkDefinition {
    parameters: GraveMarkDefinitionParameters,
    projectile_lifetime_ticks: u32,
}

impl GraveMarkDefinition {
    pub fn new(parameters: GraveMarkDefinitionParameters) -> Result<Self, AbilityDefinitionError> {
        if parameters.content_id.trim().is_empty() {
            return Err(AbilityDefinitionError::EmptyContentId);
        }
        for (value, error) in [
            (
                parameters.cooldown_ticks,
                AbilityDefinitionError::ZeroCooldown,
            ),
            (
                parameters.global_cooldown_ticks,
                AbilityDefinitionError::ZeroGlobalCooldown,
            ),
            (
                parameters.input_buffer_ticks,
                AbilityDefinitionError::ZeroInputBuffer,
            ),
            (
                parameters.projectile_speed_milli_tiles_per_second,
                AbilityDefinitionError::ZeroProjectileSpeed,
            ),
            (
                parameters.range_milli_tiles,
                AbilityDefinitionError::ZeroRange,
            ),
            (
                parameters.projectile_radius_milli_tiles,
                AbilityDefinitionError::ZeroProjectileRadius,
            ),
            (
                parameters.weapon_damage_multiplier_basis_points,
                AbilityDefinitionError::ZeroDamageMultiplier,
            ),
            (
                parameters.duration_ticks,
                AbilityDefinitionError::ZeroDuration,
            ),
        ] {
            if value == 0 {
                return Err(error);
            }
        }
        if parameters.marked_primary_bonus_basis_points == 0 {
            return Err(AbilityDefinitionError::ZeroMarkedPrimaryBonus);
        }
        if parameters.maximum_marked_targets != 1 {
            return Err(AbilityDefinitionError::UnsupportedMarkedTargetCount(
                parameters.maximum_marked_targets,
            ));
        }
        if !parameters.consumes_on_solid {
            return Err(AbilityDefinitionError::MustConsumeOnSolid);
        }
        let scaled_range = u64::from(parameters.range_milli_tiles)
            .checked_mul(u64::from(TICKS_PER_SECOND))
            .ok_or(AbilityDefinitionError::LifetimeOverflow)?;
        let lifetime = scaled_range.div_ceil(u64::from(
            parameters.projectile_speed_milli_tiles_per_second,
        ));
        let projectile_lifetime_ticks =
            u32::try_from(lifetime).map_err(|_| AbilityDefinitionError::LifetimeOverflow)?;
        if projectile_lifetime_ticks == 0 {
            return Err(AbilityDefinitionError::ZeroProjectileLifetime);
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
    pub const fn cooldown_ticks(&self) -> u32 {
        self.parameters.cooldown_ticks
    }

    #[must_use]
    pub const fn global_cooldown_ticks(&self) -> u32 {
        self.parameters.global_cooldown_ticks
    }

    #[must_use]
    pub const fn input_buffer_ticks(&self) -> u32 {
        self.parameters.input_buffer_ticks
    }

    #[must_use]
    pub const fn projectile_speed_milli_tiles_per_second(&self) -> u32 {
        self.parameters.projectile_speed_milli_tiles_per_second
    }

    #[must_use]
    pub const fn range_milli_tiles(&self) -> u32 {
        self.parameters.range_milli_tiles
    }

    #[must_use]
    pub const fn projectile_radius_milli_tiles(&self) -> u32 {
        self.parameters.projectile_radius_milli_tiles
    }

    #[must_use]
    pub const fn weapon_damage_multiplier_basis_points(&self) -> u32 {
        self.parameters.weapon_damage_multiplier_basis_points
    }

    #[must_use]
    pub const fn duration_ticks(&self) -> u32 {
        self.parameters.duration_ticks
    }

    #[must_use]
    pub const fn marked_primary_bonus_basis_points(&self) -> u32 {
        self.parameters.marked_primary_bonus_basis_points
    }

    #[must_use]
    pub const fn maximum_marked_targets(&self) -> u32 {
        self.parameters.maximum_marked_targets
    }

    #[must_use]
    pub const fn consumes_on_solid(&self) -> bool {
        self.parameters.consumes_on_solid
    }

    #[must_use]
    pub const fn projectile_lifetime_ticks(&self) -> u32 {
        self.projectile_lifetime_ticks
    }

    #[must_use]
    pub fn projectile_speed_tiles_per_second(&self) -> f32 {
        milli_tiles_to_tiles(self.parameters.projectile_speed_milli_tiles_per_second)
    }

    #[must_use]
    pub fn range_tiles(&self) -> f32 {
        milli_tiles_to_tiles(self.parameters.range_milli_tiles)
    }

    #[must_use]
    pub fn projectile_radius_tiles(&self) -> f32 {
        milli_tiles_to_tiles(self.parameters.projectile_radius_milli_tiles)
    }

    pub fn grave_mark_raw_intent(&self, weapon_raw_damage: u32) -> Result<u32, IntentMathError> {
        multiply_basis_points(
            weapon_raw_damage,
            self.parameters.weapon_damage_multiplier_basis_points,
        )
    }

    pub fn marked_primary_raw_intent(
        &self,
        weapon_raw_damage: u32,
    ) -> Result<u32, IntentMathError> {
        let multiplier = BASIS_POINTS_PER_ONE
            .checked_add(self.parameters.marked_primary_bonus_basis_points)
            .ok_or(IntentMathError::Overflow)?;
        multiply_basis_points(weapon_raw_damage, multiplier)
    }

    pub fn with_range_and_marked_primary_bonus(
        &self,
        range_milli_tiles: u32,
        marked_primary_bonus_basis_points: u32,
    ) -> Result<Self, AbilityDefinitionError> {
        let mut parameters = self.parameters.clone();
        parameters.range_milli_tiles = range_milli_tiles;
        parameters.marked_primary_bonus_basis_points = marked_primary_bonus_basis_points;
        Self::new(parameters)
    }

    pub fn with_equipment_overrides(
        &self,
        range_milli_tiles: Option<u32>,
        projectile_speed_milli_tiles_per_second: Option<u32>,
        weapon_damage_multiplier_basis_points: Option<u32>,
        duration_ticks: Option<u32>,
        marked_primary_bonus_basis_points: Option<u32>,
    ) -> Result<Self, AbilityDefinitionError> {
        let mut parameters = self.parameters.clone();
        if let Some(value) = range_milli_tiles {
            parameters.range_milli_tiles = value;
        }
        if let Some(value) = projectile_speed_milli_tiles_per_second {
            parameters.projectile_speed_milli_tiles_per_second = value;
        }
        if let Some(value) = weapon_damage_multiplier_basis_points {
            parameters.weapon_damage_multiplier_basis_points = value;
        }
        if let Some(value) = duration_ticks {
            parameters.duration_ticks = value;
        }
        if let Some(value) = marked_primary_bonus_basis_points {
            parameters.marked_primary_bonus_basis_points = value;
        }
        Self::new(parameters)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AbilityDefinitionError {
    #[error("ability content ID must not be empty")]
    EmptyContentId,
    #[error("ability cooldown must be at least one tick")]
    ZeroCooldown,
    #[error("global cooldown must be at least one tick")]
    ZeroGlobalCooldown,
    #[error("input buffer must be at least one tick")]
    ZeroInputBuffer,
    #[error("projectile speed must be positive")]
    ZeroProjectileSpeed,
    #[error("projectile range must be positive")]
    ZeroRange,
    #[error("projectile radius must be positive")]
    ZeroProjectileRadius,
    #[error("weapon damage multiplier must be positive")]
    ZeroDamageMultiplier,
    #[error("mark duration must be at least one tick")]
    ZeroDuration,
    #[error("marked-primary bonus must be positive")]
    ZeroMarkedPrimaryBonus,
    #[error("Grave Mark supports exactly one marked target, received {0}")]
    UnsupportedMarkedTargetCount(u32),
    #[error("Grave Mark must consume on solid contact")]
    MustConsumeOnSolid,
    #[error("projectile lifetime must be at least one tick")]
    ZeroProjectileLifetime,
    #[error("projectile lifetime arithmetic overflowed")]
    LifetimeOverflow,
    #[error("movement ability travel distance must be positive")]
    ZeroTravelDistance,
    #[error("movement ability travel duration must be at least one tick")]
    ZeroTravelDuration,
    #[error("direct-damage reduction must be between 0 and 10000 basis points")]
    InvalidDirectDamageReduction,
    #[error("empowered-primary window must be at least one tick")]
    ZeroEmpoweredWindow,
    #[error("projectile-speed bonus must be positive")]
    ZeroProjectileSpeedBonus,
    #[error("pierce bonus must be positive")]
    ZeroPierceBonus,
    #[error("Exhaustion duration must be at least one tick")]
    ZeroExhaustionDuration,
    #[error("Stillness activation must be at least one tick")]
    ZeroStillnessActivation,
    #[error("Stillness movement threshold must be between 0 and 10000 basis points")]
    InvalidMovementThreshold,
    #[error("Focused primary-damage bonus must be positive")]
    ZeroPrimaryDamageBonus,
    #[error("Stillness must break on both damage and Slipstep")]
    MissingStillnessBreakRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum IntentMathError {
    #[error("raw-damage intent arithmetic overflowed")]
    Overflow,
}

fn multiply_basis_points(value: u32, multiplier: u32) -> Result<u32, IntentMathError> {
    let numerator = u64::from(value)
        .checked_mul(u64::from(multiplier))
        .and_then(|product| product.checked_add(u64::from(BASIS_POINTS_PER_ONE / 2)))
        .ok_or(IntentMathError::Overflow)?;
    u32::try_from(numerator / u64::from(BASIS_POINTS_PER_ONE))
        .map_err(|_| IntentMathError::Overflow)
}

#[allow(clippy::cast_precision_loss)]
fn milli_tiles_to_tiles(value: u32) -> f32 {
    value as f32 / MILLI_TILES_PER_TILE as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact_definition() -> GraveMarkDefinition {
        GraveMarkDefinition::new(GraveMarkDefinitionParameters {
            content_id: "ability.arbalist.grave_mark".to_owned(),
            cooldown_ticks: 150,
            global_cooldown_ticks: 5,
            input_buffer_ticks: 3,
            projectile_speed_milli_tiles_per_second: 12_000,
            range_milli_tiles: 11_000,
            projectile_radius_milli_tiles: 120,
            weapon_damage_multiplier_basis_points: 18_000,
            duration_ticks: 120,
            marked_primary_bonus_basis_points: 1_500,
            maximum_marked_targets: 1,
            consumes_on_solid: true,
        })
        .expect("Grave Mark")
    }

    #[test]
    fn exact_values_and_half_up_intents_compile() {
        let definition = exact_definition();
        assert_eq!(definition.projectile_lifetime_ticks(), 28);
        assert!((definition.projectile_speed_tiles_per_second() - 12.0).abs() < f32::EPSILON);
        assert!((definition.range_tiles() - 11.0).abs() < f32::EPSILON);
        assert!((definition.projectile_radius_tiles() - 0.12).abs() < f32::EPSILON);
        assert_eq!(definition.grave_mark_raw_intent(20).expect("intent"), 36);
        assert_eq!(
            definition.marked_primary_raw_intent(20).expect("intent"),
            23
        );
        assert_eq!(multiply_basis_points(1, 15_000).expect("half up"), 2);
    }

    #[test]
    fn invalid_contract_values_fail_closed() {
        let mut parameters = exact_definition().parameters;
        parameters.projectile_radius_milli_tiles = 0;
        assert_eq!(
            GraveMarkDefinition::new(parameters),
            Err(AbilityDefinitionError::ZeroProjectileRadius)
        );
        let mut parameters = exact_definition().parameters;
        parameters.maximum_marked_targets = 2;
        assert_eq!(
            GraveMarkDefinition::new(parameters),
            Err(AbilityDefinitionError::UnsupportedMarkedTargetCount(2))
        );
    }

    #[test]
    fn exact_slipstep_values_compile() {
        let definition = SlipstepDefinition::new(SlipstepDefinitionParameters {
            content_id: "ability.arbalist.slipstep".to_owned(),
            cooldown_ticks: 240,
            global_cooldown_ticks: 5,
            input_buffer_ticks: 3,
            travel_milli_tiles: 2_000,
            travel_ticks: 5,
            direct_damage_reduction_basis_points: 2_500,
            empowered_window_ticks: 45,
            projectile_speed_bonus_basis_points: 3_000,
            pierce_bonus: 1,
            exhaustion_ticks: 45,
        })
        .expect("Slipstep");
        assert!((definition.travel_tiles() - 2.0).abs() < f32::EPSILON);
        assert!((definition.travel_per_tick_tiles() - 0.4).abs() < f32::EPSILON);
        assert_eq!(definition.cooldown_ticks(), 240);
        assert_eq!(definition.empowered_window_ticks(), 45);
    }

    #[test]
    fn exact_stillness_values_compile_and_round_damage_half_up() {
        let definition = StillnessDefinition::new(StillnessDefinitionParameters {
            content_id: "ability.arbalist.stillness".to_owned(),
            activation_ticks: 18,
            movement_threshold_basis_points: 2_000,
            projectile_speed_bonus_basis_points: 1_000,
            primary_damage_bonus_basis_points: 800,
            break_on_damage: true,
            break_on_slipstep: true,
        })
        .expect("Stillness");
        assert_eq!(definition.activation_ticks(), 18);
        assert_eq!(
            definition.focused_primary_raw_damage(20).expect("damage"),
            22
        );
    }
}
