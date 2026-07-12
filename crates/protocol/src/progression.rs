use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CHARACTER_ID_BYTES, ManifestHash};

pub const PROGRESSION_REWARD_EVENT_ID_BYTES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressionQueryFrame {
    pub sequence: u32,
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub progression_content_revision: ManifestHash,
}

impl ProgressionQueryFrame {
    pub fn validate(&self) -> Result<(), ProgressionValidationError> {
        if self.sequence == 0 {
            return Err(ProgressionValidationError::ZeroSequence);
        }
        if all_zero(&self.character_id) {
            return Err(ProgressionValidationError::ZeroCharacterId);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressionCapState {
    Advancing {
        level_start_total_xp: u32,
        next_level_total_xp: u32,
    },
    CoreCapped {
        cap_total_xp: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressionProjection {
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub progression_version: u64,
    pub level: u16,
    pub total_xp: u32,
    pub current_health: u32,
    pub maximum_health: u32,
    pub armor: u16,
    pub movement_milli_tiles_per_second: u32,
    pub level_damage_multiplier_basis_points: u32,
    pub cap_state: ProgressionCapState,
}

impl ProgressionProjection {
    pub fn validate(&self) -> Result<(), ProgressionValidationError> {
        if all_zero(&self.character_id) {
            return Err(ProgressionValidationError::ZeroCharacterId);
        }
        if self.progression_version == 0 {
            return Err(ProgressionValidationError::ZeroVersion);
        }
        if !(1..=10).contains(&self.level) {
            return Err(ProgressionValidationError::InvalidLevel);
        }
        if self.maximum_health == 0
            || self.current_health == 0
            || self.current_health > self.maximum_health
            || self.movement_milli_tiles_per_second == 0
            || self.level_damage_multiplier_basis_points < 10_000
        {
            return Err(ProgressionValidationError::InvalidStats);
        }
        match self.cap_state {
            ProgressionCapState::Advancing {
                level_start_total_xp,
                next_level_total_xp,
            } if self.level < 10
                && level_start_total_xp <= self.total_xp
                && self.total_xp < next_level_total_xp => {}
            ProgressionCapState::CoreCapped { cap_total_xp }
                if self.level == 10 && cap_total_xp > 0 && self.total_xp == cap_total_xp => {}
            _ => return Err(ProgressionValidationError::InvalidCapState),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressionResultCode {
    CharacterNotFound,
    CharacterNotOwned,
    CharacterDead,
    ContentMismatch,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressionResult {
    Snapshot {
        request_sequence: u32,
        projection: ProgressionProjection,
    },
    Changed {
        reward_event_id: [u8; PROGRESSION_REWARD_EVENT_ID_BYTES],
        projection: ProgressionProjection,
        base_xp: u32,
        first_clear_bonus_xp: u32,
        applied_xp: u32,
        discarded_at_core_cap: u32,
        first_clear_awarded: bool,
    },
    Error {
        request_sequence: u32,
        code: ProgressionResultCode,
    },
}

impl ProgressionResult {
    pub fn validate(&self) -> Result<(), ProgressionValidationError> {
        match self {
            Self::Snapshot {
                request_sequence,
                projection,
            } => {
                require_sequence(*request_sequence)?;
                projection.validate()
            }
            Self::Changed {
                reward_event_id,
                projection,
                base_xp,
                first_clear_bonus_xp,
                applied_xp,
                discarded_at_core_cap,
                first_clear_awarded,
            } => {
                if all_zero(reward_event_id) {
                    return Err(ProgressionValidationError::ZeroRewardEventId);
                }
                projection.validate()?;
                let requested = base_xp
                    .checked_add(*first_clear_bonus_xp)
                    .ok_or(ProgressionValidationError::InvalidAwardAmounts)?;
                if applied_xp
                    .checked_add(*discarded_at_core_cap)
                    .is_none_or(|resolved| resolved != requested)
                    || (*first_clear_awarded != (*first_clear_bonus_xp > 0))
                {
                    return Err(ProgressionValidationError::InvalidAwardAmounts);
                }
                Ok(())
            }
            Self::Error {
                request_sequence, ..
            } => require_sequence(*request_sequence),
        }
    }
}

fn require_sequence(sequence: u32) -> Result<(), ProgressionValidationError> {
    if sequence == 0 {
        Err(ProgressionValidationError::ZeroSequence)
    } else {
        Ok(())
    }
}

const fn all_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    let mut index = 0;
    while index < N {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProgressionValidationError {
    #[error("progression sequence must be nonzero")]
    ZeroSequence,
    #[error("character ID must be nonzero")]
    ZeroCharacterId,
    #[error("reward event ID must be nonzero")]
    ZeroRewardEventId,
    #[error("progression version must be nonzero")]
    ZeroVersion,
    #[error("level is outside the Core range")]
    InvalidLevel,
    #[error("progression stat projection is invalid")]
    InvalidStats,
    #[error("progression cap state is inconsistent")]
    InvalidCapState,
    #[error("progression award amounts are inconsistent")]
    InvalidAwardAmounts,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection() -> ProgressionProjection {
        ProgressionProjection {
            character_id: [1; 16],
            progression_version: 4,
            level: 4,
            total_xp: 500,
            current_health: 91,
            maximum_health: 132,
            armor: 2,
            movement_milli_tiles_per_second: 5_100,
            level_damage_multiplier_basis_points: 10_450,
            cap_state: ProgressionCapState::Advancing {
                level_start_total_xp: 450,
                next_level_total_xp: 700,
            },
        }
    }

    #[test]
    fn projection_and_changed_award_are_bounded_and_consistent() {
        assert_eq!(projection().validate(), Ok(()));
        let result = ProgressionResult::Changed {
            reward_event_id: [2; 16],
            projection: projection(),
            base_xp: 450,
            first_clear_bonus_xp: 225,
            applied_xp: 200,
            discarded_at_core_cap: 475,
            first_clear_awarded: true,
        };
        assert_eq!(result.validate(), Ok(()));
    }

    #[test]
    fn invalid_cap_stats_and_award_shapes_fail_closed() {
        let mut invalid = projection();
        invalid.current_health = 0;
        assert_eq!(
            invalid.validate(),
            Err(ProgressionValidationError::InvalidStats)
        );
        let mut invalid = projection();
        invalid.total_xp = 700;
        assert_eq!(
            invalid.validate(),
            Err(ProgressionValidationError::InvalidCapState)
        );
        let result = ProgressionResult::Changed {
            reward_event_id: [2; 16],
            projection: projection(),
            base_xp: 5,
            first_clear_bonus_xp: 0,
            applied_xp: 4,
            discarded_at_core_cap: 0,
            first_clear_awarded: false,
        };
        assert_eq!(
            result.validate(),
            Err(ProgressionValidationError::InvalidAwardAmounts)
        );
    }
}
