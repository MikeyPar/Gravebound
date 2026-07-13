//! Authoritative initial-Oath projection and mutation service for `GB-M03-05B`.

use persistence::{
    OathSelectionTransaction, OathSelectionTransactionState, PersistenceError, PostgresPersistence,
    StoredCharacterLifeEvent, StoredOathCharacter, StoredOathMutationResult,
};
use protocol::{
    InitialOathSelectionFrame, InitialOathSelectionResult, ManifestHash, OathContentRevisionV1,
    OathProjection, OathResultCode, OathSelectionState, OathViewFrame, OathViewResult, WireText,
};

use crate::{AuthenticatedAccount, AuthenticatedNamespace, IdentityClock};

const LANTERN_HALLS_ID: &str = "hub.lantern_halls_01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtomicInventoryStatus {
    /// Item persistence has not yet joined the character-life transaction.
    Unavailable,
    Safe,
}

#[derive(Debug, Clone)]
pub struct PostgresOathSelectionService<Clock> {
    persistence: PostgresPersistence,
    clock: Clock,
    content_revision: OathContentRevisionV1,
}

impl<Clock> PostgresOathSelectionService<Clock>
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
            content_revision: OathContentRevisionV1 {
                records_blake3: ManifestHash::new(hashes.records_blake3.clone())?,
                assets_blake3: ManifestHash::new(hashes.assets_blake3.clone())?,
                localization_blake3: ManifestHash::new(hashes.localization_blake3.clone())?,
            },
        })
    }

    pub async fn view(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &OathViewFrame,
    ) -> OathViewResult {
        if frame.validate().is_err()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        {
            return view_error(frame.sequence, OathResultCode::ServiceUnavailable);
        }
        if frame.content_revision != self.content_revision {
            return view_error(frame.sequence, OathResultCode::ContentMismatch);
        }
        let snapshot = match self
            .persistence
            .oath_selection_snapshot(authenticated.account_id.as_bytes(), frame.character_id)
            .await
        {
            Ok(Some(value)) => value,
            Ok(None) => return view_error(frame.sequence, OathResultCode::CharacterNotOwned),
            Err(_) => return view_error(frame.sequence, OathResultCode::ServiceUnavailable),
        };
        if snapshot.selected_character_id != Some(frame.character_id) {
            return view_error(frame.sequence, OathResultCode::CharacterNotSelected);
        }
        if snapshot.life_state != 0 {
            return view_error(frame.sequence, OathResultCode::CharacterDead);
        }
        let Ok(projection) = projection(frame.character_id, &snapshot) else {
            return view_error(frame.sequence, OathResultCode::ServiceUnavailable);
        };
        OathViewResult {
            sequence: frame.sequence,
            code: OathResultCode::Available,
            projection: Some(projection),
        }
    }

    /// Handles the mutation while inventory remains deliberately fail-closed.
    ///
    /// `GB-M03-04D/04F` will replace `Unavailable` with inventory state loaded under the same
    /// serializable transaction. There is intentionally no public switch that can bypass it.
    pub async fn select(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &InitialOathSelectionFrame,
    ) -> InitialOathSelectionResult {
        if frame.validate().is_err()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        {
            return selection_error(frame.mutation_id, OathResultCode::ServiceUnavailable);
        }
        if frame.issued_at_unix_millis > self.clock.unix_millis() {
            return selection_error(frame.mutation_id, OathResultCode::IssuedAtInvalid);
        }
        let transaction = self
            .persistence
            .transact_initial_oath_selection(
                authenticated.account_id.as_bytes(),
                frame.payload.character_id,
                frame.mutation_id,
                |state| {
                    plan_selection(
                        state,
                        authenticated.account_id.as_bytes(),
                        frame,
                        &self.content_revision,
                        AtomicInventoryStatus::Unavailable,
                    )
                },
            )
            .await;
        match transaction {
            Ok(OathSelectionTransaction::Committed(result)) => result,
            Ok(OathSelectionTransaction::Replayed(receipt)) => replay(frame, &receipt),
            Err(PersistenceError::OathCharacterNotFound) => {
                selection_error(frame.mutation_id, OathResultCode::CharacterNotOwned)
            }
            Err(_) => selection_error(frame.mutation_id, OathResultCode::ServiceUnavailable),
        }
    }
}

