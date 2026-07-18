//! Durable Sir Caldus personal reward and stable-exit coordinator.
//!
//! Existing item and progression services remain the durable owners. Partial failure leaves
//! committed personal results replayable; the exit gate remains absent until every eligible
//! owner has both exact terminals.

use std::path::Path;

use persistence::{
    CaldusVictoryExitCommit, PostgresPersistence, StoredCaldusVictoryExit, StoredCaldusVictoryOwner,
};
use protocol::ManifestHash;
use sim_core::{
    CALDUS_REWARD_ID, CoreBossParticipant, CoreBossParticipantLock, CoreCaldusAntiCheatState,
    CoreCaldusDefeatPresence, CoreCaldusEligibilityDecision, CoreCaldusEligibilityEvidence,
    CoreCaldusRecallState, CoreCaldusSessionState, CoreCaldusVictoryError,
    CoreCaldusVictoryIdentities, EncounterXpEvidence, RewardLifeState, RewardRecallState,
    RewardTrustState, evaluate_caldus_eligibility,
};
use thiserror::Error;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CaldusExitPresentationCommit,
    CaldusInstancePresentation, CaldusInstancePresentationError, PostgresProgressionAwardService,
    PostgresRewardService, ProgressionAwardCode, ProgressionAwardCommand, ProgressionAwardError,
    ProgressionAwardEvidence, ProgressionAwardOutcome, ProgressionAwardPayload, RewardGrantContext,
    RewardGrantError, RewardGrantTransaction, SecretRewardEpoch,
};

