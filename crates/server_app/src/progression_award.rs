//! Server-owned XP award planning for `GB-M03-04A`.
//!
//! Reward producers supply immutable evidence and a reward-event identity. This module resolves
//! only validated progression content and produces a transaction plan; it exposes no client XP
//! mutation and does not activate any combat route.

use std::collections::BTreeMap;

use content_schema::{CoreXpEligibilityKind, CoreXpSourceKind};
use protocol::{ManifestHash, ProgressionCapState, ProgressionProjection};
use serde::{Deserialize, Serialize};
use sim_core::{
    CoreProgressionState, EncounterXpEvidence, GraveArbalistProgressionDefinition, LevelCurve,
    NormalXpEvidence, apply_core_xp, evaluate_encounter_xp_eligibility,
    evaluate_normal_xp_eligibility, first_clear_bonus, grave_arbalist_level_stats,
};
use thiserror::Error;

const ID_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressionAwardEvidence {
    Ordinary(NormalXpEvidence),
    Encounter(EncounterXpEvidence),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressionAwardPayload {
    pub character_id: [u8; ID_BYTES],
    pub expected_progression_version: u64,
    pub source_content_id: String,
    pub progression_content_revision: ManifestHash,
    pub evidence: ProgressionAwardEvidence,
}

impl ProgressionAwardPayload {
    #[must_use]
    pub fn canonical_hash(&self) -> [u8; 32] {
        let bytes = postcard::to_stdvec(self).expect("bounded internal XP payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressionAwardCommand {
    pub reward_event_id: [u8; ID_BYTES],
    pub payload_hash: [u8; 32],
    pub payload: ProgressionAwardPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressionAwardContext {
    pub selected_character_id: Option<[u8; ID_BYTES]>,
    pub life_state: i16,
    pub security_state: i16,
    pub progression: CoreProgressionState,
    pub current_health: u32,
    pub first_clear_available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i16)]
pub enum ProgressionAwardCode {
    Accepted = 0,
    NotEligible = 1,
    NoSelectedCharacter = 2,
    InvalidSource = 3,
    CharacterDead = 4,
    StateVersionMismatch = 5,
    ContentMismatch = 6,
    SourceDisabled = 7,
    PayloadHashMismatch = 8,
    InvalidEvidence = 9,
    IdempotencyConflict = 10,
    ServiceUnavailable = 11,
    CharacterNotFound = 12,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressionAwardOutcome {
    pub reward_event_id: [u8; ID_BYTES],
    pub code: ProgressionAwardCode,
    pub projection: Option<ProgressionProjection>,
    pub base_xp: u32,
    pub first_clear_bonus_xp: u32,
    pub applied_xp: u32,
    pub discarded_at_core_cap: u32,
    pub first_clear_awarded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressionAwardPlan {
    pub outcome: ProgressionAwardOutcome,
    pub after: CoreProgressionState,
    pub eligible: bool,
    pub xp_profile_id: Option<String>,
    pub create_first_clear_for_boss_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedXpProfile {
    id: String,
    source_kind: CoreXpSourceKind,
    eligibility_kind: CoreXpEligibilityKind,
    base_xp: u32,
    first_clear_bonus_basis_points: u16,
    enabled: bool,
}

#[derive(Debug, Clone)]
pub struct CoreProgressionRules {
    curve: LevelCurve,
    arbalist: GraveArbalistProgressionDefinition,
    records_revision: ManifestHash,
    profiles: BTreeMap<String, ResolvedXpProfile>,
    bindings: BTreeMap<String, String>,
}

impl CoreProgressionRules {
    pub fn from_content(
        content: &sim_content::CoreDevelopmentProgression,
    ) -> Result<Self, ProgressionAwardError> {
        let records_revision = ManifestHash::new(content.hashes().records_blake3.clone())
            .map_err(|_| ProgressionAwardError::InvalidContent)?;
        let profiles = content
            .xp_profiles()
            .iter()
            .map(|profile| {
                (
                    profile.header.id.as_str().to_owned(),
                    ResolvedXpProfile {
                        id: profile.header.id.as_str().to_owned(),
                        source_kind: profile.source_kind,
                        eligibility_kind: profile.eligibility_kind,
                        base_xp: profile.base_xp,
                        first_clear_bonus_basis_points: profile
                            .first_account_clear_bonus_basis_points,
                        enabled: profile.header.enabled,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let bindings = content
            .source_bindings()
            .iter()
            .filter(|binding| binding.header.enabled && binding.authored_core_enabled)
            .map(|binding| {
                (
                    binding.source_id.as_str().to_owned(),
                    binding.xp_profile_id.as_str().to_owned(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if bindings.len() != content.source_bindings().len() {
            return Err(ProgressionAwardError::InvalidContent);
        }
        Ok(Self {
            curve: content.level_curve(),
            arbalist: content.arbalist().clone(),
            records_revision,
            profiles,
            bindings,
        })
    }

    #[must_use]
    pub const fn curve(&self) -> LevelCurve {
        self.curve
    }

    #[must_use]
    pub const fn records_revision(&self) -> &ManifestHash {
        &self.records_revision
    }

    #[must_use]
    pub fn profile_id_for_source(&self, source_content_id: &str) -> Option<&str> {
        self.bindings.get(source_content_id).map(String::as_str)
    }

    #[must_use]
    pub fn first_clear_boss_id_for_source<'a>(
        &self,
        source_content_id: &'a str,
    ) -> Option<&'a str> {
        let profile = self
            .profile_id_for_source(source_content_id)
            .and_then(|profile_id| self.profiles.get(profile_id))?;
        (profile.enabled
            && matches!(
                profile.source_kind,
                CoreXpSourceKind::Boss | CoreXpSourceKind::WorldClimax
            )
            && profile.first_clear_bonus_basis_points == 5_000)
            .then_some(source_content_id)
    }

    pub fn project(
        &self,
        character_id: [u8; ID_BYTES],
        context: ProgressionAwardContext,
    ) -> Result<ProgressionProjection, ProgressionAwardError> {
        context.progression.validate(self.curve)?;
        self.projection(character_id, context)
    }

    pub fn plan_fresh(
        &self,
        command: &ProgressionAwardCommand,
        context: ProgressionAwardContext,
    ) -> Result<ProgressionAwardPlan, ProgressionAwardError> {
        context.progression.validate(self.curve)?;
        if command.reward_event_id.iter().all(|byte| *byte == 0)
            || command.payload.character_id.iter().all(|byte| *byte == 0)
            || command.payload.expected_progression_version == 0
            || command.payload.source_content_id.len() > 96
            || command.payload.source_content_id.len() < 3
        {
            return Err(ProgressionAwardError::InvalidCommand);
        }
        let projection = self.projection(command.payload.character_id, context)?;
        let reject = |code| ProgressionAwardPlan {
            outcome: ProgressionAwardOutcome {
                reward_event_id: command.reward_event_id,
                code,
                projection: Some(projection.clone()),
                base_xp: 0,
                first_clear_bonus_xp: 0,
                applied_xp: 0,
                discarded_at_core_cap: 0,
                first_clear_awarded: false,
            },
            after: context.progression,
            eligible: false,
            xp_profile_id: None,
            create_first_clear_for_boss_id: None,
        };
        if command.payload_hash != command.payload.canonical_hash() {
            return Ok(reject(ProgressionAwardCode::PayloadHashMismatch));
        }
        if command.payload.progression_content_revision != self.records_revision {
            return Ok(reject(ProgressionAwardCode::ContentMismatch));
        }
        if context.selected_character_id.is_none() {
            return Ok(reject(ProgressionAwardCode::NoSelectedCharacter));
        }
        if context.selected_character_id != Some(command.payload.character_id)
            || context.security_state < 0
        {
            return Ok(reject(ProgressionAwardCode::InvalidSource));
        }
        if context.life_state != 0 {
            return Ok(reject(ProgressionAwardCode::CharacterDead));
        }
        if context.progression.progression_version != command.payload.expected_progression_version {
            return Ok(reject(ProgressionAwardCode::StateVersionMismatch));
        }
        let Some(profile_id) = self.bindings.get(&command.payload.source_content_id) else {
            return Ok(reject(ProgressionAwardCode::SourceDisabled));
        };
        let Some(profile) = self.profiles.get(profile_id) else {
            return Err(ProgressionAwardError::InvalidContent);
        };
        if !profile.enabled {
            return Ok(reject(ProgressionAwardCode::SourceDisabled));
        }
        let eligibility = match (profile.eligibility_kind, command.payload.evidence) {
            (
                CoreXpEligibilityKind::OrdinaryEnemy,
                ProgressionAwardEvidence::Ordinary(evidence),
            ) => evaluate_normal_xp_eligibility(evidence),
            (
                CoreXpEligibilityKind::EncounterContribution,
                ProgressionAwardEvidence::Encounter(evidence),
            ) => evaluate_encounter_xp_eligibility(evidence),
            _ => return Ok(reject(ProgressionAwardCode::InvalidEvidence)),
        };
        let eligible = match eligibility {
            Ok(value) => value.eligible,
            Err(_) => return Ok(reject(ProgressionAwardCode::InvalidEvidence)),
        };
        if !eligible {
            let mut plan = reject(ProgressionAwardCode::NotEligible);
            plan.xp_profile_id = Some(profile.id.clone());
            return Ok(plan);
        }

        self.plan_eligible(command, context, profile)
    }

    fn plan_eligible(
        &self,
        command: &ProgressionAwardCommand,
        context: ProgressionAwardContext,
        profile: &ResolvedXpProfile,
    ) -> Result<ProgressionAwardPlan, ProgressionAwardError> {
        let is_first_clear_source = matches!(
            profile.source_kind,
            CoreXpSourceKind::Boss | CoreXpSourceKind::WorldClimax
        ) && profile.first_clear_bonus_basis_points == 5_000;
        let first_clear_awarded = is_first_clear_source && context.first_clear_available;
        let first_clear_bonus_xp = if first_clear_awarded {
            first_clear_bonus(profile.base_xp)
        } else {
            0
        };
        let requested_xp = profile
            .base_xp
            .checked_add(first_clear_bonus_xp)
            .ok_or(ProgressionAwardError::InvalidContent)?;
        let grant = apply_core_xp(self.curve, context.progression, requested_xp)?;
        let after_context = ProgressionAwardContext {
            progression: grant.after,
            ..context
        };
        let projection = self.projection(command.payload.character_id, after_context)?;
        Ok(ProgressionAwardPlan {
            outcome: ProgressionAwardOutcome {
                reward_event_id: command.reward_event_id,
                code: ProgressionAwardCode::Accepted,
                projection: Some(projection),
                base_xp: profile.base_xp,
                first_clear_bonus_xp,
                applied_xp: grant.applied_xp,
                discarded_at_core_cap: grant.discarded_at_core_cap,
                first_clear_awarded,
            },
            after: grant.after,
            eligible: true,
            xp_profile_id: Some(profile.id.clone()),
            create_first_clear_for_boss_id: first_clear_awarded
                .then(|| command.payload.source_content_id.clone()),
        })
    }

    pub fn replay(
        &self,
        command: &ProgressionAwardCommand,
        stored_character_id: [u8; ID_BYTES],
        stored_payload_hash: [u8; 32],
        stored_payload: &[u8],
    ) -> Result<ProgressionAwardOutcome, ProgressionAwardError> {
        if stored_character_id != command.payload.character_id
            || stored_payload_hash != command.payload_hash
        {
            return Ok(ProgressionAwardOutcome {
                reward_event_id: command.reward_event_id,
                code: ProgressionAwardCode::IdempotencyConflict,
                projection: None,
                base_xp: 0,
                first_clear_bonus_xp: 0,
                applied_xp: 0,
                discarded_at_core_cap: 0,
                first_clear_awarded: false,
            });
        }
        let outcome: ProgressionAwardOutcome = postcard::from_bytes(stored_payload)
            .map_err(|_| ProgressionAwardError::CorruptStoredResult)?;
        if outcome.reward_event_id != command.reward_event_id {
            return Err(ProgressionAwardError::CorruptStoredResult);
        }
        Ok(outcome)
    }

    fn projection(
        &self,
        character_id: [u8; ID_BYTES],
        context: ProgressionAwardContext,
    ) -> Result<ProgressionProjection, ProgressionAwardError> {
        let stats = grave_arbalist_level_stats(&self.arbalist, context.progression.level)?;
        let level_index = usize::from(context.progression.level - 1);
        let cap_state = if level_index + 1 == self.curve.cumulative_xp.len() {
            ProgressionCapState::CoreCapped {
                cap_total_xp: self.curve.xp_cap(),
            }
        } else {
            ProgressionCapState::Advancing {
                level_start_total_xp: self.curve.cumulative_xp[level_index],
                next_level_total_xp: self.curve.cumulative_xp[level_index + 1],
            }
        };
        let projection = ProgressionProjection {
            character_id,
            progression_version: context.progression.progression_version,
            level: context.progression.level,
            total_xp: context.progression.total_xp,
            current_health: context.current_health,
            maximum_health: stats.maximum_health,
            armor: stats.armor,
            movement_milli_tiles_per_second: stats.movement_milli_tiles_per_second,
            level_damage_multiplier_basis_points: stats.damage_multiplier_basis_points,
            cap_state,
        };
        projection
            .validate()
            .map_err(|_| ProgressionAwardError::InvalidProjection)?;
        Ok(projection)
    }
}

#[derive(Debug, Error)]
pub enum ProgressionAwardError {
    #[error("compiled Core progression content is invalid")]
    InvalidContent,
    #[error("progression award command is structurally invalid")]
    InvalidCommand,
    #[error("progression eligibility or reduction failed: {0}")]
    Simulation(#[from] sim_core::CoreProgressionError),
    #[error("authoritative progression projection is invalid")]
    InvalidProjection,
    #[error("stored progression award result is corrupt")]
    CorruptStoredResult,
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sim_core::{RewardLifeState, RewardRecallState, RewardTrustState};

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn rules() -> CoreProgressionRules {
        CoreProgressionRules::from_content(
            &sim_content::load_core_development_progression(&content_root()).unwrap(),
        )
        .unwrap()
    }

    fn ordinary_command(source: &str, version: u64) -> ProgressionAwardCommand {
        let payload = ProgressionAwardPayload {
            character_id: [2; 16],
            expected_progression_version: version,
            source_content_id: source.to_owned(),
            progression_content_revision: rules().records_revision().clone(),
            evidence: ProgressionAwardEvidence::Ordinary(NormalXpEvidence {
                living_at_enemy_death: true,
                delta_x_milli_tiles: 16_000,
                delta_y_milli_tiles: 0,
                contribution_window_ticks: 300,
                actual_health_damage_to_enemy: 1,
                effective_support_to_qualifying_player: false,
            }),
        };
        ProgressionAwardCommand {
            reward_event_id: [3; 16],
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    fn context(total_xp: u32, level: u16, version: u64) -> ProgressionAwardContext {
        ProgressionAwardContext {
            selected_character_id: Some([2; 16]),
            life_state: 0,
            security_state: 0,
            progression: CoreProgressionState {
                total_xp,
                level,
                progression_version: version,
            },
            current_health: 100,
            first_clear_available: false,
        }
    }

    #[test]
    fn ordinary_core_source_uses_content_profile_and_exact_cap() {
        let rules = rules();
        let plan = rules
            .plan_fresh(
                &ordinary_command("enemy.drowned_pilgrim", 1),
                context(0, 1, 1),
            )
            .unwrap();
        assert_eq!(plan.outcome.code, ProgressionAwardCode::Accepted);
        assert_eq!(plan.outcome.base_xp, 5);
        assert_eq!(plan.after.total_xp, 5);

        let plan = rules
            .plan_fresh(
                &ordinary_command("enemy.drowned_pilgrim", 9),
                context(2_699, 9, 9),
            )
            .unwrap();
        assert_eq!(
            (plan.outcome.applied_xp, plan.outcome.discarded_at_core_cap),
            (1, 4)
        );
        assert_eq!(plan.after.total_xp, 2_700);
    }

    #[test]
    fn caldus_first_clear_commits_bonus_even_at_cap() {
        let rules = rules();
        let payload = ProgressionAwardPayload {
            character_id: [2; 16],
            expected_progression_version: 4,
            source_content_id: "boss.sir_caldus".to_owned(),
            progression_content_revision: rules.records_revision().clone(),
            evidence: ProgressionAwardEvidence::Encounter(EncounterXpEvidence {
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
            }),
        };
        let command = ProgressionAwardCommand {
            reward_event_id: [4; 16],
            payload_hash: payload.canonical_hash(),
            payload,
        };
        let mut context = context(2_700, 10, 4);
        context.current_health = 156;
        context.first_clear_available = true;
        let plan = rules.plan_fresh(&command, context).unwrap();
        assert!(plan.outcome.first_clear_awarded);
        assert_eq!(plan.outcome.first_clear_bonus_xp, 225);
        assert_eq!(plan.outcome.applied_xp, 0);
        assert_eq!(plan.outcome.discarded_at_core_cap, 675);
        assert_eq!(
            plan.create_first_clear_for_boss_id.as_deref(),
            Some("boss.sir_caldus")
        );
    }

    #[test]
    fn invalid_hash_future_source_stale_and_replay_conflict_are_typed() {
        let rules = rules();
        let mut command = ordinary_command("enemy.drowned_pilgrim", 1);
        command.payload_hash = [9; 32];
        assert_eq!(
            rules
                .plan_fresh(&command, context(0, 1, 1))
                .unwrap()
                .outcome
                .code,
            ProgressionAwardCode::PayloadHashMismatch
        );

        let future = ordinary_command("enemy.root_thrall", 1);
        assert_eq!(
            rules
                .plan_fresh(&future, context(0, 1, 1))
                .unwrap()
                .outcome
                .code,
            ProgressionAwardCode::SourceDisabled
        );

        let stale = ordinary_command("enemy.drowned_pilgrim", 2);
        assert_eq!(
            rules
                .plan_fresh(&stale, context(0, 1, 1))
                .unwrap()
                .outcome
                .code,
            ProgressionAwardCode::StateVersionMismatch
        );

        let stored = ProgressionAwardOutcome {
            reward_event_id: [3; 16],
            code: ProgressionAwardCode::Accepted,
            projection: None,
            base_xp: 5,
            first_clear_bonus_xp: 0,
            applied_xp: 5,
            discarded_at_core_cap: 0,
            first_clear_awarded: false,
        };
        let encoded = postcard::to_stdvec(&stored).unwrap();
        assert_eq!(
            rules
                .replay(&stale, [8; 16], stale.payload_hash, &encoded)
                .unwrap()
                .code,
            ProgressionAwardCode::IdempotencyConflict
        );
    }
}
