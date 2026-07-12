use serde::{
    Deserialize, Deserializer, Serialize,
    de::{Error as _, SeqAccess, Visitor},
};
use thiserror::Error;

use crate::{ManifestHash, NetworkChannel, WireText};

pub const CHARACTER_ID_BYTES: usize = 16;
pub const MUTATION_ID_BYTES: usize = 16;
pub const PAYLOAD_HASH_BYTES: usize = 32;
pub const CORE_CHARACTER_SLOT_CAPACITY: u8 = 2;
pub const MAX_ACCOUNT_CHARACTERS: usize = CORE_CHARACTER_SLOT_CAPACITY as usize;
pub const CLASS_ID_MAX_BYTES: usize = 96;
pub const GRAVE_ARBALIST_CLASS_ID: &str = "class.grave_arbalist";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountNamespace {
    WipeableTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterLifeState {
    Living,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterSecurityState {
    SafeCharacterSelect,
}

/// A safe roster projection. Editable names and appearance entitlements are deliberately absent
/// under approved `SPEC-CONFLICT-004`; the client derives a localized `Hero {ordinal}` label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterSnapshot {
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub roster_ordinal: u8,
    pub class_id: WireText<CLASS_ID_MAX_BYTES>,
    pub level: u16,
    pub oath_id: Option<WireText<CLASS_ID_MAX_BYTES>>,
    pub life_state: CharacterLifeState,
    pub security_state: CharacterSecurityState,
}

impl CharacterSnapshot {
    pub fn validate(&self) -> Result<(), AccountMessageValidationError> {
        if all_zero(&self.character_id) {
            return Err(AccountMessageValidationError::ZeroCharacterId);
        }
        if !(1..=CORE_CHARACTER_SLOT_CAPACITY).contains(&self.roster_ordinal) {
            return Err(AccountMessageValidationError::InvalidRosterOrdinal);
        }
        if self.class_id.as_str() != GRAVE_ARBALIST_CLASS_ID
            || self.level != 1
            || self.oath_id.is_some()
            || self.life_state != CharacterLifeState::Living
            || self.security_state != CharacterSecurityState::SafeCharacterSelect
        {
            return Err(AccountMessageValidationError::IllegalCoreCharacterState);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub namespace: AccountNamespace,
    pub account_version: u64,
    pub slot_capacity: u8,
    #[serde(deserialize_with = "deserialize_bounded_roster")]
    pub characters: Vec<CharacterSnapshot>,
    pub selected_character_id: Option<[u8; CHARACTER_ID_BYTES]>,
}

impl AccountSnapshot {
    pub fn validate(&self) -> Result<(), AccountMessageValidationError> {
        if self.account_version == 0 {
            return Err(AccountMessageValidationError::ZeroAccountVersion);
        }
        if self.slot_capacity != CORE_CHARACTER_SLOT_CAPACITY
            || self.characters.len() > MAX_ACCOUNT_CHARACTERS
        {
            return Err(AccountMessageValidationError::CharacterCount);
        }
        let mut seen_ids = Vec::with_capacity(MAX_ACCOUNT_CHARACTERS);
        let mut seen_ordinals = Vec::with_capacity(MAX_ACCOUNT_CHARACTERS);
        for character in &self.characters {
            character.validate()?;
            if seen_ids.contains(&character.character_id) {
                return Err(AccountMessageValidationError::DuplicateCharacterId);
            }
            if seen_ordinals.contains(&character.roster_ordinal) {
                return Err(AccountMessageValidationError::DuplicateRosterOrdinal);
            }
            seen_ids.push(character.character_id);
            seen_ordinals.push(character.roster_ordinal);
        }
        if let Some(selected) = self.selected_character_id
            && !seen_ids.contains(&selected)
        {
            return Err(AccountMessageValidationError::SelectedCharacterMissing);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountBootstrapRequest {
    Bootstrap,
    Refresh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountBootstrapFrame {
    pub sequence: u32,
    pub request: AccountBootstrapRequest,
    pub content_manifest_hash: ManifestHash,
}

impl AccountBootstrapFrame {
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Control
    }

    pub const fn validate(&self) -> Result<(), AccountMessageValidationError> {
        if self.sequence == 0 {
            return Err(AccountMessageValidationError::ZeroSequence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountBootstrapResult {
    Snapshot(AccountSnapshot),
    Error(AccountErrorCode),
}

impl AccountBootstrapResult {
    pub fn validate(&self) -> Result<(), AccountMessageValidationError> {
        match self {
            Self::Snapshot(snapshot) => snapshot.validate(),
            Self::Error(_) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterMutationPayload {
    Create {
        class_id: WireText<CLASS_ID_MAX_BYTES>,
    },
    Select {
        character_id: [u8; CHARACTER_ID_BYTES],
    },
}

impl CharacterMutationPayload {
    pub fn canonical_hash(&self) -> [u8; PAYLOAD_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded account payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    pub fn validate(&self) -> Result<(), AccountMessageValidationError> {
        match self {
            Self::Select { character_id } if all_zero(character_id) => {
                Err(AccountMessageValidationError::ZeroCharacterId)
            }
            Self::Create { .. } | Self::Select { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterMutationFrame {
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub expected_account_version: u64,
    pub payload_hash: [u8; PAYLOAD_HASH_BYTES],
    pub issued_at_unix_millis: u64,
    pub payload: CharacterMutationPayload,
}

impl CharacterMutationFrame {
    pub fn validate(&self) -> Result<(), AccountMessageValidationError> {
        if all_zero(&self.mutation_id) {
            return Err(AccountMessageValidationError::ZeroMutationId);
        }
        if self.expected_account_version == 0 {
            return Err(AccountMessageValidationError::ZeroAccountVersion);
        }
        if all_zero(&self.payload_hash) {
            return Err(AccountMessageValidationError::ZeroPayloadHash);
        }
        if self.issued_at_unix_millis == 0 {
            return Err(AccountMessageValidationError::ZeroIssuedAt);
        }
        self.payload.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountErrorCode {
    Unauthenticated,
    ProductionNamespaceForbidden,
    AccountMismatch,
    CharacterNotFound,
    CharacterNotOwned,
    CharacterDead,
    ClassDisabled,
    AppearanceUnavailable,
    InvalidName,
    CharacterSlotFull,
    StateVersionMismatch,
    IdempotencyConflict,
    PayloadHashMismatch,
    IssuedAtInvalid,
    ContentMismatch,
    RateLimited,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterMutationResult {
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub accepted: bool,
    pub error: Option<AccountErrorCode>,
    /// Present for accepted mutations and authenticated state errors (including stale versions).
    /// Authentication/namespace/service failures cannot safely expose an account projection.
    pub snapshot: Option<AccountSnapshot>,
}

impl CharacterMutationResult {
    pub fn validate(&self) -> Result<(), AccountMessageValidationError> {
        if all_zero(&self.mutation_id) {
            return Err(AccountMessageValidationError::ZeroMutationId);
        }
        if self.accepted == self.error.is_some() || (self.accepted && self.snapshot.is_none()) {
            return Err(AccountMessageValidationError::MutationResultMismatch);
        }
        self.snapshot
            .as_ref()
            .map_or(Ok(()), AccountSnapshot::validate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AccountMessageValidationError {
    #[error("message sequence must be nonzero")]
    ZeroSequence,
    #[error("account version must be nonzero")]
    ZeroAccountVersion,
    #[error("character ID must be nonzero")]
    ZeroCharacterId,
    #[error("mutation ID must be nonzero")]
    ZeroMutationId,
    #[error("mutation payload hash must be nonzero")]
    ZeroPayloadHash,
    #[error("mutation issue time must be nonzero")]
    ZeroIssuedAt,
    #[error("account snapshot exceeds the Core character capacity")]
    CharacterCount,
    #[error("roster ordinal is outside the Core slot range")]
    InvalidRosterOrdinal,
    #[error("account snapshot contains duplicate character IDs")]
    DuplicateCharacterId,
    #[error("account snapshot contains duplicate roster ordinals")]
    DuplicateRosterOrdinal,
    #[error("selected character is absent from the safe roster")]
    SelectedCharacterMissing,
    #[error("character snapshot contains state unavailable in GB-M03-01")]
    IllegalCoreCharacterState,
    #[error("mutation result acceptance and error fields disagree")]
    MutationResultMismatch,
}

fn all_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

fn deserialize_bounded_roster<'de, D>(deserializer: D) -> Result<Vec<CharacterSnapshot>, D::Error>
where
    D: Deserializer<'de>,
{
    struct RosterVisitor;

    impl<'de> Visitor<'de> for RosterVisitor {
        type Value = Vec<CharacterSnapshot>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                formatter,
                "at most {MAX_ACCOUNT_CHARACTERS} character snapshots"
            )
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if sequence
                .size_hint()
                .is_some_and(|length| length > MAX_ACCOUNT_CHARACTERS)
            {
                return Err(A::Error::custom("account roster exceeds Core capacity"));
            }
            let mut characters = Vec::with_capacity(MAX_ACCOUNT_CHARACTERS);
            while let Some(character) = sequence.next_element()? {
                if characters.len() == MAX_ACCOUNT_CHARACTERS {
                    return Err(A::Error::custom("account roster exceeds Core capacity"));
                }
                characters.push(character);
            }
            Ok(characters)
        }
    }

    deserializer.deserialize_seq(RosterVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(id: u8, ordinal: u8) -> CharacterSnapshot {
        CharacterSnapshot {
            character_id: [id; CHARACTER_ID_BYTES],
            roster_ordinal: ordinal,
            class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID).unwrap(),
            level: 1,
            oath_id: None,
            life_state: CharacterLifeState::Living,
            security_state: CharacterSecurityState::SafeCharacterSelect,
        }
    }

    #[test]
    fn snapshot_is_bounded_and_contains_only_approved_core_state() {
        let snapshot = AccountSnapshot {
            namespace: AccountNamespace::WipeableTest,
            account_version: 1,
            slot_capacity: CORE_CHARACTER_SLOT_CAPACITY,
            characters: vec![character(1, 1), character(2, 2)],
            selected_character_id: Some([1; CHARACTER_ID_BYTES]),
        };
        assert_eq!(snapshot.validate(), Ok(()));
        let mut oversized = snapshot.clone();
        oversized.characters.push(character(3, 1));
        assert_eq!(
            oversized.validate(),
            Err(AccountMessageValidationError::CharacterCount)
        );
        let oversized_bytes = postcard::to_stdvec(&oversized).unwrap();
        assert!(postcard::from_bytes::<AccountSnapshot>(&oversized_bytes).is_err());
        let mut illegal = snapshot;
        illegal.characters[0].oath_id = Some(WireText::new("oath.arbalist.long_vigil").unwrap());
        assert_eq!(
            illegal.validate(),
            Err(AccountMessageValidationError::IllegalCoreCharacterState)
        );
    }

    #[test]
    fn payload_hash_is_deterministic_and_bound_to_payload() {
        let create = CharacterMutationPayload::Create {
            class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID).unwrap(),
        };
        let select = CharacterMutationPayload::Select {
            character_id: [1; CHARACTER_ID_BYTES],
        };
        assert_eq!(create.canonical_hash(), create.canonical_hash());
        assert_ne!(create.canonical_hash(), select.canonical_hash());
    }

    #[test]
    fn approved_contract_has_no_name_or_appearance_wire_fields() {
        let encoded = postcard::to_stdvec(&character(1, 1)).unwrap();
        let decoded: CharacterSnapshot = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(decoded.roster_ordinal, 1);
        assert_eq!(decoded, character(1, 1));
    }
}
