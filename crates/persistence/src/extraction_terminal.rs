//! Versioned production contract for one successful-extraction terminal.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-011`, `LOOT-002`,
//! `LOOT-033`, `LOOT-050`, `LOOT-060`, and `TECH-015`/`021`-`023`;
//! `Gravebound_Content_Production_Spec_v1.md` `CONT-HUB-001`/`002`, the Core
//! Bell Sepulcher/Caldus route, and `CONT-VALID-001`; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08`, plus accepted
//! `SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md`.
//!
//! The client never constructs these stored outcomes. The server binds a validated protocol
//! request to a terminal-coordinator winner; the repository derives the placement/material plan
//! from locked `PostgreSQL` custody and stores this bounded result for exact retry and restart.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{PersistenceError, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE};

pub const PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1: u16 = 1;
pub const PRODUCTION_EXTRACTION_TERMINAL_KIND: u8 = 2;
pub const PRODUCTION_EXTRACTION_HALL_ID: &str = "hub.lantern_halls_01";
pub const PRODUCTION_EXTRACTION_EXIT_ID: &str = "portal.exit.dungeon.bell_sepulcher";
pub const MAX_PRODUCTION_EXTRACTION_PLACEMENTS: usize = 64;
pub const MAX_PRODUCTION_EXTRACTION_MATERIAL_CREDITS: usize = 4;

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_RESULT_BYTES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionExtractionExpectedVersionsV1 {
    pub account: u64,
    pub character: u64,
    pub world: u64,
    pub inventory: u64,
    pub life_metrics: u64,
}

impl ProductionExtractionExpectedVersionsV1 {
    fn validate(self) -> Result<(), PersistenceError> {
        if [
            self.account,
            self.character,
            self.world,
            self.inventory,
            self.life_metrics,
        ]
        .contains(&0)
            || self.character != self.world
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionExtractionCommitRequestV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub mutation_id: [u8; ID_BYTES],
    pub terminal_id: [u8; ID_BYTES],
    pub extraction_request_id: [u8; ID_BYTES],
    pub extraction_receipt_id: [u8; ID_BYTES],
    pub encounter_id: [u8; ID_BYTES],
    pub instance_lineage_id: [u8; ID_BYTES],
    pub entry_restore_point_id: [u8; ID_BYTES],
    pub exit_instance_id: [u8; ID_BYTES],
    pub expected_versions: ProductionExtractionExpectedVersionsV1,
    pub content_revision: StoredWorldFlowRevisionV1,
    pub issued_at_unix_ms: u64,
    pub observed_tick: u64,
}

impl ProductionExtractionCommitRequestV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.contract_version != PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || [
                self.account_id,
                self.character_id,
                self.mutation_id,
                self.terminal_id,
                self.extraction_request_id,
                self.extraction_receipt_id,
                self.encounter_id,
                self.instance_lineage_id,
                self.entry_restore_point_id,
                self.exit_instance_id,
            ]
            .contains(&[0; ID_BYTES])
            || !pairwise_distinct(&[
                self.mutation_id,
                self.terminal_id,
                self.extraction_request_id,
                self.extraction_receipt_id,
            ])
            || self.issued_at_unix_ms == 0
            || self.observed_tick == 0
            || !valid_revision(&self.content_revision)
        {
            return Err(corrupt());
        }
        self.expected_versions.validate()
    }

    pub fn canonical_hash(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        self.validate()?;
        canonical_hash("gravebound.production-extraction-request.v1", self)
    }
}

/// Read-only repository preparation for one exact production extraction.
///
/// The shared terminal coordinator binds both hashes before it chooses a winner. Commit then
/// replans under the same `PostgreSQL` locks and rejects any intervening inventory/material change
/// before the first durable write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedProductionExtractionV1 {
    request: ProductionExtractionCommitRequestV1,
    canonical_request_hash: [u8; HASH_BYTES],
    canonical_plan_hash: [u8; HASH_BYTES],
    replayed: bool,
}

impl PreparedProductionExtractionV1 {
    pub(crate) fn new(
        request: ProductionExtractionCommitRequestV1,
        canonical_request_hash: [u8; HASH_BYTES],
        canonical_plan_hash: [u8; HASH_BYTES],
        replayed: bool,
    ) -> Result<Self, PersistenceError> {
        let prepared = Self {
            request,
            canonical_request_hash,
            canonical_plan_hash,
            replayed,
        };
        prepared.validate()?;
        Ok(prepared)
    }

