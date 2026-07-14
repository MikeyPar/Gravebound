//! Stable Sir Caldus victory identities and SOC-010 personal eligibility.
//!
//! The canonical GDD `SOC-010`, `PROG-003`, `LOOT-010`, and `ENC-010`, content spec
//! `CONT-REWARD-003` plus the Core override, roadmap `GB-M03-03`, and approved
//! `SPEC-CONFLICT-023` define this pure boundary. Persistence and world flow consume these
//! decisions; they do not recompute identity or eligibility independently.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{CoreBossParticipant, CoreBossParticipantLock};

pub const CALDUS_BOSS_XP: u32 = 450;
pub const CALDUS_FIRST_CLEAR_XP: u32 = 225;
pub const CALDUS_MAX_INACTIVITY_TICKS: u32 = 600;
pub const CALDUS_SHORT_FIGHT_TICKS: u32 = 600;
pub const CALDUS_MAX_OBJECTIVE_CREDITS: u8 = 2;
pub const CALDUS_REWARD_ID: &str = "reward.boss_caldus";
pub const CALDUS_EXIT_ID: &str = "portal.exit.dungeon.bell_sepulcher";

const ID_BYTES: usize = 16;
const ENCOUNTER_DOMAIN: &[u8] = b"gravebound.caldus.encounter.v1";
const REWARD_DOMAIN: &[u8] = b"gravebound.caldus.personal-reward.v1";
const EXIT_DOMAIN: &[u8] = b"gravebound.caldus.exit-instance.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreCaldusStableId([u8; ID_BYTES]);

