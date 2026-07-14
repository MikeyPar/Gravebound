//! Deterministic COM-006 evidence for the eight Core-authored hostile patterns.
//!
//! The fixture consumes compiled simulation definitions, never renderer state or authored pass/fail
//! flags. Distances are milli-tiles and timings are authored milliseconds so the evidence remains
//! directly reviewable against the three design authorities.

use thiserror::Error;

use crate::{
    CoreEnemyDefinition, CoreEnemyLocomotionDefinition, CorePatternDefinition,
    CorePatternGeometryDefinition, CorePatternWarningDefinition, CoreRadialGapRelation,
    Counterplay, projectile_arrival_ms,
};

pub const CORE_COM006_PLAYER_SPEED_MILLI_TILES_PER_SECOND: u32 = 4_500;
pub const CORE_COM006_PLAYER_HURTBOX_RADIUS_MILLI_TILES: u32 = 250;
pub const CORE_COM006_ROUND_TRIP_LATENCY_MILLISECONDS: u32 = 120;
pub const CORE_COM006_NORMAL_SAFE_CORRIDOR_MILLI_TILES: u32 = 800;
pub const CORE_COM006_CLOSE_SPAWN_DISTANCE_MILLI_TILES: u32 = 1_250;
pub const CORE_COM006_CLOSE_SPAWN_GROUND_WARNING_MILLISECONDS: u32 = 750;
pub const CORE_COM006_MINIMUM_PROJECTILE_ARRIVAL_MILLISECONDS: u32 = 350;
pub const CORE_COM006_STANDARD_PROJECTILE_CAP: u32 = 300;

