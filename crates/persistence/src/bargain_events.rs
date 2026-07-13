//! Canonical transactional-outbox payloads for the Core Veil Bargain lifecycle.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    PersistenceError, StoredBargainCandidate, StoredBargainDecisionResult, StoredBargainOffer,
};

const MAX_EVENT_PAYLOAD_BYTES: usize = 65_536;
pub const BARGAIN_OFFER_EVENT_SCHEMA_VERSION: u16 = 1;
pub const BARGAIN_DECLINED_EVENT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainEventCandidateV1 {
    pub candidate_ordinal: i16,
    pub bargain_id: String,
    pub score: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainOfferedEventV1 {
    pub schema_version: u16,
    pub offer_id: [u8; 16],
    pub source_reward_event_id: [u8; 16],
    pub source_content_id: String,
    pub source_layout_id: String,
    pub instance_lineage_id: [u8; 16],
    pub entry_restore_point_id: [u8; 16],
    pub content_version: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
    pub oath_bargain_version: i64,
    pub candidates: Vec<BargainEventCandidateV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainDeclinedEventV1 {
    pub schema_version: u16,
    pub mutation_id: [u8; 16],
    pub offer_id: [u8; 16],
    pub oath_bargain_version: i64,
    pub source_content_id: String,
    pub source_layout_id: String,
    pub instance_lineage_id: [u8; 16],
    pub entry_restore_point_id: [u8; 16],
    pub content_version: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
    pub candidates: Vec<BargainEventCandidateV1>,
}

impl BargainOfferedEventV1 {
    pub fn decode(bytes: &[u8]) -> Result<Self, PersistenceError> {
        let event = decode_canonical(bytes, BARGAIN_OFFER_EVENT_SCHEMA_VERSION, |event: &Self| {
            event.schema_version
        })?;
        event.validate()?;
        Ok(event)
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        validate_common(
            self.offer_id,
            self.source_content_id.as_str(),
            self.source_layout_id.as_str(),
            self.instance_lineage_id,
            self.entry_restore_point_id,
            self.content_version.as_str(),
            [
                &self.records_blake3,
                &self.assets_blake3,
                &self.localization_blake3,
            ],
            self.oath_bargain_version,
            &self.candidates,
        )?;
        if self.source_reward_event_id != self.offer_id {
            return Err(PersistenceError::CorruptStoredBargain);
        }
        Ok(())
    }
}

impl BargainDeclinedEventV1 {
    pub fn decode(bytes: &[u8]) -> Result<Self, PersistenceError> {
        let event = decode_canonical(
            bytes,
            BARGAIN_DECLINED_EVENT_SCHEMA_VERSION,
            |event: &Self| event.schema_version,
        )?;
        event.validate()?;
        Ok(event)
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        if self.mutation_id == [0; 16] {
            return Err(PersistenceError::CorruptStoredBargain);
        }
        validate_common(
            self.offer_id,
            self.source_content_id.as_str(),
            self.source_layout_id.as_str(),
            self.instance_lineage_id,
            self.entry_restore_point_id,
            self.content_version.as_str(),
            [
                &self.records_blake3,
                &self.assets_blake3,
                &self.localization_blake3,
            ],
            self.oath_bargain_version,
            &self.candidates,
        )
    }
}

pub(crate) fn encode_bargain_offered(
    offer: &StoredBargainOffer,
) -> Result<Vec<u8>, PersistenceError> {
    let event = BargainOfferedEventV1 {
        schema_version: BARGAIN_OFFER_EVENT_SCHEMA_VERSION,
        offer_id: offer.offer_id,
        source_reward_event_id: offer.source_reward_event_id,
        source_content_id: offer.source_content_id.clone(),
        source_layout_id: offer.source_layout_id.clone(),
        instance_lineage_id: offer.instance_lineage_id,
        entry_restore_point_id: offer.entry_restore_point_id,
        content_version: offer.content_version.clone(),
        records_blake3: offer.records_blake3.clone(),
        assets_blake3: offer.assets_blake3.clone(),
        localization_blake3: offer.localization_blake3.clone(),
        oath_bargain_version: offer.created_oath_bargain_version,
        candidates: event_candidates(&offer.candidates),
    };
    event.validate()?;
    encode_canonical(&event)
}

pub(crate) fn encode_bargain_declined(
    result: &StoredBargainDecisionResult,
    offer: &StoredBargainOffer,
) -> Result<Vec<u8>, PersistenceError> {
    let event = BargainDeclinedEventV1 {
        schema_version: BARGAIN_DECLINED_EVENT_SCHEMA_VERSION,
        mutation_id: result.mutation_id,
        offer_id: result.offer_id,
        oath_bargain_version: result.post_oath_bargain_version,
        source_content_id: offer.source_content_id.clone(),
        source_layout_id: offer.source_layout_id.clone(),
        instance_lineage_id: offer.instance_lineage_id,
        entry_restore_point_id: offer.entry_restore_point_id,
        content_version: offer.content_version.clone(),
        records_blake3: offer.records_blake3.clone(),
        assets_blake3: offer.assets_blake3.clone(),
        localization_blake3: offer.localization_blake3.clone(),
        candidates: event_candidates(&offer.candidates),
    };
    event.validate()?;
    encode_canonical(&event)
}

#[allow(clippy::too_many_arguments)] // Mirrors the exact shared event envelope projection.
fn validate_common(
    offer_id: [u8; 16],
    source_content_id: &str,
    source_layout_id: &str,
    instance_lineage_id: [u8; 16],
    entry_restore_point_id: [u8; 16],
    content_version: &str,
    revision_hashes: [&str; 3],
    oath_bargain_version: i64,
    candidates: &[BargainEventCandidateV1],
) -> Result<(), PersistenceError> {
    let revisions_valid = revision_hashes
        .into_iter()
        .all(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let mut ids = BTreeSet::new();
    let candidates_valid = !candidates.is_empty()
        && candidates.len() <= 3
        && candidates.iter().enumerate().all(|(index, candidate)| {
            candidate.candidate_ordinal == i16::try_from(index).unwrap_or(-1)
                && matches!(
                    candidate.bargain_id.as_str(),
                    "bargain.bell_debt" | "bargain.cinder_hunger" | "bargain.lantern_ash"
                )
                && candidate.score != [0; 32]
                && ids.insert(candidate.bargain_id.as_str())
        });
    if offer_id == [0; 16]
        || source_content_id != "miniboss.sepulcher_knight"
        || source_layout_id != "layout.core_private_life_01"
        || instance_lineage_id == [0; 16]
        || entry_restore_point_id == [0; 16]
        || content_version.is_empty()
        || content_version.len() > 96
        || !revisions_valid
        || oath_bargain_version < 1
        || !candidates_valid
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(())
}

fn event_candidates(candidates: &[StoredBargainCandidate]) -> Vec<BargainEventCandidateV1> {
    candidates
        .iter()
        .map(|candidate| BargainEventCandidateV1 {
            candidate_ordinal: candidate.candidate_ordinal,
            bargain_id: candidate.bargain_id.clone(),
            score: candidate.score,
        })
        .collect()
}

fn encode_canonical<T: Serialize>(event: &T) -> Result<Vec<u8>, PersistenceError> {
    let bytes = postcard::to_stdvec(event).map_err(|_| PersistenceError::CorruptStoredBargain)?;
    if bytes.is_empty() || bytes.len() > MAX_EVENT_PAYLOAD_BYTES {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(bytes)
}

fn decode_canonical<T>(
    bytes: &[u8],
    expected_schema_version: u16,
    schema_version: impl FnOnce(&T) -> u16,
) -> Result<T, PersistenceError>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    if bytes.is_empty() || bytes.len() > MAX_EVENT_PAYLOAD_BYTES {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    let event = postcard::from_bytes(bytes).map_err(|_| PersistenceError::CorruptStoredBargain)?;
    if schema_version(&event) != expected_schema_version
        || postcard::to_stdvec(&event).map_err(|_| PersistenceError::CorruptStoredBargain)? != bytes
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_payloads_are_versioned_and_canonical() {
        let offered = BargainOfferedEventV1 {
            schema_version: BARGAIN_OFFER_EVENT_SCHEMA_VERSION,
            offer_id: [1; 16],
            source_reward_event_id: [1; 16],
            source_content_id: "miniboss.sepulcher_knight".into(),
            source_layout_id: "layout.core_private_life_01".into(),
            instance_lineage_id: [3; 16],
            entry_restore_point_id: [4; 16],
            content_version: "core-dev".into(),
            records_blake3: "a".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
            oath_bargain_version: 2,
            candidates: vec![BargainEventCandidateV1 {
                candidate_ordinal: 0,
                bargain_id: "bargain.bell_debt".into(),
                score: [5; 32],
            }],
        };
        let bytes = postcard::to_stdvec(&offered).unwrap();
        assert_eq!(BargainOfferedEventV1::decode(&bytes).unwrap(), offered);

        let mut wrong_version = offered;
        wrong_version.schema_version += 1;
        let bytes = postcard::to_stdvec(&wrong_version).unwrap();
        assert!(matches!(
            BargainOfferedEventV1::decode(&bytes),
            Err(PersistenceError::CorruptStoredBargain)
        ));
    }
}
