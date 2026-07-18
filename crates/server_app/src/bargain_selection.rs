//! Authoritative Veil Bargain shrine projection and terminal decision service.

use persistence::{
    BargainDecisionTransaction, BargainDecisionTransactionState, CORE_BARGAIN_LAYOUT_ID,
    CORE_BARGAIN_MILESTONE_ID, CORE_BARGAIN_SOURCE_ID, PersistenceError, PostgresPersistence,
    StoredActiveBargain, StoredBargainDecisionResult, StoredBargainLife,
    StoredBargainMilestoneResult, StoredBargainOffer, StoredBargainRestBinding,
    StoredCharacterLifeEvent,
};
use protocol::{
    BELL_DEBT_ID, BargainContentRevisionV1, BargainDecision, BargainDecisionFrame,
    BargainDecisionResult, BargainOfferCell, BargainOfferProjection, BargainOfferState,
    BargainProjection, BargainResultCode, BargainStatComparison, BargainViewFrame,
    BargainViewResult, CINDER_HUNGER_ID, LANTERN_ASH_ID, ManifestHash, WireText,
};

use crate::{AuthenticatedAccount, AuthenticatedNamespace, IdentityClock};

const OPEN: i16 = 0;
const SELECTED: i16 = 1;
const REFUSED: i16 = 2;

/// Server-only durable proof that a B4 rest-room outcome already committed. Fields are private and
/// there is no public constructor: transport code can carry this value after an authority call,
/// but cannot manufacture account, character, lineage, offer, version, or outcome material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDurableBargainRestResolution {
    account_id: [u8; 16],
    character_id: [u8; 16],
    instance_lineage_id: [u8; 16],
    entry_restore_point_id: [u8; 16],
    source_receipt_id: [u8; 16],
    offer_id: Option<[u8; 16]>,
    oath_bargain_version: u64,
    resolution: sim_content::CoreFixedDungeonRestResolution,
}

impl CoreDurableBargainRestResolution {
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
    pub const fn entry_restore_point_id(&self) -> [u8; 16] {
        self.entry_restore_point_id
    }

    #[must_use]
    pub const fn source_receipt_id(&self) -> [u8; 16] {
        self.source_receipt_id
    }

    #[must_use]
    pub const fn offer_id(&self) -> Option<[u8; 16]> {
        self.offer_id
    }

    #[must_use]
    pub const fn oath_bargain_version(&self) -> u64 {
        self.oath_bargain_version
    }

    #[must_use]
    pub const fn resolution(&self) -> sim_content::CoreFixedDungeonRestResolution {
        self.resolution
    }

