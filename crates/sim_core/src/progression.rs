//! Deterministic character-progression algorithms for `GB-M03-04A`.
//!
//! Validated content supplies every gameplay value. This module owns only integer eligibility,
//! level reduction, stat growth, and health-rebuild behavior. It performs no content lookup,
//! persistence, network I/O, or route activation.

use thiserror::Error;

use crate::TICK_RATE_HZ;

pub const CORE_LEVEL_COUNT: usize = 10;
pub const NORMAL_XP_RADIUS_MILLI_TILES: i32 = 16_000;
pub const NORMAL_XP_CONTRIBUTION_WINDOW_TICKS: u32 = 10 * TICK_RATE_HZ;
pub const SOC_SHORT_ENCOUNTER_TICKS: u64 = 600;
pub const SOC_INACTIVITY_LIMIT_TICKS: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelCurve {
    pub cumulative_xp: [u32; CORE_LEVEL_COUNT],
}

impl LevelCurve {
    pub fn validate(self) -> Result<(), CoreProgressionError> {
        if self.cumulative_xp[0] != 0
            || self.cumulative_xp.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(CoreProgressionError::InvalidLevelCurve);
        }
        Ok(())
    }

    #[must_use]
    pub const fn xp_cap(self) -> u32 {
        self.cumulative_xp[CORE_LEVEL_COUNT - 1]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraveArbalistProgressionDefinition {
    pub starting_maximum_health: u32,
    pub health_per_level_after_one: u32,
    pub starting_armor: u16,
    pub armor_growth_levels: Vec<u16>,
    pub movement_milli_tiles_per_second: u32,
    pub level_damage_growth_basis_points: u32,
}

impl GraveArbalistProgressionDefinition {
    pub fn validate(&self) -> Result<(), CoreProgressionError> {
        if self.starting_maximum_health == 0
            || self.health_per_level_after_one == 0
            || self.movement_milli_tiles_per_second == 0
            || self.level_damage_growth_basis_points == 0
            || self.armor_growth_levels.is_empty()
            || self
                .armor_growth_levels
                .windows(2)
                .any(|levels| levels[0] >= levels[1])
        {
            return Err(CoreProgressionError::InvalidClassProgression);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreProgressionState {
    pub total_xp: u32,
    pub level: u16,
    pub progression_version: u64,
}

impl CoreProgressionState {
    pub const fn new() -> Self {
        Self {
            total_xp: 0,
            level: 1,
            progression_version: 1,
        }
    }

    pub fn validate(self, curve: LevelCurve) -> Result<(), CoreProgressionError> {
        curve.validate()?;
        if self.progression_version == 0 {
            return Err(CoreProgressionError::ZeroVersion);
        }
        if self.total_xp > curve.xp_cap() || self.level != level_for_core_xp(curve, self.total_xp)?
        {
            return Err(CoreProgressionError::InvalidLevelXpPair);
        }
        Ok(())
    }
}

impl Default for CoreProgressionState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreProgressionGrant {
    pub before: CoreProgressionState,
    pub after: CoreProgressionState,
    pub requested_xp: u32,
    pub applied_xp: u32,
    pub discarded_at_core_cap: u32,
    pub levels_gained: u16,
}

pub fn apply_core_xp(
    curve: LevelCurve,
    state: CoreProgressionState,
    requested_xp: u32,
) -> Result<CoreProgressionGrant, CoreProgressionError> {
    state.validate(curve)?;
    let uncapped = state.total_xp.saturating_add(requested_xp);
    let total_xp = uncapped.min(curve.xp_cap());
    let applied_xp = total_xp - state.total_xp;
    let level = level_for_core_xp(curve, total_xp)?;
    let progression_version = if applied_xp == 0 {
        state.progression_version
    } else {
        state
            .progression_version
            .checked_add(1)
            .ok_or(CoreProgressionError::VersionExhausted)?
    };
    let after = CoreProgressionState {
        total_xp,
        level,
        progression_version,
    };
    after.validate(curve)?;
    Ok(CoreProgressionGrant {
        before: state,
        after,
        requested_xp,
        applied_xp,
        discarded_at_core_cap: requested_xp - applied_xp,
        levels_gained: level - state.level,
    })
}

pub fn level_for_core_xp(curve: LevelCurve, total_xp: u32) -> Result<u16, CoreProgressionError> {
    curve.validate()?;
    let bounded = total_xp.min(curve.xp_cap());
    u16::try_from(
        curve
            .cumulative_xp
            .partition_point(|threshold| *threshold <= bounded),
    )
    .map_err(|_| CoreProgressionError::InvalidLevelCurve)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraveArbalistLevelStats {
    pub level: u16,
    pub maximum_health: u32,
    pub armor: u16,
    pub movement_milli_tiles_per_second: u32,
    pub damage_multiplier_basis_points: u32,
}

pub fn grave_arbalist_level_stats(
    definition: &GraveArbalistProgressionDefinition,
    level: u16,
) -> Result<GraveArbalistLevelStats, CoreProgressionError> {
    definition.validate()?;
    let core_cap = u16::try_from(CORE_LEVEL_COUNT).expect("Core level count fits u16");
    if !(1..=core_cap).contains(&level) {
        return Err(CoreProgressionError::LevelOutsideCore);
    }
    let levels_after_one = u32::from(level - 1);
    let armor_growth = u16::try_from(
        definition
            .armor_growth_levels
            .iter()
            .filter(|growth_level| **growth_level <= level)
            .count(),
    )
    .map_err(|_| CoreProgressionError::InvalidClassProgression)?;
    Ok(GraveArbalistLevelStats {
        level,
        maximum_health: definition.starting_maximum_health
            + definition.health_per_level_after_one * levels_after_one,
        armor: definition.starting_armor + armor_growth,
        movement_milli_tiles_per_second: definition.movement_milli_tiles_per_second,
        damage_multiplier_basis_points: 10_000
            + definition.level_damage_growth_basis_points * levels_after_one,
    })
}

pub fn rebuild_current_health(
    current_health: u32,
    old_maximum_health: u32,
    new_maximum_health: u32,
) -> Result<u32, CoreProgressionError> {
    if old_maximum_health == 0
        || new_maximum_health == 0
        || current_health == 0
        || current_health > old_maximum_health
    {
        return Err(CoreProgressionError::InvalidLivingVitals);
    }
    Ok(current_health.min(new_maximum_health).max(1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalXpEvidence {
    pub living_at_enemy_death: bool,
    pub delta_x_milli_tiles: i32,
    pub delta_y_milli_tiles: i32,
    pub contribution_window_ticks: u32,
    pub actual_health_damage_to_enemy: u64,
    pub effective_support_to_qualifying_player: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncounterXpEvidence {
    pub active_ticks: u64,
    pub present_ticks: u64,
    pub longest_inactivity_ticks: u64,
    pub encounter_contribution_reference_health: u64,
    pub direct_damage: u64,
    pub effective_healing_to_others: u64,
    pub damage_prevented_on_others: u64,
    pub qualifying_objective_credits: u8,
    pub life_state: RewardLifeState,
    pub recall_state: RewardRecallState,
    pub trust_state: RewardTrustState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewardLifeState {
    Living,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewardRecallState {
    Eligible,
    EmergencyRecallCompleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewardTrustState {
    Valid,
    InvalidSession,
    AntiCheatRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XpEligibility {
    pub eligible: bool,
    pub contribution_centi_units: u128,
}

pub fn evaluate_normal_xp_eligibility(
    evidence: NormalXpEvidence,
) -> Result<XpEligibility, CoreProgressionError> {
    if evidence.contribution_window_ticks > NORMAL_XP_CONTRIBUTION_WINDOW_TICKS {
        return Err(CoreProgressionError::InvalidEligibilityEvidence);
    }
    let x = i64::from(evidence.delta_x_milli_tiles);
    let y = i64::from(evidence.delta_y_milli_tiles);
    let radius = i64::from(NORMAL_XP_RADIUS_MILLI_TILES);
    let in_range = x * x + y * y <= radius * radius;
    let contributed = evidence.actual_health_damage_to_enemy >= 1
        || evidence.effective_support_to_qualifying_player;
    Ok(XpEligibility {
        eligible: evidence.living_at_enemy_death && in_range && contributed,
        contribution_centi_units: u128::from(evidence.actual_health_damage_to_enemy) * 100,
    })
}

pub fn evaluate_encounter_xp_eligibility(
    evidence: EncounterXpEvidence,
) -> Result<XpEligibility, CoreProgressionError> {
    if evidence.active_ticks == 0
        || evidence.present_ticks > evidence.active_ticks
        || evidence.encounter_contribution_reference_health == 0
        || evidence.qualifying_objective_credits > 2
    {
        return Err(CoreProgressionError::InvalidEligibilityEvidence);
    }
    // Centi-units preserve SOC-010's 1.00/0.80/0.60 coefficients exactly. One objective credit is
    // exactly 2% of reference health, or `2 * reference` centi-units.
    let contribution_centi_units = u128::from(evidence.direct_damage) * 100
        + u128::from(evidence.effective_healing_to_others) * 80
        + u128::from(evidence.damage_prevented_on_others) * 60
        + u128::from(evidence.qualifying_objective_credits)
            * 2
            * u128::from(evidence.encounter_contribution_reference_health);
    let presence_qualified = evidence.active_ticks < SOC_SHORT_ENCOUNTER_TICKS
        || u128::from(evidence.present_ticks) * 2 >= u128::from(evidence.active_ticks);
    let contribution_qualified = contribution_centi_units * 2
        >= u128::from(evidence.encounter_contribution_reference_health);
    Ok(XpEligibility {
        eligible: presence_qualified
            && contribution_qualified
            && evidence.longest_inactivity_ticks <= SOC_INACTIVITY_LIMIT_TICKS
            && evidence.life_state == RewardLifeState::Living
            && evidence.recall_state == RewardRecallState::Eligible
            && evidence.trust_state == RewardTrustState::Valid,
        contribution_centi_units,
    })
}

#[must_use]
pub const fn first_clear_bonus(base_xp: u32) -> u32 {
    base_xp.div_ceil(2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreProgressionError {
    #[error("progression version must be nonzero")]
    ZeroVersion,
    #[error("Core level and XP are inconsistent")]
    InvalidLevelXpPair,
    #[error("level curve is invalid")]
    InvalidLevelCurve,
    #[error("class progression definition is invalid")]
    InvalidClassProgression,
    #[error("progression version is exhausted")]
    VersionExhausted,
    #[error("level is outside the Core 1-10 range")]
    LevelOutsideCore,
    #[error("living health values are invalid")]
    InvalidLivingVitals,
    #[error("XP eligibility evidence is structurally invalid")]
    InvalidEligibilityEvidence,
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVE: LevelCurve = LevelCurve {
        cumulative_xp: [0, 100, 250, 450, 700, 1_000, 1_350, 1_750, 2_200, 2_700],
    };

    fn arbalist() -> GraveArbalistProgressionDefinition {
        GraveArbalistProgressionDefinition {
            starting_maximum_health: 120,
            health_per_level_after_one: 4,
            starting_armor: 2,
            armor_growth_levels: vec![7, 14],
            movement_milli_tiles_per_second: 5_100,
            level_damage_growth_basis_points: 150,
        }
    }

    #[test]
    fn core_curve_is_exact_at_every_threshold_and_caps_without_hidden_xp() {
        for (index, threshold) in CURVE.cumulative_xp.iter().copied().enumerate() {
            let expected = u16::try_from(index + 1).unwrap();
            assert_eq!(level_for_core_xp(CURVE, threshold), Ok(expected));
            if threshold > 0 {
                assert_eq!(level_for_core_xp(CURVE, threshold - 1), Ok(expected - 1));
            }
        }
        let state = CoreProgressionState {
            total_xp: 2_690,
            level: 9,
            progression_version: 8,
        };
        let grant = apply_core_xp(CURVE, state, 450).unwrap();
        assert_eq!(grant.after.total_xp, CURVE.xp_cap());
        assert_eq!(grant.after.level, 10);
        assert_eq!(grant.applied_xp, 10);
        assert_eq!(grant.discarded_at_core_cap, 440);
        assert_eq!(grant.after.progression_version, 9);
        let capped = apply_core_xp(CURVE, grant.after, 5).unwrap();
        assert_eq!(capped.applied_xp, 0);
        assert_eq!(capped.after.progression_version, 9);
    }

    #[test]
    fn arbalist_level_stats_and_health_rebuild_are_exact() {
        assert_eq!(
            grave_arbalist_level_stats(&arbalist(), 1).unwrap(),
            GraveArbalistLevelStats {
                level: 1,
                maximum_health: 120,
                armor: 2,
                movement_milli_tiles_per_second: 5_100,
                damage_multiplier_basis_points: 10_000,
            }
        );
        let level_ten = grave_arbalist_level_stats(&arbalist(), 10).unwrap();
        assert_eq!(level_ten.maximum_health, 156);
        assert_eq!(level_ten.armor, 3);
        assert_eq!(level_ten.damage_multiplier_basis_points, 11_350);
        assert_eq!(rebuild_current_health(75, 120, 156), Ok(75));
        assert_eq!(rebuild_current_health(130, 156, 120), Ok(120));
        assert_eq!(rebuild_current_health(1, 156, 90), Ok(1));
        assert_eq!(
            rebuild_current_health(0, 120, 156),
            Err(CoreProgressionError::InvalidLivingVitals)
        );
    }

    #[test]
    fn normal_xp_uses_exact_range_life_and_recent_contribution_boundaries() {
        let eligible = NormalXpEvidence {
            living_at_enemy_death: true,
            delta_x_milli_tiles: 16_000,
            delta_y_milli_tiles: 0,
            contribution_window_ticks: 300,
            actual_health_damage_to_enemy: 1,
            effective_support_to_qualifying_player: false,
        };
        assert!(evaluate_normal_xp_eligibility(eligible).unwrap().eligible);
        assert!(
            !evaluate_normal_xp_eligibility(NormalXpEvidence {
                delta_x_milli_tiles: 16_001,
                ..eligible
            })
            .unwrap()
            .eligible
        );
        assert!(
            !evaluate_normal_xp_eligibility(NormalXpEvidence {
                actual_health_damage_to_enemy: 0,
                ..eligible
            })
            .unwrap()
            .eligible
        );
        assert!(
            evaluate_normal_xp_eligibility(NormalXpEvidence {
                actual_health_damage_to_enemy: 0,
                effective_support_to_qualifying_player: true,
                ..eligible
            })
            .unwrap()
            .eligible
        );
    }

    #[test]
    fn encounter_xp_matches_soc_010_at_every_boundary() {
        let exact = EncounterXpEvidence {
            active_ticks: 600,
            present_ticks: 300,
            longest_inactivity_ticks: 600,
            encounter_contribution_reference_health: 10_000,
            direct_damage: 50,
            effective_healing_to_others: 0,
            damage_prevented_on_others: 0,
            qualifying_objective_credits: 0,
            life_state: RewardLifeState::Living,
            recall_state: RewardRecallState::Eligible,
            trust_state: RewardTrustState::Valid,
        };
        let result = evaluate_encounter_xp_eligibility(exact).unwrap();
        assert!(result.eligible);
        assert_eq!(result.contribution_centi_units, 5_000);
        assert!(
            !evaluate_encounter_xp_eligibility(EncounterXpEvidence {
                present_ticks: 299,
                ..exact
            })
            .unwrap()
            .eligible
        );
        assert!(
            !evaluate_encounter_xp_eligibility(EncounterXpEvidence {
                direct_damage: 49,
                ..exact
            })
            .unwrap()
            .eligible
        );
        assert!(
            evaluate_encounter_xp_eligibility(EncounterXpEvidence {
                active_ticks: 599,
                present_ticks: 1,
                direct_damage: 0,
                qualifying_objective_credits: 1,
                ..exact
            })
            .unwrap()
            .eligible
        );
        assert_eq!(first_clear_bonus(5), 3);
        assert_eq!(first_clear_bonus(450), 225);
    }
}