    pub fn validate(&self) -> Result<(), PersistenceError> {
        self.request.validate()?;
        if self.canonical_request_hash == [0; HASH_BYTES]
            || self.canonical_plan_hash == [0; HASH_BYTES]
            || self.request.canonical_hash()? != self.canonical_request_hash
        {
            return Err(corrupt());
        }
        Ok(())
    }

    #[must_use]
    pub const fn request(&self) -> &ProductionExtractionCommitRequestV1 {
        &self.request
    }

    #[must_use]
    pub const fn canonical_request_hash(&self) -> [u8; HASH_BYTES] {
        self.canonical_request_hash
    }

    #[must_use]
    pub const fn canonical_plan_hash(&self) -> [u8; HASH_BYTES] {
        self.canonical_plan_hash
    }

    #[must_use]
    pub const fn replayed(&self) -> bool {
        self.replayed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StoredExtractionLocationV1 {
    Equipped(u8),
    Belt(u8),
    RunBackpack(u8),
    CharacterSafe(u8),
    Vault(u16),
    Overflow(u8),
    ResolutionHold(u8),
}

impl StoredExtractionLocationV1 {
    pub const fn durable_kind(self) -> i16 {
        match self {
            Self::Equipped(_) => 0,
            Self::Belt(_) => 1,
            Self::RunBackpack(_) => 2,
            Self::CharacterSafe(_) => 5,
            Self::Vault(_) => 6,
            Self::Overflow(_) => 8,
            Self::ResolutionHold(_) => 9,
        }
    }

    pub const fn slot_index(self) -> u16 {
        match self {
            Self::Equipped(index)
            | Self::Belt(index)
            | Self::RunBackpack(index)
            | Self::CharacterSafe(index)
            | Self::Overflow(index)
            | Self::ResolutionHold(index) => index as u16,
            Self::Vault(index) => index,
        }
    }

    fn validate(self) -> Result<(), PersistenceError> {
        let valid = match self {
            Self::Equipped(index) => index < 4,
            Self::Belt(index) => index < 2,
            Self::RunBackpack(index) | Self::CharacterSafe(index) | Self::ResolutionHold(index) => {
                index < 8
            }
            Self::Vault(index) => index < 160,
            Self::Overflow(index) => index < 20,
        };
        if valid { Ok(()) } else { Err(corrupt()) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProductionExtractionPlacementV1 {
    pub ordinal: u16,
    pub item_uid: [u8; ID_BYTES],
    pub template_id: String,
    pub item_kind: u8,
    pub source: StoredExtractionLocationV1,
    pub destination: StoredExtractionLocationV1,
    pub pre_item_version: u64,
    pub post_item_version: u64,
    pub ledger_event_id: [u8; ID_BYTES],
}

impl StoredProductionExtractionPlacementV1 {
    fn validate(&self, expected_ordinal: u16) -> Result<(), PersistenceError> {
        if self.ordinal != expected_ordinal
            || self.item_uid == [0; ID_BYTES]
            || self.ledger_event_id == [0; ID_BYTES]
            || !valid_stable_id(&self.template_id)
            || !matches!(self.item_kind, 0 | 1)
            || self.pre_item_version == 0
            || self.pre_item_version.checked_add(1) != Some(self.post_item_version)
        {
            return Err(corrupt());
        }
        self.source.validate()?;
        self.destination.validate()?;
        let legal = match (self.item_kind, self.source, self.destination) {
            (
                0,
                StoredExtractionLocationV1::Equipped(source),
                StoredExtractionLocationV1::Equipped(destination),
            )
            | (
                1,
                StoredExtractionLocationV1::Belt(source),
                StoredExtractionLocationV1::Belt(destination),
            ) => source == destination,
            (
                1,
                StoredExtractionLocationV1::RunBackpack(_),
                StoredExtractionLocationV1::Belt(_)
                | StoredExtractionLocationV1::CharacterSafe(_)
                | StoredExtractionLocationV1::Vault(_)
                | StoredExtractionLocationV1::Overflow(_)
                | StoredExtractionLocationV1::ResolutionHold(_),
            )
            | (
                0,
                StoredExtractionLocationV1::RunBackpack(_),
                StoredExtractionLocationV1::CharacterSafe(_)
                | StoredExtractionLocationV1::Vault(_)
                | StoredExtractionLocationV1::Overflow(_)
                | StoredExtractionLocationV1::ResolutionHold(_),
            ) => true,
            _ => false,
        };
        if legal { Ok(()) } else { Err(corrupt()) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProductionExtractionMaterialCreditV1 {
    pub ordinal: u8,
    pub material_id: String,
    pub credited_quantity: u8,
    pub wallet_cap: u32,
    pub pre_wallet_quantity: u32,
    pub post_wallet_quantity: u32,
    pub pre_wallet_version: u64,
    pub post_wallet_version: u64,
    pub pre_pouch_version: u64,
    pub post_pouch_version: u64,
    pub wallet_ledger_event_id: [u8; ID_BYTES],
}

impl StoredProductionExtractionMaterialCreditV1 {
    fn validate(
        &self,
        expected_ordinal: u8,
        previous_material_id: Option<&str>,
    ) -> Result<(), PersistenceError> {
        if self.ordinal != expected_ordinal
            || !valid_stable_id(&self.material_id)
            || previous_material_id.is_some_and(|previous| previous >= self.material_id.as_str())
            || self.credited_quantity == 0
            || self.credited_quantity > 99
            || self.wallet_cap == 0
            || self.post_wallet_quantity
                != self
                    .pre_wallet_quantity
                    .checked_add(u32::from(self.credited_quantity))
                    .ok_or_else(corrupt)?
            || self.post_wallet_quantity > self.wallet_cap
            || self.pre_wallet_version == 0
            || self.pre_wallet_version.checked_add(1) != Some(self.post_wallet_version)
            || self.pre_pouch_version == 0
            || self.pre_pouch_version.checked_add(1) != Some(self.post_pouch_version)
            || self.wallet_ledger_event_id == [0; ID_BYTES]
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionExtractionVersionAdvanceV1 {
    pub pre: u64,
    pub post: u64,
}

impl ProductionExtractionVersionAdvanceV1 {
    fn validate(self, may_remain: bool) -> Result<(), PersistenceError> {
        if self.pre == 0
            || (self.post != self.pre.checked_add(1).ok_or_else(corrupt)?
                && !(may_remain && self.post == self.pre))
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionExtractionVersionsV1 {
    pub account: ProductionExtractionVersionAdvanceV1,
    pub character: ProductionExtractionVersionAdvanceV1,
    pub world: ProductionExtractionVersionAdvanceV1,
    pub inventory: ProductionExtractionVersionAdvanceV1,
    pub life_metrics: ProductionExtractionVersionAdvanceV1,
}

impl ProductionExtractionVersionsV1 {
    fn validate(self) -> Result<(), PersistenceError> {
        self.account.validate(true)?;
        self.character.validate(false)?;
        self.world.validate(false)?;
        self.inventory.validate(false)?;
        self.life_metrics.validate(false)?;
        if self.character != self.world {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProductionExtractionResultV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub mutation_id: [u8; ID_BYTES],
    pub terminal_id: [u8; ID_BYTES],
    pub extraction_request_id: [u8; ID_BYTES],
    pub extraction_receipt_id: [u8; ID_BYTES],
    pub canonical_request_hash: [u8; HASH_BYTES],
    pub canonical_plan_hash: [u8; HASH_BYTES],
    pub result_code: u8,
    pub issued_at_unix_ms: u64,
    pub observed_tick: u64,
    pub committed_at_unix_ms: u64,
    pub destination_content_id: String,
    pub versions: ProductionExtractionVersionsV1,
    pub placements: Vec<StoredProductionExtractionPlacementV1>,
    pub material_credits: Vec<StoredProductionExtractionMaterialCreditV1>,
    pub storage_resolution_required: bool,
}

impl StoredProductionExtractionResultV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.contract_version != PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || [
                self.account_id,
                self.character_id,
                self.mutation_id,
                self.terminal_id,
                self.extraction_request_id,
                self.extraction_receipt_id,
            ]
            .contains(&[0; ID_BYTES])
            || !pairwise_distinct(&[
                self.mutation_id,
                self.terminal_id,
                self.extraction_request_id,
                self.extraction_receipt_id,
            ])
            || self.canonical_request_hash == [0; HASH_BYTES]
            || self.canonical_plan_hash == [0; HASH_BYTES]
            || self.result_code != 1
            || self.issued_at_unix_ms == 0
            || self.observed_tick == 0
            || self.committed_at_unix_ms < self.issued_at_unix_ms
            || self.destination_content_id != PRODUCTION_EXTRACTION_HALL_ID
            || self.placements.len() > MAX_PRODUCTION_EXTRACTION_PLACEMENTS
            || self.material_credits.len() > MAX_PRODUCTION_EXTRACTION_MATERIAL_CREDITS
        {
            return Err(corrupt());
        }
        self.versions.validate()?;
        let mut item_uids = BTreeSet::new();
        let mut has_hold = false;
        for (index, placement) in self.placements.iter().enumerate() {
            placement.validate(u16::try_from(index).map_err(|_| corrupt())?)?;
            if !item_uids.insert(placement.item_uid) {
                return Err(corrupt());
            }
            has_hold |= matches!(
                placement.destination,
                StoredExtractionLocationV1::ResolutionHold(_)
            );
        }
        let mut previous_material = None;
        for (index, credit) in self.material_credits.iter().enumerate() {
            credit.validate(
                u8::try_from(index).map_err(|_| corrupt())?,
                previous_material,
            )?;
            previous_material = Some(credit.material_id.as_str());
        }
        if self.storage_resolution_required != has_hold {
            return Err(corrupt());
        }
        if self.canonical_plan_hash
            != canonical_production_extraction_plan_hash_v1(
                &self.placements,
                &self.material_credits,
            )?
        {
            return Err(corrupt());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, PersistenceError> {
        self.validate()?;
        let payload = postcard::to_stdvec(self).map_err(|_| corrupt())?;
        if payload.is_empty() || payload.len() > MAX_RESULT_BYTES {
            return Err(corrupt());
        }
        Ok(payload)
    }

    pub fn decode(payload: &[u8]) -> Result<Self, PersistenceError> {
        if payload.is_empty() || payload.len() > MAX_RESULT_BYTES {
            return Err(corrupt());
        }
        let result: Self = postcard::from_bytes(payload).map_err(|_| corrupt())?;
        result.validate()?;
        Ok(result)
    }

    pub fn digest(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        canonical_hash("gravebound.production-extraction-result.v1", self)
    }
}

pub fn canonical_production_extraction_plan_hash_v1(
    placements: &[StoredProductionExtractionPlacementV1],
    material_credits: &[StoredProductionExtractionMaterialCreditV1],
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    canonical_hash(
        "gravebound.production-extraction-plan.v1",
        &(placements, material_credits),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionExtractionTransactionV1 {
    Fresh(StoredProductionExtractionResultV1),
    Replayed(StoredProductionExtractionResultV1),
    Conflict {
        extraction_request_id: [u8; ID_BYTES],
        terminal_id: [u8; ID_BYTES],
    },
}

fn canonical_hash<T: Serialize>(
    context: &str,
    value: &T,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let payload = postcard::to_stdvec(value).map_err(|_| corrupt())?;
    if payload.is_empty() || payload.len() > MAX_RESULT_BYTES {
        return Err(corrupt());
    }
    let mut hasher = blake3::Hasher::new_derive_key(context);
    hasher.update(&(payload.len() as u64).to_be_bytes());
    hasher.update(&payload);
    Ok(*hasher.finalize().as_bytes())
}

fn valid_revision(revision: &StoredWorldFlowRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .into_iter()
    .all(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn valid_stable_id(value: &str) -> bool {
    (3..=96).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn pairwise_distinct(values: &[[u8; ID_BYTES]]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(index, value)| !values[index + 1..].contains(value))
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredExtraction
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revision() -> StoredWorldFlowRevisionV1 {
        StoredWorldFlowRevisionV1 {
            records_blake3: "a".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
        }
    }

    fn request() -> ProductionExtractionCommitRequestV1 {
        ProductionExtractionCommitRequestV1 {
            contract_version: PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            terminal_id: [4; 16],
            extraction_request_id: [5; 16],
            extraction_receipt_id: [6; 16],
            encounter_id: [7; 16],
            instance_lineage_id: [8; 16],
            entry_restore_point_id: [9; 16],
            exit_instance_id: [10; 16],
            expected_versions: ProductionExtractionExpectedVersionsV1 {
                account: 1,
                character: 2,
                world: 2,
                inventory: 3,
                life_metrics: 4,
            },
            content_revision: revision(),
            issued_at_unix_ms: 10,
            observed_tick: 20,
        }
    }

    fn result() -> StoredProductionExtractionResultV1 {
        let request = request();
        let placements = vec![StoredProductionExtractionPlacementV1 {
            ordinal: 0,
            item_uid: [12; 16],
            template_id: "equipment.test".into(),
            item_kind: 0,
            source: StoredExtractionLocationV1::Equipped(0),
            destination: StoredExtractionLocationV1::Equipped(0),
            pre_item_version: 1,
            post_item_version: 2,
            ledger_event_id: [13; 16],
        }];
        let material_credits = vec![StoredProductionExtractionMaterialCreditV1 {
            ordinal: 0,
            material_id: "material.bell_brass".into(),
            credited_quantity: 2,
            wallet_cap: 999,
            pre_wallet_quantity: 3,
            post_wallet_quantity: 5,
            pre_wallet_version: 1,
            post_wallet_version: 2,
            pre_pouch_version: 7,
            post_pouch_version: 8,
            wallet_ledger_event_id: [14; 16],
        }];
        let canonical_plan_hash =
            canonical_production_extraction_plan_hash_v1(&placements, &material_credits).unwrap();
        StoredProductionExtractionResultV1 {
            contract_version: PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            terminal_id: request.terminal_id,
            extraction_request_id: request.extraction_request_id,
            extraction_receipt_id: request.extraction_receipt_id,
            canonical_request_hash: request.canonical_hash().unwrap(),
            canonical_plan_hash,
            result_code: 1,
            issued_at_unix_ms: request.issued_at_unix_ms,
            observed_tick: request.observed_tick,
            committed_at_unix_ms: 30,
            destination_content_id: PRODUCTION_EXTRACTION_HALL_ID.into(),
            versions: ProductionExtractionVersionsV1 {
                account: ProductionExtractionVersionAdvanceV1 { pre: 1, post: 1 },
                character: ProductionExtractionVersionAdvanceV1 { pre: 2, post: 3 },
                world: ProductionExtractionVersionAdvanceV1 { pre: 2, post: 3 },
                inventory: ProductionExtractionVersionAdvanceV1 { pre: 3, post: 4 },
                life_metrics: ProductionExtractionVersionAdvanceV1 { pre: 4, post: 5 },
            },
            placements,
            material_credits,
            storage_resolution_required: false,
        }
    }

    #[test]
    fn request_hash_binds_every_terminal_and_version_axis() {
        let request = request();
        request.validate().unwrap();
        let digest = request.canonical_hash().unwrap();
        let mut changed = request;
        changed.expected_versions.inventory += 1;
        assert_ne!(changed.canonical_hash().unwrap(), digest);
    }

    #[test]
    fn prepared_authority_binds_the_request_and_plan_hashes() {
        let request = request();
        let request_hash = request.canonical_hash().unwrap();
        let plan_hash = [44; 32];
        let prepared =
            PreparedProductionExtractionV1::new(request.clone(), request_hash, plan_hash, false)
                .unwrap();
        assert_eq!(prepared.request(), &request);
        assert_eq!(prepared.canonical_request_hash(), request_hash);
        assert_eq!(prepared.canonical_plan_hash(), plan_hash);
        assert!(!prepared.replayed());

        assert!(matches!(
            PreparedProductionExtractionV1::new(request.clone(), [0; 32], plan_hash, false),
            Err(PersistenceError::CorruptStoredExtraction)
        ));
        assert!(matches!(
            PreparedProductionExtractionV1::new(request, request_hash, [0; 32], false),
            Err(PersistenceError::CorruptStoredExtraction)
        ));
    }

    #[test]
    fn stored_result_round_trips_and_rejects_noncanonical_projection() {
        let result = result();
        let payload = result.encode().unwrap();
        assert_eq!(
            StoredProductionExtractionResultV1::decode(&payload).unwrap(),
            result
        );
        assert_ne!(result.digest().unwrap(), [0; 32]);

        let mut bad = result.clone();
        bad.placements[0].destination = StoredExtractionLocationV1::Vault(0);
        assert!(matches!(
            bad.validate(),
            Err(PersistenceError::CorruptStoredExtraction)
        ));

        let mut bad = result;
        bad.storage_resolution_required = true;
        assert!(matches!(
            bad.validate(),
            Err(PersistenceError::CorruptStoredExtraction)
        ));
    }
}
