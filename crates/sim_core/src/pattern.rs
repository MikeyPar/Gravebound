//! Data-driven hostile pattern primitives and fairness validation for GB-M01-04A.
//!
//! This reconciles GDD COM-005/006, Content CONT-010 through CONT-013, and the roadmap's
//! pattern-before-boss dependency. Validation returns stable typed diagnostics without rendering.

use std::collections::BTreeSet;

use crate::{
    Counterplay, DamageBand, DamageType, EchoMemoryFamily, HostileDisposition,
    LaneAttackDefinition, ProjectileAttackDefinition, duration_ms_to_ticks_ceil,
};

const BASELINE_SPEED: u32 = 4_500;
const FROSTBIND_SPEED: u32 = 4_000;
const BASELINE_RADIUS: u32 = 250;
const BASELINE_RTT_MS: u32 = 120;
const NORMAL_CORRIDOR: u32 = 800;
const PINNACLE_CORRIDOR: u32 = 650;
const BOUNDARY_CLEARANCE: u32 = 150;
const MINIMUM_ARRIVAL_MS: u32 = 350;
const CLOSE_SPAWN_DISTANCE: u32 = 1_250;
const CLOSE_SPAWN_WARNING_MS: u32 = 750;
const NORMAL_CAP: u32 = 300;
const BOSS_CAP: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatternContext {
    Normal,
    Boss,
    Pinnacle,
}

impl PatternContext {
    pub const fn projectile_cap(self) -> u32 {
        match self {
            Self::Normal => NORMAL_CAP,
            Self::Boss | Self::Pinnacle => BOSS_CAP,
        }
    }