const AUTHORED_PATTERN_IDS: [&str; 8] = [
    "pattern.enemy.mire_leech.charge",
    "pattern.enemy.bell_acolyte.alternating_fan",
    "pattern.enemy.choir_skull.rotor",
    "miniboss.sepulcher_knight.charge_lane",
    "miniboss.sepulcher_knight.stop_ring",
    "miniboss.sepulcher_knight.shield_fan",
    "miniboss.choir_abbot.rotor",
    "miniboss.choir_abbot.recovery_ring",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCounterplayRouteKind {
    LeaveChargePath,
    StrafeAlternatingFan,
    TrackChoirSkullRotor,
    LeaveChargeLane,
    FollowChargeStopGap,
    LeaveShieldFanEnvelope,
    TrackChoirAbbotRotor,
    FollowRecoveryGap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreCounterplayMotionProof {
    Displacement {
        effective_response_milliseconds: u32,
        available_displacement_milli_tiles: u32,
        required_displacement_milli_tiles: u32,
    },
    AngularTracking {
        effective_setup_milliseconds: u32,
        available_setup_displacement_milli_tiles: u32,
        required_setup_displacement_milli_tiles: u32,
        tracking_radius_milli_tiles: u32,
        available_tracking_speed_milli_tiles_per_second: u32,
        required_tracking_speed_milli_tiles_per_second: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreProjectileFairnessProof {
    pub minimum_start_distance_milli_tiles: u32,
    pub ground_origin_warning_milliseconds: u32,
    pub available_origin_escape_milli_tiles: u32,
    pub required_origin_escape_milli_tiles: u32,
    pub minimum_release_distance_milli_tiles: u32,
    pub projectile_arrival_milliseconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCounterplayRouteEvidence {
    pub pattern_id: String,
    pub route_kind: CoreCounterplayRouteKind,
    pub counterplay: Counterplay,
    pub motion: CoreCounterplayMotionProof,
    pub safe_corridor_milli_tiles: u32,
    pub projectile: Option<CoreProjectileFairnessProof>,
    pub threat_cost: u16,
    pub maximum_active_instances: u16,
    pub encounter_projectile_cap: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreAuthoredMinSpeedPaths {
    pub player_speed_milli_tiles_per_second: u32,
    pub player_hurtbox_radius_milli_tiles: u32,
    pub round_trip_latency_milliseconds: u32,
    pub movement_ability_used: bool,
    pub routes: Vec<CoreCounterplayRouteEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Error)]
pub enum CoreCounterplayDiagnostic {
    #[error("missing Core-authored pattern {pattern_id}")]
    MissingPattern { pattern_id: String },
    #[error("duplicate Core-authored pattern {pattern_id}")]
    DuplicatePattern { pattern_id: String },
    #[error("Core-authored counterplay fixture drifted for {pattern_id}")]
    FixtureDrift { pattern_id: String },
    #[error("{pattern_id} has only {actual_milli_tiles} milli-tiles of safe corridor")]
    SafeCorridorTooNarrow {
        pattern_id: String,
        actual_milli_tiles: u32,
    },
    #[error("{pattern_id} route displacement is insufficient")]
    DisplacementInsufficient {
        pattern_id: String,
        available_milli_tiles: u32,
        required_milli_tiles: u32,
    },
    #[error("{pattern_id} rotor tracking speed is insufficient")]
    TrackingSpeedInsufficient {
        pattern_id: String,
        available_milli_tiles_per_second: u32,
        required_milli_tiles_per_second: u32,
    },
    #[error("{pattern_id} close-spawn ground warning is insufficient")]
    CloseSpawnGroundWarningInsufficient {
        pattern_id: String,
        actual_milliseconds: u32,
    },
    #[error("{pattern_id} projectile arrival is too fast")]
    ProjectileArrivalTooFast {
        pattern_id: String,
        actual_milliseconds: u32,
    },
}

/// Solves the eight exact Core-authored counterplay routes at the canonical COM-006 baseline.
///
/// Reused First Playable routes remain owned by `solve_first_playable_min_speed_paths`; callers
/// combine both reports for the complete six-normal/two-miniboss roster.
pub fn solve_core_authored_min_speed_paths(
    enemies: &[CoreEnemyDefinition],
) -> Result<CoreAuthoredMinSpeedPaths, Vec<CoreCounterplayDiagnostic>> {
    let mut diagnostics = Vec::new();
    let mut routes = Vec::new();
    for pattern_id in AUTHORED_PATTERN_IDS {
        let matches = enemies
            .iter()
            .flat_map(|enemy| {
                enemy
                    .parameters()
                    .patterns
                    .iter()
                    .map(move |pattern| (enemy, pattern))
            })
            .filter(|(_, pattern)| pattern.parameters().id == pattern_id)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => diagnostics.push(CoreCounterplayDiagnostic::MissingPattern {
                pattern_id: pattern_id.to_owned(),
            }),
            [(enemy, pattern)] => match solve_exact_route(pattern_id, enemy, pattern) {
                Ok(route) => routes.push(route),
                Err(mut route_diagnostics) => diagnostics.append(&mut route_diagnostics),
            },
            _ => diagnostics.push(CoreCounterplayDiagnostic::DuplicatePattern {
                pattern_id: pattern_id.to_owned(),
            }),
        }
    }
    diagnostics.sort();
    diagnostics.dedup();
    if diagnostics.is_empty() {
        Ok(CoreAuthoredMinSpeedPaths {
            player_speed_milli_tiles_per_second: CORE_COM006_PLAYER_SPEED_MILLI_TILES_PER_SECOND,
            player_hurtbox_radius_milli_tiles: CORE_COM006_PLAYER_HURTBOX_RADIUS_MILLI_TILES,
            round_trip_latency_milliseconds: CORE_COM006_ROUND_TRIP_LATENCY_MILLISECONDS,
            movement_ability_used: false,
            routes,
        })
    } else {
        Err(diagnostics)
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "all eight authority-audited routes stay together for field-by-field review"
)]
fn solve_exact_route(
    pattern_id: &str,
    enemy: &CoreEnemyDefinition,
    pattern: &CorePatternDefinition,
) -> Result<CoreCounterplayRouteEvidence, Vec<CoreCounterplayDiagnostic>> {
    let parameters = pattern.parameters();
    let (route_kind, counterplay, motion, corridor, projectile, exact) = match pattern_id {
        // The charge body uses the enemy's 0.35 radius. Adding the 0.25 player radius and 0.15
        // Standard clearance requires a 0.75-tile lateral exit from a centerline start.
        "pattern.enemy.mire_leech.charge" => (
            CoreCounterplayRouteKind::LeaveChargePath,
            Counterplay::LeaveTelegraph,
            displacement_proof(300, 750),
            800,
            None,
            matches!(
                (enemy.locomotion(), pattern.warning(), pattern.geometry()),
                (
                    CoreEnemyLocomotionDefinition::RushRetreat {
                        trigger_distance_milli_tiles: 2_500,
                        charge_distance_milli_tiles: 2_000,
                        charge_ticks: 15,
                        ..
                    },
                    CorePatternWarningDefinition::Standalone {
                        first_ticks: 12,
                        repeated_ticks: 9,
                    },
                    CorePatternGeometryDefinition::Charge {
                        distance_milli_tiles: 2_000,
                        duration_ticks: 15,
                    },
                )
            ),
        ),
        "pattern.enemy.bell_acolyte.alternating_fan" => {
            let arrival = projectile_arrival_ms(6_000, 6_000);
            // At six tiles, adjacent 15-degree rays are 1.566 tiles apart; removing two 0.11
            // projectile radii leaves 1.346. Clearing the outer 10-degree ray requires its 1.042
            // lateral offset plus player radius, projectile radius, and Standard clearance = 1.552.
            (
                CoreCounterplayRouteKind::StrafeAlternatingFan,
                Counterplay::Strafe,
                displacement_proof(300 + arrival, 1_552),
                1_346,
                Some(projectile_proof(6_000, 300, 6_000, 6_000)),
                matches!(
                    (enemy.locomotion(), pattern.warning(), pattern.geometry()),
                    (
                        CoreEnemyLocomotionDefinition::MaintainDistance {
                            preferred_distance_milli_tiles: 6_000,
                            ..
                        },
                        CorePatternWarningDefinition::Standalone {
                            first_ticks: 12,
                            repeated_ticks: 9,
                        },
                        CorePatternGeometryDefinition::AlternatingFan {
                            projectile_speed_milli_tiles_per_second: 6_000,
                            projectile_radius_milli_tiles: 110,
                            ..
                        },
                    )
                ),
            )
        }
        // Both opposite arms leave a continuous half-plane route. The player can establish the
        // authored three-tile orbit before first emission, then 35 degrees/s at radius three needs
        // only 1.833 tiles/s of tangential speed.
        "pattern.enemy.choir_skull.rotor" => (
            CoreCounterplayRouteKind::TrackChoirSkullRotor,
            Counterplay::MoveWithRotation,
            angular_tracking_proof(900, 2_400, 3_000, 35_000),
            800,
            Some(projectile_proof(600, 900, 3_000, 4_500)),
            matches!(
                (enemy.locomotion(), pattern.warning(), pattern.geometry()),
                (
                    CoreEnemyLocomotionDefinition::OrbitAnchor {
                        orbit_radius_milli_tiles: 3_000,
                        ..
                    },
                    CorePatternWarningDefinition::Standalone {
                        first_ticks: 20,
                        repeated_ticks: 15,
                    },
                    CorePatternGeometryDefinition::RotatingArms {
                        arm_count: 2,
                        clockwise_milli_degrees_per_second: 35_000,
                        emission_interval_ticks: 12,
                        active_ticks: 120,
                        ..
                    },
                )
            ),
        ),
        // Lane half-width 0.50 + player radius 0.25 + Standard clearance 0.15 = 0.90.
        "miniboss.sepulcher_knight.charge_lane" => (
            CoreCounterplayRouteKind::LeaveChargeLane,
            Counterplay::LeaveTelegraph,
            displacement_proof(900, 900),
            800,
            None,
            matches!(
                pattern.geometry(),
                CorePatternGeometryDefinition::ChargeLane {
                    width_milli_tiles: 1_000,
                    length_milli_tiles: 5_000,
                    charge_ticks: 17,
                }
            ) && matches!(
                pattern.warning(),
                CorePatternWarningDefinition::Standalone {
                    first_ticks: 27,
                    repeated_ticks: 27,
                }
            ),
        ),
        // The parent warning plus charge travel provides the response window. A 1.75-tile release
        // distance is exactly 350 ms at speed five. The two-index gap spans 108 degrees; its
        // one-tile chord minus two projectile radii leaves a conservative 1.378-tile corridor.
        "miniboss.sepulcher_knight.stop_ring" => (
            CoreCounterplayRouteKind::FollowChargeStopGap,
            Counterplay::FollowGap,
            displacement_proof(900 + 550, 1_750),
            1_378,
            Some(projectile_proof(800, 900, 1_750, 5_000)),
            matches!(pattern.warning(), CorePatternWarningDefinition::ParentOnly)
                && matches!(
                    pattern.geometry(),
                    CorePatternGeometryDefinition::RadialGap {
                        index_count: 10,
                        omitted_adjacent_count: 2,
                        relation: CoreRadialGapRelation::TargetOpposite,
                        projectile_speed_milli_tiles_per_second: 5_000,
                        projectile_radius_milli_tiles: 120,
                        ..
                    }
                ),
        ),
        "miniboss.sepulcher_knight.shield_fan" => {
            let arrival = projectile_arrival_ms(3_500, 6_000);
            // Leaving the full +/-25-degree envelope at the 3.5-tile stop distance requires 1.479
            // lateral tiles plus player radius, projectile radius, and clearance = 1.999.
            (
                CoreCounterplayRouteKind::LeaveShieldFanEnvelope,
                Counterplay::Strafe,
                displacement_proof(300 + arrival, 1_999),
                800,
                Some(projectile_proof(3_500, 300, 3_500, 6_000)),
                matches!(
                    (enemy.locomotion(), pattern.warning(), pattern.geometry()),
                    (
                        CoreEnemyLocomotionDefinition::PursueStopChargeHome {
                            stop_distance_milli_tiles: 3_500,
                            ..
                        },
                        CorePatternWarningDefinition::Standalone {
                            first_ticks: 12,
                            repeated_ticks: 9,
                        },
                        CorePatternGeometryDefinition::ProjectileFan {
                            shot_count: 5,
                            total_arc_milli_degrees: 50_000,
                            projectile_speed_milli_tiles_per_second: 6_000,
                            projectile_radius_milli_tiles: 120,
                            ..
                        },
                    )
                ),
            )
        }
        // The stationary Abbot gives the same radius-three tracking route. Its 500 ms repeated arm
        // preview remains on the ground until the first scheduled emission 350 ms later.
        "miniboss.choir_abbot.rotor" => (
            CoreCounterplayRouteKind::TrackChoirAbbotRotor,
            Counterplay::MoveWithRotation,
            angular_tracking_proof(850, 2_200, 3_000, 35_000),
            800,
            Some(projectile_proof(800, 850, 3_000, 4_500)),
            matches!(
                enemy.locomotion(),
                CoreEnemyLocomotionDefinition::Stationary
            ) && matches!(
                (pattern.warning(), pattern.geometry()),
                (
                    CorePatternWarningDefinition::Standalone {
                        first_ticks: 20,
                        repeated_ticks: 15,
                    },
                    CorePatternGeometryDefinition::RotatingArms {
                        arm_count: 2,
                        clockwise_milli_degrees_per_second: 35_000,
                        emission_interval_ticks: 11,
                        active_ticks: 105,
                        ..
                    },
                )
            ),
        ),
        // The approved 2.5-second origin cue provides release-distance preparation; only the exact
        // final 650 ms is used to align with the target-facing gap. Four omitted indices span a
        // 112.5-degree opening, whose one-tile chord minus projectile radii is 1.422 tiles.
        "miniboss.choir_abbot.recovery_ring" => (
            CoreCounterplayRouteKind::FollowRecoveryGap,
            Counterplay::FollowGap,
            displacement_proof(650, 520),
            1_422,
            Some(projectile_proof(800, 2_500, 1_575, 4_500)),
            matches!(
                enemy.locomotion(),
                CoreEnemyLocomotionDefinition::Stationary
            ) && matches!(
                (pattern.warning(), pattern.geometry()),
                (
                    CorePatternWarningDefinition::RecoveryPreview {
                        ground_origin_warning_ticks: 75,
                        directional_gap_preview_ticks: 20,
                        major_audio: true,
                    },
                    CorePatternGeometryDefinition::RadialGap {
                        index_count: 16,
                        omitted_adjacent_count: 4,
                        relation: CoreRadialGapRelation::TargetFacing,
                        projectile_speed_milli_tiles_per_second: 4_500,
                        projectile_radius_milli_tiles: 120,
                        ..
                    },
                )
            ),
        ),
        _ => unreachable!("caller supplies one of eight exact IDs"),
    };

    let mut diagnostics = Vec::new();
    if parameters.counterplay != counterplay
        || parameters.persisted_maximum_active_instances == 0
        || u32::from(parameters.persisted_maximum_active_instances)
            > CORE_COM006_STANDARD_PROJECTILE_CAP
        || parameters.threat_cost == 0
        || !exact
    {
        diagnostics.push(CoreCounterplayDiagnostic::FixtureDrift {
            pattern_id: pattern_id.to_owned(),
        });
    }
    if corridor < CORE_COM006_NORMAL_SAFE_CORRIDOR_MILLI_TILES {
        diagnostics.push(CoreCounterplayDiagnostic::SafeCorridorTooNarrow {
            pattern_id: pattern_id.to_owned(),
            actual_milli_tiles: corridor,
        });
    }
    validate_motion(pattern_id, &motion, &mut diagnostics);
    if let Some(projectile) = &projectile {
        validate_projectile(pattern_id, projectile, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        Ok(CoreCounterplayRouteEvidence {
            pattern_id: pattern_id.to_owned(),
            route_kind,
            counterplay,
            motion,
            safe_corridor_milli_tiles: corridor,
            projectile,
            threat_cost: parameters.threat_cost,
            maximum_active_instances: parameters.persisted_maximum_active_instances,
            encounter_projectile_cap: CORE_COM006_STANDARD_PROJECTILE_CAP,
        })
    } else {
        Err(diagnostics)
    }
}

fn displacement_proof(
    response_milliseconds: u32,
    required_displacement_milli_tiles: u32,
) -> CoreCounterplayMotionProof {
    let effective =
        response_milliseconds.saturating_sub(CORE_COM006_ROUND_TRIP_LATENCY_MILLISECONDS);
    CoreCounterplayMotionProof::Displacement {
        effective_response_milliseconds: effective,
        available_displacement_milli_tiles: displacement_at_baseline(effective),
        required_displacement_milli_tiles,
    }
}

fn angular_tracking_proof(
    setup_milliseconds: u32,
    required_setup_displacement_milli_tiles: u32,
    tracking_radius_milli_tiles: u32,
    angular_speed_milli_degrees_per_second: u32,
) -> CoreCounterplayMotionProof {
    let effective = setup_milliseconds.saturating_sub(CORE_COM006_ROUND_TRIP_LATENCY_MILLISECONDS);
    // 57.296 degrees per radian, rounded down, makes this ceiling conservative.
    let required_tracking_speed = u32::try_from(
        (u64::from(tracking_radius_milli_tiles)
            * u64::from(angular_speed_milli_degrees_per_second))
        .div_ceil(57_296),
    )
    .unwrap_or(u32::MAX);
    CoreCounterplayMotionProof::AngularTracking {
        effective_setup_milliseconds: effective,
        available_setup_displacement_milli_tiles: displacement_at_baseline(effective),
        required_setup_displacement_milli_tiles,
        tracking_radius_milli_tiles,
        available_tracking_speed_milli_tiles_per_second:
            CORE_COM006_PLAYER_SPEED_MILLI_TILES_PER_SECOND,
        required_tracking_speed_milli_tiles_per_second: required_tracking_speed,
    }
}

fn projectile_proof(
    minimum_start_distance_milli_tiles: u32,
    ground_origin_warning_milliseconds: u32,
    minimum_release_distance_milli_tiles: u32,
    projectile_speed_milli_tiles_per_second: u32,
) -> CoreProjectileFairnessProof {
    let effective = ground_origin_warning_milliseconds
        .saturating_sub(CORE_COM006_ROUND_TRIP_LATENCY_MILLISECONDS);
    CoreProjectileFairnessProof {
        minimum_start_distance_milli_tiles,
        ground_origin_warning_milliseconds,
        available_origin_escape_milli_tiles: displacement_at_baseline(effective),
        required_origin_escape_milli_tiles: minimum_release_distance_milli_tiles
            .saturating_sub(minimum_start_distance_milli_tiles),
        minimum_release_distance_milli_tiles,
        projectile_arrival_milliseconds: projectile_arrival_ms(
            minimum_release_distance_milli_tiles,
            projectile_speed_milli_tiles_per_second,
        ),
    }
}

fn validate_motion(
    pattern_id: &str,
    motion: &CoreCounterplayMotionProof,
    diagnostics: &mut Vec<CoreCounterplayDiagnostic>,
) {
    match *motion {
        CoreCounterplayMotionProof::Displacement {
            available_displacement_milli_tiles,
            required_displacement_milli_tiles,
            ..
        } if available_displacement_milli_tiles < required_displacement_milli_tiles => {
            diagnostics.push(CoreCounterplayDiagnostic::DisplacementInsufficient {
                pattern_id: pattern_id.to_owned(),
                available_milli_tiles: available_displacement_milli_tiles,
                required_milli_tiles: required_displacement_milli_tiles,
            });
        }
        CoreCounterplayMotionProof::AngularTracking {
            available_setup_displacement_milli_tiles,
            required_setup_displacement_milli_tiles,
            available_tracking_speed_milli_tiles_per_second,
            required_tracking_speed_milli_tiles_per_second,
            ..
        } => {
            if available_setup_displacement_milli_tiles < required_setup_displacement_milli_tiles {
                diagnostics.push(CoreCounterplayDiagnostic::DisplacementInsufficient {
                    pattern_id: pattern_id.to_owned(),
                    available_milli_tiles: available_setup_displacement_milli_tiles,
                    required_milli_tiles: required_setup_displacement_milli_tiles,
                });
            }
            if available_tracking_speed_milli_tiles_per_second
                < required_tracking_speed_milli_tiles_per_second
            {
                diagnostics.push(CoreCounterplayDiagnostic::TrackingSpeedInsufficient {
                    pattern_id: pattern_id.to_owned(),
                    available_milli_tiles_per_second:
                        available_tracking_speed_milli_tiles_per_second,
                    required_milli_tiles_per_second: required_tracking_speed_milli_tiles_per_second,
                });
            }
        }
        CoreCounterplayMotionProof::Displacement { .. } => {}
    }
}

fn validate_projectile(
    pattern_id: &str,
    projectile: &CoreProjectileFairnessProof,
    diagnostics: &mut Vec<CoreCounterplayDiagnostic>,
) {
    if projectile.minimum_start_distance_milli_tiles < CORE_COM006_CLOSE_SPAWN_DISTANCE_MILLI_TILES
        && projectile.ground_origin_warning_milliseconds
            < CORE_COM006_CLOSE_SPAWN_GROUND_WARNING_MILLISECONDS
    {
        diagnostics.push(
            CoreCounterplayDiagnostic::CloseSpawnGroundWarningInsufficient {
                pattern_id: pattern_id.to_owned(),
                actual_milliseconds: projectile.ground_origin_warning_milliseconds,
            },
        );
    }
    if projectile.available_origin_escape_milli_tiles
        < projectile.required_origin_escape_milli_tiles
    {
        diagnostics.push(CoreCounterplayDiagnostic::DisplacementInsufficient {
            pattern_id: pattern_id.to_owned(),
            available_milli_tiles: projectile.available_origin_escape_milli_tiles,
            required_milli_tiles: projectile.required_origin_escape_milli_tiles,
        });
    }
    if projectile.projectile_arrival_milliseconds
        < CORE_COM006_MINIMUM_PROJECTILE_ARRIVAL_MILLISECONDS
    {
        diagnostics.push(CoreCounterplayDiagnostic::ProjectileArrivalTooFast {
            pattern_id: pattern_id.to_owned(),
            actual_milliseconds: projectile.projectile_arrival_milliseconds,
        });
    }
}

fn displacement_at_baseline(milliseconds: u32) -> u32 {
    u32::try_from(
        u64::from(CORE_COM006_PLAYER_SPEED_MILLI_TILES_PER_SECOND)
            .saturating_mul(u64::from(milliseconds))
            / 1_000,
    )
    .unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_fixture_reports_all_eight_missing_patterns_in_stable_order() {
        let diagnostics = solve_core_authored_min_speed_paths(&[]).expect_err("missing fixture");
        assert_eq!(diagnostics.len(), 8);
        assert!(diagnostics.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(diagnostics.iter().all(|diagnostic| matches!(
            diagnostic,
            CoreCounterplayDiagnostic::MissingPattern { .. }
        )));
    }

    #[test]
    fn close_spawn_and_arrival_boundaries_fail_closed() {
        let pattern_id = "fixture.close_spawn";
        let proof = projectile_proof(800, 749, 1_570, 4_500);
        let mut diagnostics = Vec::new();
        validate_projectile(pattern_id, &proof, &mut diagnostics);

        assert!(diagnostics.contains(
            &CoreCounterplayDiagnostic::CloseSpawnGroundWarningInsufficient {
                pattern_id: pattern_id.to_owned(),
                actual_milliseconds: 749,
            }
        ));
        assert!(
            diagnostics.contains(&CoreCounterplayDiagnostic::ProjectileArrivalTooFast {
                pattern_id: pattern_id.to_owned(),
                actual_milliseconds: 349,
            })
        );

        let boundary = projectile_proof(800, 750, 1_575, 4_500);
        let mut boundary_diagnostics = Vec::new();
        validate_projectile(pattern_id, &boundary, &mut boundary_diagnostics);
        assert!(boundary_diagnostics.is_empty());
        assert_eq!(boundary.projectile_arrival_milliseconds, 350);
    }

    #[test]
    fn rotor_tracking_uses_conservative_integer_ceiling() {
        let proof = angular_tracking_proof(900, 2_400, 3_000, 35_000);
        assert_eq!(
            proof,
            CoreCounterplayMotionProof::AngularTracking {
                effective_setup_milliseconds: 780,
                available_setup_displacement_milli_tiles: 3_510,
                required_setup_displacement_milli_tiles: 2_400,
                tracking_radius_milli_tiles: 3_000,
                available_tracking_speed_milli_tiles_per_second: 4_500,
                required_tracking_speed_milli_tiles_per_second: 1_833,
            }
        );
    }
}
