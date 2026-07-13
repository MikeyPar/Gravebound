//! Transactional `PostgreSQL` adapter for server-owned Core XP awards.

use persistence::{
    PersistenceError, PostgresPersistence, ProgressionAwardTransaction,
    ProgressionAwardTransactionState, StoredBossFirstClear, StoredBossFirstClearState,
    StoredEncounterLifeState, StoredEncounterRecallState, StoredEncounterTrustState,
    StoredEncounterXpEvidence, StoredOrdinaryXpEvidence, StoredProgression,
    StoredProgressionContract, StoredXpAwardResult, StoredXpEligibilityEvidence,
};
use sim_core::{CoreProgressionState, RewardLifeState, RewardRecallState, RewardTrustState};

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreProgressionRules, ProgressionAwardCode,
    ProgressionAwardCommand, ProgressionAwardContext, ProgressionAwardEvidence,
    ProgressionAwardOutcome, ProgressionAwardPlan,
};

#[derive(Debug, Clone)]
pub struct PostgresProgressionAwardService {
    persistence: PostgresPersistence,
    rules: CoreProgressionRules,
    contract: StoredProgressionContract,
}

impl PostgresProgressionAwardService {
    pub fn new(
        persistence: PostgresPersistence,
        content: &sim_content::CoreDevelopmentProgression,
    ) -> Result<Self, crate::ProgressionAwardError> {
        let rules = CoreProgressionRules::from_content(content)?;
        let cumulative_xp = rules
            .curve()
            .cumulative_xp
            .map(i32::try_from)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| crate::ProgressionAwardError::InvalidContent)?
            .try_into()
            .map_err(|_| crate::ProgressionAwardError::InvalidContent)?;
        Ok(Self {
            persistence,
            rules,
            contract: StoredProgressionContract { cumulative_xp },
        })
    }

    pub async fn award(
        &self,
        authenticated: AuthenticatedAccount,
        command: &ProgressionAwardCommand,
    ) -> ProgressionAwardOutcome {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return terminal_outcome(command, ProgressionAwardCode::ServiceUnavailable);
        }
        let boss_id = self
            .rules
            .first_clear_boss_id_for_source(&command.payload.source_content_id);
        let account_id = authenticated.account_id.as_bytes();
        let transaction = self
            .persistence
            .transact_progression_award(
                account_id,
                command.payload.character_id,
                command.reward_event_id,
                boss_id,
                &self.contract,
                |state| self.plan_and_stage(state, command, account_id),
            )
            .await;
        match transaction {
            Ok(ProgressionAwardTransaction::Committed(outcome)) => outcome,
            Ok(ProgressionAwardTransaction::Replayed(stored)) => self
                .rules
                .replay(
                    command,
                    stored.character_id,
                    stored.payload_hash,
                    &stored.result_payload,
                )
                .unwrap_or_else(|_| {
                    terminal_outcome(command, ProgressionAwardCode::ServiceUnavailable)
                }),
            Err(PersistenceError::ProgressionCharacterNotFound) => {
                terminal_outcome(command, ProgressionAwardCode::CharacterNotFound)
            }
            Err(_) => terminal_outcome(command, ProgressionAwardCode::ServiceUnavailable),
        }
    }

    fn plan_and_stage(
        &self,
        state: &mut ProgressionAwardTransactionState,
        command: &ProgressionAwardCommand,
        account_id: [u8; 16],
    ) -> Result<ProgressionAwardOutcome, PersistenceError> {
        let before = from_stored_progression(&state.progression)?;
        let context = ProgressionAwardContext {
            selected_character_id: state.selected_character_id,
            life_state: state.character.life_state,
            security_state: state.character.security_state,
            progression: before,
            current_health: u32::try_from(state.progression.current_health)
                .map_err(|_| PersistenceError::CorruptStoredProgression)?,
            first_clear_available: matches!(
                state.boss_first_clear,
                StoredBossFirstClearState::Vacant { .. }
            ),
        };
        let plan = self
            .rules
            .plan_fresh(command, context)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?;
        stage_plan(state, command, account_id, before, &plan)?;
        Ok(plan.outcome)
    }
}