    /// Converts a milestone result already returned by the progression transaction into a B4
    /// proof. Callers must pass the stored result, never client-authored milestone material.
    pub fn from_no_offer_milestone(
        authenticated: AuthenticatedAccount,
        result: &StoredBargainMilestoneResult,
    ) -> Result<Self, PersistenceError> {
        let unavailable_offer = result.result_code == 1 && result.offer_id.is_some();
        let no_slot = result.result_code == 2 && result.offer_id.is_none();
        if result.account_id != authenticated.account_id.as_bytes()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || !(unavailable_offer || no_slot)
            || result.milestone_id != CORE_BARGAIN_MILESTONE_ID
            || result.source_content_id != CORE_BARGAIN_SOURCE_ID
            || result.source_layout_id != CORE_BARGAIN_LAYOUT_ID
            || result.post_oath_bargain_version <= 0
            || [
                result.character_id,
                result.source_reward_event_id,
                result.instance_lineage_id,
                result.entry_restore_point_id,
            ]
            .iter()
            .any(|value| value.iter().all(|byte| *byte == 0))
        {
            return Err(PersistenceError::CorruptStoredBargain);
        }
        Ok(Self {
            account_id: result.account_id,
            character_id: result.character_id,
            instance_lineage_id: result.instance_lineage_id,
            entry_restore_point_id: result.entry_restore_point_id,
            source_receipt_id: result.source_reward_event_id,
            offer_id: result.offer_id,
            oath_bargain_version: u64::try_from(result.post_oath_bargain_version)
                .map_err(|_| PersistenceError::CorruptStoredBargain)?,
            resolution: sim_content::CoreFixedDungeonRestResolution::NoOffer,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBargainDecisionAuthorityResult {
    pub response: BargainDecisionResult,
    pub rest_resolution: Option<CoreDurableBargainRestResolution>,
}

#[derive(Debug, Clone)]
pub enum CoreBargainAuthority<Clock> {
    Disabled,
    Persistent(PostgresBargainService<Clock>),
}

impl<Clock> CoreBargainAuthority<Clock>
where
    Clock: IdentityClock,
{
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    pub const fn persistent(service: PostgresBargainService<Clock>) -> Self {
        Self::Persistent(service)
    }

    pub async fn view(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &BargainViewFrame,
    ) -> BargainViewResult {
        match self {
            Self::Disabled => view_error(frame.sequence, BargainResultCode::ServiceUnavailable),
            Self::Persistent(service) => service.view(authenticated, frame).await,
        }
    }

    pub async fn decide(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &BargainDecisionFrame,
    ) -> BargainDecisionResult {
        match self {
            Self::Disabled => {
                decision_error(frame.mutation_id, BargainResultCode::ServiceUnavailable)
            }
            Self::Persistent(service) => service.decide(authenticated, frame).await,
        }
    }

    /// Returns the wire result plus an optional opaque B4 proof. Error results still return their
    /// exact wire response but can never advance the rest room.
    pub async fn decide_with_rest_resolution(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &BargainDecisionFrame,
    ) -> CoreBargainDecisionAuthorityResult {
        match self {
            Self::Disabled => CoreBargainDecisionAuthorityResult {
                response: decision_error(frame.mutation_id, BargainResultCode::ServiceUnavailable),
                rest_resolution: None,
            },
            Self::Persistent(service) => {
                service
                    .decide_with_rest_resolution(authenticated, frame)
                    .await
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PostgresBargainService<Clock> {
    persistence: PostgresPersistence,
    clock: Clock,
    content_revision: BargainContentRevisionV1,
}

impl<Clock> PostgresBargainService<Clock>
where
    Clock: IdentityClock,
{
    pub fn new(
        persistence: PostgresPersistence,
        clock: Clock,
        content: &sim_content::CompiledOathBargainCatalog,
    ) -> Result<Self, protocol::BoundedValueError> {
        let hashes = content.hashes();
        Ok(Self {
            persistence,
            clock,
            content_revision: BargainContentRevisionV1 {
                records_blake3: ManifestHash::new(hashes.records_blake3.clone())?,
                assets_blake3: ManifestHash::new(hashes.assets_blake3.clone())?,
                localization_blake3: ManifestHash::new(hashes.localization_blake3.clone())?,
            },
        })
    }

    pub async fn view(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &BargainViewFrame,
    ) -> BargainViewResult {
        if frame.validate().is_err()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        {
            return view_error(frame.sequence, BargainResultCode::ServiceUnavailable);
        }
        if frame.content_revision != self.content_revision {
            return view_error(frame.sequence, BargainResultCode::ContentMismatch);
        }
        let snapshot = match self
            .persistence
            .bargain_snapshot(authenticated.account_id.as_bytes(), frame.character_id)
            .await
        {
            Ok(value) => value,
            Err(PersistenceError::BargainCharacterNotFound) => {
                return view_error(frame.sequence, BargainResultCode::CharacterNotOwned);
            }
            Err(_) => return view_error(frame.sequence, BargainResultCode::ServiceUnavailable),
        };
        let code = if snapshot.life.selected_character_id != Some(frame.character_id) {
            BargainResultCode::CharacterNotSelected
        } else if snapshot.life.life_state != 0 {
            BargainResultCode::CharacterDead
        } else if snapshot.open_offer.is_some() {
            BargainResultCode::Available
        } else {
            BargainResultCode::NoOffer
        };
        if !matches!(
            code,
            BargainResultCode::Available | BargainResultCode::NoOffer
        ) {
            return view_error(frame.sequence, code);
        }
        let Ok(projection) = projection(
            frame.character_id,
            &snapshot.life,
            snapshot.open_offer.as_ref(),
        ) else {
            return view_error(frame.sequence, BargainResultCode::ServiceUnavailable);
        };
        BargainViewResult {
            sequence: frame.sequence,
            code,
            projection: Some(projection),
        }
    }

    pub async fn decide(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &BargainDecisionFrame,
    ) -> BargainDecisionResult {
        self.decide_response(authenticated, frame).await
    }

    pub async fn decide_with_rest_resolution(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &BargainDecisionFrame,
    ) -> CoreBargainDecisionAuthorityResult {
        let response = self.decide_response(authenticated, frame).await;
        let rest_resolution = if matches!(
            response.code,
            BargainResultCode::Accepted | BargainResultCode::Refused
        ) {
            self.persistence
                .bargain_rest_binding(authenticated.account_id.as_bytes(), frame.mutation_id)
                .await
                .ok()
                .and_then(|binding| {
                    durable_rest_resolution(
                        authenticated,
                        frame,
                        &response,
                        &binding,
                        &self.content_revision,
                    )
                    .ok()
                })
        } else {
            None
        };
        CoreBargainDecisionAuthorityResult {
            response,
            rest_resolution,
        }
    }

    async fn decide_response(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &BargainDecisionFrame,
    ) -> BargainDecisionResult {
        if frame.validate().is_err()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        {
            return decision_error(frame.mutation_id, BargainResultCode::ServiceUnavailable);
        }
        if frame.issued_at_unix_millis > self.clock.unix_millis() {
            return decision_error(frame.mutation_id, BargainResultCode::IssuedAtInvalid);
        }
        let transaction = self
            .persistence
            .transact_bargain_decision(
                authenticated.account_id.as_bytes(),
                frame.payload.character_id,
                frame.payload.offer_id,
                frame.mutation_id,
                |state| {
                    plan_decision(
                        state,
                        authenticated.account_id.as_bytes(),
                        frame,
                        &self.content_revision,
                    )
                },
            )
            .await;
        match transaction {
            Ok(BargainDecisionTransaction::Committed(result)) => result,
            Ok(BargainDecisionTransaction::Replayed(result)) => replay(frame, &result),
            Err(PersistenceError::BargainCharacterNotFound) => {
                decision_error(frame.mutation_id, BargainResultCode::CharacterNotOwned)
            }
            Err(PersistenceError::BargainCharacterDead) => {
                decision_error(frame.mutation_id, BargainResultCode::CharacterDead)
            }
            Err(PersistenceError::BargainOfferNotFound) => {
                decision_error(frame.mutation_id, BargainResultCode::OfferResolved)
            }
            Err(_) => decision_error(frame.mutation_id, BargainResultCode::ServiceUnavailable),
        }
    }
}

fn durable_rest_resolution(
    authenticated: AuthenticatedAccount,
    frame: &BargainDecisionFrame,
    response: &BargainDecisionResult,
    binding: &StoredBargainRestBinding,
    required_revision: &BargainContentRevisionV1,
) -> Result<CoreDurableBargainRestResolution, PersistenceError> {
    let decision = &binding.decision;
    let stored_response: BargainDecisionResult = postcard::from_bytes(&decision.result_payload)
        .map_err(|_| PersistenceError::CorruptStoredBargain)?;
    let resolution = match response.code {
        BargainResultCode::Accepted => {
            sim_content::CoreFixedDungeonRestResolution::BargainSelected(
                match decision.bargain_id.as_deref() {
                    Some(CINDER_HUNGER_ID) => sim_core::CoreBargainKind::CinderHunger,
                    Some(BELL_DEBT_ID) => sim_core::CoreBargainKind::BellDebt,
                    Some(LANTERN_ASH_ID) => sim_core::CoreBargainKind::LanternAsh,
                    _ => return Err(PersistenceError::CorruptStoredBargain),
                },
            )
        }
        BargainResultCode::Refused => sim_content::CoreFixedDungeonRestResolution::BargainRefused,
        _ => return Err(PersistenceError::CorruptStoredBargain),
    };
    if authenticated.namespace != AuthenticatedNamespace::WipeableTest
        || decision.account_id != authenticated.account_id.as_bytes()
        || decision.character_id != frame.payload.character_id
        || decision.offer_id != frame.payload.offer_id
        || decision.mutation_id != frame.mutation_id
        || decision.payload_hash != frame.payload_hash
        || frame.payload.content_revision != *required_revision
        || stored_response != *response
        || decision.post_oath_bargain_version <= 0
        || [binding.instance_lineage_id, binding.entry_restore_point_id]
            .iter()
            .any(|value| value.iter().all(|byte| *byte == 0))
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(CoreDurableBargainRestResolution {
        account_id: decision.account_id,
        character_id: decision.character_id,
        instance_lineage_id: binding.instance_lineage_id,
        entry_restore_point_id: binding.entry_restore_point_id,
        source_receipt_id: decision.mutation_id,
        offer_id: Some(decision.offer_id),
        oath_bargain_version: u64::try_from(decision.post_oath_bargain_version)
            .map_err(|_| PersistenceError::CorruptStoredBargain)?,
        resolution,
    })
}

fn plan_decision(
    state: &mut BargainDecisionTransactionState,
    account_id: [u8; 16],
    frame: &BargainDecisionFrame,
    required_revision: &BargainContentRevisionV1,
) -> Result<BargainDecisionResult, PersistenceError> {
    let pre_version = state.life.oath_bargain_version;
    let code = decision_code(state, frame, required_revision);
    if code == BargainResultCode::Accepted {
        let BargainDecision::Select { bargain_id } = &frame.payload.decision else {
            return Err(PersistenceError::CorruptStoredBargain);
        };
        state.life.oath_bargain_version += 1;
        state.life.active_bargains.push(StoredActiveBargain {
            bargain_id: bargain_id.as_str().into(),
            acquisition_ordinal: i16::try_from(state.life.active_bargains.len() + 1)
                .map_err(|_| PersistenceError::CorruptStoredBargain)?,
            acquired_by_offer_id: state.offer.offer_id,
        });
        state.offer.offer_state = SELECTED;
        state.offer.selected_bargain_id = Some(bargain_id.as_str().into());
        state.offer.resolved_oath_bargain_version = Some(state.life.oath_bargain_version);
    } else if code == BargainResultCode::Refused {
        state.offer.offer_state = REFUSED;
        state.offer.resolved_oath_bargain_version = Some(state.life.oath_bargain_version);
    }
    let projection = projection(frame.payload.character_id, &state.life, Some(&state.offer))
        .map_err(|()| PersistenceError::CorruptStoredBargain)?;
    let result = BargainDecisionResult {
        mutation_id: frame.mutation_id,
        code,
        projection: Some(projection),
    };
    let payload =
        postcard::to_stdvec(&result).map_err(|_| PersistenceError::CorruptStoredBargain)?;
    state.new_result = Some(StoredBargainDecisionResult {
        account_id,
        character_id: frame.payload.character_id,
        mutation_id: frame.mutation_id,
        offer_id: frame.payload.offer_id,
        payload_hash: frame.payload_hash,
        decision_kind: i16::from(matches!(frame.payload.decision, BargainDecision::Refuse)),
        bargain_id: match &frame.payload.decision {
            BargainDecision::Select { bargain_id } => Some(bargain_id.as_str().into()),
            BargainDecision::Refuse => None,
        },
        pre_oath_bargain_version: pre_version,
        post_oath_bargain_version: state.life.oath_bargain_version,
        result_code: result_code_number(code),
        result_payload: payload.clone(),
    });
    if code == BargainResultCode::Accepted {
        state.new_event = Some(StoredCharacterLifeEvent {
            event_id: frame.mutation_id,
            aggregate_version: state.life.oath_bargain_version,
            event_payload: payload,
        });
    }
    Ok(result)
}

fn decision_code(
    state: &BargainDecisionTransactionState,
    frame: &BargainDecisionFrame,
    required_revision: &BargainContentRevisionV1,
) -> BargainResultCode {
    if state.life.selected_character_id != Some(frame.payload.character_id) {
        BargainResultCode::CharacterNotSelected
    } else if state.life.life_state != 0 {
        BargainResultCode::CharacterDead
    } else if state.life.security_state != 0
        || state.life.character_state_version != state.life.location_character_version
        || state.life.location_kind != 2
        || state.life.instance_lineage_id != Some(state.offer.instance_lineage_id)
        || state.life.entry_restore_point_id != Some(state.offer.entry_restore_point_id)
    {
        BargainResultCode::LocationRequired
    } else if u64::try_from(state.life.oath_bargain_version)
        != Ok(frame.expected_oath_bargain_version)
    {
        BargainResultCode::StateVersionMismatch
    } else if &frame.payload.content_revision != required_revision {
        BargainResultCode::ContentMismatch
    } else if state.offer.offer_state != OPEN {
        BargainResultCode::OfferResolved
    } else {
        match &frame.payload.decision {
            BargainDecision::Refuse => BargainResultCode::Refused,
            BargainDecision::Select { bargain_id }
                if state
                    .offer
                    .candidates
                    .iter()
                    .any(|candidate| candidate.bargain_id == bargain_id.as_str())
                    && !state
                        .life
                        .active_bargains
                        .iter()
                        .any(|active| active.bargain_id == bargain_id.as_str()) =>
            {
                BargainResultCode::Accepted
            }
            BargainDecision::Select { .. } => BargainResultCode::CandidateUnavailable,
        }
    }
}

fn projection(
    character_id: [u8; 16],
    life: &StoredBargainLife,
    offer: Option<&StoredBargainOffer>,
) -> Result<BargainProjection, ()> {
    let active_ids = life
        .active_bargains
        .iter()
        .map(|value| WireText::new(value.bargain_id.clone()).map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()?;
    let offer = offer
        .map(|value| offer_projection(value, life))
        .transpose()?;
    let projection = BargainProjection {
        character_id,
        oath_bargain_version: life.oath_bargain_version.try_into().map_err(|_| ())?,
        earned_bargain_slots: life.earned_bargain_slots.try_into().map_err(|_| ())?,
        active_bargain_ids: active_ids,
        offer,
    };
    projection.validate().map_err(|_| ())?;
    Ok(projection)
}

fn offer_projection(
    offer: &StoredBargainOffer,
    life: &StoredBargainLife,
) -> Result<BargainOfferProjection, ()> {
    let state = match offer.offer_state {
        OPEN => BargainOfferState::Open,
        SELECTED => BargainOfferState::Selected {
            bargain_id: WireText::new(offer.selected_bargain_id.clone().ok_or(())?)
                .map_err(|_| ())?,
        },
        REFUSED => BargainOfferState::Refused,
        3 => BargainOfferState::Unavailable,
        _ => return Err(()),
    };
    let active = life
        .active_bargains
        .iter()
        .filter(|value| value.acquired_by_offer_id != offer.offer_id)
        .map(|value| value.bargain_id.as_str())
        .collect::<Vec<_>>();
    let mut cells = offer
        .candidates
        .iter()
        .map(|candidate| {
            Ok(BargainOfferCell::Available {
                bargain_id: WireText::new(candidate.bargain_id.clone()).map_err(|_| ())?,
                comparison: comparison(&active, &candidate.bargain_id),
            })
        })
        .collect::<Result<Vec<_>, ()>>()?;
    cells.resize(3, BargainOfferCell::Unavailable);
    Ok(BargainOfferProjection {
        offer_id: offer.offer_id,
        state,
        cells,
    })
}

#[derive(Debug, Clone, Copy)]
struct Stats {
    health: u32,
    damage: u32,
    cooldown: u32,
    movement: u32,
    healing: u32,
    attack_rate: u32,
    belt_slots: u8,
}

fn comparison(active: &[&str], candidate: &str) -> BargainStatComparison {
    let before = active
        .iter()
        .fold(base_stats(), |stats, id| apply(stats, id));
    let after = apply(before, candidate);
    BargainStatComparison {
        max_health_before_basis_points: before.health,
        max_health_after_basis_points: after.health,
        direct_damage_before_basis_points: before.damage,
        direct_damage_after_basis_points: after.damage,
        cooldown_before_basis_points: before.cooldown,
        cooldown_after_basis_points: after.cooldown,
        movement_before_basis_points: before.movement,
        movement_after_basis_points: after.movement,
        healing_before_basis_points: before.healing,
        healing_after_basis_points: after.healing,
        attack_rate_before_basis_points: before.attack_rate,
        attack_rate_after_basis_points: after.attack_rate,
        active_belt_slots_before: before.belt_slots,
        active_belt_slots_after: after.belt_slots,
    }
}

const fn base_stats() -> Stats {
    Stats {
        health: 10_000,
        damage: 10_000,
        cooldown: 10_000,
        movement: 10_000,
        healing: 10_000,
        attack_rate: 10_000,
        belt_slots: 2,
    }
}

fn apply(mut stats: Stats, bargain_id: &str) -> Stats {
    match bargain_id {
        CINDER_HUNGER_ID => {
            stats.health = stats.health * 8_800 / 10_000;
            stats.damage = stats.damage * 11_800 / 10_000;
        }
        BELL_DEBT_ID => stats.attack_rate = stats.attack_rate * 8_500 / 10_000,
        LANTERN_ASH_ID => {
            stats.healing = stats.healing * 14_000 / 10_000;
            stats.belt_slots = 1;
        }
        _ => {}
    }
    stats
}

fn replay(
    frame: &BargainDecisionFrame,
    stored: &StoredBargainDecisionResult,
) -> BargainDecisionResult {
    if stored.payload_hash != frame.payload_hash
        || stored.character_id != frame.payload.character_id
        || stored.offer_id != frame.payload.offer_id
    {
        return decision_error(frame.mutation_id, BargainResultCode::IdempotencyConflict);
    }
    postcard::from_bytes(&stored.result_payload)
        .ok()
        .filter(|result: &BargainDecisionResult| result.mutation_id == frame.mutation_id)
        .unwrap_or_else(|| decision_error(frame.mutation_id, BargainResultCode::ServiceUnavailable))
}

const fn result_code_number(code: BargainResultCode) -> i16 {
    match code {
        BargainResultCode::Accepted => 0,
        BargainResultCode::Refused => 1,
        BargainResultCode::CharacterNotSelected => 2,
        BargainResultCode::CharacterDead => 3,
        BargainResultCode::LocationRequired => 4,
        BargainResultCode::ContentMismatch => 5,
        BargainResultCode::StateVersionMismatch => 6,
        BargainResultCode::OfferResolved => 7,
        BargainResultCode::CandidateUnavailable => 8,
        BargainResultCode::PayloadHashMismatch => 9,
        BargainResultCode::ConfirmationRequired => 10,
        _ => 15,
    }
}

const fn view_error(sequence: u32, code: BargainResultCode) -> BargainViewResult {
    BargainViewResult {
        sequence,
        code,
        projection: None,
    }
}

const fn decision_error(mutation_id: [u8; 16], code: BargainResultCode) -> BargainDecisionResult {
    BargainDecisionResult {
        mutation_id,
        code,
        projection: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::StoredBargainCandidate;
    use protocol::BargainDecisionPayload;

    fn revision() -> BargainContentRevisionV1 {
        BargainContentRevisionV1 {
            records_blake3: ManifestHash::new("1".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("2".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("3".repeat(64)).unwrap(),
        }
    }

    fn frame(decision: BargainDecision) -> BargainDecisionFrame {
        let payload = BargainDecisionPayload {
            character_id: [2; 16],
            offer_id: [3; 16],
            decision,
            content_revision: revision(),
            confirmed: true,
        };
        BargainDecisionFrame {
            mutation_id: [4; 16],
            expected_oath_bargain_version: 2,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: 1,
            payload,
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: crate::AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn state() -> BargainDecisionTransactionState {
        BargainDecisionTransactionState {
            life: StoredBargainLife {
                selected_character_id: Some([2; 16]),
                level: 5,
                life_state: 0,
                security_state: 0,
                character_state_version: 3,
                location_character_version: 3,
                location_kind: 2,
                location_content_id: Some("world.core_microrealm_01".into()),
                instance_lineage_id: Some([5; 16]),
                entry_restore_point_id: Some([6; 16]),
                earned_bargain_slots: 1,
                oath_bargain_version: 2,
                active_bargains: Vec::new(),
            },
            offer: StoredBargainOffer {
                offer_id: [3; 16],
                source_reward_event_id: [3; 16],
                source_content_id: persistence::CORE_BARGAIN_SOURCE_ID.into(),
                source_layout_id: persistence::CORE_BARGAIN_LAYOUT_ID.into(),
                instance_lineage_id: [5; 16],
                entry_restore_point_id: [6; 16],
                content_version: "core-dev.blake3.test".into(),
                records_blake3: "1".repeat(64),
                assets_blake3: "2".repeat(64),
                localization_blake3: "3".repeat(64),
                offer_state: OPEN,
                selected_bargain_id: None,
                created_oath_bargain_version: 2,
                resolved_oath_bargain_version: None,
                candidates: vec![StoredBargainCandidate {
                    candidate_ordinal: 0,
                    bargain_id: CINDER_HUNGER_ID.into(),
                    score: [7; 32],
                }],
            },
            new_result: None,
            new_event: None,
        }
    }

    #[test]
    fn exact_core_comparisons_expose_every_required_axis() {
        let cinder = comparison(&[], CINDER_HUNGER_ID);
        assert_eq!(cinder.max_health_after_basis_points, 8_800);
        assert_eq!(cinder.direct_damage_after_basis_points, 11_800);
        let bell = comparison(&[], BELL_DEBT_ID);
        assert_eq!(bell.attack_rate_after_basis_points, 8_500);
        let lantern = comparison(&[], LANTERN_ASH_ID);
        assert_eq!(lantern.healing_after_basis_points, 14_000);
        assert_eq!(lantern.active_belt_slots_before, 2);
        assert_eq!(lantern.active_belt_slots_after, 1);
    }

    #[test]
    fn select_and_refuse_are_terminal_and_only_selection_appends_life_state() {
        let mut selected = state();
        let result = plan_decision(
            &mut selected,
            [1; 16],
            &frame(BargainDecision::Select {
                bargain_id: WireText::new(CINDER_HUNGER_ID).unwrap(),
            }),
            &revision(),
        )
        .unwrap();
        assert_eq!(result.code, BargainResultCode::Accepted);
        assert_eq!(selected.life.active_bargains.len(), 1);
        assert_eq!(selected.life.oath_bargain_version, 3);
        assert!(selected.new_event.is_some());

        let mut refused = state();
        let result = plan_decision(
            &mut refused,
            [1; 16],
            &frame(BargainDecision::Refuse),
            &revision(),
        )
        .unwrap();
        assert_eq!(result.code, BargainResultCode::Refused);
        assert!(refused.life.active_bargains.is_empty());
        assert_eq!(refused.life.oath_bargain_version, 2);
        assert!(refused.new_event.is_none());
    }

    #[test]
    fn stored_selection_creates_an_opaque_lineage_bound_b4_resolution() {
        let request = frame(BargainDecision::Select {
            bargain_id: WireText::new(CINDER_HUNGER_ID).unwrap(),
        });
        let mut transaction = state();
        let response = plan_decision(&mut transaction, [1; 16], &request, &revision()).unwrap();
        let decision = transaction.new_result.clone().expect("stored receipt");
        let binding = StoredBargainRestBinding {
            decision,
            instance_lineage_id: [5; 16],
            entry_restore_point_id: [6; 16],
        };

        let durable =
            durable_rest_resolution(authenticated(), &request, &response, &binding, &revision())
                .expect("durable B4 resolution");

        assert_eq!(durable.account_id(), [1; 16]);
        assert_eq!(durable.character_id(), [2; 16]);
        assert_eq!(durable.instance_lineage_id(), [5; 16]);
        assert_eq!(durable.entry_restore_point_id(), [6; 16]);
        assert_eq!(durable.source_receipt_id(), [4; 16]);
        assert_eq!(durable.offer_id(), Some([3; 16]));
        assert_eq!(durable.oath_bargain_version(), 3);
        assert_eq!(
            durable.resolution(),
            sim_content::CoreFixedDungeonRestResolution::BargainSelected(
                sim_core::CoreBargainKind::CinderHunger
            )
        );
    }

    #[test]
    fn altered_or_error_results_cannot_create_b4_authority() {
        let request = frame(BargainDecision::Refuse);
        let mut transaction = state();
        let response = plan_decision(&mut transaction, [1; 16], &request, &revision()).unwrap();
        let mut decision = transaction.new_result.clone().expect("stored receipt");
        decision.payload_hash = [0xAA; 32];
        let binding = StoredBargainRestBinding {
            decision,
            instance_lineage_id: [5; 16],
            entry_restore_point_id: [6; 16],
        };
        assert!(
            durable_rest_resolution(authenticated(), &request, &response, &binding, &revision(),)
                .is_err()
        );
        let error = decision_error(request.mutation_id, BargainResultCode::IdempotencyConflict);
        let mut valid_transaction = state();
        let _ = plan_decision(&mut valid_transaction, [1; 16], &request, &revision()).unwrap();
        let valid_binding = StoredBargainRestBinding {
            decision: valid_transaction.new_result.expect("stored receipt"),
            instance_lineage_id: [5; 16],
            entry_restore_point_id: [6; 16],
        };
        assert!(
            durable_rest_resolution(
                authenticated(),
                &request,
                &error,
                &valid_binding,
                &revision(),
            )
            .is_err()
        );
    }

    #[test]
    fn durable_no_offer_requires_the_exact_core_milestone_contract() {
        let result = StoredBargainMilestoneResult {
            account_id: [1; 16],
            character_id: [2; 16],
            source_reward_event_id: [8; 16],
            payload_hash: [9; 32],
            result_code: 2,
            pre_oath_bargain_version: 2,
            post_oath_bargain_version: 2,
            pre_earned_bargain_slots: 1,
            post_earned_bargain_slots: 1,
            offer_id: None,
            ash_mutation_id: Some([8; 16]),
            milestone_id: CORE_BARGAIN_MILESTONE_ID.into(),
            source_content_id: CORE_BARGAIN_SOURCE_ID.into(),
            source_layout_id: CORE_BARGAIN_LAYOUT_ID.into(),
            instance_lineage_id: [5; 16],
            entry_restore_point_id: [6; 16],
            result_payload: vec![1],
        };
        let durable =
            CoreDurableBargainRestResolution::from_no_offer_milestone(authenticated(), &result)
                .expect("durable no-offer result");
        assert_eq!(
            durable.resolution(),
            sim_content::CoreFixedDungeonRestResolution::NoOffer
        );

        let mut foreign = result;
        foreign.account_id = [0xFF; 16];
        assert!(
            CoreDurableBargainRestResolution::from_no_offer_milestone(authenticated(), &foreign,)
                .is_err()
        );
    }
}
