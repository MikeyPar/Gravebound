//! Opaque defeat-to-reward binding for the route-bound Core Sir Caldus owner.
//!
//! The canonical GDD `DNG-006`, `SOC-010`, `LOOT-002`, `TECH-015`, and `TECH-021`, the Content
//! Production Specification `CONT-BOSS-001`/`002` and `CONT-REWARD-003`, and roadmap
//! `GB-M03-03` require immutable defeat evidence, durable personal reward terminality, and a
//! reward-gated stable exit. This module carries that evidence without becoming a persistence
//! writer or exposing client-authored eligibility.

use persistence::StoredCaldusVictoryExit;
use sim_core::{
    CoreBossConnectionState, CoreBossParticipant, CoreBossParticipantLock,
    CoreCaldusAntiCheatState, CoreCaldusDefeatPresence, CoreCaldusEligibilityEvidence,
    CoreCaldusRecallState, CoreCaldusSessionState, CoreCaldusVictoryIdentities, Tick,
    evaluate_caldus_eligibility,
};
use thiserror::Error;

use crate::{
    CaldusExitPresentation, CaldusVictoryCommitResult, CorePrivateCaldusRuntimeInput,
    CorePrivateRouteActorLease,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateCaldusDefeatHandoff {
    pub(crate) route_lease: CorePrivateRouteActorLease,
    pub(crate) route_state_version: u64,
    pub(crate) instance_lineage_id: [u8; 16],
    pub(crate) lock: CoreBossParticipantLock,
    pub(crate) active_duration_ticks: u32,
    pub(crate) defeat_tick: Tick,
    pub(crate) character_id: [u8; 16],
    pub(crate) expected_progression_version: u64,
    pub(crate) eligibility: Vec<CoreCaldusEligibilityEvidence>,
}

impl CorePrivateCaldusDefeatHandoff {
    #[must_use]
    pub const fn route_lease(&self) -> CorePrivateRouteActorLease {
        self.route_lease
    }

    #[must_use]
    pub const fn route_state_version(&self) -> u64 {
        self.route_state_version
    }

    #[must_use]
    pub const fn instance_lineage_id(&self) -> [u8; 16] {
        self.instance_lineage_id
    }

    #[must_use]
    pub const fn lock(&self) -> &CoreBossParticipantLock {
        &self.lock
    }

    #[must_use]
    pub const fn active_duration_ticks(&self) -> u32 {
        self.active_duration_ticks
    }

    #[must_use]
    pub const fn defeat_tick(&self) -> Tick {
        self.defeat_tick
    }

    #[must_use]
    pub const fn character_id(&self) -> [u8; 16] {
        self.character_id
    }

    #[must_use]
    pub const fn expected_progression_version(&self) -> u64 {
        self.expected_progression_version
    }

    #[must_use]
    pub fn eligibility(&self) -> &[CoreCaldusEligibilityEvidence] {
        &self.eligibility
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDurableCaldusResolution {
    handoff: CorePrivateCaldusDefeatHandoff,
    exit: StoredCaldusVictoryExit,
}

impl CoreDurableCaldusResolution {
    pub fn from_commit(
        handoff: CorePrivateCaldusDefeatHandoff,
        committed: &CaldusVictoryCommitResult,
    ) -> Result<Self, CorePrivateCaldusRewardError> {
        let identities =
            CoreCaldusVictoryIdentities::derive(handoff.instance_lineage_id, &handoff.lock)?;
        let eligibility = evaluate_caldus_eligibility(
            &handoff.lock,
            handoff.active_duration_ticks,
            &handoff.eligibility,
        )?;
        if committed.identities != identities
            || committed.eligibility != eligibility
            || committed
                .owners
                .iter()
                .map(|owner| owner.participant)
                .ne(eligibility
                    .iter()
                    .filter(|decision| decision.eligible)
                    .map(|decision| decision.participant))
        {
            return Err(CorePrivateCaldusRewardError::DurableBindingMismatch);
        }
        Self::new(handoff, committed.exit.clone())
    }

    fn new(
        handoff: CorePrivateCaldusDefeatHandoff,
        mut exit: StoredCaldusVictoryExit,
    ) -> Result<Self, CorePrivateCaldusRewardError> {
        let identities =
            CoreCaldusVictoryIdentities::derive(handoff.instance_lineage_id, &handoff.lock)?;
        if exit.encounter_id != identities.encounter_id.bytes()
            || exit.instance_lineage_id != handoff.instance_lineage_id
            || exit.attempt_ordinal != handoff.lock.attempt_ordinal
            || exit.exit_instance_id != identities.exit_instance_id.bytes()
            || exit.owners.is_empty()
        {
            return Err(CorePrivateCaldusRewardError::DurableBindingMismatch);
        }
        let decisions = evaluate_caldus_eligibility(
            &handoff.lock,
            handoff.active_duration_ticks,
            &handoff.eligibility,
        )?;
        let eligible = decisions
            .iter()
            .filter(|decision| decision.eligible)
            .collect::<Vec<_>>();
        if exit.owners.len() != eligible.len()
            || exit.owners.iter().zip(eligible).any(|(owner, decision)| {
                owner.party_slot != decision.participant.party_slot
                    || owner.participant_entity_id != decision.participant.entity_id.get()
                    || owner.account_id != handoff.route_lease.account_id()
                    || owner.character_id != handoff.character_id
                    || owner.reward_request_id
                        != identities
                            .reward_for(decision.participant)
                            .map_or([0; 16], sim_core::CoreCaldusStableId::bytes)
            })
        {
            return Err(CorePrivateCaldusRewardError::DurableBindingMismatch);
        }
        // `replayed` describes this read, not durable outcome material. Normalize it so a fresh
        // response and a response-loss replay compare as the same terminal result.
        exit.replayed = false;
        Ok(Self { handoff, exit })
    }

    #[cfg(test)]
    pub(crate) fn from_stored_for_test(
        handoff: CorePrivateCaldusDefeatHandoff,
        exit: StoredCaldusVictoryExit,
    ) -> Result<Self, CorePrivateCaldusRewardError> {
        Self::new(handoff, exit)
    }

    #[must_use]
    pub const fn handoff(&self) -> &CorePrivateCaldusDefeatHandoff {
        &self.handoff
    }

    #[must_use]
    pub const fn exit(&self) -> &StoredCaldusVictoryExit {
        &self.exit
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateCaldusRewardCommitDisposition {
    Committed,
    Replayed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateCaldusRewardCommit {
    pub route: protocol::CorePrivateRouteStateV1,
    pub exit: CaldusExitPresentation,
    pub disposition: CorePrivateCaldusRewardCommitDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoreCaldusRewardTracker {
    combat_started_at: Option<Tick>,
    presence_ticks: u32,
    current_inactivity_ticks: u32,
    longest_inactivity_ticks: u32,
    last_activity_sequence: u64,
    session_valid: bool,
    anti_cheat_valid: bool,
    present_at_latest_tick: bool,
}

impl CoreCaldusRewardTracker {
    #[must_use]
    pub const fn new(last_activity_sequence: u64) -> Self {
        Self {
            combat_started_at: None,
            presence_ticks: 0,
            current_inactivity_ticks: 0,
            longest_inactivity_ticks: 0,
            last_activity_sequence,
            session_valid: true,
            anti_cheat_valid: true,
            present_at_latest_tick: false,
        }
    }

    pub fn reset_for_attempt(&mut self) {
        *self = Self::new(self.last_activity_sequence);
    }

    pub fn observe(
        &mut self,
        tick: Tick,
        input: &CorePrivateCaldusRuntimeInput,
        living: bool,
        combat_active: bool,
    ) -> Result<(), CorePrivateCaldusRewardError> {
        if input.action.reward_activity_sequence < self.last_activity_sequence {
            return Err(CorePrivateCaldusRewardError::ActivitySequenceRegressed);
        }
        if !combat_active {
            self.last_activity_sequence = input.action.reward_activity_sequence;
            return Ok(());
        }
        self.combat_started_at.get_or_insert(tick);
        let present = living
            && input.action.reward_session_active
            && input.connection != CoreBossConnectionState::Disconnected;
        self.present_at_latest_tick = present;
        self.session_valid &= input.action.reward_session_active;
        self.anti_cheat_valid &= input.action.reward_trust_valid;
        if present {
            self.presence_ticks = self
                .presence_ticks
                .checked_add(1)
                .ok_or(CorePrivateCaldusRewardError::ArithmeticOverflow)?;
            if input.action.reward_activity_sequence > self.last_activity_sequence {
                self.current_inactivity_ticks = 0;
            } else {
                self.current_inactivity_ticks = self
                    .current_inactivity_ticks
                    .checked_add(1)
                    .ok_or(CorePrivateCaldusRewardError::ArithmeticOverflow)?;
                self.longest_inactivity_ticks = self
                    .longest_inactivity_ticks
                    .max(self.current_inactivity_ticks);
            }
        } else {
            self.current_inactivity_ticks = 0;
        }
        self.last_activity_sequence = input.action.reward_activity_sequence;
        Ok(())
    }

    pub fn finish(
        &self,
        participant: CoreBossParticipant,
        defeat_tick: Tick,
        contribution_damage: u64,
        living: bool,
    ) -> Result<(u32, CoreCaldusEligibilityEvidence), CorePrivateCaldusRewardError> {
        let started_at = self
            .combat_started_at
            .ok_or(CorePrivateCaldusRewardError::CombatNotStarted)?;
        let duration = defeat_tick
            .0
            .checked_sub(started_at.0)
            .and_then(|elapsed| elapsed.checked_add(1))
            .and_then(|ticks| u32::try_from(ticks).ok())
            .ok_or(CorePrivateCaldusRewardError::ArithmeticOverflow)?;
        Ok((
            duration,
            CoreCaldusEligibilityEvidence {
                participant,
                presence_ticks: self.presence_ticks,
                direct_damage: contribution_damage,
                effective_healing_to_others: 0,
                damage_prevented_on_others: 0,
                objective_credits: 0,
                longest_inactivity_ticks: self.longest_inactivity_ticks,
                defeat_presence: if living && self.present_at_latest_tick {
                    CoreCaldusDefeatPresence::AliveAndPresent
                } else {
                    CoreCaldusDefeatPresence::NotAliveOrAbsent
                },
                // A completed Recall retires the sole danger owner before another B6 frame.
                recall_state: CoreCaldusRecallState::Stayed,
                session_state: if self.session_valid {
                    CoreCaldusSessionState::Valid
                } else {
                    CoreCaldusSessionState::Invalid
                },
                anti_cheat_state: if self.anti_cheat_valid {
                    CoreCaldusAntiCheatState::Valid
                } else {
                    CoreCaldusAntiCheatState::Invalid
                },
            },
        ))
    }
}

#[derive(Debug, Error)]
pub enum CorePrivateCaldusRewardError {
    #[error("Caldus reward activity sequence regressed")]
    ActivitySequenceRegressed,
    #[error("Caldus reward evidence was requested before combat started")]
    CombatNotStarted,
    #[error("Caldus reward evidence arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("durable Caldus reward result does not bind the frozen defeat")]
    DurableBindingMismatch,
    #[error(transparent)]
    Victory(#[from] sim_core::CoreCaldusVictoryError),
}

#[cfg(test)]
mod tests {
    use sim_core::{
        CoreBossConnectionState, CoreBossParticipant, EntityId, MovementAction,
        evaluate_caldus_eligibility,
    };

    use super::*;
    use crate::{CorePrivateCaldusRuntimeInput, CorePrivateMicrorealmInput};

    fn participant() -> CoreBossParticipant {
        CoreBossParticipant {
            entity_id: EntityId::new(71).expect("participant"),
            party_slot: 0,
        }
    }

    fn input(activity_sequence: u64) -> CorePrivateCaldusRuntimeInput {
        CorePrivateCaldusRuntimeInput {
            action: CorePrivateMicrorealmInput {
                input_sequence: activity_sequence,
                movement: MovementAction::default(),
                aim: sim_core::AimDirection::east(),
                primary_held: false,
                primary_sequence: 0,
                ability_1_sequence: 0,
                ability_2_sequence: 0,
                reward_session_active: true,
                reward_trust_valid: true,
                reward_activity_sequence: activity_sequence,
            },
            connection: CoreBossConnectionState::ConnectedLoaded,
        }
    }

    #[test]
    fn tracker_preserves_exact_inactivity_boundary_and_inclusive_duration() {
        let mut tracker = CoreCaldusRewardTracker::new(0);
        tracker
            .observe(Tick(10), &input(1), true, true)
            .expect("combat start");
        for tick in 11..=610 {
            tracker
                .observe(Tick(tick), &input(1), true, true)
                .expect("inactive tick");
        }
        let (duration, evidence) = tracker
            .finish(participant(), Tick(610), 7_200, true)
            .expect("evidence");
        assert_eq!(duration, 601);
        assert_eq!(evidence.presence_ticks, 601);
        assert_eq!(evidence.longest_inactivity_ticks, 600);
        let lock = CoreBossParticipantLock {
            participants: vec![participant()],
            attempt_ordinal: 1,
            maximum_health: 7_200,
        };
        assert!(
            evaluate_caldus_eligibility(&lock, duration, &[evidence]).expect("boundary decision")
                [0]
            .eligible
        );

        tracker
            .observe(Tick(611), &input(1), true, true)
            .expect("over-boundary tick");
        let (duration, evidence) = tracker
            .finish(participant(), Tick(611), 7_200, true)
            .expect("over-boundary evidence");
        assert_eq!(evidence.longest_inactivity_ticks, 601);
        assert!(
            !evaluate_caldus_eligibility(&lock, duration, &[evidence])
                .expect("over-boundary decision")[0]
                .eligible
        );
    }

    #[test]
    fn tracker_fails_closed_on_regression_and_resets_attempt_evidence() {
        let mut tracker = CoreCaldusRewardTracker::new(7);
        assert!(matches!(
            tracker.observe(Tick(1), &input(6), true, true),
            Err(CorePrivateCaldusRewardError::ActivitySequenceRegressed)
        ));
        tracker
            .observe(Tick(1), &input(8), true, true)
            .expect("first attempt");
        tracker.reset_for_attempt();
        assert!(matches!(
            tracker.finish(participant(), Tick(1), 7_200, true),
            Err(CorePrivateCaldusRewardError::CombatNotStarted)
        ));
        tracker
            .observe(Tick(2), &input(9), true, true)
            .expect("second attempt");
        let (duration, evidence) = tracker
            .finish(participant(), Tick(2), 7_200, true)
            .expect("second attempt evidence");
        assert_eq!(duration, 1);
        assert_eq!(evidence.presence_ticks, 1);
    }

    #[test]
    fn tracker_accumulates_session_and_trust_failure() {
        let mut tracker = CoreCaldusRewardTracker::new(0);
        tracker
            .observe(Tick(1), &input(1), true, true)
            .expect("valid tick");
        let mut invalid = input(2);
        invalid.action.reward_session_active = false;
        invalid.action.reward_trust_valid = false;
        tracker
            .observe(Tick(2), &invalid, true, true)
            .expect("invalid tick");
        tracker
            .observe(Tick(3), &input(3), true, true)
            .expect("later valid tick");
        let (_, evidence) = tracker
            .finish(participant(), Tick(3), 7_200, true)
            .expect("evidence");
        assert_eq!(evidence.session_state, CoreCaldusSessionState::Invalid);
        assert_eq!(evidence.anti_cheat_state, CoreCaldusAntiCheatState::Invalid);
    }
}
