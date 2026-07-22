//! Exact deterministic schedulers for Core-authored enemy kits.
//!
//! Schedulers emit renderer-independent simulation intents at authoritative ticks. Target
//! acquisition, immutable telegraph locks, projectile construction, collision, and presentation
//! remain in their owning layers.

use thiserror::Error;

use crate::{
    CoreEnemyDefinition, CoreEnemyLocomotionDefinition, CorePatternGeometryDefinition,
    CorePatternWarningDefinition, Tick, duration_ms_to_ticks_nearest,
};

const SIX_SECOND_CYCLE_TICKS: u32 = 180;
const ROTOR_VOLLEY_COUNT: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreEnemyKitKind {
    MireLeech,
    BellAcolyte,
    ChoirSkull,
    SepulcherKnight,
    ChoirAbbot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEnemyKitEvent {
    TelegraphDue {
        tick: Tick,
        pattern_index: usize,
        warning_ticks: u32,
        first_use: bool,
    },
    MireChargeDue {
        tick: Tick,
        pattern_index: usize,
        distance_milli_tiles: u32,
        duration_ticks: u32,
    },
    MireRetreatDue {
        tick: Tick,
        speed_milli_tiles_per_second: u32,
        duration_ticks: u32,
    },
    AcolyteFanDue {
        tick: Tick,
        pattern_index: usize,
        offsets_milli_degrees: Vec<i32>,
    },
    RotorStarted {
        tick: Tick,
        pattern_index: usize,
        cycle_index: u32,
        active_ticks: u32,
    },
    RotorVolleyDue {
        tick: Tick,
        pattern_index: usize,
        cycle_index: u32,
        volley_index: u8,
        arm_count: u16,
    },
    RotorRecoveryStarted {
        tick: Tick,
        pattern_index: usize,
        recovery_ticks: u32,
    },
    KnightChargeDue {
        tick: Tick,
        pattern_index: usize,
        charge_ticks: u32,
    },
    KnightStopRingDue {
        tick: Tick,
        pattern_index: usize,
    },
    KnightShieldFanDue {
        tick: Tick,
        pattern_index: usize,
    },
    RecoveryWarningDue {
        tick: Tick,
        pattern_index: usize,
        warning_ticks: u32,
        directional_preview_ticks: u32,
    },
    DirectionalGapPreviewDue {
        tick: Tick,
        pattern_index: usize,
        warning_ticks: u32,
    },
    AbbotRecoveryRingDue {
        tick: Tick,
        pattern_index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreEnemyKitError {
    #[error("Core enemy kit is not implemented for {content_id}")]
    UnsupportedContentId { content_id: String },
    #[error("Core enemy kit definition drifted from its exact authored contract")]
    DefinitionDrift,
    #[error("Core enemy kit scheduler tick arithmetic overflowed")]
    TickOverflow,
}

#[derive(Debug, Clone)]
enum CoreEnemyKitState {
    MireLeech {
        next_cycle_at: Tick,
        charge_at: Option<Tick>,
        retreat_at: Option<Tick>,
        busy_until: Option<Tick>,
        pending_first_use: bool,
        first_use_complete: bool,
    },
    BellAcolyte {
        next_cycle_at: Tick,
        fan_at: Option<Tick>,
        pending_first_use: bool,
        first_use_complete: bool,
        use_second_offsets: bool,
    },
    Rotor {
        kind: CoreEnemyKitKind,
        initialized: bool,
        cycle_index: u32,
        release_at: Option<Tick>,
        preview_at: Option<Tick>,
        current_release: Option<Tick>,
        next_volley_index: Option<u8>,
        recovery_at: Option<Tick>,
        gap_preview_at: Option<Tick>,
    },
    SepulcherKnight {
        next_loop_at: Tick,
        charge_at: Option<Tick>,
        stop_ring_at: Option<Tick>,
        fan_telegraph_at: Option<(Tick, u8)>,
        fan_at: Option<Tick>,
        charge_first_use_complete: bool,
        fan_first_use_complete: bool,
    },
}

/// Stateful tick scheduler for one exact Core-authored kit.
#[derive(Debug, Clone)]
pub struct CoreEnemyKitScheduler {
    definition: CoreEnemyDefinition,
    tick: Tick,
    state: CoreEnemyKitState,
}

impl CoreEnemyKitScheduler {
    pub fn new(definition: CoreEnemyDefinition) -> Result<Self, CoreEnemyKitError> {
        let state = state_for_definition(&definition, Tick(0))?;
        Ok(Self {
            definition,
            tick: Tick(0),
            state,
        })
    }

    #[must_use]
    pub const fn definition(&self) -> &CoreEnemyDefinition {
        &self.definition
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn kind(&self) -> CoreEnemyKitKind {
        match self.state {
            CoreEnemyKitState::MireLeech { .. } => CoreEnemyKitKind::MireLeech,
            CoreEnemyKitState::BellAcolyte { .. } => CoreEnemyKitKind::BellAcolyte,
            CoreEnemyKitState::Rotor { kind, .. } => kind,
            CoreEnemyKitState::SepulcherKnight { .. } => CoreEnemyKitKind::SepulcherKnight,
        }
    }

    /// Exact fan offsets for the Acolyte cast that is currently entering telegraph.
    #[must_use]
    pub fn pending_acolyte_fan_offsets(&self) -> Option<Vec<i32>> {
        let CoreEnemyKitState::BellAcolyte {
            use_second_offsets, ..
        } = &self.state
        else {
            return None;
        };
        let CorePatternGeometryDefinition::AlternatingFan {
            first_offsets_milli_degrees,
            second_offsets_milli_degrees,
            ..
        } = self.definition.parameters().patterns[0].geometry()
        else {
            return None;
        };
        Some(if *use_second_offsets {
            second_offsets_milli_degrees.clone()
        } else {
            first_offsets_milli_degrees.clone()
        })
    }

    /// Advances one authoritative tick.
    ///
    /// `positioned_for_attack` gates a new discrete cycle. Once a telegraph has started, its
    /// immutable release and recovery intents continue. Continuous rotor cadence remains fixed
    /// after its first telegraph; the shared lifecycle suppresses hostile output after invalidation.
    pub fn advance(
        &mut self,
        positioned_for_attack: bool,
    ) -> Result<Vec<CoreEnemyKitEvent>, CoreEnemyKitError> {
        let mut staged = self.clone();
        let events = staged.advance_inner(positioned_for_attack)?;
        *self = staged;
        Ok(events)
    }

    /// Restores first-use warnings, alternation, and cadence at the current authoritative tick.
    pub fn reset(&mut self) -> Result<(), CoreEnemyKitError> {
        self.state = state_for_definition(&self.definition, self.tick)?;
        Ok(())
    }

    fn advance_inner(
        &mut self,
        positioned_for_attack: bool,
    ) -> Result<Vec<CoreEnemyKitEvent>, CoreEnemyKitError> {
        let now = self.tick;
        let events = match &mut self.state {
            CoreEnemyKitState::MireLeech { .. } => {
                advance_mire(&mut self.state, now, positioned_for_attack)?
            }
            CoreEnemyKitState::BellAcolyte { .. } => advance_acolyte(
                &self.definition,
                &mut self.state,
                now,
                positioned_for_attack,
            )?,
            CoreEnemyKitState::Rotor { .. } => {
                advance_rotor(&mut self.state, now, positioned_for_attack)?
            }
            CoreEnemyKitState::SepulcherKnight { .. } => {
                advance_knight(&mut self.state, now, positioned_for_attack)?
            }
        };
        self.tick = self
            .tick
            .checked_next()
            .ok_or(CoreEnemyKitError::TickOverflow)?;
        Ok(events)
    }
}

fn state_for_definition(
    definition: &CoreEnemyDefinition,
    start: Tick,
) -> Result<CoreEnemyKitState, CoreEnemyKitError> {
    match definition.parameters().content_id.as_str() {
        "enemy.mire_leech" => {
            validate_mire(definition)?;
            Ok(CoreEnemyKitState::MireLeech {
                next_cycle_at: start,
                charge_at: None,
                retreat_at: None,
                busy_until: None,
                pending_first_use: false,
                first_use_complete: false,
            })
        }
        "enemy.bell_acolyte" => {
            validate_acolyte(definition)?;
            Ok(CoreEnemyKitState::BellAcolyte {
                next_cycle_at: start,
                fan_at: None,
                pending_first_use: false,
                first_use_complete: false,
                use_second_offsets: false,
            })
        }
        "enemy.choir_skull" => {
            validate_skull(definition)?;
            Ok(rotor_state(CoreEnemyKitKind::ChoirSkull))
        }
        "miniboss.sepulcher_knight" => {
            validate_knight(definition)?;
            Ok(CoreEnemyKitState::SepulcherKnight {
                next_loop_at: start,
                charge_at: None,
                stop_ring_at: None,
                fan_telegraph_at: None,
                fan_at: None,
                charge_first_use_complete: false,
                fan_first_use_complete: false,
            })
        }
        "miniboss.choir_abbot" => {
            validate_abbot(definition)?;
            Ok(rotor_state(CoreEnemyKitKind::ChoirAbbot))
        }
        content_id => Err(CoreEnemyKitError::UnsupportedContentId {
            content_id: content_id.to_owned(),
        }),
    }
}

const fn rotor_state(kind: CoreEnemyKitKind) -> CoreEnemyKitState {
    CoreEnemyKitState::Rotor {
        kind,
        initialized: false,
        cycle_index: 0,
        release_at: None,
        preview_at: None,
        current_release: None,
        next_volley_index: None,
        recovery_at: None,
        gap_preview_at: None,
    }
}

fn advance_mire(
    state: &mut CoreEnemyKitState,
    now: Tick,
    positioned: bool,
) -> Result<Vec<CoreEnemyKitEvent>, CoreEnemyKitError> {
    let CoreEnemyKitState::MireLeech {
        next_cycle_at,
        charge_at,
        retreat_at,
        busy_until,
        pending_first_use,
        first_use_complete,
    } = state
    else {
        unreachable!("Mire state is type-checked by the caller")
    };
    let mut events = Vec::with_capacity(2);
    if charge_at.is_some_and(|due| now >= due) {
        events.push(CoreEnemyKitEvent::MireChargeDue {
            tick: now,
            pattern_index: 0,
            distance_milli_tiles: 2_000,
            duration_ticks: 15,
        });
        *charge_at = None;
        *retreat_at = Some(add_ticks(now, 15)?);
        if *pending_first_use {
            *first_use_complete = true;
            *pending_first_use = false;
        }
    }
    if retreat_at.is_some_and(|due| now >= due) {
        events.push(CoreEnemyKitEvent::MireRetreatDue {
            tick: now,
            speed_milli_tiles_per_second: 3_500,
            duration_ticks: 45,
        });
        *retreat_at = None;
        *busy_until = Some(add_ticks(now, 45)?);
    }
    if busy_until.is_some_and(|due| now >= due) {
        *busy_until = None;
    }
    if charge_at.is_none()
        && retreat_at.is_none()
        && busy_until.is_none()
        && now >= *next_cycle_at
        && positioned
    {
        let warning_ticks = if *first_use_complete { 9 } else { 12 };
        events.push(CoreEnemyKitEvent::TelegraphDue {
            tick: now,
            pattern_index: 0,
            warning_ticks,
            first_use: !*first_use_complete,
        });
        *pending_first_use = !*first_use_complete;
        *charge_at = Some(add_ticks(now, warning_ticks)?);
        *next_cycle_at = add_ticks(now, 75)?;
    }
    Ok(events)
}

fn advance_acolyte(
    definition: &CoreEnemyDefinition,
    state: &mut CoreEnemyKitState,
    now: Tick,
    positioned: bool,
) -> Result<Vec<CoreEnemyKitEvent>, CoreEnemyKitError> {
    let CoreEnemyKitState::BellAcolyte {
        next_cycle_at,
        fan_at,
        pending_first_use,
        first_use_complete,
        use_second_offsets,
    } = state
    else {
        unreachable!("Acolyte state is type-checked by the caller")
    };
    let mut events = Vec::with_capacity(1);
    if fan_at.is_some_and(|due| now >= due) {
        let CorePatternGeometryDefinition::AlternatingFan {
            first_offsets_milli_degrees,
            second_offsets_milli_degrees,
            ..
        } = definition.parameters().patterns[0].geometry()
        else {
            return Err(CoreEnemyKitError::DefinitionDrift);
        };
        events.push(CoreEnemyKitEvent::AcolyteFanDue {
            tick: now,
            pattern_index: 0,
            offsets_milli_degrees: if *use_second_offsets {
                second_offsets_milli_degrees.clone()
            } else {
                first_offsets_milli_degrees.clone()
            },
        });
        *use_second_offsets = !*use_second_offsets;
        *fan_at = None;
        if *pending_first_use {
            *first_use_complete = true;
            *pending_first_use = false;
        }
    }
    if fan_at.is_none() && now >= *next_cycle_at && positioned {
        let warning_ticks = if *first_use_complete { 9 } else { 12 };
        events.push(CoreEnemyKitEvent::TelegraphDue {
            tick: now,
            pattern_index: 0,
            warning_ticks,
            first_use: !*first_use_complete,
        });
        *pending_first_use = !*first_use_complete;
        *fan_at = Some(add_ticks(now, warning_ticks)?);
        *next_cycle_at = add_ticks(now, 54)?;
    }
    Ok(events)
}

#[expect(
    clippy::too_many_lines,
    reason = "the complete rotor boundary priority is intentionally visible in one state transition"
)]
fn advance_rotor(
    state: &mut CoreEnemyKitState,
    now: Tick,
    positioned: bool,
) -> Result<Vec<CoreEnemyKitEvent>, CoreEnemyKitError> {
    let CoreEnemyKitState::Rotor {
        kind,
        initialized,
        cycle_index,
        release_at,
        preview_at,
        current_release,
        next_volley_index,
        recovery_at,
        gap_preview_at,
    } = state
    else {
        unreachable!("Rotor state is type-checked by the caller")
    };
    let (interval_ms, active_ticks, recovery_ticks) = match kind {
        CoreEnemyKitKind::ChoirSkull => (400_u64, 120_u32, 60_u32),
        CoreEnemyKitKind::ChoirAbbot => (350_u64, 105_u32, 75_u32),
        _ => unreachable!("only rotor kits use rotor state"),
    };
    let mut events = Vec::with_capacity(3);

    if !*initialized && positioned {
        events.push(CoreEnemyKitEvent::TelegraphDue {
            tick: now,
            pattern_index: 0,
            warning_ticks: 20,
            first_use: true,
        });
        *release_at = Some(add_ticks(now, 20)?);
        *initialized = true;
    }

    if preview_at.is_some_and(|due| now >= due) {
        events.push(CoreEnemyKitEvent::TelegraphDue {
            tick: now,
            pattern_index: 0,
            warning_ticks: 15,
            first_use: false,
        });
        *preview_at = None;
    }

    if next_volley_index.is_some_and(|index| {
        let Some(release) = *current_release else {
            return false;
        };
        let offset = duration_ms_to_ticks_nearest(interval_ms * u64::from(index));
        now.0 >= release.0.saturating_add(offset)
    }) {
        let index = next_volley_index.expect("checked as present");
        events.push(CoreEnemyKitEvent::RotorVolleyDue {
            tick: now,
            pattern_index: 0,
            cycle_index: cycle_index.saturating_sub(1),
            volley_index: index - 1,
            arm_count: 2,
        });
        *next_volley_index = (index < ROTOR_VOLLEY_COUNT).then_some(index + 1);
    }

    if recovery_at.is_some_and(|due| now >= due) {
        events.push(CoreEnemyKitEvent::RotorRecoveryStarted {
            tick: now,
            pattern_index: 0,
            recovery_ticks,
        });
        if *kind == CoreEnemyKitKind::ChoirAbbot {
            events.push(CoreEnemyKitEvent::RecoveryWarningDue {
                tick: now,
                pattern_index: 1,
                warning_ticks: 75,
                directional_preview_ticks: 20,
            });
        }
        *recovery_at = None;
    }

    if gap_preview_at.is_some_and(|due| now >= due) {
        events.push(CoreEnemyKitEvent::DirectionalGapPreviewDue {
            tick: now,
            pattern_index: 1,
            warning_ticks: 20,
        });
        *gap_preview_at = None;
    }

    if release_at.is_some_and(|due| now >= due) {
        if *kind == CoreEnemyKitKind::ChoirAbbot && *cycle_index > 0 {
            events.push(CoreEnemyKitEvent::AbbotRecoveryRingDue {
                tick: now,
                pattern_index: 1,
            });
        }
        events.push(CoreEnemyKitEvent::RotorStarted {
            tick: now,
            pattern_index: 0,
            cycle_index: *cycle_index,
            active_ticks,
        });
        *current_release = Some(now);
        *next_volley_index = Some(1);
        *cycle_index = cycle_index
            .checked_add(1)
            .ok_or(CoreEnemyKitError::TickOverflow)?;
        let next_release = add_ticks(now, SIX_SECOND_CYCLE_TICKS)?;
        *release_at = Some(next_release);
        *preview_at = Some(add_ticks(now, SIX_SECOND_CYCLE_TICKS - 15)?);
        *recovery_at = Some(add_ticks(now, active_ticks)?);
        *gap_preview_at = (*kind == CoreEnemyKitKind::ChoirAbbot)
            .then(|| subtract_ticks(next_release, 20))
            .transpose()?;
    }
    Ok(events)
}

fn advance_knight(
    state: &mut CoreEnemyKitState,
    now: Tick,
    positioned: bool,
) -> Result<Vec<CoreEnemyKitEvent>, CoreEnemyKitError> {
    let CoreEnemyKitState::SepulcherKnight {
        next_loop_at,
        charge_at,
        stop_ring_at,
        fan_telegraph_at,
        fan_at,
        charge_first_use_complete,
        fan_first_use_complete,
    } = state
    else {
        unreachable!("Knight state is type-checked by the caller")
    };
    let mut events = Vec::with_capacity(2);
    if charge_at.is_some_and(|due| now >= due) {
        events.push(CoreEnemyKitEvent::KnightChargeDue {
            tick: now,
            pattern_index: 0,
            charge_ticks: 17,
        });
        *charge_at = None;
        *stop_ring_at = Some(add_ticks(now, 17)?);
    }
    if stop_ring_at.is_some_and(|due| now >= due) {
        events.push(CoreEnemyKitEvent::KnightStopRingDue {
            tick: now,
            pattern_index: 1,
        });
        *stop_ring_at = None;
    }
    if fan_at.is_some_and(|due| now >= due) {
        events.push(CoreEnemyKitEvent::KnightShieldFanDue {
            tick: now,
            pattern_index: 2,
        });
        *fan_at = None;
        *fan_first_use_complete = true;
    }
    if fan_telegraph_at.is_some_and(|(due, _)| now >= due) {
        let (_, slot) = fan_telegraph_at.take().expect("checked as present");
        let warning_ticks = if *fan_first_use_complete { 9 } else { 12 };
        events.push(CoreEnemyKitEvent::TelegraphDue {
            tick: now,
            pattern_index: 2,
            warning_ticks,
            first_use: !*fan_first_use_complete,
        });
        *fan_at = Some(add_ticks(now, warning_ticks)?);
        if slot == 0 {
            *fan_telegraph_at = Some((add_ticks(now, 66)?, 1));
        }
    }
    if now >= *next_loop_at && positioned {
        events.push(CoreEnemyKitEvent::TelegraphDue {
            tick: now,
            pattern_index: 0,
            warning_ticks: 27,
            first_use: !*charge_first_use_complete,
        });
        *charge_first_use_complete = true;
        *charge_at = Some(add_ticks(now, 27)?);
        *fan_telegraph_at = Some((add_ticks(now, 66)?, 0));
        *next_loop_at = add_ticks(now, SIX_SECOND_CYCLE_TICKS)?;
    }
    Ok(events)
}

fn validate_mire(definition: &CoreEnemyDefinition) -> Result<(), CoreEnemyKitError> {
    exact(
        definition.parameters().patterns.len() == 1
            && pattern_id(definition, 0) == "pattern.enemy.mire_leech.charge"
            && definition.parameters().patterns[0].cycle_ticks() == 75
            && matches!(
                definition.parameters().patterns[0].warning(),
                CorePatternWarningDefinition::Standalone {
                    first_ticks: 12,
                    repeated_ticks: 9
                }
            )
            && matches!(
                definition.parameters().patterns[0].geometry(),
                CorePatternGeometryDefinition::Charge {
                    distance_milli_tiles: 2_000,
                    duration_ticks: 15
                }
            )
            && matches!(
                definition.locomotion(),
                CoreEnemyLocomotionDefinition::RushRetreat {
                    trigger_distance_milli_tiles: 2_500,
                    retreat_speed_milli_tiles_per_second: 3_500,
                    retreat_ticks: 45,
                    ..
                }
            ),
    )
}

fn validate_acolyte(definition: &CoreEnemyDefinition) -> Result<(), CoreEnemyKitError> {
    exact(
        definition.parameters().patterns.len() == 1
            && pattern_id(definition, 0) == "pattern.enemy.bell_acolyte.alternating_fan"
            && definition.parameters().patterns[0].cycle_ticks() == 54
            && matches!(
                definition.parameters().patterns[0].warning(),
                CorePatternWarningDefinition::Standalone {
                    first_ticks: 12,
                    repeated_ticks: 9
                }
            )
            && matches!(
                definition.parameters().patterns[0].geometry(),
                CorePatternGeometryDefinition::AlternatingFan {
                    projectile_speed_milli_tiles_per_second: 6_000,
                    range_milli_tiles: 9_000,
                    projectile_radius_milli_tiles: 110,
                    ..
                }
            )
            && matches!(
                definition.locomotion(),
                CoreEnemyLocomotionDefinition::MaintainDistance {
                    movement_speed_milli_tiles_per_second: 3_000,
                    preferred_distance_milli_tiles: 6_000
                }
            ),
    )
}

fn validate_skull(definition: &CoreEnemyDefinition) -> Result<(), CoreEnemyKitError> {
    exact(
        definition.parameters().patterns.len() == 1
            && pattern_id(definition, 0) == "pattern.enemy.choir_skull.rotor"
            && definition.parameters().patterns[0].cycle_ticks() == 180
            && definition.parameters().patterns[0].quiet_ticks() == 60
            && matches!(
                definition.parameters().patterns[0].warning(),
                CorePatternWarningDefinition::Standalone {
                    first_ticks: 20,
                    repeated_ticks: 15
                }
            )
            && matches!(
                definition.parameters().patterns[0].geometry(),
                CorePatternGeometryDefinition::RotatingArms {
                    arm_count: 2,
                    clockwise_milli_degrees_per_second: 35_000,
                    emission_interval_ticks: 12,
                    active_ticks: 120,
                    ..
                }
            ),
    )
}

fn validate_knight(definition: &CoreEnemyDefinition) -> Result<(), CoreEnemyKitError> {
    let patterns = &definition.parameters().patterns;
    exact(
        patterns.len() == 3
            && pattern_id(definition, 0) == "miniboss.sepulcher_knight.charge_lane"
            && pattern_id(definition, 1) == "miniboss.sepulcher_knight.stop_ring"
            && pattern_id(definition, 2) == "miniboss.sepulcher_knight.shield_fan"
            && patterns[0].cycle_ticks() == 180
            && patterns[2].cycle_ticks() == 66
            && matches!(
                patterns[0].warning(),
                CorePatternWarningDefinition::Standalone {
                    first_ticks: 27,
                    repeated_ticks: 27
                }
            )
            && matches!(
                patterns[1].warning(),
                CorePatternWarningDefinition::ParentOnly
            )
            && matches!(
                patterns[2].warning(),
                CorePatternWarningDefinition::Standalone {
                    first_ticks: 12,
                    repeated_ticks: 9
                }
            )
            && matches!(
                patterns[0].geometry(),
                CorePatternGeometryDefinition::ChargeLane {
                    width_milli_tiles: 1_000,
                    length_milli_tiles: 5_000,
                    charge_ticks: 17
                }
            )
            && matches!(
                patterns[1].geometry(),
                CorePatternGeometryDefinition::RadialGap {
                    index_count: 10,
                    omitted_adjacent_count: 2,
                    ..
                }
            )
            && matches!(
                patterns[2].geometry(),
                CorePatternGeometryDefinition::ProjectileFan {
                    shot_count: 5,
                    total_arc_milli_degrees: 50_000,
                    ..
                }
            ),
    )
}

fn validate_abbot(definition: &CoreEnemyDefinition) -> Result<(), CoreEnemyKitError> {
    let patterns = &definition.parameters().patterns;
    exact(
        patterns.len() == 2
            && pattern_id(definition, 0) == "miniboss.choir_abbot.rotor"
            && pattern_id(definition, 1) == "miniboss.choir_abbot.recovery_ring"
            && patterns[0].cycle_ticks() == 180
            && patterns[0].quiet_ticks() == 75
            && matches!(
                patterns[0].warning(),
                CorePatternWarningDefinition::Standalone {
                    first_ticks: 20,
                    repeated_ticks: 15
                }
            )
            && matches!(
                patterns[0].geometry(),
                CorePatternGeometryDefinition::RotatingArms {
                    arm_count: 2,
                    clockwise_milli_degrees_per_second: 35_000,
                    emission_interval_ticks: 11,
                    active_ticks: 105,
                    ..
                }
            )
            && matches!(
                patterns[1].warning(),
                CorePatternWarningDefinition::RecoveryPreview {
                    ground_origin_warning_ticks: 75,
                    directional_gap_preview_ticks: 20,
                    major_audio: true
                }
            )
            && matches!(
                patterns[1].geometry(),
                CorePatternGeometryDefinition::RadialGap {
                    index_count: 16,
                    omitted_adjacent_count: 4,
                    ..
                }
            ),
    )
}

fn pattern_id(definition: &CoreEnemyDefinition, index: usize) -> &str {
    definition.parameters().patterns[index]
        .parameters()
        .id
        .as_str()
}

fn exact(condition: bool) -> Result<(), CoreEnemyKitError> {
    if condition {
        Ok(())
    } else {
        Err(CoreEnemyKitError::DefinitionDrift)
    }
}

fn add_ticks(tick: Tick, amount: u32) -> Result<Tick, CoreEnemyKitError> {
    tick.0
        .checked_add(u64::from(amount))
        .map(Tick)
        .ok_or(CoreEnemyKitError::TickOverflow)
}

fn subtract_ticks(tick: Tick, amount: u32) -> Result<Tick, CoreEnemyKitError> {
    tick.0
        .checked_sub(u64::from(amount))
        .map(Tick)
        .ok_or(CoreEnemyKitError::TickOverflow)
}