impl CoreCaldusStableId {
    #[must_use]
    pub const fn bytes(self) -> [u8; ID_BYTES] {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusPersonalRewardIdentity {
    pub participant: CoreBossParticipant,
    pub request_id: CoreCaldusStableId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusVictoryIdentities {
    pub encounter_id: CoreCaldusStableId,
    pub exit_instance_id: CoreCaldusStableId,
    pub personal_rewards: Vec<CoreCaldusPersonalRewardIdentity>,
}

impl CoreCaldusVictoryIdentities {
    pub fn derive(
        instance_lineage_id: [u8; ID_BYTES],
        lock: &CoreBossParticipantLock,
    ) -> Result<Self, CoreCaldusVictoryError> {
        validate_identity_input(instance_lineage_id, lock)?;
        let attempt = lock.attempt_ordinal.to_le_bytes();
        let encounter_id = derive_id(
            ENCOUNTER_DOMAIN,
            &[&instance_lineage_id, &attempt, b"boss.sir_caldus"],
        )?;
        let exit_instance_id = derive_id(
            EXIT_DOMAIN,
            &[
                &instance_lineage_id,
                &attempt,
                &encounter_id.0,
                CALDUS_EXIT_ID.as_bytes(),
            ],
        )?;
        let personal_rewards = lock
            .participants
            .iter()
            .map(|participant| {
                let entity = participant.entity_id.get().to_le_bytes();
                let slot = [participant.party_slot];
                derive_id(
                    REWARD_DOMAIN,
                    &[
                        &instance_lineage_id,
                        &attempt,
                        &encounter_id.0,
                        &slot,
                        &entity,
                        CALDUS_REWARD_ID.as_bytes(),
                    ],
                )
                .map(|request_id| CoreCaldusPersonalRewardIdentity {
                    participant: *participant,
                    request_id,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            encounter_id,
            exit_instance_id,
            personal_rewards,
        })
    }

    #[must_use]
    pub fn reward_for(&self, participant: CoreBossParticipant) -> Option<CoreCaldusStableId> {
        self.personal_rewards
            .iter()
            .find(|reward| reward.participant == participant)
            .map(|reward| reward.request_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCaldusEligibilityEvidence {
    pub participant: CoreBossParticipant,
    pub presence_ticks: u32,
    pub direct_damage: u64,
    pub effective_healing_to_others: u64,
    pub damage_prevented_on_others: u64,
    pub objective_credits: u8,
    pub longest_inactivity_ticks: u32,
    pub defeat_presence: CoreCaldusDefeatPresence,
    pub recall_state: CoreCaldusRecallState,
    pub session_state: CoreCaldusSessionState,
    pub anti_cheat_state: CoreCaldusAntiCheatState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCaldusDefeatPresence {
    AliveAndPresent,
    NotAliveOrAbsent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCaldusRecallState {
    Stayed,
    RecalledBeforeDefeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCaldusSessionState {
    Valid,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCaldusAntiCheatState {
    Valid,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCaldusIneligibilityReason {
    Presence,
    Contribution,
    Inactivity,
    NotAliveAndPresent,
    Recalled,
    InvalidSession,
    InvalidAntiCheat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusEligibilityDecision {
    pub participant: CoreBossParticipant,
    pub contribution_centi_units: u128,
    pub eligible: bool,
    pub reasons: Vec<CoreCaldusIneligibilityReason>,
}

pub fn evaluate_caldus_eligibility(
    lock: &CoreBossParticipantLock,
    active_duration_ticks: u32,
    evidence: &[CoreCaldusEligibilityEvidence],
) -> Result<Vec<CoreCaldusEligibilityDecision>, CoreCaldusVictoryError> {
    validate_eligibility_input(lock, active_duration_ticks, evidence)?;
    evidence
        .iter()
        .map(|entry| {
            let contribution_centi_units = u128::from(entry.direct_damage)
                .checked_mul(100)
                .and_then(|value| {
                    u128::from(entry.effective_healing_to_others)
                        .checked_mul(80)
                        .and_then(|support| value.checked_add(support))
                })
                .and_then(|value| {
                    u128::from(entry.damage_prevented_on_others)
                        .checked_mul(60)
                        .and_then(|support| value.checked_add(support))
                })
                .and_then(|value| {
                    u128::from(lock.maximum_health)
                        .checked_mul(2)
                        .and_then(|credit| credit.checked_mul(u128::from(entry.objective_credits)))
                        .and_then(|objective| value.checked_add(objective))
                })
                .ok_or(CoreCaldusVictoryError::ArithmeticOverflow)?;
            let mut reasons = Vec::new();
            if active_duration_ticks >= CALDUS_SHORT_FIGHT_TICKS
                && u64::from(entry.presence_ticks) * 2 < u64::from(active_duration_ticks)
            {
                reasons.push(CoreCaldusIneligibilityReason::Presence);
            }
            if contribution_centi_units
                .checked_mul(2)
                .ok_or(CoreCaldusVictoryError::ArithmeticOverflow)?
                < u128::from(lock.maximum_health)
            {
                reasons.push(CoreCaldusIneligibilityReason::Contribution);
            }
            if entry.longest_inactivity_ticks > CALDUS_MAX_INACTIVITY_TICKS {
                reasons.push(CoreCaldusIneligibilityReason::Inactivity);
            }
            if entry.defeat_presence != CoreCaldusDefeatPresence::AliveAndPresent {
                reasons.push(CoreCaldusIneligibilityReason::NotAliveAndPresent);
            }
            if entry.recall_state == CoreCaldusRecallState::RecalledBeforeDefeat {
                reasons.push(CoreCaldusIneligibilityReason::Recalled);
            }
            if entry.session_state != CoreCaldusSessionState::Valid {
                reasons.push(CoreCaldusIneligibilityReason::InvalidSession);
            }
            if entry.anti_cheat_state != CoreCaldusAntiCheatState::Valid {
                reasons.push(CoreCaldusIneligibilityReason::InvalidAntiCheat);
            }
            Ok(CoreCaldusEligibilityDecision {
                participant: entry.participant,
                contribution_centi_units,
                eligible: reasons.is_empty(),
                reasons,
            })
        })
        .collect()
}

fn validate_identity_input(
    instance_lineage_id: [u8; ID_BYTES],
    lock: &CoreBossParticipantLock,
) -> Result<(), CoreCaldusVictoryError> {
    if instance_lineage_id == [0; ID_BYTES] {
        return Err(CoreCaldusVictoryError::ZeroLineageId);
    }
    if lock.attempt_ordinal == 0 || lock.participants.is_empty() || lock.maximum_health == 0 {
        return Err(CoreCaldusVictoryError::InvalidParticipantLock);
    }
    validate_participant_order(&lock.participants)
}

fn validate_eligibility_input(
    lock: &CoreBossParticipantLock,
    active_duration_ticks: u32,
    evidence: &[CoreCaldusEligibilityEvidence],
) -> Result<(), CoreCaldusVictoryError> {
    if active_duration_ticks == 0 || lock.maximum_health == 0 {
        return Err(CoreCaldusVictoryError::InvalidActiveDuration);
    }
    validate_participant_order(&lock.participants)?;
    if evidence.len() != lock.participants.len() {
        return Err(CoreCaldusVictoryError::IncompleteEligibilityEvidence);
    }
    for (entry, participant) in evidence.iter().zip(&lock.participants) {
        if entry.participant != *participant {
            return Err(CoreCaldusVictoryError::UnstableEligibilityOrder);
        }
        if entry.presence_ticks > active_duration_ticks {
            return Err(CoreCaldusVictoryError::PresenceExceedsDuration);
        }
        if entry.objective_credits > CALDUS_MAX_OBJECTIVE_CREDITS {
            return Err(CoreCaldusVictoryError::TooManyObjectiveCredits);
        }
    }
    Ok(())
}

fn validate_participant_order(
    participants: &[CoreBossParticipant],
) -> Result<(), CoreCaldusVictoryError> {
    let mut entities = BTreeSet::new();
    if !participants.windows(2).all(|pair| {
        (pair[0].party_slot, pair[0].entity_id) < (pair[1].party_slot, pair[1].entity_id)
    }) || participants
        .iter()
        .any(|participant| !entities.insert(participant.entity_id))
    {
        return Err(CoreCaldusVictoryError::InvalidParticipantOrder);
    }
    Ok(())
}

fn derive_id(
    domain: &[u8],
    fields: &[&[u8]],
) -> Result<CoreCaldusStableId, CoreCaldusVictoryError> {
    let mut hasher = blake3::Hasher::new();
    update_field(&mut hasher, domain)?;
    for field in fields {
        update_field(&mut hasher, field)?;
    }
    let mut bytes = [0; ID_BYTES];
    bytes.copy_from_slice(&hasher.finalize().as_bytes()[..ID_BYTES]);
    if bytes == [0; ID_BYTES] {
        return Err(CoreCaldusVictoryError::DerivedZeroId);
    }
    Ok(CoreCaldusStableId(bytes))
}

fn update_field(hasher: &mut blake3::Hasher, value: &[u8]) -> Result<(), CoreCaldusVictoryError> {
    let length = u32::try_from(value.len()).map_err(|_| CoreCaldusVictoryError::FieldTooLong)?;
    hasher.update(&length.to_le_bytes());
    hasher.update(value);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreCaldusVictoryError {
    #[error("Caldus victory identity requires a nonzero instance lineage")]
    ZeroLineageId,
    #[error("Caldus victory identity requires a valid immutable participant lock")]
    InvalidParticipantLock,
    #[error("Caldus participants must be uniquely ordered by immutable slot and entity")]
    InvalidParticipantOrder,
    #[error("Caldus active duration must be nonzero with valid reference health")]
    InvalidActiveDuration,
    #[error("Caldus eligibility evidence must cover every locked participant")]
    IncompleteEligibilityEvidence,
    #[error("Caldus eligibility evidence order differs from the immutable lock")]
    UnstableEligibilityOrder,
    #[error("Caldus presence cannot exceed active encounter duration")]
    PresenceExceedsDuration,
    #[error("Caldus objective contribution is capped at two credits")]
    TooManyObjectiveCredits,
    #[error("Caldus victory identity field exceeds canonical encoding capacity")]
    FieldTooLong,
    #[error("Caldus victory identity derivation produced the reserved zero value")]
    DerivedZeroId,
    #[error("Caldus victory arithmetic overflowed")]
    ArithmeticOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntityId;

    fn participant(entity: u64, slot: u8) -> CoreBossParticipant {
        CoreBossParticipant {
            entity_id: EntityId::new(entity).expect("entity"),
            party_slot: slot,
        }
    }

    fn lock(attempt_ordinal: u32) -> CoreBossParticipantLock {
        CoreBossParticipantLock {
            attempt_ordinal,
            participants: vec![participant(10, 0), participant(20, 1)],
            maximum_health: 12_384,
        }
    }

    fn evidence(participant: CoreBossParticipant) -> CoreCaldusEligibilityEvidence {
        CoreCaldusEligibilityEvidence {
            participant,
            presence_ticks: 3_000,
            direct_damage: 62,
            effective_healing_to_others: 0,
            damage_prevented_on_others: 0,
            objective_credits: 0,
            longest_inactivity_ticks: 600,
            defeat_presence: CoreCaldusDefeatPresence::AliveAndPresent,
            recall_state: CoreCaldusRecallState::Stayed,
            session_state: CoreCaldusSessionState::Valid,
            anti_cheat_state: CoreCaldusAntiCheatState::Valid,
        }
    }

    #[test]
    fn identity_retries_are_stable_domains_are_disjoint_and_attempts_never_reuse() {
        let first = CoreCaldusVictoryIdentities::derive([7; 16], &lock(1)).expect("first");
        let replay = CoreCaldusVictoryIdentities::derive([7; 16], &lock(1)).expect("replay");
        let retry = CoreCaldusVictoryIdentities::derive([7; 16], &lock(2)).expect("retry");
        assert_eq!(first, replay);
        let first_ids = [
            first.encounter_id,
            first.exit_instance_id,
            first.personal_rewards[0].request_id,
            first.personal_rewards[1].request_id,
        ];
        assert_eq!(first_ids.iter().copied().collect::<BTreeSet<_>>().len(), 4);
        assert!(first_ids.iter().all(|id| {
            !retry
                .personal_rewards
                .iter()
                .any(|reward| reward.request_id == *id)
        }));
        assert_ne!(first.encounter_id, retry.encounter_id);
        assert_ne!(first.exit_instance_id, retry.exit_instance_id);
    }

    #[test]
    fn soc_010_exact_boundaries_qualify_and_short_fight_only_waives_presence() {
        let lock = lock(1);
        let entries = [evidence(participant(10, 0)), evidence(participant(20, 1))];
        let decisions = evaluate_caldus_eligibility(&lock, 6_000, &entries).expect("eligible");
        assert!(decisions.iter().all(|decision| decision.eligible));
        assert_eq!(decisions[0].contribution_centi_units, 6_200);

        let mut short = entries;
        short[0].presence_ticks = 0;
        short[0].direct_damage = 0;
        short[1].presence_ticks = 599;
        let decisions = evaluate_caldus_eligibility(&lock, 599, &short).expect("short");
        assert_eq!(
            decisions[0].reasons,
            [CoreCaldusIneligibilityReason::Contribution]
        );
    }

    #[test]
    fn every_soc_010_failure_is_explicit_and_input_errors_roll_back_purely() {
        let lock = lock(1);
        let mut first = evidence(participant(10, 0));
        first.presence_ticks = 2_999;
        first.direct_damage = 61;
        first.longest_inactivity_ticks = 601;
        first.defeat_presence = CoreCaldusDefeatPresence::NotAliveOrAbsent;
        first.recall_state = CoreCaldusRecallState::RecalledBeforeDefeat;
        first.session_state = CoreCaldusSessionState::Invalid;
        first.anti_cheat_state = CoreCaldusAntiCheatState::Invalid;
        let decisions =
            evaluate_caldus_eligibility(&lock, 6_000, &[first, evidence(participant(20, 1))])
                .expect("decisions");
        assert_eq!(
            decisions[0].reasons,
            [
                CoreCaldusIneligibilityReason::Presence,
                CoreCaldusIneligibilityReason::Contribution,
                CoreCaldusIneligibilityReason::Inactivity,
                CoreCaldusIneligibilityReason::NotAliveAndPresent,
                CoreCaldusIneligibilityReason::Recalled,
                CoreCaldusIneligibilityReason::InvalidSession,
                CoreCaldusIneligibilityReason::InvalidAntiCheat,
            ]
        );
        let mut invalid = [evidence(participant(10, 0)), evidence(participant(20, 1))];
        invalid[0].objective_credits = 3;
        assert_eq!(
            evaluate_caldus_eligibility(&lock, 6_000, &invalid).expect_err("objective cap"),
            CoreCaldusVictoryError::TooManyObjectiveCredits
        );
    }

    #[test]
    fn authored_xp_values_are_exact() {
        assert_eq!(CALDUS_BOSS_XP, 450);
        assert_eq!(CALDUS_FIRST_CLEAR_XP, 225);
    }
}