fn stage_plan(
    state: &mut ProgressionAwardTransactionState,
    command: &ProgressionAwardCommand,
    account_id: [u8; 16],
    before: CoreProgressionState,
    plan: &ProgressionAwardPlan,
) -> Result<(), PersistenceError> {
    state.progression.total_xp = i32::try_from(plan.after.total_xp)
        .map_err(|_| PersistenceError::CorruptStoredProgression)?;
    state.progression.level =
        i16::try_from(plan.after.level).map_err(|_| PersistenceError::CorruptStoredProgression)?;
    state.progression.progression_version = i64::try_from(plan.after.progression_version)
        .map_err(|_| PersistenceError::CorruptStoredProgression)?;
    if let Some(boss_id) = &plan.create_first_clear_for_boss_id {
        state.new_boss_first_clear = Some(StoredBossFirstClear {
            boss_id: boss_id.clone(),
            reward_event_id: command.reward_event_id,
            character_id: command.payload.character_id,
        });
    }
    let encoded = postcard::to_stdvec(&plan.outcome)
        .map_err(|_| PersistenceError::CorruptStoredProgression)?;
    let requested_xp = plan
        .outcome
        .base_xp
        .checked_add(plan.outcome.first_clear_bonus_xp)
        .ok_or(PersistenceError::CorruptStoredProgression)?;
    state.new_result = Some(StoredXpAwardResult {
        account_id,
        character_id: command.payload.character_id,
        reward_event_id: command.reward_event_id,
        payload_hash: command.payload_hash,
        source_content_id: command.payload.source_content_id.clone(),
        xp_profile_id: plan.xp_profile_id.clone(),
        progression_content_revision: command
            .payload
            .progression_content_revision
            .as_str()
            .to_owned(),
        entry_restore_point_id: state.entry_restore_point_id,
        revoked_by_restore_point_id: None,
        revocation_progression_version: None,
        evidence: stored_evidence(command.payload.evidence)?,
        eligible: plan.eligible,
        first_clear_awarded: plan.outcome.first_clear_awarded,
        base_xp: as_i32(plan.outcome.base_xp)?,
        bonus_xp: as_i32(plan.outcome.first_clear_bonus_xp)?,
        requested_xp: as_i32(requested_xp)?,
        applied_xp: as_i32(plan.outcome.applied_xp)?,
        discarded_xp: as_i32(plan.outcome.discarded_at_core_cap)?,
        pre_total_xp: as_i32(before.total_xp)?,
        post_total_xp: as_i32(plan.after.total_xp)?,
        pre_level: as_i16(before.level)?,
        post_level: as_i16(plan.after.level)?,
        pre_progression_version: as_i64(before.progression_version)?,
        post_progression_version: as_i64(plan.after.progression_version)?,
        result_code: plan.outcome.code as i16,
        result_payload: encoded,
    });
    Ok(())
}

fn stored_evidence(
    evidence: ProgressionAwardEvidence,
) -> Result<StoredXpEligibilityEvidence, PersistenceError> {
    match evidence {
        ProgressionAwardEvidence::Ordinary(evidence) => Ok(StoredXpEligibilityEvidence::Ordinary(
            StoredOrdinaryXpEvidence {
                delta_x_milli_tiles: evidence.delta_x_milli_tiles,
                delta_y_milli_tiles: evidence.delta_y_milli_tiles,
                window_ticks: i32::try_from(evidence.contribution_window_ticks)
                    .map_err(|_| PersistenceError::CorruptStoredProgression)?,
                actual_health_damage: i64::try_from(evidence.actual_health_damage_to_enemy)
                    .map_err(|_| PersistenceError::CorruptStoredProgression)?,
                effective_support: evidence.effective_support_to_qualifying_player,
                living_at_enemy_death: evidence.living_at_enemy_death,
            },
        )),
        ProgressionAwardEvidence::Encounter(evidence) => Ok(
            StoredXpEligibilityEvidence::Encounter(StoredEncounterXpEvidence {
                active_ticks: as_i64(evidence.active_ticks)?,
                present_ticks: as_i64(evidence.present_ticks)?,
                longest_inactivity_ticks: as_i64(evidence.longest_inactivity_ticks)?,
                reference_health: as_i64(evidence.encounter_contribution_reference_health)?,
                direct_damage: as_i64(evidence.direct_damage)?,
                effective_healing: as_i64(evidence.effective_healing_to_others)?,
                damage_prevented: as_i64(evidence.damage_prevented_on_others)?,
                objective_credits: i16::from(evidence.qualifying_objective_credits),
                life_state: match evidence.life_state {
                    RewardLifeState::Living => StoredEncounterLifeState::Living,
                    RewardLifeState::Dead => StoredEncounterLifeState::Dead,
                },
                recall_state: match evidence.recall_state {
                    RewardRecallState::Eligible => StoredEncounterRecallState::Present,
                    RewardRecallState::EmergencyRecallCompleted => {
                        StoredEncounterRecallState::Recalled
                    }
                },
                trust_state: match evidence.trust_state {
                    RewardTrustState::Valid => StoredEncounterTrustState::Valid,
                    RewardTrustState::InvalidSession => StoredEncounterTrustState::InvalidSession,
                    RewardTrustState::AntiCheatRejected => {
                        StoredEncounterTrustState::AntiCheatRejected
                    }
                },
            }),
        ),
    }
}

fn from_stored_progression(
    stored: &StoredProgression,
) -> Result<CoreProgressionState, PersistenceError> {
    Ok(CoreProgressionState {
        total_xp: u32::try_from(stored.total_xp)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        level: u16::try_from(stored.level)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        progression_version: u64::try_from(stored.progression_version)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
    })
}

fn terminal_outcome(
    command: &ProgressionAwardCommand,
    code: ProgressionAwardCode,
) -> ProgressionAwardOutcome {
    ProgressionAwardOutcome {
        reward_event_id: command.reward_event_id,
        code,
        projection: None,
        base_xp: 0,
        first_clear_bonus_xp: 0,
        applied_xp: 0,
        discarded_at_core_cap: 0,
        first_clear_awarded: false,
    }
}

fn as_i16(value: u16) -> Result<i16, PersistenceError> {
    i16::try_from(value).map_err(|_| PersistenceError::CorruptStoredProgression)
}

fn as_i32(value: u32) -> Result<i32, PersistenceError> {
    i32::try_from(value).map_err(|_| PersistenceError::CorruptStoredProgression)
}

fn as_i64(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::CorruptStoredProgression)
}
