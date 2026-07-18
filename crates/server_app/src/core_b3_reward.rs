//! Durable normal-route B3 reward coordinator.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`PROG-003`,
//! `BRG-001`-`005`, `DNG-005`, and `SOC-010`),
//! `Gravebound_Content_Production_Spec_v1.md` (`CONT-014`, `CONT-REWARD-003`-`004`,
//! `CONT-ROOM-007`, and `CONT-ENEMY-003`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`-`05`). The coordinator consumes only the
//! immutable handoff produced by the B3 simulation owner. It never accepts client-authored reward
//! IDs, item destinations, XP values, Bargain candidates, or aggregate versions.

use sim_core::{EncounterXpEvidence, RewardLifeState, RewardRecallState, RewardTrustState};
use thiserror::Error;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreDurableBargainRestResolution,
    PostgresProgressionAwardService, PostgresRewardService, ProgressionAwardCode,
    ProgressionAwardOutcome, RewardGrantContext, RewardGrantError, RewardGrantTransaction,
};

const B3_SOURCE_CONTENT_ID: &str = "miniboss.sepulcher_knight";
const B3_REWARD_PROFILE_ID: &str = "reward.miniboss_t1";
const B3_XP_PROFILE_ID: &str = "xp.miniboss_t1";
const B3_REFERENCE_HEALTH: u64 = 1_600;
const B3_REWARD_DELAY_TICKS: u64 = 8;
const TWENTY_SECONDS_AT_30_HZ: u64 = 600;

/// Opaque proof that the exact B3 personal item result and progression/milestone result are both
/// durable. Runtime code can compare its private bindings, but transport code cannot construct a
/// reward identity, handoff, result hash, offer, or no-offer result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDurableB3RewardCommit {
    account_id: [u8; 16],
    character_id: [u8; 16],
    instance_lineage_id: [u8; 16],
    reward_event_id: [u8; 16],
    source_instance_id: [u8; 16],
    handoff: sim_content::CoreB3RewardHandoff,
    reward_result_hash: [u8; 32],
    reward_replayed: bool,
    progression_payload_hash: [u8; 32],
    progression: ProgressionAwardOutcome,
    bargain_offer_id: Option<[u8; 16]>,
    no_offer_resolution: Option<CoreDurableBargainRestResolution>,
}

impl CoreDurableB3RewardCommit {
    #[must_use]
    pub const fn account_id(&self) -> [u8; 16] {
        self.account_id
    }

    #[must_use]
    pub const fn character_id(&self) -> [u8; 16] {
        self.character_id
    }

    #[must_use]
    pub const fn instance_lineage_id(&self) -> [u8; 16] {
        self.instance_lineage_id
    }

    #[must_use]
    pub const fn reward_event_id(&self) -> [u8; 16] {
        self.reward_event_id
    }

    #[must_use]
    pub const fn source_instance_id(&self) -> [u8; 16] {
        self.source_instance_id
    }

    #[must_use]
    pub const fn handoff(&self) -> &sim_content::CoreB3RewardHandoff {
        &self.handoff
    }

    #[must_use]
    pub const fn reward_result_hash(&self) -> [u8; 32] {
        self.reward_result_hash
    }

    #[must_use]
    pub const fn reward_replayed(&self) -> bool {
        self.reward_replayed
    }

    #[must_use]
    pub const fn progression_payload_hash(&self) -> [u8; 32] {
        self.progression_payload_hash
    }

    #[must_use]
    pub const fn progression(&self) -> &ProgressionAwardOutcome {
        &self.progression
    }

    #[must_use]
    pub const fn bargain_offer_id(&self) -> Option<[u8; 16]> {
        self.bargain_offer_id
    }

