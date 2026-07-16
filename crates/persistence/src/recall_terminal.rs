//! Versioned production contract for explicit and `LinkLost` Emergency Recall.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-010`, `LOOT-002`,
//! `LOOT-033`, `LOOT-060`, and `TECH-015`/`021`-`023`;
//! `Gravebound_Content_Production_Spec_v1.md` `CONT-HUB-001`/`002`, the Core
//! microrealm/dungeon/boss route, and `CONT-VALID-001`; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08`, plus accepted
//! `SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md`.
//!
//! The client may start or cancel an explicit channel. It never authors the terminal identity,
//! completion tick, loss plan, post versions, or stored result. `LinkLost` uses the same durable
//! loss result with a distinct trigger and exact ninety-tick vulnerable window.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{PersistenceError, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE};

pub const PRODUCTION_RECALL_CONTRACT_VERSION_V1: u16 = 1;
pub const PRODUCTION_RECALL_EXPLICIT_TERMINAL_KIND: u8 = 3;
pub const PRODUCTION_RECALL_LINK_LOST_TERMINAL_KIND: u8 = 4;
pub const PRODUCTION_RECALL_HALL_ID: &str = "hub.lantern_halls_01";
pub const PRODUCTION_RECALL_EXPLICIT_CHANNEL_TICKS: u64 = 12;
pub const PRODUCTION_RECALL_LINK_LOST_TICKS: u64 = 90;
pub const MAX_PRODUCTION_RECALL_STABILIZED_ITEMS: usize = 16;
pub const MAX_PRODUCTION_RECALL_DESTROYED_ITEMS: usize = 4_096;
pub const MAX_PRODUCTION_RECALL_DESTROYED_MATERIALS: usize = 4;

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_RESULT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductionRecallTriggerV1 {
    Explicit,
    LinkLost,
}

impl ProductionRecallTriggerV1 {
    #[must_use]
    pub const fn terminal_kind(self) -> u8 {
        match self {
            Self::Explicit => PRODUCTION_RECALL_EXPLICIT_TERMINAL_KIND,
            Self::LinkLost => PRODUCTION_RECALL_LINK_LOST_TERMINAL_KIND,
        }
    }