const CALDUS_SOURCE_ID: &str = "boss.sir_caldus";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaldusVictoryOwnerCommand {
    pub participant: CoreBossParticipant,
    pub authenticated: AuthenticatedAccount,
    pub character_id: [u8; 16],
    pub expected_progression_version: u64,
    pub progression_content_revision: ManifestHash,
    pub eligibility: CoreCaldusEligibilityEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaldusVictoryOwnerCommit {
    pub participant: CoreBossParticipant,
    pub reward: RewardGrantTransaction,
    pub progression: ProgressionAwardOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaldusVictoryCommitResult {
    pub identities: CoreCaldusVictoryIdentities,
    pub eligibility: Vec<CoreCaldusEligibilityDecision>,
    pub owners: Vec<CaldusVictoryOwnerCommit>,
    pub exit: StoredCaldusVictoryExit,
}

impl CaldusVictoryCommitResult {
    /// Projects the durable result into the matching live attempt. A caller cannot obtain this
    /// method's receiver until item, progression, and schema-25 exit terminality all succeed.
    pub fn present_exit(
        &self,
        content: &sim_content::CoreDevelopmentCaldus,
        presentation: &mut CaldusInstancePresentation,
    ) -> Result<CaldusExitPresentationCommit, CaldusInstancePresentationError> {
        presentation.present_committed_exit(content, &self.exit)
    }
}

#[derive(Debug, Clone)]
pub struct PostgresCaldusVictoryCoordinator {
    persistence: PostgresPersistence,
    rewards: PostgresRewardService,
    progression: PostgresProgressionAwardService,
}

impl PostgresCaldusVictoryCoordinator {
    #[must_use]
    pub const fn new(
        persistence: PostgresPersistence,
        rewards: PostgresRewardService,
        progression: PostgresProgressionAwardService,
    ) -> Self {
        Self {
            persistence,
            rewards,
            progression,
        }
    }

    pub fn load(
        persistence: PostgresPersistence,
        content_root: &Path,
        epoch: SecretRewardEpoch,
    ) -> Result<Self, CaldusVictoryCompositionError> {
        let progression_content = sim_content::load_core_development_progression(content_root)
            .map_err(|_| CaldusVictoryCompositionError::ProgressionContent)?;
        let oath_bargain_content = sim_content::load_core_development_oaths_bargains(content_root)
            .map_err(|_| CaldusVictoryCompositionError::OathBargainContent)?;
        let rewards = PostgresRewardService::load(persistence.clone(), content_root, epoch)?;
        let progression = PostgresProgressionAwardService::new(
            persistence.clone(),
            &progression_content,
            &oath_bargain_content,
        )?;
        Ok(Self::new(persistence, rewards, progression))
    }

    pub async fn commit(
        &self,
        instance_lineage_id: [u8; 16],
        lock: &CoreBossParticipantLock,
        active_duration_ticks: u32,
        current_tick: u64,
        owners: &[CaldusVictoryOwnerCommand],
    ) -> Result<CaldusVictoryCommitResult, CaldusVictoryCoordinatorError> {
        validate_owner_commands(lock, owners)?;
        let identities = CoreCaldusVictoryIdentities::derive(instance_lineage_id, lock)?;
        let evidence = owners
            .iter()
            .map(|owner| owner.eligibility)
            .collect::<Vec<_>>();
        let eligibility = evaluate_caldus_eligibility(lock, active_duration_ticks, &evidence)?;
        if !eligibility.iter().any(|decision| decision.eligible) {
            return Err(CaldusVictoryCoordinatorError::NoEligibleOwners);
        }
        let mut committed_owners = Vec::new();
        let mut exit_owners = Vec::new();
        for (owner, decision) in owners.iter().zip(&eligibility) {
            if !decision.eligible {
                continue;
            }
            let request_id = identities
                .reward_for(owner.participant)
                .ok_or(CaldusVictoryCoordinatorError::IdentityMismatch)?
                .bytes();
            let account_id = owner.authenticated.account_id.as_bytes();
            let reward = self
                .rewards
                .grant(RewardGrantContext {
                    reward_request_id: request_id,
                    account_id,
                    character_id: owner.character_id,
                    source_instance_id: identities.encounter_id.bytes(),
                    reward_table_id: CALDUS_REWARD_ID,
                    current_tick,
                })
                .await?;
            let progression_command = progression_command(
                owner,
                request_id,
                active_duration_ticks,
                lock.maximum_health,
            );
            let progression = self
                .progression
                .award(owner.authenticated, &progression_command)
                .await;
            if progression.code != ProgressionAwardCode::Accepted {
                return Err(CaldusVictoryCoordinatorError::ProgressionNotCommitted(
                    progression.code,
                ));
            }
            let reward_result_hash = match &reward {
                RewardGrantTransaction::Fresh { durable, .. }
                | RewardGrantTransaction::Replay { durable, .. } => durable.result_hash,
            };
            exit_owners.push(StoredCaldusVictoryOwner {
                party_slot: owner.participant.party_slot,
                participant_entity_id: owner.participant.entity_id.get(),
                account_id,
                character_id: owner.character_id,
                reward_request_id: request_id,
                reward_result_hash,
                progression_payload_hash: progression_command.payload_hash,
            });
            committed_owners.push(CaldusVictoryOwnerCommit {
                participant: owner.participant,
                reward,
                progression,
            });
        }
        let exit = self
            .persistence
            .commit_caldus_victory_exit(&CaldusVictoryExitCommit {
                encounter_id: identities.encounter_id.bytes(),
                instance_lineage_id,
                attempt_ordinal: lock.attempt_ordinal,
                exit_instance_id: identities.exit_instance_id.bytes(),
                owners: exit_owners,
            })
            .await?;
        Ok(CaldusVictoryCommitResult {
            identities,
            eligibility,
            owners: committed_owners,
            exit,
        })
    }
}

fn validate_owner_commands(
    lock: &CoreBossParticipantLock,
    owners: &[CaldusVictoryOwnerCommand],
) -> Result<(), CaldusVictoryCoordinatorError> {
    if owners.len() != lock.participants.len() {
        return Err(CaldusVictoryCoordinatorError::IncompleteOwnerBindings);
    }
    for (owner, participant) in owners.iter().zip(&lock.participants) {
        if owner.participant != *participant
            || owner.eligibility.participant != *participant
            || owner.authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || owner.character_id == [0; 16]
            || owner.expected_progression_version == 0
        {
            return Err(CaldusVictoryCoordinatorError::InvalidOwnerBinding);
        }
    }
    Ok(())
}

fn progression_command(
    owner: &CaldusVictoryOwnerCommand,
    reward_event_id: [u8; 16],
    active_duration_ticks: u32,
    reference_health: u32,
) -> ProgressionAwardCommand {
    let eligibility = owner.eligibility;
    let payload = ProgressionAwardPayload {
        character_id: owner.character_id,
        expected_progression_version: owner.expected_progression_version,
        source_content_id: CALDUS_SOURCE_ID.to_owned(),
        progression_content_revision: owner.progression_content_revision.clone(),
        evidence: ProgressionAwardEvidence::Encounter(EncounterXpEvidence {
            active_ticks: u64::from(active_duration_ticks),
            present_ticks: u64::from(eligibility.presence_ticks),
            longest_inactivity_ticks: u64::from(eligibility.longest_inactivity_ticks),
            encounter_contribution_reference_health: u64::from(reference_health),
            direct_damage: eligibility.direct_damage,
            effective_healing_to_others: eligibility.effective_healing_to_others,
            damage_prevented_on_others: eligibility.damage_prevented_on_others,
            qualifying_objective_credits: eligibility.objective_credits,
            life_state: match eligibility.defeat_presence {
                CoreCaldusDefeatPresence::AliveAndPresent => RewardLifeState::Living,
                CoreCaldusDefeatPresence::NotAliveOrAbsent => RewardLifeState::Dead,
            },
            recall_state: match eligibility.recall_state {
                CoreCaldusRecallState::Stayed => RewardRecallState::Eligible,
                CoreCaldusRecallState::RecalledBeforeDefeat => {
                    RewardRecallState::EmergencyRecallCompleted
                }
            },
            trust_state: match (eligibility.session_state, eligibility.anti_cheat_state) {
                (CoreCaldusSessionState::Valid, CoreCaldusAntiCheatState::Valid) => {
                    RewardTrustState::Valid
                }
                (CoreCaldusSessionState::Invalid, _) => RewardTrustState::InvalidSession,
                (_, CoreCaldusAntiCheatState::Invalid) => RewardTrustState::AntiCheatRejected,
            },
        }),
    };
    ProgressionAwardCommand {
        reward_event_id,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

#[derive(Debug, Error)]
pub enum CaldusVictoryCoordinatorError {
    #[error("Caldus victory owner bindings must cover the complete immutable lock")]
    IncompleteOwnerBindings,
    #[error("Caldus victory owner binding is invalid or out of lock order")]
    InvalidOwnerBinding,
    #[error("Caldus victory produced no eligible personal reward owner")]
    NoEligibleOwners,
    #[error("Caldus victory identity does not contain an eligible owner")]
    IdentityMismatch,
    #[error("Caldus progression terminal was not committed: {0:?}")]
    ProgressionNotCommitted(ProgressionAwardCode),
    #[error(transparent)]
    Victory(#[from] CoreCaldusVictoryError),
    #[error(transparent)]
    Reward(#[from] RewardGrantError),
    #[error(transparent)]
    Persistence(#[from] persistence::PersistenceError),
}

#[derive(Debug, Error)]
pub enum CaldusVictoryCompositionError {
    #[error("Core progression content could not be loaded for Caldus rewards")]
    ProgressionContent,
    #[error("Core Oath/Bargain content could not be loaded for Caldus rewards")]
    OathBargainContent,
    #[error(transparent)]
    Reward(#[from] RewardGrantError),
    #[error(transparent)]
    Progression(#[from] ProgressionAwardError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AccountId;
    use sim_core::EntityId;

    fn participant(entity: u64, slot: u8) -> CoreBossParticipant {
        CoreBossParticipant {
            entity_id: EntityId::new(entity).expect("entity"),
            party_slot: slot,
        }
    }

    fn evidence(participant: CoreBossParticipant) -> CoreCaldusEligibilityEvidence {
        CoreCaldusEligibilityEvidence {
            participant,
            presence_ticks: 3_000,
            direct_damage: 100,
            effective_healing_to_others: 0,
            damage_prevented_on_others: 0,
            objective_credits: 0,
            longest_inactivity_ticks: 0,
            defeat_presence: CoreCaldusDefeatPresence::AliveAndPresent,
            recall_state: CoreCaldusRecallState::Stayed,
            session_state: CoreCaldusSessionState::Valid,
            anti_cheat_state: CoreCaldusAntiCheatState::Valid,
        }
    }

    fn owner(participant: CoreBossParticipant) -> CaldusVictoryOwnerCommand {
        CaldusVictoryOwnerCommand {
            participant,
            authenticated: AuthenticatedAccount {
                account_id: AccountId::new([participant.party_slot + 1; 16]).expect("account"),
                namespace: AuthenticatedNamespace::WipeableTest,
            },
            character_id: [participant.party_slot + 11; 16],
            expected_progression_version: 1,
            progression_content_revision: ManifestHash::new("1".repeat(64)).expect("revision"),
            eligibility: evidence(participant),
        }
    }

    #[test]
    fn owner_bindings_require_exact_lock_order_and_wipeable_namespace() {
        let lock = CoreBossParticipantLock {
            attempt_ordinal: 1,
            participants: vec![participant(10, 0), participant(20, 1)],
            maximum_health: 12_384,
        };
        let mut owners = vec![owner(lock.participants[0]), owner(lock.participants[1])];
        validate_owner_commands(&lock, &owners).expect("valid");
        owners.swap(0, 1);
        assert!(matches!(
            validate_owner_commands(&lock, &owners),
            Err(CaldusVictoryCoordinatorError::InvalidOwnerBinding)
        ));
    }

    #[test]
    fn progression_command_is_stable_and_uses_exact_boss_source_and_reference_health() {
        let owner = owner(participant(10, 0));
        let first = progression_command(&owner, [9; 16], 5_400, 7_200);
        let replay = progression_command(&owner, [9; 16], 5_400, 7_200);
        assert_eq!(first.payload_hash, replay.payload_hash);
        assert_eq!(first.payload.source_content_id, CALDUS_SOURCE_ID);
        assert_eq!(first.reward_event_id, [9; 16]);
        let ProgressionAwardEvidence::Encounter(evidence) = first.payload.evidence else {
            panic!("encounter evidence")
        };
        assert_eq!(evidence.encounter_contribution_reference_health, 7_200);
    }
}
