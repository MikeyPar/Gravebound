//! Bounded reliable protocol for authoritative Veil Bargain shrine views and decisions.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ManifestHash, NetworkChannel, WireText};

pub const BARGAIN_ID_BYTES: usize = 96;
pub const BARGAIN_CHARACTER_ID_BYTES: usize = 16;
pub const BARGAIN_OFFER_ID_BYTES: usize = 16;
pub const BARGAIN_MUTATION_ID_BYTES: usize = 16;
pub const BARGAIN_PAYLOAD_HASH_BYTES: usize = 32;
pub const BELL_DEBT_ID: &str = "bargain.bell_debt";
pub const CINDER_HUNGER_ID: &str = "bargain.cinder_hunger";
pub const LANTERN_ASH_ID: &str = "bargain.lantern_ash";
const MAX_BARGAINS: usize = 3;
const MAX_STAT_BASIS_POINTS: u32 = 50_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainContentRevisionV1 {
    pub records_blake3: ManifestHash,
    pub assets_blake3: ManifestHash,
    pub localization_blake3: ManifestHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainViewFrame {
    pub sequence: u32,
    pub character_id: [u8; BARGAIN_CHARACTER_ID_BYTES],
    pub content_revision: BargainContentRevisionV1,
}

impl BargainViewFrame {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Control
    }

    pub const fn validate(&self) -> Result<(), BargainValidationError> {
        if self.sequence == 0 {
            return Err(BargainValidationError::ZeroSequence);
        }
        if all_zero(&self.character_id) {
            return Err(BargainValidationError::ZeroCharacterId);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BargainDecision {
    Select {
        bargain_id: WireText<BARGAIN_ID_BYTES>,
    },
    Refuse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainDecisionPayload {
    pub character_id: [u8; BARGAIN_CHARACTER_ID_BYTES],
    pub offer_id: [u8; BARGAIN_OFFER_ID_BYTES],
    pub decision: BargainDecision,
    pub content_revision: BargainContentRevisionV1,
    pub confirmed: bool,
}

impl BargainDecisionPayload {
    #[must_use]
    pub fn canonical_hash(&self) -> [u8; BARGAIN_PAYLOAD_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded Bargain payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    pub fn validate(&self) -> Result<(), BargainValidationError> {
        if all_zero(&self.character_id) {
            return Err(BargainValidationError::ZeroCharacterId);
        }
        if all_zero(&self.offer_id) {
            return Err(BargainValidationError::ZeroOfferId);
        }
        if matches!(
            &self.decision,
            BargainDecision::Select { bargain_id } if !legal_bargain_id(bargain_id.as_str())
        ) {
            return Err(BargainValidationError::IllegalBargainId);
        }
        if !self.confirmed {
            return Err(BargainValidationError::ConfirmationRequired);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainDecisionFrame {
    pub mutation_id: [u8; BARGAIN_MUTATION_ID_BYTES],
    pub expected_oath_bargain_version: u64,
    pub payload_hash: [u8; BARGAIN_PAYLOAD_HASH_BYTES],
    pub issued_at_unix_millis: u64,
    pub payload: BargainDecisionPayload,
}

impl BargainDecisionFrame {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Mutation
    }

    pub fn validate(&self) -> Result<(), BargainValidationError> {
        if all_zero(&self.mutation_id) {
            return Err(BargainValidationError::ZeroMutationId);
        }
        if self.expected_oath_bargain_version == 0 {
            return Err(BargainValidationError::ZeroLifeVersion);
        }
        if all_zero(&self.payload_hash) {
            return Err(BargainValidationError::ZeroPayloadHash);
        }
        if self.issued_at_unix_millis == 0 {
            return Err(BargainValidationError::ZeroIssuedAt);
        }
        self.payload.validate()?;
        if self.payload_hash != self.payload.canonical_hash() {
            return Err(BargainValidationError::PayloadHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainStatComparison {
    pub max_health_before_basis_points: u32,
    pub max_health_after_basis_points: u32,
    pub direct_damage_before_basis_points: u32,
    pub direct_damage_after_basis_points: u32,
    pub cooldown_before_basis_points: u32,
    pub cooldown_after_basis_points: u32,
    pub movement_before_basis_points: u32,
    pub movement_after_basis_points: u32,
    pub healing_before_basis_points: u32,
    pub healing_after_basis_points: u32,
    pub attack_rate_before_basis_points: u32,
    pub attack_rate_after_basis_points: u32,
    pub active_belt_slots_before: u8,
    pub active_belt_slots_after: u8,
}

impl BargainStatComparison {
    fn validate(self) -> Result<(), BargainValidationError> {
        let values = [
            self.max_health_before_basis_points,
            self.max_health_after_basis_points,
            self.direct_damage_before_basis_points,
            self.direct_damage_after_basis_points,
            self.cooldown_before_basis_points,
            self.cooldown_after_basis_points,
            self.movement_before_basis_points,
            self.movement_after_basis_points,
            self.healing_before_basis_points,
            self.healing_after_basis_points,
            self.attack_rate_before_basis_points,
            self.attack_rate_after_basis_points,
        ];
        if values
            .into_iter()
            .any(|value| value == 0 || value > MAX_STAT_BASIS_POINTS)
            || !(1..=2).contains(&self.active_belt_slots_before)
            || !(1..=2).contains(&self.active_belt_slots_after)
        {
            return Err(BargainValidationError::InvalidComparison);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BargainOfferCell {
    Available {
        bargain_id: WireText<BARGAIN_ID_BYTES>,
        comparison: BargainStatComparison,
    },
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BargainOfferState {
    Open,
    Selected {
        bargain_id: WireText<BARGAIN_ID_BYTES>,
    },
    Refused,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainOfferProjection {
    pub offer_id: [u8; BARGAIN_OFFER_ID_BYTES],
    pub state: BargainOfferState,
    pub cells: Vec<BargainOfferCell>,
}

impl BargainOfferProjection {
    fn validate(&self) -> Result<(), BargainValidationError> {
        if all_zero(&self.offer_id) || self.cells.len() != MAX_BARGAINS {
            return Err(BargainValidationError::InvalidOffer);
        }
        let mut available = BTreeSet::new();
        for cell in &self.cells {
            if let BargainOfferCell::Available {
                bargain_id,
                comparison,
            } = cell
            {
                if !legal_bargain_id(bargain_id.as_str()) || !available.insert(bargain_id.as_str())
                {
                    return Err(BargainValidationError::InvalidOffer);
                }
                comparison.validate()?;
            }
        }
        match &self.state {
            BargainOfferState::Open | BargainOfferState::Refused if !available.is_empty() => Ok(()),
            BargainOfferState::Selected { bargain_id }
                if available.contains(bargain_id.as_str()) =>
            {
                Ok(())
            }
            BargainOfferState::Unavailable if available.is_empty() => Ok(()),
            _ => Err(BargainValidationError::InvalidOffer),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainProjection {
    pub character_id: [u8; BARGAIN_CHARACTER_ID_BYTES],
    pub oath_bargain_version: u64,
    pub earned_bargain_slots: u8,
    pub active_bargain_ids: Vec<WireText<BARGAIN_ID_BYTES>>,
    pub offer: Option<BargainOfferProjection>,
}

impl BargainProjection {
    pub fn validate(&self) -> Result<(), BargainValidationError> {
        if all_zero(&self.character_id) {
            return Err(BargainValidationError::ZeroCharacterId);
        }
        if self.oath_bargain_version == 0
            || usize::from(self.earned_bargain_slots) > MAX_BARGAINS
            || self.active_bargain_ids.len() > usize::from(self.earned_bargain_slots)
        {
            return Err(BargainValidationError::InvalidProjection);
        }
        let mut active = BTreeSet::new();
        if self
            .active_bargain_ids
            .iter()
            .any(|id| !legal_bargain_id(id.as_str()) || !active.insert(id.as_str()))
        {
            return Err(BargainValidationError::InvalidProjection);
        }
        self.offer
            .as_ref()
            .map_or(Ok(()), BargainOfferProjection::validate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BargainResultCode {
    Available,
    Accepted,
    Refused,
    NoOffer,
    CharacterNotOwned,
    CharacterNotSelected,
    CharacterDead,
    LocationRequired,
    ContentMismatch,
    StateVersionMismatch,
    OfferResolved,
    CandidateUnavailable,
    IdempotencyConflict,
    PayloadHashMismatch,
    ConfirmationRequired,
    IssuedAtInvalid,
    StageDisabled,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainViewResult {
    pub sequence: u32,
    pub code: BargainResultCode,
    pub projection: Option<BargainProjection>,
}

impl BargainViewResult {
    pub fn validate(&self) -> Result<(), BargainValidationError> {
        if self.sequence == 0 {
            return Err(BargainValidationError::ZeroSequence);
        }
        let projects = matches!(
            self.code,
            BargainResultCode::Available | BargainResultCode::NoOffer
        );
        if projects != self.projection.is_some()
            || (self.code == BargainResultCode::NoOffer
                && self
                    .projection
                    .as_ref()
                    .is_some_and(|value| value.offer.is_some()))
        {
            return Err(BargainValidationError::ResultShapeMismatch);
        }
        self.projection
            .as_ref()
            .map_or(Ok(()), BargainProjection::validate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainDecisionResult {
    pub mutation_id: [u8; BARGAIN_MUTATION_ID_BYTES],
    pub code: BargainResultCode,
    pub projection: Option<BargainProjection>,
}

impl BargainDecisionResult {
    pub fn validate(&self) -> Result<(), BargainValidationError> {
        if all_zero(&self.mutation_id) {
            return Err(BargainValidationError::ZeroMutationId);
        }
        if matches!(
            self.code,
            BargainResultCode::Available | BargainResultCode::NoOffer
        ) {
            return Err(BargainValidationError::ResultShapeMismatch);
        }
        if matches!(
            self.code,
            BargainResultCode::Accepted | BargainResultCode::Refused
        ) && self.projection.is_none()
        {
            return Err(BargainValidationError::ResultShapeMismatch);
        }
        self.projection
            .as_ref()
            .map_or(Ok(()), BargainProjection::validate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BargainValidationError {
    #[error("Bargain message sequence must be nonzero")]
    ZeroSequence,
    #[error("Bargain character ID must be nonzero")]
    ZeroCharacterId,
    #[error("Bargain offer ID must be nonzero")]
    ZeroOfferId,
    #[error("Bargain mutation ID must be nonzero")]
    ZeroMutationId,
    #[error("Bargain life version must be nonzero")]
    ZeroLifeVersion,
    #[error("Bargain payload hash must be nonzero")]
    ZeroPayloadHash,
    #[error("Bargain payload hash does not match its canonical payload")]
    PayloadHashMismatch,
    #[error("Bargain mutation issue time must be nonzero")]
    ZeroIssuedAt,
    #[error("Bargain ID is unavailable in Core")]
    IllegalBargainId,
    #[error("explicit Bargain decision confirmation is required")]
    ConfirmationRequired,
    #[error("Bargain stat comparison is invalid")]
    InvalidComparison,
    #[error("Bargain offer projection is invalid")]
    InvalidOffer,
    #[error("Bargain life projection is invalid")]
    InvalidProjection,
    #[error("Bargain result code and projection disagree")]
    ResultShapeMismatch,
}

fn legal_bargain_id(value: &str) -> bool {
    matches!(value, BELL_DEBT_ID | CINDER_HUNGER_ID | LANTERN_ASH_ID)
}

const fn all_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    let mut index = 0;
    while index < N {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revision() -> BargainContentRevisionV1 {
        BargainContentRevisionV1 {
            records_blake3: ManifestHash::new("1".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("2".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("3".repeat(64)).unwrap(),
        }
    }

    fn comparison() -> BargainStatComparison {
        BargainStatComparison {
            max_health_before_basis_points: 10_000,
            max_health_after_basis_points: 8_800,
            direct_damage_before_basis_points: 10_000,
            direct_damage_after_basis_points: 11_800,
            cooldown_before_basis_points: 10_000,
            cooldown_after_basis_points: 10_000,
            movement_before_basis_points: 10_000,
            movement_after_basis_points: 10_000,
            healing_before_basis_points: 10_000,
            healing_after_basis_points: 10_000,
            attack_rate_before_basis_points: 10_000,
            attack_rate_after_basis_points: 10_000,
            active_belt_slots_before: 2,
            active_belt_slots_after: 2,
        }
    }

    #[test]
    fn decision_is_content_bound_confirmed_and_mutation_reliable() {
        let payload = BargainDecisionPayload {
            character_id: [1; 16],
            offer_id: [2; 16],
            decision: BargainDecision::Select {
                bargain_id: WireText::new(CINDER_HUNGER_ID).unwrap(),
            },
            content_revision: revision(),
            confirmed: true,
        };
        let frame = BargainDecisionFrame {
            mutation_id: [3; 16],
            expected_oath_bargain_version: 2,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: 1,
            payload,
        };
        assert_eq!(frame.validate(), Ok(()));
        assert_eq!(frame.channel(), NetworkChannel::Mutation);
        let mut tampered = frame;
        tampered.payload.decision = BargainDecision::Refuse;
        assert_eq!(
            tampered.validate(),
            Err(BargainValidationError::PayloadHashMismatch)
        );
    }

    #[test]
    fn emergency_offer_has_three_explicit_non_color_cells() {
        let offer = BargainOfferProjection {
            offer_id: [4; 16],
            state: BargainOfferState::Open,
            cells: vec![
                BargainOfferCell::Available {
                    bargain_id: WireText::new(BELL_DEBT_ID).unwrap(),
                    comparison: comparison(),
                },
                BargainOfferCell::Unavailable,
                BargainOfferCell::Unavailable,
            ],
        };
        assert_eq!(offer.validate(), Ok(()));
        let mut malformed = offer;
        malformed.cells.pop();
        assert_eq!(
            malformed.validate(),
            Err(BargainValidationError::InvalidOffer)
        );
    }
}