    const fn minimum_corridor(self) -> u32 {
        if matches!(self, Self::Pinnacle) {
            PINNACLE_CORRIDOR
        } else {
            NORMAL_CORRIDOR
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OriginCue {
    SourceSilhouette,
    GroundOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShapeCue {
    Fan,
    RingGap,
    Lane,
    Timeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CombatColorFamily {
    Physical,
    Veil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatternStatus {
    Frostbind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TimelineAction {
    Telegraph,
    Resolve,
    PreviewBegin,
    PreviewEnd,
    ActiveEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedTimelineEvent {
    pub offset_ticks: u32,
    pub pattern_id: String,
    pub action: TimelineAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternKind {
    Fan {
        projectile_count: u8,
        offsets_degrees: Vec<i16>,
    },
    RingWithGap {
        index_count: u8,
        omitted_count: u8,
        omitted_start_advance: u8,
    },
    TelegraphedLane {
        lane_count: u8,
        width_milli_tiles: u32,
        active_ticks: u32,
        extends_to_arena_collision: bool,
    },
    FixedTimeline {
        loop_ticks: u32,
        events: Vec<FixedTimelineEvent>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternFairnessFixture {
    pub player_speed_milli_tiles_per_second: u32,
    pub player_hurtbox_radius_milli_tiles: u32,
    pub round_trip_latency_ms: u32,
    pub movement_ability_used: bool,
    pub safe_corridor_milli_tiles: u32,
    pub minimum_spawn_distance_milli_tiles: u32,
    pub ground_telegraph_ms: u32,
    pub frostbind_speed_fixture_milli_tiles_per_second: Option<u32>,
    pub visibly_repeating_beam_with_inactive_warning: bool,
}

impl PatternFairnessFixture {
    #[must_use]
    pub const fn baseline(
        safe_corridor_milli_tiles: u32,
        minimum_spawn_distance_milli_tiles: u32,
        ground_telegraph_ms: u32,
    ) -> Self {
        Self {
            player_speed_milli_tiles_per_second: BASELINE_SPEED,
            player_hurtbox_radius_milli_tiles: BASELINE_RADIUS,
            round_trip_latency_ms: BASELINE_RTT_MS,
            movement_ability_used: false,
            safe_corridor_milli_tiles,
            minimum_spawn_distance_milli_tiles,
            ground_telegraph_ms,
            frostbind_speed_fixture_milli_tiles_per_second: Some(FROSTBIND_SPEED),
            visibly_repeating_beam_with_inactive_warning: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternDefinition {
    pub pattern_id: String,
    pub telegraph_id: String,
    pub audio_cue_id: String,
    pub major_audio_cue_id: Option<String>,
    pub kind: PatternKind,
    pub context: PatternContext,
    pub origin_cue: OriginCue,
    pub shape_cue: ShapeCue,
    pub color_family: CombatColorFamily,
    pub damage_type: DamageType,
    pub damage_band: DamageBand,
    pub raw_damage: u32,
    pub first_warning_ms: u32,
    pub repeated_warning_ms: u32,
    pub lifetime_ticks: u32,
    pub projectile_speed_milli_tiles_per_second: Option<u32>,
    pub projectile_radius_milli_tiles: Option<u32>,
    pub counterplay: Counterplay,
    pub memory_family: EchoMemoryFamily,
    pub disposition: HostileDisposition,
    pub threat_cost: u32,
    pub maximum_active_instances: u32,
    pub compatibility_tags: BTreeSet<String>,
    pub forbidden_compatibility_tags: BTreeSet<String>,
    pub statuses: BTreeSet<PatternStatus>,
    pub mandatory: bool,
    pub pierces_players: bool,
    pub acceleration_milli_tiles_per_second_squared: i32,
    pub cancel_on_phase_change: bool,
    pub fairness: PatternFairnessFixture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPattern {
    definition: PatternDefinition,
    first_warning_ticks: u32,
    repeated_warning_ticks: u32,
}

impl ValidatedPattern {
    #[must_use]
    pub const fn definition(&self) -> &PatternDefinition {
        &self.definition
    }
    #[must_use]
    pub const fn first_warning_ticks(&self) -> u32 {
        self.first_warning_ticks
    }
    #[must_use]
    pub const fn repeated_warning_ticks(&self) -> u32 {
        self.repeated_warning_ticks
    }

    #[must_use]
    pub fn into_definition(self) -> PatternDefinition {
        self.definition
    }
}

impl PatternDefinition {
    #[allow(clippy::too_many_arguments)]
    pub fn from_projectile_attack(
        attack: &ProjectileAttackDefinition,
        kind: PatternKind,
        context: PatternContext,
        first_warning_ms: u32,
        repeated_warning_ms: u32,
        origin_cue: OriginCue,
        shape_cue: ShapeCue,
        fairness: PatternFairnessFixture,
    ) -> Self {
        let pattern_id = attack.pattern_id.to_owned();
        let audio_cue_id = format!("{pattern_id}.warning");
        Self {
            telegraph_id: format!("{pattern_id}.telegraph"),
            major_audio_cue_id: requires_major_audio(attack.damage_band)
                .then(|| format!("{audio_cue_id}.major")),
            audio_cue_id,
            pattern_id,
            kind,
            context,
            origin_cue,
            shape_cue,
            color_family: color_for_type(attack.damage_type),
            damage_type: attack.damage_type,
            damage_band: attack.damage_band,
            raw_damage: attack.raw_damage,
            first_warning_ms,
            repeated_warning_ms,
            lifetime_ticks: attack.lifetime_ticks,
            projectile_speed_milli_tiles_per_second: Some(attack.speed_milli_tiles_per_second),
            projectile_radius_milli_tiles: Some(attack.radius_milli_tiles),
            counterplay: attack.counterplay,
            memory_family: attack.memory_family,
            disposition: attack.disposition,
            threat_cost: attack.threat_cost,
            maximum_active_instances: attack.maximum_active_instances,
            compatibility_tags: BTreeSet::new(),
            forbidden_compatibility_tags: BTreeSet::new(),
            statuses: BTreeSet::new(),
            mandatory: true,
            pierces_players: attack.pierces_players,
            acceleration_milli_tiles_per_second_squared: 0,
            cancel_on_phase_change: true,
            fairness,
        }
    }

    pub fn from_lane_attack(
        attack: &LaneAttackDefinition,
        context: PatternContext,
        first_warning_ms: u32,
        repeated_warning_ms: u32,
        fairness: PatternFairnessFixture,
    ) -> Self {
        let pattern_id = attack.pattern_id.to_owned();
        let audio_cue_id = format!("{pattern_id}.warning");
        Self {
            telegraph_id: format!("{pattern_id}.telegraph"),
            major_audio_cue_id: requires_major_audio(attack.damage_band)
                .then(|| format!("{audio_cue_id}.major")),
            audio_cue_id,
            pattern_id,
            kind: PatternKind::TelegraphedLane {
                lane_count: attack.lane_count,
                width_milli_tiles: attack.width_milli_tiles,
                active_ticks: attack.active_ticks,
                extends_to_arena_collision: true,
            },
            context,
            origin_cue: OriginCue::GroundOrigin,
            shape_cue: ShapeCue::Lane,
            color_family: color_for_type(attack.damage_type),
            damage_type: attack.damage_type,
            damage_band: attack.damage_band,
            raw_damage: attack.raw_damage,
            first_warning_ms,
            repeated_warning_ms,
            lifetime_ticks: attack.active_ticks,
            projectile_speed_milli_tiles_per_second: None,
            projectile_radius_milli_tiles: None,
            counterplay: attack.counterplay,
            memory_family: attack.memory_family,
            disposition: attack.disposition,
            threat_cost: attack
                .threat_cost_per_lane
                .saturating_mul(u32::from(attack.lane_count)),
            maximum_active_instances: attack.maximum_active_instances,
            compatibility_tags: BTreeSet::new(),
            forbidden_compatibility_tags: BTreeSet::new(),
            statuses: BTreeSet::new(),
            mandatory: true,
            pierces_players: false,
            acceleration_milli_tiles_per_second_squared: 0,
            cancel_on_phase_change: true,
            fairness,
        }
    }

    pub fn validate(self) -> Result<ValidatedPattern, Vec<PatternDiagnostic>> {
        let diagnostics = validate_definition(&self);
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        let first_warning_ticks =
            u32::try_from(duration_ms_to_ticks_ceil(u64::from(self.first_warning_ms)))
                .expect("u32 milliseconds fit u32 ticks");
        let repeated_warning_ticks = u32::try_from(duration_ms_to_ticks_ceil(u64::from(
            self.repeated_warning_ms,
        )))
        .expect("u32 milliseconds fit u32 ticks");
        Ok(ValidatedPattern {
            definition: self,
            first_warning_ticks,
            repeated_warning_ticks,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PatternDiagnostic {
    InvalidPatternId,
    TelegraphIdMismatch {
        expected: String,
    },
    AudioCueIdMismatch {
        expected: String,
    },
    MissingMajorAudioCue {
        expected: String,
    },
    UnexpectedMajorAudioCue,
    FirstWarningBelowMinimum {
        required_ms: u32,
        actual_ms: u32,
    },
    RepeatedWarningBelowMinimum {
        required_ms: u32,
        actual_ms: u32,
    },
    RepeatedWarningExceedsFirst,
    ZeroLifetime,
    ZeroRawDamage,
    ZeroThreat,
    ZeroMaximumActiveInstances,
    ProjectileCapExceeded {
        cap: u32,
        actual: u32,
    },
    InvalidProjectileGeometry,
    InvalidFan,
    InvalidRingGap,
    InvalidLane,
    InvalidTimeline,
    TimelineEventsNotStrictlyOrdered,
    TimelineReferenceInvalid,
    BaselineSpeedMismatch,
    BaselineHurtboxMismatch,
    BaselineLatencyMismatch,
    MovementAbilityRequired,
    SafeCorridorTooNarrow {
        required_milli_tiles: u32,
        actual_milli_tiles: u32,
    },
    ProjectileArrivalTooFast {
        arrival_ms: u32,
    },
    CloseSpawnNeedsGroundTelegraph,
    EmptyCompatibilityTags,
    CompatibilityTagConflict {
        tag: String,
    },
    FrostbindOverlapUnsafe {
        frostbind_pattern: String,
        other_pattern: String,
    },
    WrongShapeForKind,
    WrongCounterplayForKind,
    WrongDispositionForKind,
    AccelerationMustUseCommonDefault,
    PiercingPlayersUnsupported,
    MustCancelOnPhaseChange,
    MissingFirstPlayablePattern {
        pattern_id: String,
    },
    DuplicateFirstPlayablePattern {
        pattern_id: String,
    },
    FirstPlayableFixtureDrift {
        pattern_id: String,
    },
    RouteDisplacementInsufficient {
        pattern_id: String,
        available_milli_tiles: u32,
        required_milli_tiles: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinimumSpeedRouteKind {
    StrafeFan,
    FollowRingGap,
    LeaveCrossLanes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimumSpeedRouteEvidence {
    pub pattern_id: String,
    pub route_kind: MinimumSpeedRouteKind,
    pub counterplay: Counterplay,
    pub effective_warning_ms_after_rtt: u32,
    pub available_displacement_milli_tiles: u32,
    pub required_displacement_milli_tiles: u32,
    pub safe_corridor_milli_tiles: u32,
    pub projectile_arrival_ms: Option<u32>,
    pub threat_cost: u32,
    pub maximum_active_instances: u32,
    pub encounter_projectile_cap: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstPlayableMinSpeedPaths {
    pub player_speed_milli_tiles_per_second: u32,
    pub player_hurtbox_radius_milli_tiles: u32,
    pub round_trip_latency_ms: u32,
    pub movement_ability_used: bool,
    pub routes: Vec<MinimumSpeedRouteEvidence>,
}

/// Deterministically solves the three exact normal First Playable counterplay routes.
///
/// This is the executable `fixture.fp_min_speed_paths` seam. It rejects missing, duplicate, or
/// drifted definitions and does not accept an authored boolean in place of route evidence.
pub fn solve_first_playable_min_speed_paths(
    patterns: &[ValidatedPattern],
) -> Result<FirstPlayableMinSpeedPaths, Vec<PatternDiagnostic>> {
    const PILGRIM: &str = "pattern.enemy.drowned_pilgrim.fan";
    const REED: &str = "pattern.enemy.bell_reed.gap_ring";
    const SENTRY: &str = "pattern.enemy.chain_sentry.cross_lanes";
    let mut diagnostics = Vec::new();
    let mut routes = Vec::new();
    for pattern_id in [PILGRIM, REED, SENTRY] {
        let matches: Vec<_> = patterns
            .iter()
            .filter(|pattern| pattern.definition.pattern_id == pattern_id)
            .collect();
        match matches.as_slice() {
            [] => diagnostics.push(PatternDiagnostic::MissingFirstPlayablePattern {
                pattern_id: pattern_id.to_owned(),
            }),
            [pattern] => match solve_exact_fp_route(pattern_id, pattern) {
                Ok(route) => routes.push(route),
                Err(mut errors) => diagnostics.append(&mut errors),
            },
            _ => diagnostics.push(PatternDiagnostic::DuplicateFirstPlayablePattern {
                pattern_id: pattern_id.to_owned(),
            }),
        }
    }
    diagnostics.sort();
    diagnostics.dedup();
    if diagnostics.is_empty() {
        Ok(FirstPlayableMinSpeedPaths {
            player_speed_milli_tiles_per_second: BASELINE_SPEED,
            player_hurtbox_radius_milli_tiles: BASELINE_RADIUS,
            round_trip_latency_ms: BASELINE_RTT_MS,
            movement_ability_used: false,
            routes,
        })
    } else {
        Err(diagnostics)
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the three audited First Playable route specifications stay co-located for review"
)]
fn solve_exact_fp_route(
    pattern_id: &str,
    pattern: &ValidatedPattern,
) -> Result<MinimumSpeedRouteEvidence, Vec<PatternDiagnostic>> {
    let definition = &pattern.definition;
    let drift = || PatternDiagnostic::FirstPlayableFixtureDrift {
        pattern_id: pattern_id.to_owned(),
    };
    let common_exact = definition.context == PatternContext::Normal
        && definition.fairness.player_speed_milli_tiles_per_second == BASELINE_SPEED
        && definition.fairness.player_hurtbox_radius_milli_tiles == BASELINE_RADIUS
        && definition.fairness.round_trip_latency_ms == BASELINE_RTT_MS
        && !definition.fairness.movement_ability_used
        && definition.fairness.safe_corridor_milli_tiles >= NORMAL_CORRIDOR
        && definition.maximum_active_instances <= NORMAL_CAP
        && definition.threat_cost > 0;
    let (route_kind, warning_ms, required_displacement, geometric_corridor, arrival_ms, exact) =
        match pattern_id {
            "pattern.enemy.drowned_pilgrim.fan" => {
                let exact = matches!(
                    &definition.kind,
                    PatternKind::Fan {
                        projectile_count: 3,
                        offsets_degrees
                    } if offsets_degrees == &[-15, 0, 15]
                ) && definition.first_warning_ms == 300
                    && definition.repeated_warning_ms == 300
                    && definition.projectile_speed_milli_tiles_per_second == Some(5_500)
                    && definition.projectile_radius_milli_tiles == Some(120)
                    && definition.lifetime_ticks == 66
                    && definition.counterplay == Counterplay::Strafe
                    && definition.threat_cost == 3
                    && definition.maximum_active_instances == 6
                    && definition.fairness.minimum_spawn_distance_milli_tiles == 5_000;
                // At five tiles, adjacent 15-degree fan rays are 1.305 tiles apart. Removing both
                // 0.12 projectile radii leaves 1.065 tiles edge-to-edge.
                (
                    MinimumSpeedRouteKind::StrafeFan,
                    definition.repeated_warning_ms,
                    520,
                    1_065,
                    Some(projectile_arrival_ms(5_000, 5_500)),
                    exact,
                )
            }
            "pattern.enemy.bell_reed.gap_ring" => {
                let exact = matches!(
                    definition.kind,
                    PatternKind::RingWithGap {
                        index_count: 8,
                        omitted_count: 2,
                        omitted_start_advance: 3
                    }
                ) && definition.first_warning_ms == 450
                    && definition.repeated_warning_ms == 300
                    && definition.projectile_speed_milli_tiles_per_second == Some(4_500)
                    && definition.projectile_radius_milli_tiles == Some(130)
                    && definition.lifetime_ticks == 90
                    && definition.counterplay == Counterplay::FollowGap
                    && definition.threat_cost == 6
                    && definition.maximum_active_instances == 12
                    && definition.fairness.minimum_spawn_distance_milli_tiles == 4_000;
                // Two adjacent omitted indices in an eight-index ring leave a 135-degree opening.
                // At a conservative one-tile radius its chord is 1.847 tiles; removing both 0.13
                // radii leaves 1.587 tiles edge-to-edge.
                (
                    MinimumSpeedRouteKind::FollowRingGap,
                    definition.repeated_warning_ms,
                    530,
                    1_587,
                    Some(projectile_arrival_ms(4_000, 4_500)),
                    exact,
                )
            }
            "pattern.enemy.chain_sentry.cross_lanes" => {
                let exact = matches!(
                    definition.kind,
                    PatternKind::TelegraphedLane {
                        lane_count: 2,
                        width_milli_tiles: 900,
                        active_ticks: 11,
                        extends_to_arena_collision: true
                    }
                ) && definition.first_warning_ms == 800
                    && definition.repeated_warning_ms == 650
                    && definition.projectile_speed_milli_tiles_per_second.is_none()
                    && definition.projectile_radius_milli_tiles.is_none()
                    && definition.counterplay == Counterplay::LeaveTelegraph
                    && definition.threat_cost == 24
                    && definition.maximum_active_instances == 2;
                // From the lane center: half its 0.9 width plus 0.25 player radius and 0.15
                // clearance is an exact 0.85-tile exit requirement.
                (
                    MinimumSpeedRouteKind::LeaveCrossLanes,
                    definition.repeated_warning_ms,
                    850,
                    definition.fairness.safe_corridor_milli_tiles,
                    None,
                    exact,
                )
            }
            _ => unreachable!("caller supplies one of three exact IDs"),
        };
    let effective_warning_ms = warning_ms.saturating_sub(BASELINE_RTT_MS);
    let available_displacement =
        u32::try_from(u64::from(BASELINE_SPEED) * u64::from(effective_warning_ms) / 1_000)
            .unwrap_or(u32::MAX);
    let mut diagnostics = Vec::new();
    if !common_exact || !exact {
        diagnostics.push(drift());
    }
    if geometric_corridor < NORMAL_CORRIDOR {
        diagnostics.push(PatternDiagnostic::SafeCorridorTooNarrow {
            required_milli_tiles: NORMAL_CORRIDOR,
            actual_milli_tiles: geometric_corridor,
        });
    }
    if arrival_ms.is_some_and(|arrival| arrival < MINIMUM_ARRIVAL_MS) {
        diagnostics.push(PatternDiagnostic::ProjectileArrivalTooFast {
            arrival_ms: arrival_ms.unwrap_or_default(),
        });
    }
    if available_displacement < required_displacement {
        diagnostics.push(PatternDiagnostic::RouteDisplacementInsufficient {
            pattern_id: pattern_id.to_owned(),
            available_milli_tiles: available_displacement,
            required_milli_tiles: required_displacement,
        });
    }
    if diagnostics.is_empty() {
        Ok(MinimumSpeedRouteEvidence {
            pattern_id: pattern_id.to_owned(),
            route_kind,
            counterplay: definition.counterplay,
            effective_warning_ms_after_rtt: effective_warning_ms,
            available_displacement_milli_tiles: available_displacement,
            required_displacement_milli_tiles: required_displacement,
            safe_corridor_milli_tiles: geometric_corridor,
            projectile_arrival_ms: arrival_ms,
            threat_cost: definition.threat_cost,
            maximum_active_instances: definition.maximum_active_instances,
            encounter_projectile_cap: NORMAL_CAP,
        })
    } else {
        Err(diagnostics)
    }
}

pub fn validate_pattern_combination(
    patterns: &[ValidatedPattern],
) -> Result<(), Vec<PatternDiagnostic>> {
    let mut diagnostics = Vec::new();
    for (index, left) in patterns.iter().enumerate() {
        for right in &patterns[index + 1..] {
            for tag in left
                .definition
                .compatibility_tags
                .intersection(&right.definition.forbidden_compatibility_tags)
                .chain(
                    right
                        .definition
                        .compatibility_tags
                        .intersection(&left.definition.forbidden_compatibility_tags),
                )
            {
                diagnostics.push(PatternDiagnostic::CompatibilityTagConflict { tag: tag.clone() });
            }
            validate_frostbind_pair(left, right, &mut diagnostics);
            validate_frostbind_pair(right, left, &mut diagnostics);
        }
    }
    diagnostics.sort();
    diagnostics.dedup();
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn validate_definition(definition: &PatternDefinition) -> Vec<PatternDiagnostic> {
    let mut diagnostics = Vec::new();
    if !valid_content_id(&definition.pattern_id) {
        diagnostics.push(PatternDiagnostic::InvalidPatternId);
    }
    let expected_telegraph = format!("{}.telegraph", definition.pattern_id);
    if definition.telegraph_id != expected_telegraph {
        diagnostics.push(PatternDiagnostic::TelegraphIdMismatch {
            expected: expected_telegraph,
        });
    }
    let expected_audio = format!("{}.warning", definition.pattern_id);
    if definition.audio_cue_id != expected_audio {
        diagnostics.push(PatternDiagnostic::AudioCueIdMismatch {
            expected: expected_audio.clone(),
        });
    }
    let expected_major = format!("{expected_audio}.major");
    if requires_major_audio(definition.damage_band) {
        if definition.major_audio_cue_id.as_deref() != Some(expected_major.as_str()) {
            diagnostics.push(PatternDiagnostic::MissingMajorAudioCue {
                expected: expected_major,
            });
        }
    } else if definition.major_audio_cue_id.is_some() {
        diagnostics.push(PatternDiagnostic::UnexpectedMajorAudioCue);
    }
    let (minimum_first, minimum_repeat) = minimum_warnings(definition.damage_band);
    if definition.first_warning_ms < minimum_first {
        diagnostics.push(PatternDiagnostic::FirstWarningBelowMinimum {
            required_ms: minimum_first,
            actual_ms: definition.first_warning_ms,
        });
    }
    if definition.repeated_warning_ms < minimum_repeat {
        diagnostics.push(PatternDiagnostic::RepeatedWarningBelowMinimum {
            required_ms: minimum_repeat,
            actual_ms: definition.repeated_warning_ms,
        });
    }
    if definition.repeated_warning_ms > definition.first_warning_ms {
        diagnostics.push(PatternDiagnostic::RepeatedWarningExceedsFirst);
    }
    if definition.lifetime_ticks == 0 {
        diagnostics.push(PatternDiagnostic::ZeroLifetime);
    }
    if definition.raw_damage == 0 {
        diagnostics.push(PatternDiagnostic::ZeroRawDamage);
    }
    if definition.threat_cost == 0 {
        diagnostics.push(PatternDiagnostic::ZeroThreat);
    }
    if definition.maximum_active_instances == 0 {
        diagnostics.push(PatternDiagnostic::ZeroMaximumActiveInstances);
    } else if definition.maximum_active_instances > definition.context.projectile_cap() {
        diagnostics.push(PatternDiagnostic::ProjectileCapExceeded {
            cap: definition.context.projectile_cap(),
            actual: definition.maximum_active_instances,
        });
    }
    validate_kind(definition, &mut diagnostics);
    validate_fairness(definition, &mut diagnostics);
    if definition.compatibility_tags.is_empty() {
        diagnostics.push(PatternDiagnostic::EmptyCompatibilityTags);
    }
    if definition.acceleration_milli_tiles_per_second_squared != 0 {
        diagnostics.push(PatternDiagnostic::AccelerationMustUseCommonDefault);
    }
    if definition.pierces_players {
        diagnostics.push(PatternDiagnostic::PiercingPlayersUnsupported);
    }
    if !definition.cancel_on_phase_change {
        diagnostics.push(PatternDiagnostic::MustCancelOnPhaseChange);
    }
    diagnostics.sort();
    diagnostics.dedup();
    diagnostics
}

fn validate_kind(definition: &PatternDefinition, diagnostics: &mut Vec<PatternDiagnostic>) {
    match &definition.kind {
        PatternKind::Fan {
            projectile_count,
            offsets_degrees,
        } => {
            if *projectile_count == 0
                || offsets_degrees.len() != usize::from(*projectile_count)
                || offsets_degrees.windows(2).any(|pair| pair[0] >= pair[1])
            {
                diagnostics.push(PatternDiagnostic::InvalidFan);
            }
            require_projectile_geometry(definition, diagnostics);
            check_grammar(
                definition,
                ShapeCue::Fan,
                Counterplay::Strafe,
                HostileDisposition::ConsumeOnPlayerOrSolid,
                diagnostics,
            );
        }
        PatternKind::RingWithGap {
            index_count,
            omitted_count,
            omitted_start_advance,
        } => {
            if *index_count < 3
                || *omitted_count == 0
                || *omitted_count >= *index_count
                || *omitted_start_advance == 0
                || *omitted_start_advance >= *index_count
            {
                diagnostics.push(PatternDiagnostic::InvalidRingGap);
            }
            require_projectile_geometry(definition, diagnostics);
            check_grammar(
                definition,
                ShapeCue::RingGap,
                Counterplay::FollowGap,
                HostileDisposition::ConsumeOnPlayerOrSolid,
                diagnostics,
            );
        }
        PatternKind::TelegraphedLane {
            lane_count,
            width_milli_tiles,
            active_ticks,
            extends_to_arena_collision,
        } => {
            if *lane_count == 0
                || *width_milli_tiles == 0
                || *active_ticks == 0
                || !extends_to_arena_collision
                || definition.projectile_speed_milli_tiles_per_second.is_some()
                || definition.projectile_radius_milli_tiles.is_some()
            {
                diagnostics.push(PatternDiagnostic::InvalidLane);
            }
            check_grammar(
                definition,
                ShapeCue::Lane,
                Counterplay::LeaveTelegraph,
                HostileDisposition::ExpireAtAuthoredEnd,
                diagnostics,
            );
        }
        PatternKind::FixedTimeline { loop_ticks, events } => {
            if *loop_ticks == 0 || events.is_empty() {
                diagnostics.push(PatternDiagnostic::InvalidTimeline);
            }
            if events.windows(2).any(|pair| {
                (pair[0].offset_ticks, &pair[0].pattern_id, pair[0].action)
                    >= (pair[1].offset_ticks, &pair[1].pattern_id, pair[1].action)
            }) {
                diagnostics.push(PatternDiagnostic::TimelineEventsNotStrictlyOrdered);
            }
            if events.iter().any(|event| {
                event.offset_ticks >= *loop_ticks || !valid_content_id(&event.pattern_id)
            }) {
                diagnostics.push(PatternDiagnostic::TimelineReferenceInvalid);
            }
            if definition.shape_cue != ShapeCue::Timeline {
                diagnostics.push(PatternDiagnostic::WrongShapeForKind);
            }
        }
    }
}

fn check_grammar(
    definition: &PatternDefinition,
    shape: ShapeCue,
    counterplay: Counterplay,
    disposition: HostileDisposition,
    diagnostics: &mut Vec<PatternDiagnostic>,
) {
    if definition.shape_cue != shape {
        diagnostics.push(PatternDiagnostic::WrongShapeForKind);
    }
    if definition.counterplay != counterplay {
        diagnostics.push(PatternDiagnostic::WrongCounterplayForKind);
    }
    if definition.disposition != disposition {
        diagnostics.push(PatternDiagnostic::WrongDispositionForKind);
    }
}

fn require_projectile_geometry(
    definition: &PatternDefinition,
    diagnostics: &mut Vec<PatternDiagnostic>,
) {
    if definition
        .projectile_speed_milli_tiles_per_second
        .is_none_or(|value| value == 0)
        || definition
            .projectile_radius_milli_tiles
            .is_none_or(|value| value == 0)
    {
        diagnostics.push(PatternDiagnostic::InvalidProjectileGeometry);
    }
}

fn validate_fairness(definition: &PatternDefinition, diagnostics: &mut Vec<PatternDiagnostic>) {
    let fixture = &definition.fairness;
    if fixture.player_speed_milli_tiles_per_second != BASELINE_SPEED {
        diagnostics.push(PatternDiagnostic::BaselineSpeedMismatch);
    }
    if fixture.player_hurtbox_radius_milli_tiles != BASELINE_RADIUS {
        diagnostics.push(PatternDiagnostic::BaselineHurtboxMismatch);
    }
    if fixture.round_trip_latency_ms != BASELINE_RTT_MS {
        diagnostics.push(PatternDiagnostic::BaselineLatencyMismatch);
    }
    if fixture.movement_ability_used {
        diagnostics.push(PatternDiagnostic::MovementAbilityRequired);
    }
    let required = definition.context.minimum_corridor();
    if fixture.safe_corridor_milli_tiles < required {
        diagnostics.push(PatternDiagnostic::SafeCorridorTooNarrow {
            required_milli_tiles: required,
            actual_milli_tiles: fixture.safe_corridor_milli_tiles,
        });
    }
    if fixture.minimum_spawn_distance_milli_tiles < CLOSE_SPAWN_DISTANCE
        && fixture.ground_telegraph_ms < CLOSE_SPAWN_WARNING_MS
    {
        diagnostics.push(PatternDiagnostic::CloseSpawnNeedsGroundTelegraph);
    }
    if let Some(speed) = definition.projectile_speed_milli_tiles_per_second {
        let arrival_ms = projectile_arrival_ms(fixture.minimum_spawn_distance_milli_tiles, speed);
        if arrival_ms < MINIMUM_ARRIVAL_MS && !fixture.visibly_repeating_beam_with_inactive_warning
        {
            diagnostics.push(PatternDiagnostic::ProjectileArrivalTooFast { arrival_ms });
        }
    }
}

fn validate_frostbind_pair(
    frostbind: &ValidatedPattern,
    other: &ValidatedPattern,
    diagnostics: &mut Vec<PatternDiagnostic>,
) {
    if frostbind
        .definition
        .statuses
        .contains(&PatternStatus::Frostbind)
        && other.definition.mandatory
        && other
            .definition
            .fairness
            .frostbind_speed_fixture_milli_tiles_per_second
            != Some(FROSTBIND_SPEED)
    {
        diagnostics.push(PatternDiagnostic::FrostbindOverlapUnsafe {
            frostbind_pattern: frostbind.definition.pattern_id.clone(),
            other_pattern: other.definition.pattern_id.clone(),
        });
    }
}

#[must_use]
pub const fn minimum_warnings(band: DamageBand) -> (u32, u32) {
    match band {
        DamageBand::Chip => (250, 200),
        DamageBand::Pressure => (400, 300),
        DamageBand::Major => (650, 500),
        DamageBand::Severe => (900, 750),
        DamageBand::Execution => (1_200, 1_000),
    }
}

#[must_use]
pub const fn required_player_center_boundary_clearance_milli_tiles() -> u32 {
    BASELINE_RADIUS + BOUNDARY_CLEARANCE
}

#[must_use]
pub const fn safe_player_center_span_milli_tiles(edge_to_edge_corridor: u32) -> u32 {
    edge_to_edge_corridor
        .saturating_sub(required_player_center_boundary_clearance_milli_tiles().saturating_mul(2))
}

#[must_use]
pub fn projectile_arrival_ms(distance_milli_tiles: u32, speed_milli_tiles_per_second: u32) -> u32 {
    if speed_milli_tiles_per_second == 0 {
        return u32::MAX;
    }
    u32::try_from(
        (u64::from(distance_milli_tiles) * 1_000).div_ceil(u64::from(speed_milli_tiles_per_second)),
    )
    .unwrap_or(u32::MAX)
}

fn valid_content_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

const fn requires_major_audio(band: DamageBand) -> bool {
    matches!(
        band,
        DamageBand::Major | DamageBand::Severe | DamageBand::Execution
    )
}

const fn color_for_type(damage_type: DamageType) -> CombatColorFamily {
    match damage_type {
        DamageType::Physical => CombatColorFamily::Physical,
        DamageType::Veil => CombatColorFamily::Veil,
    }
}

/// Canonical slow-speed compatibility baseline required when Frostbind may overlap.
#[must_use]
pub const fn frostbind_compatibility_speed_milli_tiles_per_second() -> u32 {
    FROSTBIND_SPEED
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BellProctorDefinition, BellReedDefinition, ChainSentryDefinition, DrownedPilgrimDefinition,
    };

    fn tags(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn pilgrim_pattern() -> PatternDefinition {
        let definition = DrownedPilgrimDefinition::first_playable();
        let attack = &definition.parameters().attack;
        let mut definition = PatternDefinition::from_projectile_attack(
            attack,
            PatternKind::Fan {
                projectile_count: 3,
                offsets_degrees: vec![-15, 0, 15],
            },
            PatternContext::Normal,
            300,
            300,
            OriginCue::SourceSilhouette,
            ShapeCue::Fan,
            PatternFairnessFixture::baseline(800, 5_000, 0),
        );
        definition.compatibility_tags = tags(&["fan_projectile"]);
        definition
    }

    fn reed_pattern() -> PatternDefinition {
        let definition = BellReedDefinition::first_playable();
        let attack = &definition.parameters().attack;
        let mut definition = PatternDefinition::from_projectile_attack(
            attack,
            PatternKind::RingWithGap {
                index_count: 8,
                omitted_count: 2,
                omitted_start_advance: 3,
            },
            PatternContext::Normal,
            450,
            300,
            OriginCue::SourceSilhouette,
            ShapeCue::RingGap,
            PatternFairnessFixture::baseline(800, 4_000, 0),
        );
        definition.compatibility_tags = tags(&["radial_projectile"]);
        definition
    }

    fn sentry_pattern() -> PatternDefinition {
        let definition = ChainSentryDefinition::first_playable();
        let attack = &definition.parameters().attack;
        let mut definition = PatternDefinition::from_lane_attack(
            attack,
            PatternContext::Normal,
            800,
            650,
            PatternFairnessFixture::baseline(800, 2_000, 800),
        );
        definition.compatibility_tags = tags(&["lane_or_beam"]);
        definition
    }

    #[test]
    fn exact_band_warning_minima_compile_with_ceiling_ticks() {
        type WarningCase = (DamageBand, (u32, u32), (u32, u32));
        let cases: [WarningCase; 5] = [
            (DamageBand::Chip, (250, 200), (8, 6)),
            (DamageBand::Pressure, (400, 300), (12, 9)),
            (DamageBand::Major, (650, 500), (20, 15)),
            (DamageBand::Severe, (900, 750), (27, 23)),
            (DamageBand::Execution, (1_200, 1_000), (36, 30)),
        ];
        for (band, milliseconds, ticks) in cases {
            assert_eq!(minimum_warnings(band), milliseconds);
            assert_eq!(
                duration_ms_to_ticks_ceil(u64::from(milliseconds.0)),
                u64::from(ticks.0)
            );
            assert_eq!(
                duration_ms_to_ticks_ceil(u64::from(milliseconds.1)),
                u64::from(ticks.1)
            );
        }
    }

    #[test]
    fn existing_enemy_and_boss_primitives_adapt_without_semantic_loss() {
        let pilgrim = pilgrim_pattern().validate().expect("Pilgrim");
        assert_eq!(pilgrim.first_warning_ticks(), 9);
        assert!(reed_pattern().validate().is_ok());
        assert!(sentry_pattern().validate().is_ok());
        let boss = BellProctorDefinition::first_playable();
        let mut fan = PatternDefinition::from_projectile_attack(
            &boss.parameters().fan,
            PatternKind::Fan {
                projectile_count: 5,
                offsets_degrees: vec![-20, -10, 0, 10, 20],
            },
            PatternContext::Boss,
            400,
            400,
            OriginCue::SourceSilhouette,
            ShapeCue::Fan,
            PatternFairnessFixture::baseline(800, 5_000, 0),
        );
        fan.compatibility_tags = tags(&["fan_projectile"]);
        assert!(fan.validate().is_ok());
    }

    #[test]
    fn geometry_helpers_pin_corridor_and_arrival_boundaries() {
        assert_eq!(required_player_center_boundary_clearance_milli_tiles(), 400);
        assert_eq!(safe_player_center_span_milli_tiles(800), 0);
        assert_eq!(safe_player_center_span_milli_tiles(1_000), 200);
        assert_eq!(projectile_arrival_ms(1_925, 5_500), 350);
        assert_eq!(projectile_arrival_ms(1_919, 5_500), 349);
        assert_eq!(
            frostbind_compatibility_speed_milli_tiles_per_second(),
            4_000
        );
    }

    #[test]
    fn validator_accumulates_sorted_adversarial_diagnostics() {
        let mut invalid = pilgrim_pattern();
        invalid.telegraph_id = "wrong".to_owned();
        invalid.audio_cue_id = "wrong".to_owned();
        invalid.first_warning_ms = 1;
        invalid.repeated_warning_ms = 2;
        invalid.threat_cost = 0;
        invalid.maximum_active_instances = 301;
        invalid.compatibility_tags.clear();
        invalid.cancel_on_phase_change = false;
        invalid.fairness.player_speed_milli_tiles_per_second = 4_501;
        invalid.fairness.safe_corridor_milli_tiles = 799;
        invalid.fairness.minimum_spawn_distance_milli_tiles = 1_000;
        let diagnostics = invalid.validate().expect_err("invalid");
        assert!(diagnostics.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(diagnostics.contains(&PatternDiagnostic::ZeroThreat));
        assert!(
            diagnostics.contains(&PatternDiagnostic::ProjectileCapExceeded {
                cap: 300,
                actual: 301
            })
        );
        assert!(diagnostics.contains(&PatternDiagnostic::CloseSpawnNeedsGroundTelegraph));
    }

    #[test]
    fn timeline_requires_sorted_legal_references_inside_loop() {
        let mut timeline = pilgrim_pattern();
        timeline.kind = PatternKind::FixedTimeline {
            loop_ticks: 100,
            events: vec![
                FixedTimelineEvent {
                    offset_ticks: 20,
                    pattern_id: "pattern.a".to_owned(),
                    action: TimelineAction::Resolve,
                },
                FixedTimelineEvent {
                    offset_ticks: 10,
                    pattern_id: "BAD".to_owned(),
                    action: TimelineAction::Telegraph,
                },
            ],
        };
        timeline.shape_cue = ShapeCue::Timeline;
        let diagnostics = timeline.validate().expect_err("timeline");
        assert!(diagnostics.contains(&PatternDiagnostic::TimelineEventsNotStrictlyOrdered));
        assert!(diagnostics.contains(&PatternDiagnostic::TimelineReferenceInvalid));
    }

    #[test]
    fn compatibility_and_frostbind_overlap_fail_deterministically() {
        let mut frostbind = pilgrim_pattern();
        frostbind.pattern_id = "pattern.test.frostbind".to_owned();
        frostbind.telegraph_id = "pattern.test.frostbind.telegraph".to_owned();
        frostbind.audio_cue_id = "pattern.test.frostbind.warning".to_owned();
        frostbind.statuses.insert(PatternStatus::Frostbind);
        frostbind.compatibility_tags = tags(&["frost"]);
        frostbind.forbidden_compatibility_tags = tags(&["lane"]);
        let frostbind = frostbind.validate().expect("Frostbind");
        let mut lane = pilgrim_pattern();
        lane.pattern_id = "pattern.test.lane".to_owned();
        lane.telegraph_id = "pattern.test.lane.telegraph".to_owned();
        lane.audio_cue_id = "pattern.test.lane.warning".to_owned();
        lane.compatibility_tags = tags(&["lane"]);
        lane.fairness.frostbind_speed_fixture_milli_tiles_per_second = None;
        let lane = lane.validate().expect("lane");
        let diagnostics =
            validate_pattern_combination(&[frostbind, lane]).expect_err("combination");
        assert!(
            diagnostics.contains(&PatternDiagnostic::CompatibilityTagConflict {
                tag: "lane".to_owned()
            })
        );
        assert!(diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            PatternDiagnostic::FrostbindOverlapUnsafe { .. }
        )));
    }

    #[test]
    fn schema_semantics_reject_fast_spawn_and_wrong_major_audio() {
        let mut pattern = pilgrim_pattern();
        pattern.damage_band = DamageBand::Major;
        pattern.first_warning_ms = 650;
        pattern.repeated_warning_ms = 500;
        pattern.fairness.minimum_spawn_distance_milli_tiles = 1_000;
        pattern.fairness.ground_telegraph_ms = 750;
        pattern.projectile_speed_milli_tiles_per_second = Some(10_000);
        let diagnostics = pattern.validate().expect_err("major");
        assert!(diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            PatternDiagnostic::MissingMajorAudioCue { .. }
        )));
        assert!(
            diagnostics.contains(&PatternDiagnostic::ProjectileArrivalTooFast { arrival_ms: 100 })
        );
    }

    #[test]
    fn fixture_fp_min_speed_paths_proves_all_three_exact_normal_routes() {
        let patterns = [
            pilgrim_pattern().validate().expect("Pilgrim"),
            reed_pattern().validate().expect("Reed"),
            sentry_pattern().validate().expect("Sentry"),
        ];
        let fixture = solve_first_playable_min_speed_paths(&patterns).expect("minimum-speed paths");

        assert_eq!(fixture.player_speed_milli_tiles_per_second, 4_500);
        assert_eq!(fixture.player_hurtbox_radius_milli_tiles, 250);
        assert_eq!(fixture.round_trip_latency_ms, 120);
        assert!(!fixture.movement_ability_used);
        assert_eq!(fixture.routes.len(), 3);

        assert_eq!(
            fixture.routes[0],
            MinimumSpeedRouteEvidence {
                pattern_id: "pattern.enemy.drowned_pilgrim.fan".to_owned(),
                route_kind: MinimumSpeedRouteKind::StrafeFan,
                counterplay: Counterplay::Strafe,
                effective_warning_ms_after_rtt: 180,
                available_displacement_milli_tiles: 810,
                required_displacement_milli_tiles: 520,
                safe_corridor_milli_tiles: 1_065,
                projectile_arrival_ms: Some(910),
                threat_cost: 3,
                maximum_active_instances: 6,
                encounter_projectile_cap: 300,
            }
        );
        assert_eq!(
            fixture.routes[1],
            MinimumSpeedRouteEvidence {
                pattern_id: "pattern.enemy.bell_reed.gap_ring".to_owned(),
                route_kind: MinimumSpeedRouteKind::FollowRingGap,
                counterplay: Counterplay::FollowGap,
                effective_warning_ms_after_rtt: 180,
                available_displacement_milli_tiles: 810,
                required_displacement_milli_tiles: 530,
                safe_corridor_milli_tiles: 1_587,
                projectile_arrival_ms: Some(889),
                threat_cost: 6,
                maximum_active_instances: 12,
                encounter_projectile_cap: 300,
            }
        );
        assert_eq!(
            fixture.routes[2],
            MinimumSpeedRouteEvidence {
                pattern_id: "pattern.enemy.chain_sentry.cross_lanes".to_owned(),
                route_kind: MinimumSpeedRouteKind::LeaveCrossLanes,
                counterplay: Counterplay::LeaveTelegraph,
                effective_warning_ms_after_rtt: 530,
                available_displacement_milli_tiles: 2_385,
                required_displacement_milli_tiles: 850,
                safe_corridor_milli_tiles: 800,
                projectile_arrival_ms: None,
                threat_cost: 24,
                maximum_active_instances: 2,
                encounter_projectile_cap: 300,
            }
        );
    }

    #[test]
    fn minimum_path_solver_rejects_missing_duplicate_and_exact_drift() {
        let pilgrim = pilgrim_pattern().validate().expect("Pilgrim");
        let reed = reed_pattern().validate().expect("Reed");
        let sentry = sentry_pattern().validate().expect("Sentry");

        let missing = solve_first_playable_min_speed_paths(std::slice::from_ref(&pilgrim))
            .expect_err("missing patterns");
        assert!(
            missing.contains(&PatternDiagnostic::MissingFirstPlayablePattern {
                pattern_id: "pattern.enemy.bell_reed.gap_ring".to_owned(),
            })
        );
        assert!(
            missing.contains(&PatternDiagnostic::MissingFirstPlayablePattern {
                pattern_id: "pattern.enemy.chain_sentry.cross_lanes".to_owned(),
            })
        );

        let duplicate = solve_first_playable_min_speed_paths(&[
            pilgrim.clone(),
            pilgrim,
            reed.clone(),
            sentry.clone(),
        ])
        .expect_err("duplicate pattern");
        assert_eq!(
            duplicate,
            vec![PatternDiagnostic::DuplicateFirstPlayablePattern {
                pattern_id: "pattern.enemy.drowned_pilgrim.fan".to_owned(),
            }]
        );

        let mut drifted_reed = reed;
        drifted_reed.definition.repeated_warning_ms = 301;
        let drift = solve_first_playable_min_speed_paths(&[
            pilgrim_pattern().validate().expect("Pilgrim"),
            drifted_reed,
            sentry,
        ])
        .expect_err("fixture drift");
        assert_eq!(
            drift,
            vec![PatternDiagnostic::FirstPlayableFixtureDrift {
                pattern_id: "pattern.enemy.bell_reed.gap_ring".to_owned(),
            }]
        );
    }

    #[test]
    fn route_displacement_boundary_is_typed_and_deterministic() {
        let mut sentry = sentry_pattern().validate().expect("Sentry");
        sentry.definition.repeated_warning_ms = 300;
        let diagnostics = solve_first_playable_min_speed_paths(&[
            pilgrim_pattern().validate().expect("Pilgrim"),
            reed_pattern().validate().expect("Reed"),
            sentry,
        ])
        .expect_err("insufficient route displacement");

        assert!(
            diagnostics.contains(&PatternDiagnostic::FirstPlayableFixtureDrift {
                pattern_id: "pattern.enemy.chain_sentry.cross_lanes".to_owned(),
            })
        );
        assert!(
            diagnostics.contains(&PatternDiagnostic::RouteDisplacementInsufficient {
                pattern_id: "pattern.enemy.chain_sentry.cross_lanes".to_owned(),
                available_milli_tiles: 810,
                required_milli_tiles: 850,
            })
        );
        assert!(diagnostics.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
