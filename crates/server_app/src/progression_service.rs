//! Transactional `PostgreSQL` adapter for server-owned Core XP awards.

use persistence::{
    PersistenceError, PostgresPersistence, ProgressionAwardTransaction,
    ProgressionAwardTransactionState, StoredActiveDangerAuthorityV1, StoredBargainMilestoneResult,
    StoredBossFirstClear, StoredBossFirstClearState, StoredEncounterLifeState,
    StoredEncounterRecallState, StoredEncounterTrustState, StoredEncounterXpEvidence,
    StoredOrdinaryXpEvidence, StoredProgression, StoredProgressionContract, StoredXpAwardResult,
    StoredXpEligibilityEvidence,
};
use protocol::ManifestHash;
use sim_core::{
    CoreProgressionState, EncounterXpEvidence, RewardLifeState, RewardRecallState, RewardTrustState,
};

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreProgressionRules, ProgressionAwardCode,
    ProgressionAwardCommand, ProgressionAwardContext, ProgressionAwardEvidence,
    ProgressionAwardOutcome, ProgressionAwardPayload, ProgressionAwardPlan,
    bargain_milestone::CoreBargainMilestonePlanner,
};

#[derive(Debug, Clone)]
pub struct PostgresProgressionAwardService {
    persistence: PostgresPersistence,
    rules: CoreProgressionRules,
    contract: StoredProgressionContract,
    bargain_milestone: CoreBargainMilestonePlanner,
}

/// Server-only progression terminal with the immutable Core Bargain milestone row produced by
/// the same serializable transaction. Ordinary callers keep using [`Self::award`]; encounter
/// coordinators use this shape so a B4 offer can never be inferred from mutable life state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressionAwardAuthorityResult {
    pub outcome: ProgressionAwardOutcome,
    pub payload_hash: [u8; 32],
    pub bargain_milestone: Option<StoredBargainMilestoneResult>,
}