    #[must_use]
    pub const fn channel_ticks(self) -> u64 {
        match self {
            Self::Explicit => PRODUCTION_RECALL_EXPLICIT_CHANNEL_TICKS,
            Self::LinkLost => PRODUCTION_RECALL_LINK_LOST_TICKS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionRecallExpectedVersionsV1 {
    pub account: u64,
    pub character: u64,
    pub world: u64,
    pub inventory: u64,
    pub life_metrics: u64,
    pub progression: u64,
    pub oath_bargain: u64,
    pub ash_wallet: u64,
}

impl ProductionRecallExpectedVersionsV1 {
    fn validate(self) -> Result<(), PersistenceError> {
        if [
            self.account,
            self.character,
            self.world,
            self.inventory,
            self.life_metrics,
            self.progression,
            self.oath_bargain,
            self.ash_wallet,
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
pub struct ProductionRecallCommitRequestV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub mutation_id: [u8; ID_BYTES],
    pub terminal_id: [u8; ID_BYTES],
    pub trigger: ProductionRecallTriggerV1,
    pub request_sequence: Option<u32>,
    pub instance_lineage_id: [u8; ID_BYTES],
    pub entry_restore_point_id: [u8; ID_BYTES],
    pub expected_versions: ProductionRecallExpectedVersionsV1,
    pub content_revision: StoredWorldFlowRevisionV1,
    pub issued_at_unix_ms: u64,
    pub trigger_started_tick: u64,
    pub completion_tick: u64,
    pub final_lifetime_ticks: u64,
    pub final_permadeath_combat_ticks: u64,
}

impl ProductionRecallCommitRequestV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        let request_binding_valid = match self.trigger {
            ProductionRecallTriggerV1::Explicit => {
                self.request_sequence.is_some_and(|sequence| sequence != 0)
            }
            ProductionRecallTriggerV1::LinkLost => self.request_sequence.is_none(),
        };
        if self.contract_version != PRODUCTION_RECALL_CONTRACT_VERSION_V1
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || [
                self.account_id,
                self.character_id,
                self.mutation_id,
                self.terminal_id,
                self.instance_lineage_id,
                self.entry_restore_point_id,
            ]
            .contains(&[0; ID_BYTES])
            || self.mutation_id == self.terminal_id
            || !request_binding_valid
            || self.issued_at_unix_ms == 0
            || self.trigger_started_tick == 0
            || self
                .trigger_started_tick
                .checked_add(self.trigger.channel_ticks())
                != Some(self.completion_tick)
            || !valid_revision(&self.content_revision)
        {
            return Err(corrupt());
        }
        self.expected_versions.validate()
    }

    pub fn canonical_hash(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        self.validate()?;
        canonical_hash(
            "gravebound.production-recall-request.v1",
            self,
            MAX_RESULT_BYTES,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedProductionRecallV1 {
    request: ProductionRecallCommitRequestV1,
    canonical_request_hash: [u8; HASH_BYTES],
    canonical_plan_hash: [u8; HASH_BYTES],
    replayed: bool,
}

impl PreparedProductionRecallV1 {
    pub fn seal(
        request: ProductionRecallCommitRequestV1,
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
    pub const fn request(&self) -> &ProductionRecallCommitRequestV1 {
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
pub enum StoredRecallLocationV1 {
    Equipped(u8),
    Belt(u8),
    RunBackpack(u8),
    PersonalGround {
        instance_id: [u8; ID_BYTES],
        pickup_id: [u8; ID_BYTES],
        expires_at_tick: u64,
    },
}

impl StoredRecallLocationV1 {
    #[must_use]
    pub const fn durable_kind(self) -> i16 {
        match self {
            Self::Equipped(_) => 0,
            Self::Belt(_) => 1,
            Self::RunBackpack(_) => 2,
            Self::PersonalGround { .. } => 3,
        }
    }

    #[must_use]
    pub const fn slot_index(self) -> Option<u16> {
        match self {
            Self::Equipped(index) | Self::Belt(index) | Self::RunBackpack(index) => {
                Some(index as u16)
            }
            Self::PersonalGround { .. } => None,
        }
    }

    #[must_use]
    pub const fn instance_id(self) -> Option<[u8; ID_BYTES]> {
        match self {
            Self::PersonalGround { instance_id, .. } => Some(instance_id),
            _ => None,
        }
    }

    #[must_use]
    pub const fn pickup_id(self) -> Option<[u8; ID_BYTES]> {
        match self {
            Self::PersonalGround { pickup_id, .. } => Some(pickup_id),
            _ => None,
        }
    }

    #[must_use]
    pub const fn expires_at_tick(self) -> Option<u64> {
        match self {
            Self::PersonalGround {
                expires_at_tick, ..
            } => Some(expires_at_tick),
            _ => None,
        }
    }

    fn validate(self) -> Result<(), PersistenceError> {
        let valid = match self {
            Self::Equipped(index) => index < 4,
            Self::Belt(index) => index < 2,
            Self::RunBackpack(index) => index < 8,
            Self::PersonalGround {
                instance_id,
                pickup_id,
                expires_at_tick,
            } => instance_id != [0; ID_BYTES] && pickup_id != [0; ID_BYTES] && expires_at_tick > 0,
        };
        if valid { Ok(()) } else { Err(corrupt()) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProductionRecallItemV1 {
    pub ordinal: u16,
    pub item_uid: [u8; ID_BYTES],
    pub template_id: String,
    pub content_revision: String,
    pub item_kind: u8,
    pub source: StoredRecallLocationV1,
    pub pre_item_version: u64,
    pub post_item_version: u64,
    pub ledger_event_id: [u8; ID_BYTES],
}

impl StoredProductionRecallItemV1 {
    fn validate(
        &self,
        expected_ordinal: u16,
        disposition: RecallItemDisposition,
    ) -> Result<(), PersistenceError> {
        if self.ordinal != expected_ordinal
            || self.item_uid == [0; ID_BYTES]
            || self.ledger_event_id == [0; ID_BYTES]
            || !valid_stable_id(&self.template_id)
            || !valid_content_revision(&self.content_revision)
            || !matches!(self.item_kind, 0 | 1)
            || self.pre_item_version == 0
            || self.pre_item_version.checked_add(1) != Some(self.post_item_version)
        {
            return Err(corrupt());
        }
        self.source.validate()?;
        let legal = matches!(
            (disposition, self.item_kind, self.source),
            (
                RecallItemDisposition::Stabilized,
                0,
                StoredRecallLocationV1::Equipped(_)
            ) | (
                RecallItemDisposition::Stabilized,
                1,
                StoredRecallLocationV1::Belt(_)
            ) | (
                RecallItemDisposition::Destroyed,
                0 | 1,
                StoredRecallLocationV1::RunBackpack(_)
                    | StoredRecallLocationV1::PersonalGround { .. },
            )
        );
        if legal { Ok(()) } else { Err(corrupt()) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecallItemDisposition {
    Stabilized,
    Destroyed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProductionRecallMaterialDestructionV1 {
    pub ordinal: u8,
    pub material_id: String,
    pub destroyed_quantity: u8,
    pub pre_pouch_version: u64,
    pub post_pouch_version: u64,
    pub destruction_event_id: [u8; ID_BYTES],
}

impl StoredProductionRecallMaterialDestructionV1 {
    fn validate(
        &self,
        expected_ordinal: u8,
        previous_material_id: Option<&str>,
    ) -> Result<(), PersistenceError> {
        if self.ordinal != expected_ordinal
            || !valid_stable_id(&self.material_id)
            || previous_material_id.is_some_and(|previous| previous >= self.material_id.as_str())
            || self.destroyed_quantity == 0
            || self.destroyed_quantity > 99
            || self.pre_pouch_version == 0
            || self.pre_pouch_version.checked_add(1) != Some(self.post_pouch_version)
            || self.destruction_event_id == [0; ID_BYTES]
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionRecallVersionAdvanceV1 {
    pub pre: u64,
    pub post: u64,
}

impl ProductionRecallVersionAdvanceV1 {
    fn validate(self, unchanged: bool) -> Result<(), PersistenceError> {
        if self.pre == 0
            || (unchanged && self.post != self.pre)
            || (!unchanged && self.pre.checked_add(1) != Some(self.post))
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionRecallVersionsV1 {
    pub account: ProductionRecallVersionAdvanceV1,
    pub character: ProductionRecallVersionAdvanceV1,
    pub world: ProductionRecallVersionAdvanceV1,
    pub inventory: ProductionRecallVersionAdvanceV1,
    pub life_metrics: ProductionRecallVersionAdvanceV1,
    pub progression: ProductionRecallVersionAdvanceV1,
    pub oath_bargain: ProductionRecallVersionAdvanceV1,
    pub ash_wallet: ProductionRecallVersionAdvanceV1,
}

impl ProductionRecallVersionsV1 {
    fn validate(self) -> Result<(), PersistenceError> {
        self.account.validate(true)?;
        self.character.validate(false)?;
        self.world.validate(false)?;
        self.inventory.validate(false)?;
        self.life_metrics.validate(false)?;
        self.progression.validate(true)?;
        self.oath_bargain.validate(true)?;
        self.ash_wallet.validate(true)?;
        if self.character != self.world {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProductionRecallResultV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub mutation_id: [u8; ID_BYTES],
    pub terminal_id: [u8; ID_BYTES],
    pub canonical_request_hash: [u8; HASH_BYTES],
    pub canonical_plan_hash: [u8; HASH_BYTES],
    pub result_code: u8,
    pub trigger: ProductionRecallTriggerV1,
    pub request_sequence: Option<u32>,
    pub issued_at_unix_ms: u64,
    pub trigger_started_tick: u64,
    pub completion_tick: u64,
    pub committed_at_unix_ms: u64,
    pub source_content_id: String,
    pub destination_content_id: String,
    pub versions: ProductionRecallVersionsV1,
    pub pre_lifetime_ticks: u64,
    pub post_lifetime_ticks: u64,
    pub pre_permadeath_combat_ticks: u64,
    pub post_permadeath_combat_ticks: u64,
    pub stabilized_items: Vec<StoredProductionRecallItemV1>,
    pub destroyed_items: Vec<StoredProductionRecallItemV1>,
    pub destroyed_materials: Vec<StoredProductionRecallMaterialDestructionV1>,
}

impl StoredProductionRecallResultV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        let request_binding_valid = match self.trigger {
            ProductionRecallTriggerV1::Explicit => {
                self.request_sequence.is_some_and(|sequence| sequence != 0)
            }
            ProductionRecallTriggerV1::LinkLost => self.request_sequence.is_none(),
        };
        if self.contract_version != PRODUCTION_RECALL_CONTRACT_VERSION_V1
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || [
                self.account_id,
                self.character_id,
                self.mutation_id,
                self.terminal_id,
            ]
            .contains(&[0; ID_BYTES])
            || self.mutation_id == self.terminal_id
            || self.canonical_request_hash == [0; HASH_BYTES]
            || self.canonical_plan_hash == [0; HASH_BYTES]
            || self.result_code != 1
            || !request_binding_valid
            || self.issued_at_unix_ms == 0
            || self.trigger_started_tick == 0
            || self
                .trigger_started_tick
                .checked_add(self.trigger.channel_ticks())
                != Some(self.completion_tick)
            || self.committed_at_unix_ms < self.issued_at_unix_ms
            || !valid_stable_id(&self.source_content_id)
            || self.destination_content_id != PRODUCTION_RECALL_HALL_ID
            || self.stabilized_items.len() > MAX_PRODUCTION_RECALL_STABILIZED_ITEMS
            || self.destroyed_items.len() > MAX_PRODUCTION_RECALL_DESTROYED_ITEMS
            || self.destroyed_materials.len() > MAX_PRODUCTION_RECALL_DESTROYED_MATERIALS
        {
            return Err(corrupt());
        }
        if self.post_lifetime_ticks < self.pre_lifetime_ticks
            || self.post_permadeath_combat_ticks < self.pre_permadeath_combat_ticks
        {
            return Err(corrupt());
        }
        self.versions.validate()?;
        let mut item_uids = BTreeSet::new();
        for (index, item) in self.stabilized_items.iter().enumerate() {
            item.validate(
                u16::try_from(index).map_err(|_| corrupt())?,
                RecallItemDisposition::Stabilized,
            )?;
            if !item_uids.insert(item.item_uid) {
                return Err(corrupt());
            }
        }
        for (index, item) in self.destroyed_items.iter().enumerate() {
            item.validate(
                u16::try_from(index).map_err(|_| corrupt())?,
                RecallItemDisposition::Destroyed,
            )?;
            if !item_uids.insert(item.item_uid) {
                return Err(corrupt());
            }
        }
        let mut previous_material = None;
        for (index, material) in self.destroyed_materials.iter().enumerate() {
            material.validate(
                u8::try_from(index).map_err(|_| corrupt())?,
                previous_material,
            )?;
            previous_material = Some(material.material_id.as_str());
        }
        if self.canonical_plan_hash
            != canonical_production_recall_plan_hash_v1(
                &self.stabilized_items,
                &self.destroyed_items,
                &self.destroyed_materials,
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
        canonical_hash(
            "gravebound.production-recall-result.v1",
            self,
            MAX_RESULT_BYTES,
        )
    }

    pub fn stabilized_items_digest(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        canonical_hash(
            "gravebound.production-recall-stabilized-items.v1",
            &self.stabilized_items,
            MAX_RESULT_BYTES,
        )
    }

    pub fn destroyed_items_digest(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        canonical_hash(
            "gravebound.production-recall-destroyed-items.v1",
            &self.destroyed_items,
            MAX_RESULT_BYTES,
        )
    }

    pub fn destroyed_materials_digest(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        canonical_hash(
            "gravebound.production-recall-destroyed-materials.v1",
            &self.destroyed_materials,
            MAX_RESULT_BYTES,
        )
    }
}

pub fn canonical_production_recall_plan_hash_v1(
    stabilized_items: &[StoredProductionRecallItemV1],
    destroyed_items: &[StoredProductionRecallItemV1],
    destroyed_materials: &[StoredProductionRecallMaterialDestructionV1],
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    canonical_hash(
        "gravebound.production-recall-plan.v1",
        &(stabilized_items, destroyed_items, destroyed_materials),
        MAX_RESULT_BYTES,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionRecallTransactionV1 {
    Fresh(StoredProductionRecallResultV1),
    Replayed(StoredProductionRecallResultV1),
    Conflict { terminal_id: [u8; ID_BYTES] },
}

impl ProductionRecallTransactionV1 {
    #[must_use]
    pub const fn result(&self) -> Option<&StoredProductionRecallResultV1> {
        match self {
            Self::Fresh(result) | Self::Replayed(result) => Some(result),
            Self::Conflict { .. } => None,
        }
    }

    #[must_use]
    pub const fn is_replay(&self) -> bool {
        matches!(self, Self::Replayed(_))
    }
}

fn canonical_hash<T: Serialize>(
    context: &str,
    value: &T,
    maximum_bytes: usize,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let payload = postcard::to_stdvec(value).map_err(|_| corrupt())?;
    if payload.is_empty() || payload.len() > maximum_bytes {
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

fn valid_content_revision(value: &str) -> bool {
    (3..=128).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredRecall
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

    fn item_revision() -> String {
        format!("core-dev.blake3.{}", "a".repeat(64))
    }

    fn request(trigger: ProductionRecallTriggerV1) -> ProductionRecallCommitRequestV1 {
        let trigger_started_tick = 20;
        ProductionRecallCommitRequestV1 {
            contract_version: PRODUCTION_RECALL_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            terminal_id: [4; 16],
            trigger,
            request_sequence: match trigger {
                ProductionRecallTriggerV1::Explicit => Some(5),
                ProductionRecallTriggerV1::LinkLost => None,
            },
            instance_lineage_id: [6; 16],
            entry_restore_point_id: [7; 16],
            expected_versions: ProductionRecallExpectedVersionsV1 {
                account: 1,
                character: 2,
                world: 2,
                inventory: 3,
                life_metrics: 4,
                progression: 5,
                oath_bargain: 6,
                ash_wallet: 7,
            },
            content_revision: revision(),
            issued_at_unix_ms: 10,
            trigger_started_tick,
            completion_tick: trigger_started_tick + trigger.channel_ticks(),
            final_lifetime_ticks: 1_000,
            final_permadeath_combat_ticks: 500,
        }
    }

    fn result(trigger: ProductionRecallTriggerV1) -> StoredProductionRecallResultV1 {
        let request = request(trigger);
        let stabilized_items = vec![StoredProductionRecallItemV1 {
            ordinal: 0,
            item_uid: [8; 16],
            template_id: "equipment.test".into(),
            content_revision: item_revision(),
            item_kind: 0,
            source: StoredRecallLocationV1::Equipped(0),
            pre_item_version: 1,
            post_item_version: 2,
            ledger_event_id: [9; 16],
        }];
        let destroyed_items = vec![StoredProductionRecallItemV1 {
            ordinal: 0,
            item_uid: [10; 16],
            template_id: "consumable.red_tonic".into(),
            content_revision: item_revision(),
            item_kind: 1,
            source: StoredRecallLocationV1::PersonalGround {
                instance_id: [11; 16],
                pickup_id: [12; 16],
                expires_at_tick: 1_000,
            },
            pre_item_version: 3,
            post_item_version: 4,
            ledger_event_id: [13; 16],
        }];
        let destroyed_materials = vec![StoredProductionRecallMaterialDestructionV1 {
            ordinal: 0,
            material_id: "material.bell_brass".into(),
            destroyed_quantity: 2,
            pre_pouch_version: 7,
            post_pouch_version: 8,
            destruction_event_id: [14; 16],
        }];
        let canonical_plan_hash = canonical_production_recall_plan_hash_v1(
            &stabilized_items,
            &destroyed_items,
            &destroyed_materials,
        )
        .unwrap();
        StoredProductionRecallResultV1 {
            contract_version: PRODUCTION_RECALL_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            terminal_id: request.terminal_id,
            canonical_request_hash: request.canonical_hash().unwrap(),
            canonical_plan_hash,
            result_code: 1,
            trigger,
            request_sequence: request.request_sequence,
            issued_at_unix_ms: request.issued_at_unix_ms,
            trigger_started_tick: request.trigger_started_tick,
            completion_tick: request.completion_tick,
            committed_at_unix_ms: 30,
            source_content_id: "dungeon.bell_sepulcher".into(),
            destination_content_id: PRODUCTION_RECALL_HALL_ID.into(),
            versions: ProductionRecallVersionsV1 {
                account: ProductionRecallVersionAdvanceV1 { pre: 1, post: 1 },
                character: ProductionRecallVersionAdvanceV1 { pre: 2, post: 3 },
                world: ProductionRecallVersionAdvanceV1 { pre: 2, post: 3 },
                inventory: ProductionRecallVersionAdvanceV1 { pre: 3, post: 4 },
                life_metrics: ProductionRecallVersionAdvanceV1 { pre: 4, post: 5 },
                progression: ProductionRecallVersionAdvanceV1 { pre: 5, post: 5 },
                oath_bargain: ProductionRecallVersionAdvanceV1 { pre: 6, post: 6 },
                ash_wallet: ProductionRecallVersionAdvanceV1 { pre: 7, post: 7 },
            },
            pre_lifetime_ticks: 988,
            post_lifetime_ticks: request.final_lifetime_ticks,
            pre_permadeath_combat_ticks: 488,
            post_permadeath_combat_ticks: request.final_permadeath_combat_ticks,
            stabilized_items,
            destroyed_items,
            destroyed_materials,
        }
    }

    #[test]
    fn explicit_and_link_lost_requests_bind_their_exact_timing_and_sequence_shape() {
        let explicit = request(ProductionRecallTriggerV1::Explicit);
        explicit.validate().unwrap();
        let explicit_hash = explicit.canonical_hash().unwrap();
        let link_lost = request(ProductionRecallTriggerV1::LinkLost);
        link_lost.validate().unwrap();
        assert_ne!(link_lost.canonical_hash().unwrap(), explicit_hash);

        let mut invalid = explicit;
        invalid.completion_tick -= 1;
        assert!(matches!(
            invalid.validate(),
            Err(PersistenceError::CorruptStoredRecall)
        ));

        let mut invalid = link_lost;
        invalid.request_sequence = Some(1);
        assert!(matches!(
            invalid.validate(),
            Err(PersistenceError::CorruptStoredRecall)
        ));
    }

    #[test]
    fn prepared_recall_binds_request_and_plan_hashes() {
        let request = request(ProductionRecallTriggerV1::Explicit);
        let request_hash = request.canonical_hash().unwrap();
        let prepared =
            PreparedProductionRecallV1::seal(request.clone(), request_hash, [8; 32], false)
                .unwrap();
        assert_eq!(prepared.request(), &request);
        assert_eq!(prepared.canonical_request_hash(), request_hash);
        assert_eq!(prepared.canonical_plan_hash(), [8; 32]);
        assert!(!prepared.replayed());

        assert!(matches!(
            PreparedProductionRecallV1::seal(request, request_hash, [0; 32], false),
            Err(PersistenceError::CorruptStoredRecall)
        ));
    }

    #[test]
    fn stored_results_round_trip_and_expose_bounded_projection_digests() {
        for trigger in [
            ProductionRecallTriggerV1::Explicit,
            ProductionRecallTriggerV1::LinkLost,
        ] {
            let result = result(trigger);
            let payload = result.encode().unwrap();
            assert_eq!(
                StoredProductionRecallResultV1::decode(&payload).unwrap(),
                result
            );
            assert_ne!(result.digest().unwrap(), [0; 32]);
            assert_ne!(result.stabilized_items_digest().unwrap(), [0; 32]);
            assert_ne!(result.destroyed_items_digest().unwrap(), [0; 32]);
            assert_ne!(result.destroyed_materials_digest().unwrap(), [0; 32]);
        }
    }

    #[test]
    fn stored_results_reject_cross_disposition_items_and_changed_plan_hashes() {
        let mut bad = result(ProductionRecallTriggerV1::Explicit);
        bad.stabilized_items[0].source = StoredRecallLocationV1::RunBackpack(0);
        assert!(matches!(
            bad.validate(),
            Err(PersistenceError::CorruptStoredRecall)
        ));

        let mut bad = result(ProductionRecallTriggerV1::Explicit);
        bad.canonical_plan_hash = [99; 32];
        assert!(matches!(
            bad.validate(),
            Err(PersistenceError::CorruptStoredRecall)
        ));
    }
}
