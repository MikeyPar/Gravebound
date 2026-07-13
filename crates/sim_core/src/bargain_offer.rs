//! Deterministic, renderer-free Veil Bargain offer planning for `GB-M03-05D`.

use std::collections::BTreeSet;

use thiserror::Error;

pub const MAX_ACTIVE_BARGAINS: usize = 3;
pub const MAX_BARGAIN_OFFER_CANDIDATES: usize = 3;
pub const BARGAIN_CONTENT_ID_MAX_BYTES: usize = 96;
const OFFER_SCORE_DOMAIN: &[u8] = b"bargain-offer-v1\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoredBargainCandidate {
    pub bargain_id: String,
    pub score: [u8; 32],
}

/// Scores already-legal candidates using the byte-exact `CONT-014` contract and returns the
/// first three in canonical order. Eligibility remains content-owned; this function owns only
/// cross-platform identity, scoring, ordering, and boundedness.
pub fn plan_bargain_offer(
    source_reward_event_id: [u8; 16],
    character_id: [u8; 16],
    content_version: &str,
    candidate_ids: &[&str],
) -> Result<Vec<ScoredBargainCandidate>, BargainOfferError> {
    if source_reward_event_id == [0; 16] {
        return Err(BargainOfferError::ZeroSourceRewardEventId);
    }
    if character_id == [0; 16] {
        return Err(BargainOfferError::ZeroCharacterId);
    }
    validate_field(content_version, BargainOfferError::InvalidContentVersion)?;

    let mut seen = BTreeSet::new();
    let mut candidates = Vec::with_capacity(candidate_ids.len());
    for candidate_id in candidate_ids {
        validate_field(candidate_id, BargainOfferError::InvalidCandidateId)?;
        if !seen.insert(*candidate_id) {
            return Err(BargainOfferError::DuplicateCandidateId);
        }
        candidates.push(ScoredBargainCandidate {
            bargain_id: (*candidate_id).to_owned(),
            score: bargain_offer_score(
                source_reward_event_id,
                character_id,
                content_version,
                candidate_id,
            )?,
        });
    }
    candidates.sort_unstable_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| left.bargain_id.as_bytes().cmp(right.bargain_id.as_bytes()))
    });
    candidates.truncate(MAX_BARGAIN_OFFER_CANDIDATES);
    Ok(candidates)
}

pub fn validate_bargain_life_state(
    earned_bargain_slots: u8,
    active_bargain_ids: &[&str],
) -> Result<(), BargainOfferError> {
    if usize::from(earned_bargain_slots) > MAX_ACTIVE_BARGAINS
        || active_bargain_ids.len() > usize::from(earned_bargain_slots)
    {
        return Err(BargainOfferError::InvalidLifeState);
    }
    let mut seen = BTreeSet::new();
    for bargain_id in active_bargain_ids {
        validate_field(bargain_id, BargainOfferError::InvalidCandidateId)?;
        if !seen.insert(*bargain_id) {
            return Err(BargainOfferError::DuplicateActiveBargain);
        }
    }
    Ok(())
}

fn bargain_offer_score(
    source_reward_event_id: [u8; 16],
    character_id: [u8; 16],
    content_version: &str,
    candidate_id: &str,
) -> Result<[u8; 32], BargainOfferError> {
    let version_length = u32::try_from(content_version.len())
        .map_err(|_| BargainOfferError::InvalidContentVersion)?;
    let candidate_length =
        u32::try_from(candidate_id.len()).map_err(|_| BargainOfferError::InvalidCandidateId)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(OFFER_SCORE_DOMAIN);
    hasher.update(&source_reward_event_id);
    hasher.update(&character_id);
    hasher.update(&version_length.to_le_bytes());
    hasher.update(content_version.as_bytes());
    hasher.update(&candidate_length.to_le_bytes());
    hasher.update(candidate_id.as_bytes());
    Ok(*hasher.finalize().as_bytes())
}

