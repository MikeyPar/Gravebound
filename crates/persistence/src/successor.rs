//! Durable successor authority captured by the ordinary final-death transaction.
//!
//! The contract is derived from the canonical Production GDD (`DTH-020`/`021` and
//! `TECH-021`-`023`), Content Production Spec (`CONT-CATALOG-003`), Development Roadmap
//! (`GB-M03-07`), and accepted `SPEC-CONFLICT-031`. It deliberately stores the non-entitlement
//! Core silhouette token rather than reconstructing appearance authority from mutable content or
//! an optional Echo projection.

use serde::{Deserialize, Serialize};
use sim_core::derive_starter_item_uid;

use crate::{
    AuthoritativeDeathPlanV1, CORE_ITEM_CONTENT_REVISION, DurableDeathProvenanceV1,
    PersistenceError, STARTER_INITIALIZER_REVISION, STARTER_ITEM_COUNT, StoredStarterItem,
    WIPEABLE_CORE_NAMESPACE, items::validate_initializer_input,
};

pub const SUCCESSOR_PRESET_REVISION_V1: u16 = 1;
pub const SUCCESSOR_CONTRACT_VERSION_V1: u16 = 1;
pub const SUCCESSOR_PROTOCOL_MAJOR_V1: u16 = 1;
pub const SUCCESSOR_PROTOCOL_MINOR_V1: u16 = 17;
pub const SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE: u16 = 0;
pub const CORE_SUCCESSOR_CLASS_ID: &str = "class.grave_arbalist";
pub const CORE_SUCCESSOR_BASE_SILHOUETTE_ID: &str = "sprite.class.grave_arbalist";
const SUCCESSOR_PRESET_HASH_CONTEXT_V1: &str = "gravebound.successor-preset.v1";
const SUCCESSOR_CHARACTER_ID_CONTEXT_V1: &str = "gravebound.successor-character.v1";
const SUCCESSOR_RECEIPT_ID_CONTEXT_V1: &str = "gravebound.successor-receipt.v1";
const SUCCESSOR_RESULT_MAX_BYTES: usize = 65_536;
const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;

#[derive(Serialize)]
struct SuccessorRequestHashMaterial<'a> {
    death_id: [u8; ID_BYTES],
    content_revision: &'a str,
}

/// Immutable death-time authority required to initialize one successor later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableSuccessorPresetV1 {
    pub namespace_id: String,
    pub account_id: [u8; 16],
    pub former_character_id: [u8; 16],
    pub death_id: [u8; 16],
    pub former_roster_ordinal: u8,
    pub class_id: String,
    pub appearance_kind: u16,
    pub base_silhouette_id: String,
    pub content_revision: String,
    pub created_at_unix_ms: u64,
    pub preset_hash: [u8; 32],
}

impl DurableSuccessorPresetV1 {
    /// Derives the universal preset for an ordinary player-visible death.
    ///
    /// Incident and administrative terminal records remain durable but cannot mint successor
    /// authority. This preserves the provenance split established by migration `0053`.
    pub fn from_death_plan(
        plan: &AuthoritativeDeathPlanV1,
    ) -> Result<Option<Self>, PersistenceError> {
        if plan.event.provenance != DurableDeathProvenanceV1::OrdinaryGameplay {
            return Ok(None);
        }
        let mut preset = Self {
            namespace_id: plan.event.namespace_id.clone(),
            account_id: plan.event.account_id,
            former_character_id: plan.event.character_id,
            death_id: plan.event.death_id,
            former_roster_ordinal: plan.event.former_roster_ordinal,
            class_id: plan.summary.class_id.clone(),
            appearance_kind: SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE,
            base_silhouette_id: CORE_SUCCESSOR_BASE_SILHOUETTE_ID.to_owned(),
            content_revision: plan.event.content_revision.clone(),
            created_at_unix_ms: plan.event.committed_at_unix_ms,
            preset_hash: [0; 32],
        };
        preset.preset_hash = preset.expected_hash()?;
        preset.validate(plan)?;
        Ok(Some(preset))
    }

