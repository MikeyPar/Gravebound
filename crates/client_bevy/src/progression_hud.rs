//! Accessible native health, level, and XP presentation for `GB-M03-04A`.

use bevy::prelude::Resource;
use protocol::{
    ProgressionCapState, ProgressionProjection, ProgressionResult, ProgressionResultCode,
};

const BAR_SEGMENTS: u32 = 20;

#[derive(Debug, Clone, Default, Resource)]
pub(crate) struct ProgressionHudModel {
    projection: Option<ProgressionProjection>,
    error: Option<ProgressionResultCode>,
}

impl ProgressionHudModel {
    pub(crate) fn apply(&mut self, result: ProgressionResult) {
        match result {
            ProgressionResult::Snapshot { projection, .. }
            | ProgressionResult::Changed { projection, .. } => {
                self.projection = Some(projection);
                self.error = None;
            }
            ProgressionResult::Error { code, .. } => {
                self.projection = None;
                self.error = Some(code);
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.projection = None;
        self.error = None;
    }

    pub(crate) fn render(&self) -> String {
        let Some(projection) = &self.projection else {
            return self.error.map_or_else(
                || "VITALS\nSelect a living character to load progression.".to_owned(),
                |code| format!("VITALS UNAVAILABLE\n{}", error_label(code)),
            );
        };
        let health = segmented_bar(projection.current_health, projection.maximum_health);
        let health_percent = projection
            .current_health
            .saturating_mul(100)
            .checked_div(projection.maximum_health)
            .unwrap_or(0);
        let health_state = match health_percent {
            0..=15 => "CRITICAL",
            16..=35 => "LOW",
            _ => "STABLE",
        };
        let xp = match projection.cap_state {
            ProgressionCapState::Advancing {
                level_start_total_xp,
                next_level_total_xp,
            } => {
                let earned = projection.total_xp.saturating_sub(level_start_total_xp);
                let required = next_level_total_xp.saturating_sub(level_start_total_xp);
                format!(
                    "XP [{}] {} / {} TO LEVEL {}",
                    segmented_bar(earned, required),
                    projection.total_xp,
                    next_level_total_xp,
                    projection.level + 1
                )
            }
            ProgressionCapState::CoreCapped { cap_total_xp } => format!(
                "XP [{}] {} — CORE LEVEL CAP",
                segmented_bar(cap_total_xp, cap_total_xp),
                cap_total_xp
            ),
        };
        format!(
            "GRAVE ARBALIST  •  LEVEL {}\nHP [{}] {} / {}  •  {}\n{}",
            projection.level,
            health,
            projection.current_health,
            projection.maximum_health,
            health_state,
            xp
        )
    }
}

fn segmented_bar(value: u32, maximum: u32) -> String {
    let filled = value
        .min(maximum)
        .saturating_mul(BAR_SEGMENTS)
        .saturating_add(maximum.saturating_sub(1))
        .checked_div(maximum)
        .unwrap_or(0);
    let filled = usize::try_from(filled).unwrap_or(BAR_SEGMENTS as usize);
    let empty = usize::try_from(BAR_SEGMENTS).unwrap_or_default() - filled;
    format!("{}{}", "■".repeat(filled), "·".repeat(empty))
}

const fn error_label(code: ProgressionResultCode) -> &'static str {
    match code {
        ProgressionResultCode::CharacterNotFound => "CHARACTER NOT FOUND",
        ProgressionResultCode::CharacterNotOwned => "CHARACTER OWNERSHIP MISMATCH",
        ProgressionResultCode::CharacterDead => "CHARACTER IS MEMORIALIZED",
        ProgressionResultCode::ContentMismatch => "PROGRESSION CONTENT UPDATE REQUIRED",
        ProgressionResultCode::ServiceUnavailable => "PROGRESSION SERVICE UNAVAILABLE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection(cap_state: ProgressionCapState) -> ProgressionProjection {
        ProgressionProjection {
            character_id: [1; 16],
            progression_version: 2,
            level: 4,
            total_xp: 500,
            current_health: 40,
            maximum_health: 132,
            armor: 2,
            movement_milli_tiles_per_second: 5_100,
            level_damage_multiplier_basis_points: 10_450,
            cap_state,
        }
    }

    #[test]
    fn advancing_hud_uses_numbers_shape_and_text_not_color_alone() {
        let mut model = ProgressionHudModel::default();
        model.apply(ProgressionResult::Snapshot {
            request_sequence: 1,
            projection: projection(ProgressionCapState::Advancing {
                level_start_total_xp: 450,
                next_level_total_xp: 700,
            }),
        });
        let rendered = model.render();
        assert!(rendered.contains("LEVEL 4"));
        assert!(rendered.contains("HP ["));
        assert!(rendered.contains("40 / 132  •  LOW"));
        assert!(rendered.contains("500 / 700 TO LEVEL 5"));
    }

    #[test]
    fn cap_and_service_errors_are_explicit() {
        let mut model = ProgressionHudModel::default();
        let mut capped = projection(ProgressionCapState::CoreCapped {
            cap_total_xp: 2_700,
        });
        capped.level = 10;
        capped.total_xp = 2_700;
        model.apply(ProgressionResult::Snapshot {
            request_sequence: 1,
            projection: capped,
        });
        assert!(model.render().contains("2700 — CORE LEVEL CAP"));
        model.apply(ProgressionResult::Error {
            request_sequence: 2,
            code: ProgressionResultCode::ContentMismatch,
        });
        assert!(model.render().contains("CONTENT UPDATE REQUIRED"));
    }
}
