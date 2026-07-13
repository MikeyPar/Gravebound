use thiserror::Error;

use crate::{SceneInteractionAccess, SceneInteractionProjection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneInteractionRejection {
    StageDisabled,
    ConditionUnmet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneInteractionEvent {
    Progress {
        object_id: String,
        held_ticks: u16,
        required_ticks: u16,
    },
    Opened {
        object_id: String,
    },
    Closed {
        object_id: String,
    },
    Rejected {
        object_id: String,
        reason: SceneInteractionRejection,
    },
}

/// One-panel-per-player authority state for exact instant and hold interactions.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SceneInteractionSession {
    focused_object_id: Option<String>,
    held_ticks: u16,
    open_panel_object_id: Option<String>,
    rejected_latch_object_id: Option<String>,
}

impl SceneInteractionSession {
    #[must_use]
    pub fn open_panel_object_id(&self) -> Option<&str> {
        self.open_panel_object_id.as_deref()
    }

    pub fn step(
        &mut self,
        projection: Option<&SceneInteractionProjection>,
        interact_held: bool,
        close_panel: bool,
    ) -> Result<Vec<SceneInteractionEvent>, SceneInteractionSessionError> {
        if close_panel {
            self.reset_hold();
            return Ok(self
                .open_panel_object_id
                .take()
                .map_or_else(Vec::new, |object_id| {
                    vec![SceneInteractionEvent::Closed { object_id }]
                }));
        }
        if self.open_panel_object_id.is_some() {
            self.reset_hold();
            return Ok(Vec::new());
        }
        let Some(projection) = projection else {
            self.reset_hold();
            return Ok(Vec::new());
        };
        if !interact_held {
            self.reset_hold();
            return Ok(Vec::new());
        }
        if projection.access != SceneInteractionAccess::Available {
            self.focused_object_id = None;
            self.held_ticks = 0;
            if self.rejected_latch_object_id.as_deref() == Some(&projection.object_id) {
                return Ok(Vec::new());
            }
            self.rejected_latch_object_id = Some(projection.object_id.clone());
            return Ok(vec![SceneInteractionEvent::Rejected {
                object_id: projection.object_id.clone(),
                reason: match projection.access {
                    SceneInteractionAccess::StageDisabled => {
                        SceneInteractionRejection::StageDisabled
                    }
                    SceneInteractionAccess::ConditionUnmet => {
                        SceneInteractionRejection::ConditionUnmet
                    }
                    SceneInteractionAccess::Available => unreachable!(),
                },
            }]);
        }
        self.rejected_latch_object_id = None;
        if self.focused_object_id.as_deref() != Some(&projection.object_id) {
            self.focused_object_id = Some(projection.object_id.clone());
            self.held_ticks = 0;
        }
        if projection.hold_ticks == 0 {
            return Ok(self.open(projection.object_id.clone()));
        }
        self.held_ticks = self
            .held_ticks
            .checked_add(1)
            .ok_or(SceneInteractionSessionError::TickOverflow)?;
        if self.held_ticks >= projection.hold_ticks {
            Ok(self.open(projection.object_id.clone()))
        } else {
            Ok(vec![SceneInteractionEvent::Progress {
                object_id: projection.object_id.clone(),
                held_ticks: self.held_ticks,
                required_ticks: projection.hold_ticks,
            }])
        }
    }

    fn open(&mut self, object_id: String) -> Vec<SceneInteractionEvent> {
        self.reset_hold();
        self.open_panel_object_id = Some(object_id.clone());
        vec![SceneInteractionEvent::Opened { object_id }]
    }

    fn reset_hold(&mut self) {
        self.focused_object_id = None;
        self.held_ticks = 0;
        self.rejected_latch_object_id = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SceneInteractionSessionError {
    #[error("scene interaction hold duration overflowed")]
    TickOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection(
        object_id: &str,
        hold_ticks: u16,
        access: SceneInteractionAccess,
    ) -> SceneInteractionProjection {
        SceneInteractionProjection {
            object_id: object_id.to_owned(),
            hold_ticks,
            access,
            distance_squared_milli_tiles: 0,
        }
    }

    #[test]
    fn exact_hold_opens_one_panel_and_escape_closes_without_chaining() {
        let target = projection(
            "station.memorial_wall",
            15,
            SceneInteractionAccess::Available,
        );
        let mut session = SceneInteractionSession::default();
        for held_tick in 1..15 {
            assert_eq!(
                session.step(Some(&target), true, false).expect("progress"),
                vec![SceneInteractionEvent::Progress {
                    object_id: target.object_id.clone(),
                    held_ticks: held_tick,
                    required_ticks: 15,
                }]
            );
        }
        assert_eq!(
            session.step(Some(&target), true, false).expect("open"),
            vec![SceneInteractionEvent::Opened {
                object_id: target.object_id.clone()
            }]
        );
        assert_eq!(
            session.open_panel_object_id(),
            Some("station.memorial_wall")
        );
        assert!(
            session
                .step(Some(&target), true, false)
                .expect("held")
                .is_empty()
        );
        assert_eq!(
            session.step(Some(&target), true, true).expect("close"),
            vec![SceneInteractionEvent::Closed {
                object_id: target.object_id
            }]
        );
        assert_eq!(session.open_panel_object_id(), None);
    }

    #[test]
    fn instant_interaction_opens_on_first_held_tick() {
        let target = projection("station.realm_gate", 0, SceneInteractionAccess::Available);
        let mut session = SceneInteractionSession::default();
        assert_eq!(
            session.step(Some(&target), true, false).expect("open"),
            vec![SceneInteractionEvent::Opened {
                object_id: target.object_id
            }]
        );
    }

    #[test]
    fn release_or_focus_change_resets_hold_progress() {
        let first = projection(
            "station.memorial_wall",
            2,
            SceneInteractionAccess::Available,
        );
        let second = projection("station.oath_shrine", 2, SceneInteractionAccess::Available);
        let mut session = SceneInteractionSession::default();
        session.step(Some(&first), true, false).expect("first tick");
        assert!(
            session
                .step(Some(&first), false, false)
                .expect("release")
                .is_empty()
        );
        assert!(matches!(
            session.step(Some(&first), true, false).expect("restart")[..],
            [SceneInteractionEvent::Progress { held_ticks: 1, .. }]
        ));
        assert!(matches!(
            session
                .step(Some(&second), true, false)
                .expect("focus change")[..],
            [SceneInteractionEvent::Progress { held_ticks: 1, .. }]
        ));
    }

    #[test]
    fn disabled_reason_is_typed_and_latched_until_release() {
        let target = projection(
            "station.realm_gate",
            0,
            SceneInteractionAccess::StageDisabled,
        );
        let mut session = SceneInteractionSession::default();
        assert_eq!(
            session.step(Some(&target), true, false).expect("reject"),
            vec![SceneInteractionEvent::Rejected {
                object_id: target.object_id.clone(),
                reason: SceneInteractionRejection::StageDisabled,
            }]
        );
        assert!(
            session
                .step(Some(&target), true, false)
                .expect("latched")
                .is_empty()
        );
        session.step(Some(&target), false, false).expect("release");
        assert_eq!(
            session
                .step(Some(&target), true, false)
                .expect("reject again")
                .len(),
            1
        );
    }
}