    pub fn expected_hash(&self) -> Result<[u8; 32], PersistenceError> {
        #[derive(Serialize)]
        struct Material<'a> {
            preset_revision: u16,
            namespace_id: &'a str,
            account_id: [u8; 16],
            former_character_id: [u8; 16],
            death_id: [u8; 16],
            former_roster_ordinal: u8,
            class_id: &'a str,
            appearance_kind: u16,
            base_silhouette_id: &'a str,
            content_revision: &'a str,
        }

        let bytes = postcard::to_stdvec(&Material {
            preset_revision: SUCCESSOR_PRESET_REVISION_V1,
            namespace_id: &self.namespace_id,
            account_id: self.account_id,
            former_character_id: self.former_character_id,
            death_id: self.death_id,
            former_roster_ordinal: self.former_roster_ordinal,
            class_id: &self.class_id,
            appearance_kind: self.appearance_kind,
            base_silhouette_id: &self.base_silhouette_id,
            content_revision: &self.content_revision,
        })
        .map_err(|_| PersistenceError::CorruptStoredDurableDeath)?;
        Ok(blake3::derive_key(SUCCESSOR_PRESET_HASH_CONTEXT_V1, &bytes))
    }

    fn validate(&self, plan: &AuthoritativeDeathPlanV1) -> Result<(), PersistenceError> {
        if self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || self.account_id == [0; 16]
            || self.former_character_id == [0; 16]
            || self.death_id == [0; 16]
            || !(1..=2).contains(&self.former_roster_ordinal)
            || self.class_id != CORE_SUCCESSOR_CLASS_ID
            || self.appearance_kind != SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE
            || self.base_silhouette_id != CORE_SUCCESSOR_BASE_SILHOUETTE_ID
            || self.created_at_unix_ms == 0
            || self.content_revision != plan.summary.content_revision
            || self.account_id != plan.event.account_id
            || self.former_character_id != plan.event.character_id
            || self.death_id != plan.event.death_id
            || self.former_roster_ordinal != plan.event.former_roster_ordinal
            || self.preset_hash == [0; 32]
            || self.preset_hash != self.expected_hash()?
        {
            return Err(PersistenceError::CorruptStoredDurableDeath);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredSuccessorAppearanceV1 {
    CoreBaseSilhouette,
}

impl StoredSuccessorAppearanceV1 {
    #[must_use]
    pub const fn durable_kind(self) -> u16 {
        match self {
            Self::CoreBaseSilhouette => SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSuccessorStarterItemsV1 {
    pub weapon_uid: [u8; ID_BYTES],
    pub relic_uid: [u8; ID_BYTES],
    pub tonic_unit_uids: [[u8; ID_BYTES]; 2],
}

impl StoredSuccessorStarterItemsV1 {
    #[must_use]
    pub const fn ordered_uids(self) -> [[u8; ID_BYTES]; STARTER_ITEM_COUNT] {
        [
            self.weapon_uid,
            self.relic_uid,
            self.tonic_unit_uids[0],
            self.tonic_unit_uids[1],
        ]
    }

    fn from_plan(items: &[StoredStarterItem]) -> Result<Self, PersistenceError> {
        let [weapon, relic, tonic_0, tonic_1] = items else {
            return Err(corrupt());
        };
        Ok(Self {
            weapon_uid: weapon.item_uid,
            relic_uid: relic.item_uid,
            tonic_unit_uids: [tonic_0.item_uid, tonic_1.item_uid],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSuccessorVersionsV1 {
    pub account: u64,
    pub character: u64,
    pub progression: u64,
    pub world: u64,
    pub inventory: u64,
    pub life_metrics: u64,
    pub oath_bargain: u64,
}

impl StoredSuccessorVersionsV1 {
    fn validate(self) -> Result<(), PersistenceError> {
        if self.account < 2
            || self.character != 1
            || self.progression != 1
            || self.world != 1
            || self.inventory != 2
            || self.life_metrics != 1
            || self.oath_bargain != 1
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

/// Server-planned authority for one successor mutation.
///
/// The authenticated client contributes only `mutation_id`, `death_id`, and the canonical
/// payload hash. Successor, receipt, and starter identities are deterministic server products and
/// are re-derived during validation before any database lock or write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessorCreateRequestV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub account_id: [u8; ID_BYTES],
    pub mutation_id: [u8; ID_BYTES],
    pub death_id: [u8; ID_BYTES],
    pub successor_id: [u8; ID_BYTES],
    pub receipt_id: [u8; ID_BYTES],
    pub canonical_request_hash: [u8; HASH_BYTES],
    pub content_revision: String,
    pub starter_request_hash: [u8; HASH_BYTES],
    pub starter_result_hash: [u8; HASH_BYTES],
    pub starter_items: Vec<StoredStarterItem>,
}

impl SuccessorCreateRequestV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if !is_well_formed_core_content_revision(&self.content_revision)
            || self.contract_version != SUCCESSOR_CONTRACT_VERSION_V1
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || [
                self.account_id,
                self.mutation_id,
                self.death_id,
                self.successor_id,
                self.receipt_id,
            ]
            .contains(&[0; ID_BYTES])
            || !pairwise_distinct(&[
                self.mutation_id,
                self.death_id,
                self.successor_id,
                self.receipt_id,
            ])
            || self.canonical_request_hash != self.expected_request_hash()?
            || self.successor_id
                != derive_successor_character_id_v1(
                    self.account_id,
                    self.death_id,
                    self.mutation_id,
                )
            || self.receipt_id
                != derive_successor_receipt_id_v1(
                    self.account_id,
                    self.death_id,
                    self.mutation_id,
                    self.successor_id,
                )
        {
            return Err(corrupt());
        }
        if self.content_revision != CORE_ITEM_CONTENT_REVISION {
            return Err(PersistenceError::SuccessorContentMismatch);
        }
        validate_initializer_input(
            self.successor_id,
            self.starter_request_hash,
            self.starter_result_hash,
            &self.starter_items,
        )
        .map_err(|_| corrupt())?;
        if self.starter_items.iter().any(|item| {
            [
                self.mutation_id,
                self.death_id,
                self.successor_id,
                self.receipt_id,
            ]
            .contains(&item.item_uid)
        }) {
            return Err(corrupt());
        }
        Ok(())
    }

    pub fn expected_request_hash(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        canonical_successor_request_hash_v1(self.death_id, &self.content_revision)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSuccessorResultV1 {
    pub contract_version: u16,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub namespace_id: String,
    pub account_id: [u8; ID_BYTES],
    pub mutation_id: [u8; ID_BYTES],
    pub death_id: [u8; ID_BYTES],
    pub successor_id: [u8; ID_BYTES],
    pub selected_character_id: [u8; ID_BYTES],
    pub receipt_id: [u8; ID_BYTES],
    pub canonical_request_hash: [u8; HASH_BYTES],
    pub former_roster_ordinal: u8,
    pub class_id: String,
    pub appearance: StoredSuccessorAppearanceV1,
    pub base_silhouette_id: String,
    pub preset_hash: [u8; HASH_BYTES],
    pub starter_items: StoredSuccessorStarterItemsV1,
    pub versions: StoredSuccessorVersionsV1,
    pub content_revision: String,
    pub result_hash: [u8; HASH_BYTES],
}

impl StoredSuccessorResultV1 {
    pub fn from_request(
        request: &SuccessorCreateRequestV1,
        preset: &DurableSuccessorPresetV1,
        post_account_version: u64,
    ) -> Result<Self, PersistenceError> {
        request.validate()?;
        if preset.namespace_id != request.namespace_id
            || preset.account_id != request.account_id
            || preset.death_id != request.death_id
            || preset.former_character_id == [0; ID_BYTES]
            || !(1..=2).contains(&preset.former_roster_ordinal)
            || preset.class_id != CORE_SUCCESSOR_CLASS_ID
            || preset.appearance_kind != SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE
            || preset.base_silhouette_id != CORE_SUCCESSOR_BASE_SILHOUETTE_ID
            || preset.content_revision != request.content_revision
            || preset.preset_hash == [0; HASH_BYTES]
            || preset.preset_hash != preset.expected_hash()?
        {
            return Err(corrupt());
        }
        let mut result = Self {
            contract_version: SUCCESSOR_CONTRACT_VERSION_V1,
            protocol_major: SUCCESSOR_PROTOCOL_MAJOR_V1,
            protocol_minor: SUCCESSOR_PROTOCOL_MINOR_V1,
            namespace_id: request.namespace_id.clone(),
            account_id: request.account_id,
            mutation_id: request.mutation_id,
            death_id: request.death_id,
            successor_id: request.successor_id,
            selected_character_id: request.successor_id,
            receipt_id: request.receipt_id,
            canonical_request_hash: request.canonical_request_hash,
            former_roster_ordinal: preset.former_roster_ordinal,
            class_id: preset.class_id.clone(),
            appearance: StoredSuccessorAppearanceV1::CoreBaseSilhouette,
            base_silhouette_id: preset.base_silhouette_id.clone(),
            preset_hash: preset.preset_hash,
            starter_items: StoredSuccessorStarterItemsV1::from_plan(&request.starter_items)?,
            versions: StoredSuccessorVersionsV1 {
                account: post_account_version,
                character: 1,
                progression: 1,
                world: 1,
                inventory: 2,
                life_metrics: 1,
                oath_bargain: 1,
            },
            content_revision: request.content_revision.clone(),
            result_hash: [0; HASH_BYTES],
        };
        result.result_hash = result.canonical_protocol_result_hash()?;
        result.validate()?;
        Ok(result)
    }

    /// Hashes the exact append-only protocol `StoredSuccessorResultV1` material without creating
    /// a persistence-to-protocol dependency. Strings serialize identically to bounded `WireText`.
    pub fn canonical_protocol_result_hash(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        let bytes = postcard::to_stdvec(&(
            self.mutation_id,
            self.death_id,
            self.successor_id,
            self.receipt_id,
            self.former_roster_ordinal,
            &self.class_id,
            self.appearance,
            self.starter_items,
            self.versions,
            &self.content_revision,
            self.selected_character_id,
        ))
        .map_err(|_| corrupt())?;
        Ok(*blake3::hash(&bytes).as_bytes())
    }

    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.contract_version != SUCCESSOR_CONTRACT_VERSION_V1
            || self.protocol_major != SUCCESSOR_PROTOCOL_MAJOR_V1
            || self.protocol_minor != SUCCESSOR_PROTOCOL_MINOR_V1
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || [
                self.account_id,
                self.mutation_id,
                self.death_id,
                self.successor_id,
                self.selected_character_id,
                self.receipt_id,
            ]
            .contains(&[0; ID_BYTES])
            || !pairwise_distinct(&[
                self.mutation_id,
                self.death_id,
                self.successor_id,
                self.receipt_id,
            ])
            || self.selected_character_id != self.successor_id
            || !(1..=2).contains(&self.former_roster_ordinal)
            || self.class_id != CORE_SUCCESSOR_CLASS_ID
            || self.appearance.durable_kind() != SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE
            || self.base_silhouette_id != CORE_SUCCESSOR_BASE_SILHOUETTE_ID
            || self.content_revision != CORE_ITEM_CONTENT_REVISION
            || self.canonical_request_hash == [0; HASH_BYTES]
            || self.canonical_request_hash
                != canonical_successor_request_hash_v1(self.death_id, &self.content_revision)?
            || self.preset_hash == [0; HASH_BYTES]
            || self.successor_id
                != derive_successor_character_id_v1(
                    self.account_id,
                    self.death_id,
                    self.mutation_id,
                )
            || self.receipt_id
                != derive_successor_receipt_id_v1(
                    self.account_id,
                    self.death_id,
                    self.mutation_id,
                    self.successor_id,
                )
            || self.result_hash == [0; HASH_BYTES]
            || self.result_hash != self.canonical_protocol_result_hash()?
        {
            return Err(corrupt());
        }
        self.versions.validate()?;
        validate_starter_uid_set(self.successor_id, self.starter_items)?;
        if self.starter_items.ordered_uids().iter().any(|item_uid| {
            [
                self.mutation_id,
                self.death_id,
                self.successor_id,
                self.receipt_id,
            ]
            .contains(item_uid)
        }) {
            return Err(corrupt());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, PersistenceError> {
        self.validate()?;
        let payload = postcard::to_stdvec(self).map_err(|_| corrupt())?;
        if payload.is_empty() || payload.len() > SUCCESSOR_RESULT_MAX_BYTES {
            return Err(corrupt());
        }
        Ok(payload)
    }

    pub fn decode(payload: &[u8]) -> Result<Self, PersistenceError> {
        if payload.is_empty() || payload.len() > SUCCESSOR_RESULT_MAX_BYTES {
            return Err(corrupt());
        }
        let result = postcard::from_bytes::<Self>(payload).map_err(|_| corrupt())?;
        result.validate()?;
        Ok(result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuccessorCreateTransactionV1 {
    Fresh(StoredSuccessorResultV1),
    Replayed(StoredSuccessorResultV1),
    Conflict {
        stored_mutation_id: [u8; ID_BYTES],
        stored_death_id: [u8; ID_BYTES],
    },
}

#[must_use]
pub fn derive_successor_character_id_v1(
    account_id: [u8; ID_BYTES],
    death_id: [u8; ID_BYTES],
    mutation_id: [u8; ID_BYTES],
) -> [u8; ID_BYTES] {
    derive_id(
        SUCCESSOR_CHARACTER_ID_CONTEXT_V1,
        &[&account_id, &death_id, &mutation_id],
    )
}

#[must_use]
pub fn derive_successor_receipt_id_v1(
    account_id: [u8; ID_BYTES],
    death_id: [u8; ID_BYTES],
    mutation_id: [u8; ID_BYTES],
    successor_id: [u8; ID_BYTES],
) -> [u8; ID_BYTES] {
    derive_id(
        SUCCESSOR_RECEIPT_ID_CONTEXT_V1,
        &[&account_id, &death_id, &mutation_id, &successor_id],
    )
}

fn validate_starter_uid_set(
    successor_id: [u8; ID_BYTES],
    starter_items: StoredSuccessorStarterItemsV1,
) -> Result<(), PersistenceError> {
    let expected = [
        ("item.weapon.crossbow.pine_crossbow", 0_u16),
        ("item.relic.arbalist.cracked_mark_lens", 0_u16),
        ("consumable.red_tonic", 0_u16),
        ("consumable.red_tonic", 1_u16),
    ];
    for (actual, (template_id, unit_ordinal)) in
        starter_items.ordered_uids().into_iter().zip(expected)
    {
        let expected_uid = derive_starter_item_uid(
            successor_id,
            STARTER_INITIALIZER_REVISION,
            template_id,
            unit_ordinal,
        )
        .map_err(|_| corrupt())?
        .bytes();
        if actual != expected_uid {
            return Err(corrupt());
        }
    }
    Ok(())
}

fn canonical_successor_request_hash_v1(
    death_id: [u8; ID_BYTES],
    content_revision: &str,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let payload = postcard::to_stdvec(&SuccessorRequestHashMaterial {
        death_id,
        content_revision,
    })
    .map_err(|_| corrupt())?;
    Ok(*blake3::hash(&payload).as_bytes())
}

fn is_well_formed_core_content_revision(value: &str) -> bool {
    value
        .strip_prefix("core-dev.blake3.")
        .is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn derive_id(context: &str, parts: &[&[u8]]) -> [u8; ID_BYTES] {
    let mut bytes = Vec::new();
    for part in parts {
        let length = u32::try_from(part.len()).expect("successor identity input is bounded");
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(part);
    }
    let digest = blake3::derive_key(context, &bytes);
    let mut id = [0_u8; ID_BYTES];
    id.copy_from_slice(&digest[..ID_BYTES]);
    id
}

fn pairwise_distinct(ids: &[[u8; ID_BYTES]]) -> bool {
    ids.iter()
        .enumerate()
        .all(|(index, id)| !ids[..index].contains(id))
}

fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredSuccessor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn starter_items(character_id: [u8; ID_BYTES]) -> Vec<StoredStarterItem> {
        [
            (
                "item.weapon.crossbow.pine_crossbow",
                0_i16,
                Some(1_i16),
                Some(0_i16),
                0_i32,
                0_i32,
                0_i16,
                0_i16,
            ),
            (
                "item.relic.arbalist.cracked_mark_lens",
                0,
                Some(1),
                Some(0),
                1,
                0,
                0,
                1,
            ),
            ("consumable.red_tonic", 1, None, None, 2, 0, 1, 0),
            ("consumable.red_tonic", 1, None, None, 2, 1, 1, 0),
        ]
        .into_iter()
        .map(
            |(
                template_id,
                item_kind,
                item_level,
                rarity,
                roll_index,
                unit_ordinal,
                location_kind,
                slot_index,
            )| {
                let item_uid = derive_starter_item_uid(
                    character_id,
                    STARTER_INITIALIZER_REVISION,
                    template_id,
                    u16::try_from(unit_ordinal).unwrap(),
                )
                .unwrap()
                .bytes();
                StoredStarterItem {
                    item_uid,
                    ledger_event_id: item_uid,
                    template_id: template_id.to_owned(),
                    item_kind,
                    item_level,
                    rarity,
                    roll_index,
                    unit_ordinal,
                    location_kind,
                    slot_index,
                }
            },
        )
        .collect()
    }

    fn request() -> SuccessorCreateRequestV1 {
        let account_id = [1; ID_BYTES];
        let death_id = [2; ID_BYTES];
        let mutation_id = [3; ID_BYTES];
        let successor_id = derive_successor_character_id_v1(account_id, death_id, mutation_id);
        let receipt_id =
            derive_successor_receipt_id_v1(account_id, death_id, mutation_id, successor_id);
        let starter_items = starter_items(successor_id);
        let starter_request_hash = crate::canonical_starter_request_hash_v1(successor_id).unwrap();
        let starter_result_hash =
            crate::canonical_starter_result_hash_v1(starter_request_hash, &starter_items).unwrap();
        let mut request = SuccessorCreateRequestV1 {
            contract_version: SUCCESSOR_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
            account_id,
            mutation_id,
            death_id,
            successor_id,
            receipt_id,
            canonical_request_hash: [0; HASH_BYTES],
            content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
            starter_request_hash,
            starter_result_hash,
            starter_items,
        };
        request.canonical_request_hash = request.expected_request_hash().unwrap();
        request
    }

    fn preset(request: &SuccessorCreateRequestV1) -> DurableSuccessorPresetV1 {
        let mut preset = DurableSuccessorPresetV1 {
            namespace_id: request.namespace_id.clone(),
            account_id: request.account_id,
            former_character_id: [6; ID_BYTES],
            death_id: request.death_id,
            former_roster_ordinal: 1,
            class_id: CORE_SUCCESSOR_CLASS_ID.to_owned(),
            appearance_kind: SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE,
            base_silhouette_id: CORE_SUCCESSOR_BASE_SILHOUETTE_ID.to_owned(),
            content_revision: request.content_revision.clone(),
            created_at_unix_ms: 1,
            preset_hash: [0; HASH_BYTES],
        };
        preset.preset_hash = preset.expected_hash().unwrap();
        preset
    }

    #[test]
    fn preset_hash_is_domain_separated_and_commit_time_independent() {
        let mut preset = DurableSuccessorPresetV1 {
            namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
            account_id: [1; 16],
            former_character_id: [2; 16],
            death_id: [3; 16],
            former_roster_ordinal: 1,
            class_id: CORE_SUCCESSOR_CLASS_ID.to_owned(),
            appearance_kind: SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE,
            base_silhouette_id: CORE_SUCCESSOR_BASE_SILHOUETTE_ID.to_owned(),
            content_revision: format!("core-dev.blake3.{}", "a".repeat(64)),
            created_at_unix_ms: 10,
            preset_hash: [0; 32],
        };
        let expected = preset.expected_hash().unwrap();
        preset.created_at_unix_ms = 20;
        assert_eq!(preset.expected_hash().unwrap(), expected);
        preset.former_roster_ordinal = 2;
        assert_ne!(preset.expected_hash().unwrap(), expected);
    }

    #[test]
    fn successor_request_rederives_every_server_owned_identity() {
        let request = request();
        assert!(request.validate().is_ok());

        let mut changed = request.clone();
        changed.successor_id[0] ^= 1;
        assert!(matches!(
            changed.validate(),
            Err(PersistenceError::CorruptStoredSuccessor)
        ));

        let mut changed = request;
        changed.starter_items[3].item_uid[0] ^= 1;
        changed.starter_items[3].ledger_event_id = changed.starter_items[3].item_uid;
        changed.starter_result_hash = crate::canonical_starter_result_hash_v1(
            changed.starter_request_hash,
            &changed.starter_items,
        )
        .unwrap();
        assert!(matches!(
            changed.validate(),
            Err(PersistenceError::CorruptStoredSuccessor)
        ));
    }

    #[test]
    fn successor_request_rejects_altered_starter_hashes_and_ledger_identity() {
        let request = request();

        let mut changed = request.clone();
        changed.starter_request_hash[0] ^= 1;
        assert!(matches!(
            changed.validate(),
            Err(PersistenceError::CorruptStoredSuccessor)
        ));

        let mut changed = request.clone();
        changed.starter_result_hash[0] ^= 1;
        assert!(matches!(
            changed.validate(),
            Err(PersistenceError::CorruptStoredSuccessor)
        ));

        let mut changed = request;
        changed.starter_items[0].ledger_event_id[0] ^= 1;
        assert!(matches!(
            changed.validate(),
            Err(PersistenceError::CorruptStoredSuccessor)
        ));
    }

    #[test]
    fn stale_successor_content_is_typed_but_malformed_content_is_corrupt() {
        let request = request();

        let mut stale = request.clone();
        stale.content_revision = format!("core-dev.blake3.{}", "a".repeat(64));
        stale.canonical_request_hash = stale.expected_request_hash().unwrap();
        assert!(matches!(
            stale.validate(),
            Err(PersistenceError::SuccessorContentMismatch)
        ));

        let mut malformed = request;
        malformed.content_revision = "core-dev.blake3.NOT-A-DIGEST".to_owned();
        assert!(matches!(
            malformed.validate(),
            Err(PersistenceError::CorruptStoredSuccessor)
        ));
    }

    #[test]
    fn stored_successor_result_round_trips_protocol_hash_material() {
        let request = request();
        let result = StoredSuccessorResultV1::from_request(&request, &preset(&request), 8).unwrap();
        assert!(result.validate().is_ok());
        assert_eq!(result.selected_character_id, result.successor_id);
        assert_eq!(result.versions.inventory, 2);
        assert_eq!(
            result,
            StoredSuccessorResultV1::decode(&result.encode().unwrap()).unwrap()
        );

        let mut corrupt = result;
        corrupt.versions.inventory = 1;
        corrupt.result_hash = corrupt.canonical_protocol_result_hash().unwrap();
        assert!(matches!(
            corrupt.validate(),
            Err(PersistenceError::CorruptStoredSuccessor)
        ));
    }
}