fn validate_field(value: &str, error: BargainOfferError) -> Result<(), BargainOfferError> {
    if value.is_empty() || value.len() > BARGAIN_CONTENT_ID_MAX_BYTES {
        return Err(error);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BargainOfferError {
    #[error("source reward event ID must be nonzero")]
    ZeroSourceRewardEventId,
    #[error("character ID must be nonzero")]
    ZeroCharacterId,
    #[error("Bargain offer content version is invalid")]
    InvalidContentVersion,
    #[error("Bargain candidate ID is invalid")]
    InvalidCandidateId,
    #[error("Bargain candidate IDs must be unique")]
    DuplicateCandidateId,
    #[error("active Bargain IDs must be unique")]
    DuplicateActiveBargain,
    #[error("earned slots and active Bargains are inconsistent")]
    InvalidLifeState,
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERSION: &str =
        "core-dev.blake3.91b1a3ab2157371d18dc2c8f48124b4f25c59cc6cf0be46b871274357b285533";
    const CORE_BARGAINS: [&str; 3] = [
        "bargain.bell_debt",
        "bargain.cinder_hunger",
        "bargain.lantern_ash",
    ];

    #[test]
    fn exact_core_offer_is_input_order_independent_and_bounded() {
        let first = plan_bargain_offer([1; 16], [2; 16], VERSION, &CORE_BARGAINS).unwrap();
        let reversed = plan_bargain_offer(
            [1; 16],
            [2; 16],
            VERSION,
            &[CORE_BARGAINS[2], CORE_BARGAINS[1], CORE_BARGAINS[0]],
        )
        .unwrap();
        assert_eq!(first, reversed);
        assert_eq!(first.len(), MAX_BARGAIN_OFFER_CANDIDATES);
        assert_eq!(
            first
                .iter()
                .map(|candidate| candidate.bargain_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "bargain.cinder_hunger",
                "bargain.lantern_ash",
                "bargain.bell_debt",
            ]
        );
        assert_eq!(
            first
                .iter()
                .map(|candidate| blake3::Hash::from_bytes(candidate.score)
                    .to_hex()
                    .to_string())
                .collect::<Vec<_>>(),
            vec![
                "9dddcfc67afc8c98c3ce6cb59f8494ddeca0b09c5c9d02848b168af204a1a8f1",
                "b7b593a24f1e72251e5772528da76cbf5840c6e5e44973cf7c0db21a5fc978b0",
                "cdf253848e754df1a269332a555d69193cf1f96082285fd97e08ab55bec20a99",
            ]
        );
        assert_ne!(
            first,
            plan_bargain_offer([1; 16], [3; 16], VERSION, &CORE_BARGAINS).unwrap()
        );
    }

    #[test]
    fn identity_fields_duplicates_and_life_bounds_fail_closed() {
        assert_eq!(
            plan_bargain_offer([0; 16], [2; 16], VERSION, &CORE_BARGAINS),
            Err(BargainOfferError::ZeroSourceRewardEventId)
        );
        assert_eq!(
            plan_bargain_offer([1; 16], [0; 16], VERSION, &CORE_BARGAINS),
            Err(BargainOfferError::ZeroCharacterId)
        );
        assert_eq!(
            plan_bargain_offer([1; 16], [2; 16], VERSION, &[CORE_BARGAINS[0]; 2]),
            Err(BargainOfferError::DuplicateCandidateId)
        );
        assert_eq!(validate_bargain_life_state(0, &[]), Ok(()));
        assert_eq!(validate_bargain_life_state(1, &[CORE_BARGAINS[0]]), Ok(()));
        assert_eq!(
            validate_bargain_life_state(1, &[CORE_BARGAINS[0], CORE_BARGAINS[1]]),
            Err(BargainOfferError::InvalidLifeState)
        );
        assert_eq!(
            validate_bargain_life_state(3, &[CORE_BARGAINS[0], CORE_BARGAINS[0]]),
            Err(BargainOfferError::DuplicateActiveBargain)
        );
    }
}