fn plan_selection(
    state: &mut OathSelectionTransactionState,
    account_id: [u8; 16],
    frame: &InitialOathSelectionFrame,
    required_revision: &OathContentRevisionV1,
    inventory: AtomicInventoryStatus,
) -> Result<InitialOathSelectionResult, PersistenceError> {
    let code = selection_code(state, frame, required_revision, inventory);
    if code == OathResultCode::Accepted {
        state.character.oath_id = Some(frame.payload.oath_id.as_str().to_owned());
        state.character.character_state_version += 1;
        state.character.location_character_version += 1;
    }
    let projection = projection(frame.payload.character_id, &state.character)
        .map_err(|()| PersistenceError::CorruptStoredOath)?;
    let result = InitialOathSelectionResult {
        mutation_id: frame.mutation_id,
        code,
        projection: Some(projection),
    };
    let result_payload =
        postcard::to_stdvec(&result).map_err(|_| PersistenceError::CorruptStoredOath)?;
    let pre_version = if code == OathResultCode::Accepted {
        state.character.character_state_version - 1
    } else {
        state.character.character_state_version
    };
    state.new_result = Some(StoredOathMutationResult {
        account_id,
        character_id: frame.payload.character_id,
        mutation_id: frame.mutation_id,
        payload_hash: frame.payload_hash,
        oath_id: frame.payload.oath_id.as_str().to_owned(),
        pre_character_state_version: pre_version,
        post_character_state_version: state.character.character_state_version,
        result_code: result_code_number(code),
        result_payload: result_payload.clone(),
    });
    if code == OathResultCode::Accepted {
        state.new_event = Some(StoredCharacterLifeEvent {
            event_id: frame.mutation_id,
            aggregate_version: state.character.character_state_version,
            event_payload: result_payload,
        });
    }
    Ok(result)
}

fn selection_code(
    state: &OathSelectionTransactionState,
    frame: &InitialOathSelectionFrame,
    required_revision: &OathContentRevisionV1,
    inventory: AtomicInventoryStatus,
) -> OathResultCode {
    let character = &state.character;
    if character.selected_character_id != Some(frame.payload.character_id) {
        OathResultCode::CharacterNotSelected
    } else if character.life_state != 0 {
        OathResultCode::CharacterDead
    } else if character.security_state != 0
        || character.location_character_version != character.character_state_version
    {
        OathResultCode::UnresolvedMutation
    } else if u64::try_from(character.character_state_version)
        != Ok(frame.expected_character_version)
    {
        OathResultCode::StateVersionMismatch
    } else if let Some(existing) = character.oath_id.as_deref() {
        if existing == frame.payload.oath_id.as_str() {
            OathResultCode::AlreadySelected
        } else {
            OathResultCode::StageDisabled
        }
    } else if character.level < 10 {
        OathResultCode::LevelRequired
    } else if character.location_kind != 1
        || character.location_content_id.as_deref() != Some(LANTERN_HALLS_ID)
    {
        OathResultCode::LocationRequired
    } else if &frame.payload.content_revision != required_revision {
        OathResultCode::ContentMismatch
    } else if inventory != AtomicInventoryStatus::Safe {
        OathResultCode::InventoryNotSafe
    } else {
        OathResultCode::Accepted
    }
}

fn projection(
    character_id: [u8; 16],
    character: &StoredOathCharacter,
) -> Result<OathProjection, ()> {
    let character_version = character
        .character_state_version
        .try_into()
        .map_err(|_| ())?;
    let current_level = character.level.try_into().map_err(|_| ())?;
    let state = if let Some(oath_id) = &character.oath_id {
        OathSelectionState::Selected {
            current_level,
            oath_id: WireText::new(oath_id.clone()).map_err(|_| ())?,
        }
    } else if current_level < 10 {
        OathSelectionState::Locked {
            current_level,
            required_level: 10,
        }
    } else {
        OathSelectionState::Eligible { current_level }
    };
    let projection = OathProjection {
        character_id,
        character_version,
        state,
        later_change_stage_disabled: true,
    };
    projection.validate().map_err(|_| ())?;
    Ok(projection)
}

fn replay(
    frame: &InitialOathSelectionFrame,
    receipt: &StoredOathMutationResult,
) -> InitialOathSelectionResult {
    if receipt.payload_hash != frame.payload_hash {
        return selection_error(frame.mutation_id, OathResultCode::IdempotencyConflict);
    }
    postcard::from_bytes(&receipt.result_payload)
        .ok()
        .filter(|result: &InitialOathSelectionResult| result.mutation_id == frame.mutation_id)
        .unwrap_or_else(|| selection_error(frame.mutation_id, OathResultCode::ServiceUnavailable))
}

