//! Renderer-independent Core enemy and miniboss definitions for `GB-M03-03D`.
//!
//! Authored milliseconds remain content facts. This boundary performs the single deterministic
//! conversion to the 30 Hz simulation clock and trace-proves each persisted active-instance cap.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    Counterplay, DamageBand, DamageType, EchoMemoryFamily, HostileDisposition, TICK_RATE_HZ,
    duration_ms_to_ticks_ceil, duration_ms_to_ticks_nearest, minimum_warnings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreEnemyRole {
    Fodder,
    Pressure,
    Disruptor,
    Anchor,
    Elite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreEnemyStateStage {
    SpawnTelegraph,
    Acquire,
    MoveOrPosition,
    Telegraph,
    Attack,
    Recover,
}

pub const CORE_ENEMY_STATE_SEQUENCE: [CoreEnemyStateStage; 7] = [
    CoreEnemyStateStage::SpawnTelegraph,
    CoreEnemyStateStage::Acquire,
    CoreEnemyStateStage::MoveOrPosition,
    CoreEnemyStateStage::Telegraph,
    CoreEnemyStateStage::Attack,
    CoreEnemyStateStage::Recover,
    CoreEnemyStateStage::Acquire,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreTargetSelection {
    NearestLivingDamageableInAggroTieLowestEntityId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreTelegraphLock {
    AimAndPositionAtTelegraphStart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEnemyLocomotionDefinition {
    RushRetreat {
        approach_speed_milli_tiles_per_second: u32,
        trigger_distance_milli_tiles: u32,
        charge_distance_milli_tiles: u32,
        charge_ticks: u32,
        retreat_speed_milli_tiles_per_second: u32,
        retreat_ticks: u32,
    },
    MaintainDistance {
        movement_speed_milli_tiles_per_second: u32,
        preferred_distance_milli_tiles: u32,
    },
    OrbitAnchor {
        movement_speed_milli_tiles_per_second: u32,
        orbit_radius_milli_tiles: u32,
    },
    PursueStopChargeHome {
        movement_speed_milli_tiles_per_second: u32,
        stop_distance_milli_tiles: u32,
    },
    Stationary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEnemyLocomotionParameters {
    RushRetreat {
        approach_speed_milli_tiles_per_second: u32,
        trigger_distance_milli_tiles: u32,
        charge_distance_milli_tiles: u32,
        charge_duration_milliseconds: u32,
        retreat_speed_milli_tiles_per_second: u32,
        retreat_duration_milliseconds: u32,
    },
    MaintainDistance {
        movement_speed_milli_tiles_per_second: u32,
        preferred_distance_milli_tiles: u32,
    },
    OrbitAnchor {
        movement_speed_milli_tiles_per_second: u32,
        orbit_radius_milli_tiles: u32,
    },
    PursueStopChargeHome {
        movement_speed_milli_tiles_per_second: u32,
        stop_distance_milli_tiles: u32,
    },
    Stationary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreAttackGroupRule {
    DistinctProjectileHitGroups,
    OneContactHitPerCast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorePatternWarningParameters {
    Standalone {
        first_milliseconds: u32,
        repeated_milliseconds: u32,
    },
    ParentOnly,
    RecoveryPreview {
        duration_milliseconds: u32,
        major_audio: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorePatternWarningDefinition {
    Standalone {
        first_ticks: u32,
        repeated_ticks: u32,
    },
    ParentOnly,
    RecoveryPreview {
        duration_ticks: u32,
        major_audio: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreRadialGapRelation {
    TargetOpposite,
    TargetFacing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorePatternGeometryParameters {
    Charge {
        distance_milli_tiles: u32,
        duration_milliseconds: u32,
    },
    AlternatingFan {
        first_offsets_milli_degrees: Vec<i32>,
        second_offsets_milli_degrees: Vec<i32>,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
    RotatingArms {
        arm_count: u16,
        clockwise_milli_degrees_per_second: u32,
        emission_interval_milliseconds: u32,
        active_duration_milliseconds: u32,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
    ChargeLane {
        width_milli_tiles: u32,
        length_milli_tiles: u32,
        charge_duration_milliseconds: u32,
    },
    RadialGap {
        index_count: u16,
        omitted_adjacent_count: u16,
        relation: CoreRadialGapRelation,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
    ProjectileFan {
        shot_count: u16,
        total_arc_milli_degrees: u32,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorePatternGeometryDefinition {
    Charge {
        distance_milli_tiles: u32,
        duration_ticks: u32,
    },
    AlternatingFan {
        first_offsets_milli_degrees: Vec<i32>,
        second_offsets_milli_degrees: Vec<i32>,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
        projectile_lifetime_ticks: u32,
    },
    RotatingArms {
        arm_count: u16,
        clockwise_milli_degrees_per_second: u32,
        emission_interval_ticks: u32,
        active_ticks: u32,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
        projectile_lifetime_ticks: u32,
    },
    ChargeLane {
        width_milli_tiles: u32,
        length_milli_tiles: u32,
        charge_ticks: u32,
    },
    RadialGap {
        index_count: u16,
        omitted_adjacent_count: u16,
        relation: CoreRadialGapRelation,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
        projectile_lifetime_ticks: u32,
    },
    ProjectileFan {
        shot_count: u16,
        total_arc_milli_degrees: u32,
        projectile_speed_milli_tiles_per_second: u32,
        range_milli_tiles: u32,
        projectile_radius_milli_tiles: u32,
        projectile_lifetime_ticks: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePatternDefinitionParameters {
    pub id: String,
    pub owner_id: String,
    pub telegraph_id: String,
    pub audio_cue_id: String,
    pub major_audio_cue_id: Option<String>,
    pub damage_type: DamageType,
    pub damage_band: DamageBand,
    pub raw_damage: u32,
    pub threat_cost: u16,
    pub warning: CorePatternWarningParameters,
    pub cycle_milliseconds: u32,
    pub quiet_milliseconds: u32,
    pub geometry: CorePatternGeometryParameters,
    pub counterplay: Counterplay,
    pub memory_family: EchoMemoryFamily,
    pub disposition: HostileDisposition,
    pub attack_group_rule: CoreAttackGroupRule,
    pub acceleration_milli_tiles_per_second_squared: u32,
    pub pierces_players: bool,
    pub status_count: usize,
    pub cancel_on_phase_change: bool,
    pub persisted_maximum_active_instances: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePatternDefinition {
    parameters: CorePatternDefinitionParameters,
    warning: CorePatternWarningDefinition,
    cycle_ticks: u32,
    quiet_ticks: u32,
    geometry: CorePatternGeometryDefinition,
    traced_maximum_active_instances: u16,
}

impl CorePatternDefinition {
    pub fn new(
        parameters: CorePatternDefinitionParameters,
    ) -> Result<Self, CoreEnemyDefinitionError> {
        validate_pattern_metadata(&parameters)?;
        let cycle_ticks = nearest_ticks(parameters.cycle_milliseconds)?;
        let quiet_ticks = nearest_ticks(parameters.quiet_milliseconds)?;
        if cycle_ticks == 0 || quiet_ticks > cycle_ticks {
            return Err(CoreEnemyDefinitionError::InvalidCycle);
        }
        let warning = compile_warning(&parameters.warning, parameters.damage_band)?;
        let geometry = compile_geometry(&parameters.geometry)?;
        validate_pattern_grammar(&parameters, &geometry)?;
        let traced_maximum_active_instances =
            trace_maximum_active_instances(&geometry, cycle_ticks)?;
        if traced_maximum_active_instances != parameters.persisted_maximum_active_instances {
            return Err(CoreEnemyDefinitionError::MaximumActiveInstancesDrift {
                persisted: parameters.persisted_maximum_active_instances,
                traced: traced_maximum_active_instances,
            });
        }
        Ok(Self {
            parameters,
            warning,
            cycle_ticks,
            quiet_ticks,
            geometry,
            traced_maximum_active_instances,
        })
    }

    #[must_use]
    pub const fn parameters(&self) -> &CorePatternDefinitionParameters {
        &self.parameters
    }

    #[must_use]
    pub const fn warning(&self) -> &CorePatternWarningDefinition {
        &self.warning
    }

    #[must_use]
    pub const fn cycle_ticks(&self) -> u32 {
        self.cycle_ticks
    }

    #[must_use]
    pub const fn quiet_ticks(&self) -> u32 {
        self.quiet_ticks
    }

    #[must_use]
    pub const fn geometry(&self) -> &CorePatternGeometryDefinition {
        &self.geometry
    }

    #[must_use]
    pub const fn traced_maximum_active_instances(&self) -> u16 {
        self.traced_maximum_active_instances
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreEnemyDefinitionParameters {
    pub content_id: String,
    pub role: CoreEnemyRole,
    pub state_sequence: [CoreEnemyStateStage; 7],
    pub target_selection: CoreTargetSelection,
    pub telegraph_lock: CoreTelegraphLock,
    pub maximum_health: u32,
    pub armor: u16,
    pub collision_radius_milli_tiles: u32,
    pub hurtbox_radius_milli_tiles: u32,
    pub aggro_radius_milli_tiles: u32,
    pub leash_radius_milli_tiles: u32,
    pub target_reacquire_milliseconds: u32,
    pub no_target_reset_milliseconds: u32,
    pub spawn_warning_milliseconds: u32,
    pub spawn_invulnerability_milliseconds: u32,
    pub introduction_milliseconds: u32,
    pub contact_damage: u32,
    pub drop_reward_on_reset: bool,
    pub locomotion: CoreEnemyLocomotionParameters,
    pub patterns: Vec<CorePatternDefinition>,
    pub reward_profile_id: String,
    pub xp_profile_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreEnemyDefinition {
    parameters: CoreEnemyDefinitionParameters,
    locomotion: CoreEnemyLocomotionDefinition,
    target_reacquire_ticks: u32,
    no_target_reset_ticks: u32,
    spawn_warning_ticks: u32,
    spawn_invulnerability_ticks: u32,
    introduction_ticks: u32,
}

impl CoreEnemyDefinition {
    pub fn new(
        parameters: CoreEnemyDefinitionParameters,
    ) -> Result<Self, CoreEnemyDefinitionError> {
        let pattern_ids = parameters
            .patterns
            .iter()
            .map(|pattern| pattern.parameters().id.as_str())
            .collect::<BTreeSet<_>>();
        if !valid_content_id(&parameters.content_id)
            || parameters.state_sequence != CORE_ENEMY_STATE_SEQUENCE
            || parameters.maximum_health == 0
            || parameters.collision_radius_milli_tiles == 0
            || parameters.hurtbox_radius_milli_tiles == 0
            || parameters.hurtbox_radius_milli_tiles > parameters.collision_radius_milli_tiles
            || parameters.aggro_radius_milli_tiles == 0
            || parameters.leash_radius_milli_tiles < parameters.aggro_radius_milli_tiles
            || parameters.contact_damage != 0
            || parameters.drop_reward_on_reset
            || parameters.patterns.is_empty()
            || pattern_ids.len() != parameters.patterns.len()
            || parameters
                .patterns
                .iter()
                .any(|pattern| pattern.parameters().owner_id != parameters.content_id)
            || !valid_content_id(&parameters.reward_profile_id)
            || !valid_content_id(&parameters.xp_profile_id)
        {
            return Err(CoreEnemyDefinitionError::InvalidEnemy);
        }
        let locomotion = compile_locomotion(&parameters.locomotion)?;
        let target_reacquire_ticks = nearest_ticks(parameters.target_reacquire_milliseconds)?;
        let no_target_reset_ticks = nearest_ticks(parameters.no_target_reset_milliseconds)?;
        let spawn_warning_ticks = ceiling_ticks(parameters.spawn_warning_milliseconds)?;
        let spawn_invulnerability_ticks =
            nearest_ticks(parameters.spawn_invulnerability_milliseconds)?;
        let introduction_ticks = nearest_ticks(parameters.introduction_milliseconds)?;
        if target_reacquire_ticks == 0
            || no_target_reset_ticks == 0
            || spawn_warning_ticks == 0
            || spawn_invulnerability_ticks == 0
        {
            return Err(CoreEnemyDefinitionError::InvalidEnemy);
        }
        Ok(Self {
            parameters,
            locomotion,
            target_reacquire_ticks,
            no_target_reset_ticks,
            spawn_warning_ticks,
            spawn_invulnerability_ticks,
            introduction_ticks,
        })
    }

    #[must_use]
    pub const fn parameters(&self) -> &CoreEnemyDefinitionParameters {
        &self.parameters
    }

    #[must_use]
    pub const fn locomotion(&self) -> &CoreEnemyLocomotionDefinition {
        &self.locomotion
    }

    #[must_use]
    pub const fn target_reacquire_ticks(&self) -> u32 {
        self.target_reacquire_ticks
    }

    #[must_use]
    pub const fn no_target_reset_ticks(&self) -> u32 {
        self.no_target_reset_ticks
    }

    #[must_use]
    pub const fn spawn_warning_ticks(&self) -> u32 {
        self.spawn_warning_ticks
    }

    #[must_use]
    pub const fn spawn_invulnerability_ticks(&self) -> u32 {
        self.spawn_invulnerability_ticks
    }

    #[must_use]
    pub const fn introduction_ticks(&self) -> u32 {
        self.introduction_ticks
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreEnemyDefinitionError {
    #[error("Core duration exceeds the supported 30 Hz tick range")]
    DurationOverflow,
    #[error("Core enemy definition is incomplete or internally inconsistent")]
    InvalidEnemy,
    #[error("Core locomotion definition is invalid")]
    InvalidLocomotion,
    #[error("Core pattern metadata or derived cue IDs are invalid")]
    InvalidPatternMetadata,
    #[error("Core pattern warning violates its damage-band contract")]
    InvalidWarning,
    #[error("Core pattern cycle or quiet window is invalid")]
    InvalidCycle,
    #[error("Core pattern geometry is invalid")]
    InvalidGeometry,
    #[error("Core pattern counterplay, disposition, or hit grouping does not match its geometry")]
    InvalidPatternGrammar,
    #[error("persisted maximum active instances {persisted} differ from traced maximum {traced}")]
    MaximumActiveInstancesDrift { persisted: u16, traced: u16 },
}

fn validate_pattern_metadata(
    parameters: &CorePatternDefinitionParameters,
) -> Result<(), CoreEnemyDefinitionError> {
    let expected_telegraph = format!("{}.telegraph", parameters.id);
    let expected_audio = format!("{}.warning", parameters.id);
    let expected_major = format!("{expected_audio}.major");
    let major_is_exact = if matches!(
        parameters.damage_band,
        DamageBand::Major | DamageBand::Severe | DamageBand::Execution
    ) {
        parameters.major_audio_cue_id.as_deref() == Some(expected_major.as_str())
    } else {
        parameters.major_audio_cue_id.is_none()
    };
    if !valid_content_id(&parameters.id)
        || !valid_content_id(&parameters.owner_id)
        || parameters.telegraph_id != expected_telegraph
        || parameters.audio_cue_id != expected_audio
        || !major_is_exact
        || parameters.raw_damage == 0
        || parameters.threat_cost == 0
        || parameters.acceleration_milli_tiles_per_second_squared != 0
        || parameters.pierces_players
        || parameters.status_count != 0
        || !parameters.cancel_on_phase_change
        || parameters.persisted_maximum_active_instances == 0
    {
        return Err(CoreEnemyDefinitionError::InvalidPatternMetadata);
    }
    Ok(())
}

fn compile_warning(
    warning: &CorePatternWarningParameters,
    band: DamageBand,
) -> Result<CorePatternWarningDefinition, CoreEnemyDefinitionError> {
    let (minimum_first, minimum_repeated) = minimum_warnings(band);
    match *warning {
        CorePatternWarningParameters::Standalone {
            first_milliseconds,
            repeated_milliseconds,
        } if first_milliseconds >= minimum_first
            && repeated_milliseconds >= minimum_repeated
            && repeated_milliseconds <= first_milliseconds =>
        {
            Ok(CorePatternWarningDefinition::Standalone {
                first_ticks: ceiling_ticks(first_milliseconds)?,
                repeated_ticks: ceiling_ticks(repeated_milliseconds)?,
            })
        }
        CorePatternWarningParameters::ParentOnly => Ok(CorePatternWarningDefinition::ParentOnly),
        CorePatternWarningParameters::RecoveryPreview {
            duration_milliseconds,
            major_audio: true,
        } if band == DamageBand::Major && duration_milliseconds >= minimum_first => {
            Ok(CorePatternWarningDefinition::RecoveryPreview {
                duration_ticks: ceiling_ticks(duration_milliseconds)?,
                major_audio: true,
            })
        }
        _ => Err(CoreEnemyDefinitionError::InvalidWarning),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "all six closed geometry variants remain co-located for field-for-field review"
)]
fn compile_geometry(
    geometry: &CorePatternGeometryParameters,
) -> Result<CorePatternGeometryDefinition, CoreEnemyDefinitionError> {
    let compiled = match geometry {
        CorePatternGeometryParameters::Charge {
            distance_milli_tiles,
            duration_milliseconds,
        } if *distance_milli_tiles > 0 && *duration_milliseconds > 0 => {
            CorePatternGeometryDefinition::Charge {
                distance_milli_tiles: *distance_milli_tiles,
                duration_ticks: nearest_ticks(*duration_milliseconds)?,
            }
        }
        CorePatternGeometryParameters::AlternatingFan {
            first_offsets_milli_degrees,
            second_offsets_milli_degrees,
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
        } if valid_offsets(first_offsets_milli_degrees)
            && first_offsets_milli_degrees.len() == second_offsets_milli_degrees.len()
            && valid_offsets(second_offsets_milli_degrees) =>
        {
            CorePatternGeometryDefinition::AlternatingFan {
                first_offsets_milli_degrees: first_offsets_milli_degrees.clone(),
                second_offsets_milli_degrees: second_offsets_milli_degrees.clone(),
                projectile_speed_milli_tiles_per_second: *projectile_speed_milli_tiles_per_second,
                range_milli_tiles: *range_milli_tiles,
                projectile_radius_milli_tiles: *projectile_radius_milli_tiles,
                projectile_lifetime_ticks: projectile_lifetime_ticks(
                    *range_milli_tiles,
                    *projectile_speed_milli_tiles_per_second,
                )?,
            }
        }
        CorePatternGeometryParameters::RotatingArms {
            arm_count,
            clockwise_milli_degrees_per_second,
            emission_interval_milliseconds,
            active_duration_milliseconds,
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
        } if *arm_count > 0 && *clockwise_milli_degrees_per_second > 0 => {
            CorePatternGeometryDefinition::RotatingArms {
                arm_count: *arm_count,
                clockwise_milli_degrees_per_second: *clockwise_milli_degrees_per_second,
                emission_interval_ticks: nearest_ticks(*emission_interval_milliseconds)?,
                active_ticks: nearest_ticks(*active_duration_milliseconds)?,
                projectile_speed_milli_tiles_per_second: *projectile_speed_milli_tiles_per_second,
                range_milli_tiles: *range_milli_tiles,
                projectile_radius_milli_tiles: *projectile_radius_milli_tiles,
                projectile_lifetime_ticks: projectile_lifetime_ticks(
                    *range_milli_tiles,
                    *projectile_speed_milli_tiles_per_second,
                )?,
            }
        }
        CorePatternGeometryParameters::ChargeLane {
            width_milli_tiles,
            length_milli_tiles,
            charge_duration_milliseconds,
        } if *width_milli_tiles > 0 && *length_milli_tiles > 0 => {
            CorePatternGeometryDefinition::ChargeLane {
                width_milli_tiles: *width_milli_tiles,
                length_milli_tiles: *length_milli_tiles,
                charge_ticks: nearest_ticks(*charge_duration_milliseconds)?,
            }
        }
        CorePatternGeometryParameters::RadialGap {
            index_count,
            omitted_adjacent_count,
            relation,
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
        } if *index_count >= 3
            && *omitted_adjacent_count > 0
            && *omitted_adjacent_count < *index_count =>
        {
            CorePatternGeometryDefinition::RadialGap {
                index_count: *index_count,
                omitted_adjacent_count: *omitted_adjacent_count,
                relation: *relation,
                projectile_speed_milli_tiles_per_second: *projectile_speed_milli_tiles_per_second,
                range_milli_tiles: *range_milli_tiles,
                projectile_radius_milli_tiles: *projectile_radius_milli_tiles,
                projectile_lifetime_ticks: projectile_lifetime_ticks(
                    *range_milli_tiles,
                    *projectile_speed_milli_tiles_per_second,
                )?,
            }
        }
        CorePatternGeometryParameters::ProjectileFan {
            shot_count,
            total_arc_milli_degrees,
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
        } if *shot_count > 0 && *total_arc_milli_degrees > 0 => {
            CorePatternGeometryDefinition::ProjectileFan {
                shot_count: *shot_count,
                total_arc_milli_degrees: *total_arc_milli_degrees,
                projectile_speed_milli_tiles_per_second: *projectile_speed_milli_tiles_per_second,
                range_milli_tiles: *range_milli_tiles,
                projectile_radius_milli_tiles: *projectile_radius_milli_tiles,
                projectile_lifetime_ticks: projectile_lifetime_ticks(
                    *range_milli_tiles,
                    *projectile_speed_milli_tiles_per_second,
                )?,
            }
        }
        _ => return Err(CoreEnemyDefinitionError::InvalidGeometry),
    };
    if geometry_has_zero_ticks_or_projectile_data(&compiled) {
        Err(CoreEnemyDefinitionError::InvalidGeometry)
    } else {
        Ok(compiled)
    }
}

fn validate_pattern_grammar(
    parameters: &CorePatternDefinitionParameters,
    geometry: &CorePatternGeometryDefinition,
) -> Result<(), CoreEnemyDefinitionError> {
    let exact = match geometry {
        CorePatternGeometryDefinition::Charge { .. }
        | CorePatternGeometryDefinition::ChargeLane { .. } => {
            parameters.counterplay == Counterplay::LeaveTelegraph
                && parameters.disposition == HostileDisposition::OneContactHitPerCast
                && parameters.attack_group_rule == CoreAttackGroupRule::OneContactHitPerCast
        }
        CorePatternGeometryDefinition::AlternatingFan { .. }
        | CorePatternGeometryDefinition::ProjectileFan { .. } => {
            parameters.counterplay == Counterplay::Strafe
                && parameters.disposition == HostileDisposition::ConsumeOnPlayerOrSolid
                && parameters.attack_group_rule == CoreAttackGroupRule::DistinctProjectileHitGroups
        }
        CorePatternGeometryDefinition::RotatingArms { .. } => {
            parameters.counterplay == Counterplay::MoveWithRotation
                && parameters.disposition == HostileDisposition::ConsumeOnPlayerOrSolid
                && parameters.attack_group_rule == CoreAttackGroupRule::DistinctProjectileHitGroups
        }
        CorePatternGeometryDefinition::RadialGap { .. } => {
            parameters.counterplay == Counterplay::FollowGap
                && parameters.disposition == HostileDisposition::ConsumeOnPlayerOrSolid
                && parameters.attack_group_rule == CoreAttackGroupRule::DistinctProjectileHitGroups
        }
    };
    if exact {
        Ok(())
    } else {
        Err(CoreEnemyDefinitionError::InvalidPatternGrammar)
    }
}

fn trace_maximum_active_instances(
    geometry: &CorePatternGeometryDefinition,
    cycle_ticks: u32,
) -> Result<u16, CoreEnemyDefinitionError> {
    let active = match geometry {
        CorePatternGeometryDefinition::Charge { .. }
        | CorePatternGeometryDefinition::ChargeLane { .. } => 1,
        CorePatternGeometryDefinition::AlternatingFan {
            first_offsets_milli_degrees,
            projectile_lifetime_ticks,
            ..
        } => casts_active(*projectile_lifetime_ticks, cycle_ticks)
            .saturating_mul(first_offsets_milli_degrees.len() as u64),
        CorePatternGeometryDefinition::ProjectileFan {
            shot_count,
            projectile_lifetime_ticks,
            ..
        } => casts_active(*projectile_lifetime_ticks, cycle_ticks)
            .saturating_mul(u64::from(*shot_count)),
        CorePatternGeometryDefinition::RadialGap {
            index_count,
            omitted_adjacent_count,
            projectile_lifetime_ticks,
            ..
        } => casts_active(*projectile_lifetime_ticks, cycle_ticks).saturating_mul(u64::from(
            index_count.saturating_sub(*omitted_adjacent_count),
        )),
        CorePatternGeometryDefinition::RotatingArms {
            arm_count,
            emission_interval_ticks,
            active_ticks,
            projectile_lifetime_ticks,
            ..
        } => {
            let emissions_per_arm = u64::from(active_ticks / emission_interval_ticks);
            let concurrently_alive = ceil_div(
                u64::from(*projectile_lifetime_ticks),
                u64::from(*emission_interval_ticks),
            );
            u64::from(*arm_count).saturating_mul(emissions_per_arm.min(concurrently_alive))
        }
    };
    u16::try_from(active).map_err(|_| CoreEnemyDefinitionError::InvalidGeometry)
}

fn compile_locomotion(
    locomotion: &CoreEnemyLocomotionParameters,
) -> Result<CoreEnemyLocomotionDefinition, CoreEnemyDefinitionError> {
    let compiled = match *locomotion {
        CoreEnemyLocomotionParameters::RushRetreat {
            approach_speed_milli_tiles_per_second,
            trigger_distance_milli_tiles,
            charge_distance_milli_tiles,
            charge_duration_milliseconds,
            retreat_speed_milli_tiles_per_second,
            retreat_duration_milliseconds,
        } if approach_speed_milli_tiles_per_second > 0
            && trigger_distance_milli_tiles > 0
            && charge_distance_milli_tiles > 0
            && retreat_speed_milli_tiles_per_second > 0 =>
        {
            CoreEnemyLocomotionDefinition::RushRetreat {
                approach_speed_milli_tiles_per_second,
                trigger_distance_milli_tiles,
                charge_distance_milli_tiles,
                charge_ticks: nearest_ticks(charge_duration_milliseconds)?,
                retreat_speed_milli_tiles_per_second,
                retreat_ticks: nearest_ticks(retreat_duration_milliseconds)?,
            }
        }
        CoreEnemyLocomotionParameters::MaintainDistance {
            movement_speed_milli_tiles_per_second,
            preferred_distance_milli_tiles,
        } if movement_speed_milli_tiles_per_second > 0 && preferred_distance_milli_tiles > 0 => {
            CoreEnemyLocomotionDefinition::MaintainDistance {
                movement_speed_milli_tiles_per_second,
                preferred_distance_milli_tiles,
            }
        }
        CoreEnemyLocomotionParameters::OrbitAnchor {
            movement_speed_milli_tiles_per_second,
            orbit_radius_milli_tiles,
        } if movement_speed_milli_tiles_per_second > 0 && orbit_radius_milli_tiles > 0 => {
            CoreEnemyLocomotionDefinition::OrbitAnchor {
                movement_speed_milli_tiles_per_second,
                orbit_radius_milli_tiles,
            }
        }
        CoreEnemyLocomotionParameters::PursueStopChargeHome {
            movement_speed_milli_tiles_per_second,
            stop_distance_milli_tiles,
        } if movement_speed_milli_tiles_per_second > 0 && stop_distance_milli_tiles > 0 => {
            CoreEnemyLocomotionDefinition::PursueStopChargeHome {
                movement_speed_milli_tiles_per_second,
                stop_distance_milli_tiles,
            }
        }
        CoreEnemyLocomotionParameters::Stationary => CoreEnemyLocomotionDefinition::Stationary,
        _ => return Err(CoreEnemyDefinitionError::InvalidLocomotion),
    };
    Ok(compiled)
}

fn geometry_has_zero_ticks_or_projectile_data(geometry: &CorePatternGeometryDefinition) -> bool {
    match geometry {
        CorePatternGeometryDefinition::Charge { duration_ticks, .. } => *duration_ticks == 0,
        CorePatternGeometryDefinition::ChargeLane { charge_ticks, .. } => *charge_ticks == 0,
        CorePatternGeometryDefinition::AlternatingFan {
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
            projectile_lifetime_ticks,
            ..
        }
        | CorePatternGeometryDefinition::RotatingArms {
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
            projectile_lifetime_ticks,
            ..
        }
        | CorePatternGeometryDefinition::RadialGap {
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
            projectile_lifetime_ticks,
            ..
        }
        | CorePatternGeometryDefinition::ProjectileFan {
            projectile_speed_milli_tiles_per_second,
            range_milli_tiles,
            projectile_radius_milli_tiles,
            projectile_lifetime_ticks,
            ..
        } => {
            *projectile_speed_milli_tiles_per_second == 0
                || *range_milli_tiles == 0
                || *projectile_radius_milli_tiles == 0
                || *projectile_lifetime_ticks == 0
                || matches!(
                    geometry,
                    CorePatternGeometryDefinition::RotatingArms {
                        emission_interval_ticks: 0,
                        ..
                    } | CorePatternGeometryDefinition::RotatingArms {
                        active_ticks: 0,
                        ..
                    }
                )
        }
    }
}

fn valid_offsets(offsets: &[i32]) -> bool {
    !offsets.is_empty() && offsets.windows(2).all(|pair| pair[0] < pair[1])
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

fn projectile_lifetime_ticks(
    range_milli_tiles: u32,
    speed_milli_tiles_per_second: u32,
) -> Result<u32, CoreEnemyDefinitionError> {
    if range_milli_tiles == 0 || speed_milli_tiles_per_second == 0 {
        return Err(CoreEnemyDefinitionError::InvalidGeometry);
    }
    let numerator = u64::from(range_milli_tiles).saturating_mul(u64::from(TICK_RATE_HZ));
    u32::try_from(ceil_div(numerator, u64::from(speed_milli_tiles_per_second)))
        .map_err(|_| CoreEnemyDefinitionError::DurationOverflow)
}

fn casts_active(lifetime_ticks: u32, cycle_ticks: u32) -> u64 {
    ceil_div(u64::from(lifetime_ticks), u64::from(cycle_ticks))
}

const fn ceil_div(numerator: u64, denominator: u64) -> u64 {
    if numerator == 0 {
        0
    } else {
        1 + (numerator - 1) / denominator
    }
}

fn nearest_ticks(milliseconds: u32) -> Result<u32, CoreEnemyDefinitionError> {
    u32::try_from(duration_ms_to_ticks_nearest(u64::from(milliseconds)))
        .map_err(|_| CoreEnemyDefinitionError::DurationOverflow)
}

fn ceiling_ticks(milliseconds: u32) -> Result<u32, CoreEnemyDefinitionError> {
    u32::try_from(duration_ms_to_ticks_ceil(u64::from(milliseconds)))
        .map_err(|_| CoreEnemyDefinitionError::DurationOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choir_skull_rotor() -> CorePatternDefinitionParameters {
        CorePatternDefinitionParameters {
            id: "pattern.enemy.choir_skull.rotor".to_owned(),
            owner_id: "enemy.choir_skull".to_owned(),
            telegraph_id: "pattern.enemy.choir_skull.rotor.telegraph".to_owned(),
            audio_cue_id: "pattern.enemy.choir_skull.rotor.warning".to_owned(),
            major_audio_cue_id: None,
            damage_type: DamageType::Veil,
            damage_band: DamageBand::Pressure,
            raw_damage: 14,
            threat_cost: 10,
            warning: CorePatternWarningParameters::Standalone {
                first_milliseconds: 650,
                repeated_milliseconds: 500,
            },
            cycle_milliseconds: 6_000,
            quiet_milliseconds: 2_000,
            geometry: CorePatternGeometryParameters::RotatingArms {
                arm_count: 2,
                clockwise_milli_degrees_per_second: 35_000,
                emission_interval_milliseconds: 400,
                active_duration_milliseconds: 4_000,
                projectile_speed_milli_tiles_per_second: 4_500,
                range_milli_tiles: 7_000,
                projectile_radius_milli_tiles: 120,
            },
            counterplay: Counterplay::MoveWithRotation,
            memory_family: EchoMemoryFamily::RotatingProjectile,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            attack_group_rule: CoreAttackGroupRule::DistinctProjectileHitGroups,
            acceleration_milli_tiles_per_second_squared: 0,
            pierces_players: false,
            status_count: 0,
            cancel_on_phase_change: true,
            persisted_maximum_active_instances: 8,
        }
    }

    #[test]
    fn rotor_compiles_exact_ticks_and_trace_maximum() {
        let pattern = CorePatternDefinition::new(choir_skull_rotor()).expect("exact rotor");
        assert_eq!(pattern.cycle_ticks(), 180);
        assert_eq!(pattern.quiet_ticks(), 60);
        assert_eq!(pattern.traced_maximum_active_instances(), 8);
        assert!(matches!(
            pattern.geometry(),
            CorePatternGeometryDefinition::RotatingArms {
                emission_interval_ticks: 12,
                active_ticks: 120,
                projectile_lifetime_ticks: 47,
                ..
            }
        ));
    }

    #[test]
    fn trace_maximum_and_rotor_counterplay_fail_closed() {
        let mut drifted = choir_skull_rotor();
        drifted.persisted_maximum_active_instances = 7;
        assert_eq!(
            CorePatternDefinition::new(drifted),
            Err(CoreEnemyDefinitionError::MaximumActiveInstancesDrift {
                persisted: 7,
                traced: 8,
            })
        );

        let mut wrong_counterplay = choir_skull_rotor();
        wrong_counterplay.counterplay = Counterplay::Strafe;
        assert_eq!(
            CorePatternDefinition::new(wrong_counterplay),
            Err(CoreEnemyDefinitionError::InvalidPatternGrammar)
        );
    }
}
