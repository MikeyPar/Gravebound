//! Server-owned planning for the temporary Core Sepulcher Knight Bargain milestone.

use persistence::{
    AshMutationKind, AshMutationRequest, CORE_BARGAIN_LAYOUT_ID, CORE_BARGAIN_MILESTONE_ID,
    CORE_BARGAIN_SOURCE_ID, PersistenceError, ProgressionAwardTransactionState,
    StagedBargainMilestone, StoredBargainCandidate, StoredBargainMilestoneResult,
    StoredBargainOffer,
};
use serde::{Deserialize, Serialize};
use sim_core::plan_bargain_offer;

use crate::{ProgressionAwardCode, ProgressionAwardCommand, ProgressionAwardPlan};

const OFFER_CREATED: i16 = 0;
const OFFER_UNAVAILABLE_WITH_ASH: i16 = 1;
const NO_SLOT_ASH: i16 = 2;
const OPEN_OFFER_STATE: i16 = 0;
const UNAVAILABLE_OFFER_STATE: i16 = 3;
const FALLBACK_ASH: i32 = 10;
const FALLBACK_REASON: &str = "bargain_milestone_fallback";

#[derive(Debug, Clone)]
pub(crate) struct CoreBargainMilestonePlanner {
    content_version: String,
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
    enabled_bargain_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CoreBargainMilestoneReceipt {
    result_code: i16,
    offer_id: Option<[u8; 16]>,
    candidate_ids: Vec<String>,
    ash_awarded: i32,
    earned_bargain_slots: i16,
    oath_bargain_version: i64,
}

impl CoreBargainMilestonePlanner {
    pub(crate) fn new(
        content: &sim_content::CompiledOathBargainCatalog,
    ) -> Result<Self, PersistenceError> {
        if content.target_name() != "core-dev-oaths-bargains" {
            return Err(PersistenceError::CorruptStoredBargain);
        }
        let enabled_bargain_ids = content
            .bargains()
            .values()
            .filter(|record| record.header.enabled)
            .map(|record| record.header.id.as_str().to_owned())
            .collect::<Vec<_>>();
        if enabled_bargain_ids.is_empty() {
            return Err(PersistenceError::CorruptStoredBargain);
        }
        let hashes = content.hashes();
        Ok(Self {
            content_version: format!("core-dev.blake3.{}", hashes.manifest_blake3),
            records_blake3: hashes.records_blake3.clone(),
            assets_blake3: hashes.assets_blake3.clone(),
            localization_blake3: hashes.localization_blake3.clone(),
            enabled_bargain_ids,
        })
    }

    pub(crate) fn stage_if_qualifying(
        &self,
        state: &mut ProgressionAwardTransactionState,
        command: &ProgressionAwardCommand,
        pre_award_level: u16,
        plan: &ProgressionAwardPlan,
    ) -> Result<(), PersistenceError> {
        if !qualifies(
            plan.outcome.code,
            &command.payload.source_content_id,
            pre_award_level,
            state.location.location_kind,
            state.location.layout_id.as_deref(),
            state.bargain_life.core_milestone_awarded,
        ) {
            return Ok(());
        }
        let lineage_id = state
            .location
            .instance_lineage_id
            .ok_or(PersistenceError::CorruptStoredBargain)?;
        let restore_point_id = state
            .location
            .entry_restore_point_id
            .ok_or(PersistenceError::CorruptStoredBargain)?;
        state.new_bargain_milestone =
            Some(self.build_staged(state, command, lineage_id, restore_point_id)?);
        Ok(())
    }

    fn build_staged(
        &self,
        state: &ProgressionAwardTransactionState,
        command: &ProgressionAwardCommand,
        lineage_id: [u8; 16],
        restore_point_id: [u8; 16],
    ) -> Result<StagedBargainMilestone, PersistenceError> {
        let mut staged_life = state.bargain_life.clone();
        let slot_available = staged_life.earned_bargain_slots < 3;
        if slot_available {
            staged_life.earned_bargain_slots += 1;
            staged_life.oath_bargain_version += 1;
        }
        let active = staged_life
            .active_bargain_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let legal_ids = self
            .enabled_bargain_ids
            .iter()
            .map(String::as_str)
            .filter(|id| !active.contains(id))
            .collect::<Vec<_>>();
        let scored = if slot_available {
            plan_bargain_offer(
                command.reward_event_id,
                command.payload.character_id,
                &self.content_version,
                &legal_ids,
            )
            .map_err(|_| PersistenceError::CorruptStoredBargain)?
        } else {
            Vec::new()
        };
        let result_code = if !slot_available {
            NO_SLOT_ASH
        } else if scored.is_empty() {
            OFFER_UNAVAILABLE_WITH_ASH
        } else {
            OFFER_CREATED
        };
        let offer = slot_available.then(|| {
            self.offer(
                command,
                lineage_id,
                restore_point_id,
                staged_life.oath_bargain_version,
                &scored,
            )
        });
        let needs_ash = result_code != OFFER_CREATED;
        let account_id = state
            .new_result
            .as_ref()
            .ok_or(PersistenceError::ProgressionAwardResultRequired)?
            .account_id;
        let ash_request = needs_ash.then(|| AshMutationRequest {
            account_id,
            mutation_id: command.reward_event_id,
            payload_hash: command.payload_hash,
            expected_wallet_version: state.ash_wallet.wallet_version,
            kind: AshMutationKind::Earn,
            amount: FALLBACK_ASH,
            reason_code: FALLBACK_REASON.into(),
            source_id: CORE_BARGAIN_MILESTONE_ID.into(),
            content_version: self.content_version.clone(),
        });
        let receipt = CoreBargainMilestoneReceipt {
            result_code,
            offer_id: offer.as_ref().map(|value| value.offer_id),
            candidate_ids: scored
                .iter()
                .map(|value| value.bargain_id.clone())
                .collect(),
            ash_awarded: if needs_ash { FALLBACK_ASH } else { 0 },
            earned_bargain_slots: staged_life.earned_bargain_slots,
            oath_bargain_version: staged_life.oath_bargain_version,
        };
        Ok(StagedBargainMilestone {
            life: staged_life.clone(),
            result: StoredBargainMilestoneResult {
                account_id,
                character_id: command.payload.character_id,
                source_reward_event_id: command.reward_event_id,
                payload_hash: command.payload_hash,
                result_code,
                pre_oath_bargain_version: state.bargain_life.oath_bargain_version,
                post_oath_bargain_version: staged_life.oath_bargain_version,
                pre_earned_bargain_slots: state.bargain_life.earned_bargain_slots,
                post_earned_bargain_slots: staged_life.earned_bargain_slots,
                offer_id: offer.as_ref().map(|value| value.offer_id),
                ash_mutation_id: needs_ash.then_some(command.reward_event_id),
                milestone_id: CORE_BARGAIN_MILESTONE_ID.into(),
                source_content_id: CORE_BARGAIN_SOURCE_ID.into(),
                source_layout_id: CORE_BARGAIN_LAYOUT_ID.into(),
                instance_lineage_id: lineage_id,
                entry_restore_point_id: restore_point_id,
                result_payload: postcard::to_stdvec(&receipt)
                    .map_err(|_| PersistenceError::CorruptStoredBargain)?,
            },
            offer,
            ash_request,
        })
    }