const fn result_code_number(code: OathResultCode) -> i16 {
    match code {
        OathResultCode::Available => 0,
        OathResultCode::Accepted => 1,
        OathResultCode::LevelRequired => 2,
        OathResultCode::LocationRequired => 3,
        OathResultCode::CharacterNotOwned => 4,
        OathResultCode::CharacterDead => 5,
        OathResultCode::CharacterNotSelected => 6,
        OathResultCode::ContentDisabled => 7,
        OathResultCode::ContentMismatch => 8,
        OathResultCode::InventoryNotSafe => 9,
        OathResultCode::UnresolvedMutation => 10,
        OathResultCode::StateVersionMismatch => 11,
        OathResultCode::IdempotencyConflict => 12,
        OathResultCode::PayloadHashMismatch => 13,
        OathResultCode::IllegalOath => 14,
        OathResultCode::AlreadySelected => 15,
        OathResultCode::StageDisabled => 16,
        OathResultCode::IssuedAtInvalid => 17,
        OathResultCode::ServiceUnavailable => 18,
    }
}

const fn view_error(sequence: u32, code: OathResultCode) -> OathViewResult {
    OathViewResult {
        sequence,
        code,
        projection: None,
    }
}

const fn selection_error(
    mutation_id: [u8; 16],
    code: OathResultCode,
) -> InitialOathSelectionResult {
    InitialOathSelectionResult {
        mutation_id,
        code,
        projection: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{InitialOathSelectionPayload, LONG_VIGIL_ID};

    fn revision() -> OathContentRevisionV1 {
        OathContentRevisionV1 {
            records_blake3: ManifestHash::new("1".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("2".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("3".repeat(64)).unwrap(),
        }
    }

    fn frame() -> InitialOathSelectionFrame {
        let payload = InitialOathSelectionPayload {
            character_id: [2; 16],
            oath_id: WireText::new(LONG_VIGIL_ID).unwrap(),
            content_revision: revision(),
            confirmed: true,
        };
        InitialOathSelectionFrame {
            mutation_id: [3; 16],
            expected_character_version: 7,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: 1,
            payload,
        }
    }

    fn state() -> OathSelectionTransactionState {
        OathSelectionTransactionState {
            character: StoredOathCharacter {
                selected_character_id: Some([2; 16]),
                level: 10,
                life_state: 0,
                security_state: 0,
                character_state_version: 7,
                oath_id: None,
                location_character_version: 7,
                location_kind: 1,
                location_content_id: Some(LANTERN_HALLS_ID.into()),
            },
            new_result: None,
            new_event: None,
        }
    }

    #[test]
    fn initial_selection_is_fail_closed_until_inventory_joins_transaction() {
        let mut unavailable = state();
        let rejected = plan_selection(
            &mut unavailable,
            [1; 16],
            &frame(),
            &revision(),
            AtomicInventoryStatus::Unavailable,
        )
        .unwrap();
        assert_eq!(rejected.code, OathResultCode::InventoryNotSafe);
        assert_eq!(unavailable.character.character_state_version, 7);
        assert!(unavailable.new_event.is_none());

        let mut safe = state();
        let accepted = plan_selection(
            &mut safe,
            [1; 16],
            &frame(),
            &revision(),
            AtomicInventoryStatus::Safe,
        )
        .unwrap();
        assert_eq!(accepted.code, OathResultCode::Accepted);
        assert_eq!(safe.character.character_state_version, 8);
        assert!(safe.new_event.is_some());
    }

    #[test]
    fn eligibility_rejections_are_specific_and_nonmutating() {
        let mut cases = Vec::new();
        let mut locked = state();
        locked.character.level = 9;
        cases.push((locked, OathResultCode::LevelRequired));
        let mut away = state();
        away.character.location_content_id = Some("realm.mire_of_bells_01".into());
        cases.push((away, OathResultCode::LocationRequired));
        let mut stale = state();
        stale.character.character_state_version = 8;
        stale.character.location_character_version = 8;
        cases.push((stale, OathResultCode::StateVersionMismatch));
        for (mut candidate, expected) in cases {
            let result = plan_selection(
                &mut candidate,
                [1; 16],
                &frame(),
                &revision(),
                AtomicInventoryStatus::Safe,
            )
            .unwrap();
            assert_eq!(result.code, expected);
            assert!(candidate.new_event.is_none());
        }
    }
}