    #[must_use]
    pub const fn no_offer_resolution(&self) -> Option<&CoreDurableBargainRestResolution> {
        self.no_offer_resolution.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(
        authenticated: AuthenticatedAccount,
        character_id: [u8; 16],
        instance_lineage_id: [u8; 16],
        handoff: sim_content::CoreB3RewardHandoff,
    ) -> Self {
        let reward_event_id = derive_identity(
            b"gravebound.core-b3-reward-event.v1\0",
            instance_lineage_id,
            &handoff,
        );
        let source_instance_id = derive_identity(
            b"gravebound.core-b3-source-instance.v1\0",
            instance_lineage_id,
            &handoff,
        );
        Self {
            account_id: authenticated.account_id.as_bytes(),
            character_id,
            instance_lineage_id,
            reward_event_id,
            source_instance_id,
            handoff,
            reward_result_hash: [7; 32],
            reward_replayed: false,
            progression_payload_hash: [8; 32],
            progression: ProgressionAwardOutcome {
                reward_event_id,
                code: ProgressionAwardCode::Accepted,
                projection: None,
                base_xp: 120,
                first_clear_bonus_xp: 0,
                applied_xp: 120,
                discarded_at_core_cap: 0,
                first_clear_awarded: false,
            },
            bargain_offer_id: Some(reward_event_id),
            no_offer_resolution: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PostgresCoreB3RewardCoordinator {
    rewards: PostgresRewardService,
    progression: PostgresProgressionAwardService,
}

impl PostgresCoreB3RewardCoordinator {
    #[must_use]
    pub const fn new(
        rewards: PostgresRewardService,
        progression: PostgresProgressionAwardService,
    ) -> Self {
        Self {
            rewards,
            progression,
        }
    }

    pub async fn commit(
        &self,
        authenticated: AuthenticatedAccount,
        character_id: [u8; 16],
        instance_lineage_id: [u8; 16],
        current_tick: u64,
        handoff: &sim_content::CoreB3RewardHandoff,
    ) -> Result<CoreDurableB3RewardCommit, CoreB3RewardCoordinatorError> {
        validate_binding(
            authenticated,
            character_id,
            instance_lineage_id,
            current_tick,
            handoff,
        )?;
        let reward_event_id = derive_identity(
            b"gravebound.core-b3-reward-event.v1\0",
            instance_lineage_id,
            handoff,
        );
        let source_instance_id = derive_identity(
            b"gravebound.core-b3-source-instance.v1\0",
            instance_lineage_id,
            handoff,
        );
        let evidence = EncounterXpEvidence {
            active_ticks: handoff.active_ticks,
            present_ticks: handoff.present_ticks,
            longest_inactivity_ticks: handoff.longest_inactivity_ticks,
            encounter_contribution_reference_health: handoff.reference_health,
            direct_damage: handoff.direct_damage,
            effective_healing_to_others: 0,
            damage_prevented_on_others: 0,
            qualifying_objective_credits: 0,
            life_state: handoff.life_state,
            recall_state: handoff.recall_state,
            trust_state: handoff.trust_state,
        };
        // Eligibility and XP/milestone authority commit before loot. An ineligible terminal can
        // therefore never leave an item behind, while an item outage remains safely retryable from
        // the immutable progression receipt.
        let progression = self
            .progression
            .award_server_encounter_with_milestone(
                authenticated,
                reward_event_id,
                character_id,
                B3_SOURCE_CONTENT_ID,
                evidence,
            )
            .await?;
        if progression.outcome.code != ProgressionAwardCode::Accepted {
            return Err(CoreB3RewardCoordinatorError::ProgressionNotCommitted(
                progression.outcome.code,
            ));
        }
        let reward = self
            .rewards
            .grant(RewardGrantContext {
                reward_request_id: reward_event_id,
                account_id: authenticated.account_id.as_bytes(),
                character_id,
                source_instance_id,
                reward_table_id: B3_REWARD_PROFILE_ID,
                current_tick,
            })
            .await?;
        let (reward_result_hash, reward_replayed) = match &reward {
            RewardGrantTransaction::Fresh { durable, .. } => (durable.result_hash, false),
            RewardGrantTransaction::Replay { durable, .. } => (durable.result_hash, true),
        };
        let (bargain_offer_id, no_offer_resolution) = project_milestone_authority(
            authenticated,
            character_id,
            instance_lineage_id,
            reward_event_id,
            progression.payload_hash,
            progression.bargain_milestone.as_ref(),
        )?;
        Ok(CoreDurableB3RewardCommit {
            account_id: authenticated.account_id.as_bytes(),
            character_id,
            instance_lineage_id,
            reward_event_id,
            source_instance_id,
            handoff: handoff.clone(),
            reward_result_hash,
            reward_replayed,
            progression_payload_hash: progression.payload_hash,
            progression: progression.outcome,
            bargain_offer_id,
            no_offer_resolution,
        })
    }
}

fn project_milestone_authority(
    authenticated: AuthenticatedAccount,
    character_id: [u8; 16],
    instance_lineage_id: [u8; 16],
    reward_event_id: [u8; 16],
    progression_payload_hash: [u8; 32],
    milestone: Option<&persistence::StoredBargainMilestoneResult>,
) -> Result<
    (Option<[u8; 16]>, Option<CoreDurableBargainRestResolution>),
    CoreB3RewardCoordinatorError,
> {
    let Some(milestone) = milestone else {
        return Ok((None, None));
    };
    if milestone.account_id != authenticated.account_id.as_bytes()
        || milestone.character_id != character_id
        || milestone.source_reward_event_id != reward_event_id
        || milestone.payload_hash != progression_payload_hash
        || milestone.instance_lineage_id != instance_lineage_id
    {
        return Err(CoreB3RewardCoordinatorError::MilestoneAuthorityMismatch);
    }
    match milestone.result_code {
        0 => Ok((milestone.offer_id, None)),
        1..=3 => Ok((
            None,
            Some(CoreDurableBargainRestResolution::from_no_offer_milestone(
                authenticated,
                milestone,
            )?),
        )),
        _ => Err(CoreB3RewardCoordinatorError::MilestoneAuthorityMismatch),
    }
}

fn validate_binding(
    authenticated: AuthenticatedAccount,
    character_id: [u8; 16],
    instance_lineage_id: [u8; 16],
    current_tick: u64,
    handoff: &sim_content::CoreB3RewardHandoff,
) -> Result<(), CoreB3RewardCoordinatorError> {
    let minimum_contribution = handoff.reference_health.div_ceil(200);
    let exact_reward_due_tick = handoff
        .death_tick
        .0
        .checked_add(B3_REWARD_DELAY_TICKS)
        .ok_or(CoreB3RewardCoordinatorError::InvalidHandoff)?;
    let presence_qualified = handoff.active_ticks < TWENTY_SECONDS_AT_30_HZ
        || handoff.present_ticks >= handoff.active_ticks.div_ceil(2);
    if authenticated.namespace != AuthenticatedNamespace::WipeableTest
        || character_id == [0; 16]
        || instance_lineage_id == [0; 16]
        || handoff.actor_id == handoff.participant_id
        || handoff.reward_due_tick.0 != exact_reward_due_tick
        || current_tick < handoff.reward_due_tick.0
        || handoff.reward_profile_id != B3_REWARD_PROFILE_ID
        || handoff.xp_profile_id != B3_XP_PROFILE_ID
        || handoff.reference_health != B3_REFERENCE_HEALTH
        || handoff.active_ticks == 0
        || handoff.present_ticks == 0
        || handoff.present_ticks > handoff.active_ticks
        || !presence_qualified
        || handoff.longest_inactivity_ticks > TWENTY_SECONDS_AT_30_HZ
        || handoff.direct_damage < minimum_contribution
        || handoff.direct_damage > handoff.reference_health
        || handoff.life_state != RewardLifeState::Living
        || handoff.recall_state != RewardRecallState::Eligible
        || handoff.trust_state != RewardTrustState::Valid
    {
        return Err(CoreB3RewardCoordinatorError::InvalidHandoff);
    }
    Ok(())
}

fn derive_identity(
    domain: &[u8],
    instance_lineage_id: [u8; 16],
    handoff: &sim_content::CoreB3RewardHandoff,
) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    hasher.update(&instance_lineage_id);
    hasher.update(&handoff.activation_ordinal.to_le_bytes());
    hasher.update(&handoff.instance_id.run_ordinal.to_le_bytes());
    hasher.update(&handoff.instance_id.spawn_ordinal.to_le_bytes());
    hasher.update(&handoff.actor_id.get().to_le_bytes());
    hasher.update(&handoff.participant_id.get().to_le_bytes());
    hasher.update(&handoff.death_tick.0.to_le_bytes());
    hasher.update(&handoff.reward_due_tick.0.to_le_bytes());
    let digest = hasher.finalize();
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&digest.as_bytes()[..16]);
    identity
}

#[derive(Debug, Error)]
pub enum CoreB3RewardCoordinatorError {
    #[error("B3 reward handoff is not the exact eligible Core Sepulcher Knight terminal")]
    InvalidHandoff,
    #[error("B3 progression terminal was not committed: {0:?}")]
    ProgressionNotCommitted(ProgressionAwardCode),
    #[error("B3 Bargain milestone does not match the reward/progression authority")]
    MilestoneAuthorityMismatch,
    #[error(transparent)]
    Reward(#[from] RewardGrantError),
    #[error(transparent)]
    Persistence(#[from] persistence::PersistenceError),
}

#[cfg(test)]
mod tests {
    use sim_core::{EntityId, SpawnInstanceId, Tick};

    use super::*;
    use crate::{AccountId, AuthenticatedAccount};

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).expect("account"),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn handoff() -> sim_content::CoreB3RewardHandoff {
        sim_content::CoreB3RewardHandoff {
            activation_ordinal: 1,
            instance_id: SpawnInstanceId {
                run_ordinal: 4,
                spawn_ordinal: 76,
            },
            actor_id: EntityId::new(100).expect("actor"),
            participant_id: EntityId::new(900).expect("participant"),
            death_tick: Tick(1_000),
            reward_due_tick: Tick(1_008),
            reward_profile_id: B3_REWARD_PROFILE_ID.to_owned(),
            xp_profile_id: B3_XP_PROFILE_ID.to_owned(),
            active_ticks: 120,
            present_ticks: 120,
            direct_damage: 1_600,
            reference_health: 1_600,
            longest_inactivity_ticks: 0,
            life_state: RewardLifeState::Living,
            recall_state: RewardRecallState::Eligible,
            trust_state: RewardTrustState::Valid,
        }
    }

    #[test]
    fn identity_is_domain_separated_and_changes_with_authoritative_handoff() {
        let handoff = handoff();
        let reward = derive_identity(b"gravebound.core-b3-reward-event.v1\0", [3; 16], &handoff);
        let replay = derive_identity(b"gravebound.core-b3-reward-event.v1\0", [3; 16], &handoff);
        let source = derive_identity(
            b"gravebound.core-b3-source-instance.v1\0",
            [3; 16],
            &handoff,
        );
        assert_eq!(reward, replay);
        assert_ne!(reward, source);
        let mut changed = handoff;
        changed.death_tick = Tick(1_001);
        assert_ne!(
            reward,
            derive_identity(b"gravebound.core-b3-reward-event.v1\0", [3; 16], &changed)
        );
    }

    #[test]
    fn binding_rejects_early_or_underqualified_handoffs() {
        let valid = handoff();
        validate_binding(authenticated(), [2; 16], [3; 16], 1_008, &valid).expect("valid handoff");
        assert!(matches!(
            validate_binding(authenticated(), [2; 16], [3; 16], 1_007, &valid),
            Err(CoreB3RewardCoordinatorError::InvalidHandoff)
        ));
        let mut weak = valid.clone();
        weak.direct_damage = 7;
        assert!(matches!(
            validate_binding(authenticated(), [2; 16], [3; 16], 1_008, &weak),
            Err(CoreB3RewardCoordinatorError::InvalidHandoff)
        ));
        let mut wrong_delay = valid.clone();
        wrong_delay.reward_due_tick = Tick(1_009);
        assert!(matches!(
            validate_binding(authenticated(), [2; 16], [3; 16], 1_009, &wrong_delay),
            Err(CoreB3RewardCoordinatorError::InvalidHandoff)
        ));
        let mut absent = valid.clone();
        absent.active_ticks = 600;
        absent.present_ticks = 299;
        assert!(matches!(
            validate_binding(authenticated(), [2; 16], [3; 16], 1_008, &absent),
            Err(CoreB3RewardCoordinatorError::InvalidHandoff)
        ));
        let mut inactive = valid.clone();
        inactive.longest_inactivity_ticks = 601;
        assert!(matches!(
            validate_binding(authenticated(), [2; 16], [3; 16], 1_008, &inactive),
            Err(CoreB3RewardCoordinatorError::InvalidHandoff)
        ));
        for invalid in [
            {
                let mut handoff = valid.clone();
                handoff.life_state = RewardLifeState::Dead;
                handoff
            },
            {
                let mut handoff = valid.clone();
                handoff.recall_state = RewardRecallState::EmergencyRecallCompleted;
                handoff
            },
            {
                let mut handoff = valid.clone();
                handoff.trust_state = RewardTrustState::InvalidSession;
                handoff
            },
        ] {
            assert!(matches!(
                validate_binding(authenticated(), [2; 16], [3; 16], 1_008, &invalid),
                Err(CoreB3RewardCoordinatorError::InvalidHandoff)
            ));
        }
    }
}