    fn offer(
        &self,
        command: &ProgressionAwardCommand,
        lineage_id: [u8; 16],
        restore_point_id: [u8; 16],
        version: i64,
        scored: &[sim_core::ScoredBargainCandidate],
    ) -> StoredBargainOffer {
        let unavailable = scored.is_empty();
        StoredBargainOffer {
            offer_id: command.reward_event_id,
            source_reward_event_id: command.reward_event_id,
            source_content_id: CORE_BARGAIN_SOURCE_ID.into(),
            source_layout_id: CORE_BARGAIN_LAYOUT_ID.into(),
            instance_lineage_id: lineage_id,
            entry_restore_point_id: restore_point_id,
            content_version: self.content_version.clone(),
            records_blake3: self.records_blake3.clone(),
            assets_blake3: self.assets_blake3.clone(),
            localization_blake3: self.localization_blake3.clone(),
            offer_state: if unavailable {
                UNAVAILABLE_OFFER_STATE
            } else {
                OPEN_OFFER_STATE
            },
            selected_bargain_id: None,
            created_oath_bargain_version: version,
            resolved_oath_bargain_version: unavailable.then_some(version),
            candidates: scored
                .iter()
                .enumerate()
                .map(|(index, value)| StoredBargainCandidate {
                    candidate_ordinal: i16::try_from(index).unwrap_or_default(),
                    bargain_id: value.bargain_id.clone(),
                    score: value.score,
                })
                .collect(),
        }
    }
}

fn qualifies(
    code: ProgressionAwardCode,
    source_content_id: &str,
    pre_award_level: u16,
    location_kind: i16,
    layout_id: Option<&str>,
    already_awarded: bool,
) -> bool {
    code == ProgressionAwardCode::Accepted
        && source_content_id == CORE_BARGAIN_SOURCE_ID
        && pre_award_level >= 5
        && location_kind == 2
        && layout_id == Some(CORE_BARGAIN_LAYOUT_ID)
        && !already_awarded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_predicate_uses_pre_award_level_and_exact_source_layout_once() {
        let exact = || {
            qualifies(
                ProgressionAwardCode::Accepted,
                CORE_BARGAIN_SOURCE_ID,
                5,
                2,
                Some(CORE_BARGAIN_LAYOUT_ID),
                false,
            )
        };
        assert!(exact());
        assert!(!qualifies(
            ProgressionAwardCode::Accepted,
            CORE_BARGAIN_SOURCE_ID,
            4,
            2,
            Some(CORE_BARGAIN_LAYOUT_ID),
            false,
        ));
        assert!(!qualifies(
            ProgressionAwardCode::NotEligible,
            CORE_BARGAIN_SOURCE_ID,
            5,
            2,
            Some(CORE_BARGAIN_LAYOUT_ID),
            false,
        ));
        assert!(!qualifies(
            ProgressionAwardCode::Accepted,
            "boss.sir_caldus",
            5,
            2,
            Some(CORE_BARGAIN_LAYOUT_ID),
            false,
        ));
        assert!(!qualifies(
            ProgressionAwardCode::Accepted,
            CORE_BARGAIN_SOURCE_ID,
            5,
            1,
            Some(CORE_BARGAIN_LAYOUT_ID),
            false,
        ));
        assert!(!qualifies(
            ProgressionAwardCode::Accepted,
            CORE_BARGAIN_SOURCE_ID,
            5,
            2,
            Some("layout.core_private_life_02"),
            false,
        ));
        assert!(!qualifies(
            ProgressionAwardCode::Accepted,
            CORE_BARGAIN_SOURCE_ID,
            5,
            2,
            Some(CORE_BARGAIN_LAYOUT_ID),
            true,
        ));
    }
}