impl PostgresProgressionAwardService {
    pub fn new(
        persistence: PostgresPersistence,
        content: &sim_content::CoreDevelopmentProgression,
        oath_bargain_content: &sim_content::CompiledOathBargainCatalog,
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
            bargain_milestone: CoreBargainMilestonePlanner::new(oath_bargain_content)
                .map_err(|_| crate::ProgressionAwardError::InvalidContent)?,
        })
    }

    pub async fn award(
        &self,
        authenticated: AuthenticatedAccount,
        command: &ProgressionAwardCommand,
    ) -> ProgressionAwardOutcome {
        self.award_inner(authenticated, command).await
    }

    pub(crate) async fn award_in_active_danger(
        &self,
        authenticated: AuthenticatedAccount,
        command: &ProgressionAwardCommand,
        authority: StoredActiveDangerAuthorityV1,
    ) -> Result<ProgressionAwardOutcome, PersistenceError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return Err(PersistenceError::ActiveDangerAuthorityBindingMismatch);
        }
        let account_id = authenticated.account_id.as_bytes();
        let transaction = self
            .persistence
            .transact_progression_award_in_active_danger(
                account_id,
                command.payload.character_id,
                command.reward_event_id,
                self.rules
                    .first_clear_boss_id_for_source(&command.payload.source_content_id),
                &self.contract,
                authority,
                |state| self.plan_and_stage(state, command, account_id),
            )
            .await?;
        Ok(match transaction {
            ProgressionAwardTransaction::Committed(outcome) => outcome,
            ProgressionAwardTransaction::Replayed(stored) => self.replay(command, &stored),
        })
    }

    async fn award_inner(
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
            Ok(ProgressionAwardTransaction::Replayed(stored)) => self.replay(command, &stored),
            Err(PersistenceError::ProgressionCharacterNotFound) => {
                terminal_outcome(command, ProgressionAwardCode::CharacterNotFound)
            }
            Err(PersistenceError::ProgressionCharacterDead) => {
                terminal_outcome(command, ProgressionAwardCode::CharacterDead)
            }
            Err(_) => terminal_outcome(command, ProgressionAwardCode::ServiceUnavailable),
        }
    }

    pub async fn award_with_milestone(
        &self,
        authenticated: AuthenticatedAccount,
        command: &ProgressionAwardCommand,
    ) -> ProgressionAwardAuthorityResult {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return authority_terminal(command, ProgressionAwardCode::ServiceUnavailable);
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
                |state| {
                    let outcome = self.plan_and_stage(state, command, account_id)?;
                    Ok(ProgressionAwardAuthorityResult {
                        outcome,
                        payload_hash: command.payload_hash,
                        bargain_milestone: state
                            .new_bargain_milestone
                            .as_ref()
                            .map(|staged| staged.result.clone()),
                    })
                },
            )
            .await;
        match transaction {
            Ok(ProgressionAwardTransaction::Committed(result)) => result,
            Ok(ProgressionAwardTransaction::Replayed(stored)) => {
                let outcome = self.replay(command, &stored);
                if outcome.code != ProgressionAwardCode::Accepted {
                    return ProgressionAwardAuthorityResult {
                        outcome,
                        payload_hash: command.payload_hash,
                        bargain_milestone: None,
                    };
                }
                match self
                    .persistence
                    .bargain_milestone_result(account_id, command.reward_event_id)
                    .await
                {
                    Ok(bargain_milestone) => ProgressionAwardAuthorityResult {
                        outcome,
                        payload_hash: command.payload_hash,
                        bargain_milestone,
                    },
                    Err(_) => authority_terminal(command, ProgressionAwardCode::ServiceUnavailable),
                }
            }
            Err(PersistenceError::ProgressionCharacterNotFound) => {
                authority_terminal(command, ProgressionAwardCode::CharacterNotFound)
            }
            Err(PersistenceError::ProgressionCharacterDead) => {
                authority_terminal(command, ProgressionAwardCode::CharacterDead)
            }
            Err(_) => authority_terminal(command, ProgressionAwardCode::ServiceUnavailable),
        }
    }

    /// Commits one server-owned encounter award while constructing its expected progression
    /// version under the same account/character/progression locks used by the mutation. This
    /// avoids persisting a stale-version terminal between an unlocked snapshot and the award.
    /// Replay reconstructs the exact original command from the immutable receipt.
    pub(crate) async fn award_server_encounter_with_milestone(
        &self,
        authenticated: AuthenticatedAccount,
        reward_event_id: [u8; 16],
        character_id: [u8; 16],
        source_content_id: &str,
        evidence: EncounterXpEvidence,
    ) -> Result<ProgressionAwardAuthorityResult, PersistenceError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return Err(PersistenceError::CorruptStoredProgression);
        }
        let account_id = authenticated.account_id.as_bytes();
        let boss_id = self.rules.first_clear_boss_id_for_source(source_content_id);
        let transaction = self
            .persistence
            .transact_progression_award(
                account_id,
                character_id,
                reward_event_id,
                boss_id,
                &self.contract,
                |state| {
                    let command = self.locked_encounter_command(
                        state,
                        reward_event_id,
                        character_id,
                        source_content_id,
                        evidence,
                    )?;
                    let outcome = self.plan_and_stage(state, &command, account_id)?;
                    Ok(ProgressionAwardAuthorityResult {
                        outcome,
                        payload_hash: command.payload_hash,
                        bargain_milestone: state
                            .new_bargain_milestone
                            .as_ref()
                            .map(|staged| staged.result.clone()),
                    })
                },
            )
            .await?;
        match transaction {
            ProgressionAwardTransaction::Committed(result) => Ok(result),
            ProgressionAwardTransaction::Replayed(stored) => {
                let command = replay_encounter_command(
                    &stored,
                    reward_event_id,
                    character_id,
                    source_content_id,
                    evidence,
                )?;
                let outcome = self.replay(&command, &stored);
                let bargain_milestone = if outcome.code == ProgressionAwardCode::Accepted {
                    self.persistence
                        .bargain_milestone_result(account_id, reward_event_id)
                        .await?
                } else {
                    None
                };
                Ok(ProgressionAwardAuthorityResult {
                    outcome,
                    payload_hash: command.payload_hash,
                    bargain_milestone,
                })
            }
        }
    }

    fn locked_encounter_command(
        &self,
        state: &ProgressionAwardTransactionState,
        reward_event_id: [u8; 16],
        character_id: [u8; 16],
        source_content_id: &str,
        evidence: EncounterXpEvidence,
    ) -> Result<ProgressionAwardCommand, PersistenceError> {
        let payload = ProgressionAwardPayload {
            character_id,
            expected_progression_version: u64::try_from(state.progression.progression_version)
                .map_err(|_| PersistenceError::CorruptStoredProgression)?,
            source_content_id: source_content_id.to_owned(),
            progression_content_revision: self.rules.records_revision().clone(),
            evidence: ProgressionAwardEvidence::Encounter(evidence),
        };
        Ok(ProgressionAwardCommand {
            reward_event_id,
            payload_hash: payload.canonical_hash(),
            payload,
        })
    }

    fn replay(
        &self,
        command: &ProgressionAwardCommand,
        stored: &StoredXpAwardResult,
    ) -> ProgressionAwardOutcome {
        if stored.character_id != command.payload.character_id
            || stored.payload_hash != command.payload_hash
        {
            return self
                .rules
                .replay(
                    command,
                    stored.character_id,
                    stored.payload_hash,
                    &stored.result_payload,
                )
                .unwrap_or_else(|_| {
                    terminal_outcome(command, ProgressionAwardCode::ServiceUnavailable)
                });
        }
        if stored.revoked_by_restore_point_id.is_some() {
            return terminal_outcome(command, ProgressionAwardCode::RevokedByCrashRestore);
        }
        self.rules
            .replay(
                command,
                stored.character_id,
                stored.payload_hash,
                &stored.result_payload,
            )
            .unwrap_or_else(|_| terminal_outcome(command, ProgressionAwardCode::ServiceUnavailable))
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
        self.bargain_milestone
            .stage_if_qualifying(state, command, before.level, &plan)?;
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

fn encounter_evidence_from_stored(
    stored: &StoredXpEligibilityEvidence,
) -> Result<EncounterXpEvidence, PersistenceError> {
    let StoredXpEligibilityEvidence::Encounter(stored) = stored else {
        return Err(PersistenceError::CorruptStoredProgression);
    };
    Ok(EncounterXpEvidence {
        active_ticks: u64::try_from(stored.active_ticks)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        present_ticks: u64::try_from(stored.present_ticks)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        longest_inactivity_ticks: u64::try_from(stored.longest_inactivity_ticks)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        encounter_contribution_reference_health: u64::try_from(stored.reference_health)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        direct_damage: u64::try_from(stored.direct_damage)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        effective_healing_to_others: u64::try_from(stored.effective_healing)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        damage_prevented_on_others: u64::try_from(stored.damage_prevented)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        qualifying_objective_credits: u8::try_from(stored.objective_credits)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        life_state: match stored.life_state {
            StoredEncounterLifeState::Living => RewardLifeState::Living,
            StoredEncounterLifeState::Dead => RewardLifeState::Dead,
        },
        recall_state: match stored.recall_state {
            StoredEncounterRecallState::Present => RewardRecallState::Eligible,
            StoredEncounterRecallState::Recalled => RewardRecallState::EmergencyRecallCompleted,
        },
        trust_state: match stored.trust_state {
            StoredEncounterTrustState::Valid => RewardTrustState::Valid,
            StoredEncounterTrustState::InvalidSession => RewardTrustState::InvalidSession,
            StoredEncounterTrustState::AntiCheatRejected => RewardTrustState::AntiCheatRejected,
        },
    })
}

fn replay_encounter_command(
    stored: &StoredXpAwardResult,
    reward_event_id: [u8; 16],
    character_id: [u8; 16],
    source_content_id: &str,
    evidence: EncounterXpEvidence,
) -> Result<ProgressionAwardCommand, PersistenceError> {
    let stored_evidence = encounter_evidence_from_stored(&stored.evidence)?;
    if stored.reward_event_id != reward_event_id
        || stored.character_id != character_id
        || stored.source_content_id != source_content_id
        || stored_evidence != evidence
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    let payload = ProgressionAwardPayload {
        character_id,
        expected_progression_version: u64::try_from(stored.pre_progression_version)
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        source_content_id: source_content_id.to_owned(),
        progression_content_revision: ManifestHash::new(
            stored.progression_content_revision.clone(),
        )
        .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        evidence: ProgressionAwardEvidence::Encounter(stored_evidence),
    };
    if payload.canonical_hash() != stored.payload_hash {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(ProgressionAwardCommand {
        reward_event_id,
        payload_hash: stored.payload_hash,
        payload,
    })
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

fn authority_terminal(
    command: &ProgressionAwardCommand,
    code: ProgressionAwardCode,
) -> ProgressionAwardAuthorityResult {
    ProgressionAwardAuthorityResult {
        outcome: terminal_outcome(command, code),
        payload_hash: command.payload_hash,
        bargain_milestone: None,
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
