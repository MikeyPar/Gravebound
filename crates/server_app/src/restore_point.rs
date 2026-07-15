//! Typed `TECH-023` danger-entry snapshots.
//!
//! `GB-M03-03B` owns only the composition boundary. The normal route remains disabled until
//! progression/inventory and Oath/Bargain packages provide all three transactional providers.

use std::future::Future;

use persistence::{
    PersistenceTransaction, stage_danger_entry_ash_wallet_restore_v3,
    stage_danger_entry_inventory_restore_v3, stage_danger_entry_life_metrics_restore_v3,
    stage_danger_entry_oath_bargain_restore_v3,
};
use protocol::{WireText, WorldFlowContentRevisionV1};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const ID_BYTES: usize = 16;
const CONTENT_ID_BYTES: usize = 96;
const BELT_SLOT_COUNT: usize = 2;
const EQUIPMENT_SLOT_COUNT: usize = 4;
const MAX_BELT_UNITS: usize = 6;
const MAX_ACTIVE_BARGAINS: usize = 3;
const MAX_ENTRY_INVENTORY_ITEMS: usize = 64;
const V2_DIGEST_DOMAIN: &[u8] = b"gravebound.danger-entry-restore.v2\0";
const V3_DIGEST_DOMAIN: &[u8] = b"gravebound.danger-entry-restore.v3\0";

/// Required component order for a V2 crash restoration.
///
/// All providers execute inside one persistence transaction. Progression establishes the exact
/// entry health/XP baseline, inventory restores and audits item security, Oath/Bargains restore
/// the life-long build choices, and life metrics finally roll the permadeath-combat clock back to
/// its entry value without rolling back lifetime evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashRestoreComponentV2 {
    Progression,
    Inventory,
    OathBargains,
    LifeMetrics,
}

pub const CRASH_RESTORE_ORDER_V2: [CrashRestoreComponentV2; 4] = [
    CrashRestoreComponentV2::Progression,
    CrashRestoreComponentV2::Inventory,
    CrashRestoreComponentV2::OathBargains,
    CrashRestoreComponentV2::LifeMetrics,
];

/// Required component order for the accepted `SPEC-CONFLICT-027` V3 contract.
///
/// Ash remains last because its compensating ledger entries depend on the entry authority and
/// danger-bound Bargain provenance captured by the preceding components. All five providers stage
/// inside the same caller-owned transaction; none can publish an independently restored root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashRestoreComponentV3 {
    Progression,
    Inventory,
    OathBargains,
    LifeMetrics,
    AshWallet,
}

pub const CRASH_RESTORE_ORDER_V3: [CrashRestoreComponentV3; 5] = [
    CrashRestoreComponentV3::Progression,
    CrashRestoreComponentV3::Inventory,
    CrashRestoreComponentV3::OathBargains,
    CrashRestoreComponentV3::LifeMetrics,
    CrashRestoreComponentV3::AshWallet,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemUid([u8; ID_BYTES]);

impl ItemUid {
    pub const fn new(bytes: [u8; ID_BYTES]) -> Result<Self, RestorePointError> {
        if all_zero(&bytes) {
            Err(RestorePointError::ZeroItemUid)
        } else {
            Ok(Self(bytes))
        }
    }

    #[must_use]
    pub const fn into_bytes(self) -> [u8; ID_BYTES] {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressionRestoreV1 {
    pub level: u16,
    pub xp: u32,
    pub current_health: u32,
    pub progression_version: u64,
}

impl ProgressionRestoreV1 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if !(1..=20).contains(&self.level)
            || self.current_health == 0
            || self.progression_version == 0
        {
            return Err(RestorePointError::InvalidProgression);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeltStackV1 {
    pub consumable_id: Option<WireText<CONTENT_ID_BYTES>>,
    pub unit_uids: Vec<ItemUid>,
}

impl BeltStackV1 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if self.unit_uids.len() > MAX_BELT_UNITS
            || self.consumable_id.is_some() == self.unit_uids.is_empty()
            || self.unit_uids.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(RestorePointError::InvalidBeltStack);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventorySecurityRestoreV1 {
    /// Weapon, Relic, Armor, Charm order is permanent.
    pub equipment: [Option<ItemUid>; EQUIPMENT_SLOT_COUNT],
    pub belt: [BeltStackV1; BELT_SLOT_COUNT],
    pub inventory_version: u64,
}

impl InventorySecurityRestoreV1 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if self.inventory_version == 0 {
            return Err(RestorePointError::InvalidInventory);
        }
        for stack in &self.belt {
            stack.validate()?;
        }
        let mut identities = self
            .equipment
            .iter()
            .flatten()
            .copied()
            .chain(
                self.belt
                    .iter()
                    .flat_map(|stack| stack.unit_uids.iter().copied()),
            )
            .collect::<Vec<_>>();
        identities.sort_unstable();
        if identities.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RestorePointError::DuplicateItemUid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OathBargainRestoreV1 {
    pub oath_id: Option<WireText<CONTENT_ID_BYTES>>,
    /// Acquisition order is authoritative.
    pub active_bargain_ids: Vec<WireText<CONTENT_ID_BYTES>>,
    pub earned_bargain_slots: u8,
    pub oath_bargain_version: u64,
}

impl OathBargainRestoreV1 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if self.active_bargain_ids.len() > MAX_ACTIVE_BARGAINS
            || usize::from(self.earned_bargain_slots) > MAX_ACTIVE_BARGAINS
            || self.active_bargain_ids.len() > usize::from(self.earned_bargain_slots)
            || self.oath_bargain_version == 0
        {
            return Err(RestorePointError::InvalidOathBargains);
        }
        let mut sorted = self
            .active_bargain_ids
            .iter()
            .map(WireText::as_str)
            .collect::<Vec<_>>();
        sorted.sort_unstable();
        if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RestorePointError::InvalidOathBargains);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeAggregateVersionsV1 {
    pub account_version: u64,
    pub character_version: u64,
    pub progression_version: u64,
    pub inventory_version: u64,
    pub oath_bargain_version: u64,
}

impl SafeAggregateVersionsV1 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if [
            self.account_version,
            self.character_version,
            self.progression_version,
            self.inventory_version,
            self.oath_bargain_version,
        ]
        .contains(&0)
        {
            return Err(RestorePointError::ZeroAggregateVersion);
        }
        Ok(())
    }
}

/// Entry clock evidence required by owner-approved `SPEC-CONFLICT-009`.
///
/// `lifetime_ticks` is immutable crash evidence: restoration must not roll it back. The captured
/// `permadeath_combat_ticks` is the authoritative value restored after an unrecoverable instance
/// crash when no terminal outcome committed first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifeMetricsRestoreV2 {
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub life_metrics_version: u64,
}

impl LifeMetricsRestoreV2 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if self.life_metrics_version == 0 || self.permadeath_combat_ticks > self.lifetime_ticks {
            return Err(RestorePointError::InvalidLifeMetrics);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeAggregateVersionsV2 {
    pub account_version: u64,
    pub character_version: u64,
    pub progression_version: u64,
    pub inventory_version: u64,
    pub oath_bargain_version: u64,
    pub life_metrics_version: u64,
}

impl SafeAggregateVersionsV2 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if [
            self.account_version,
            self.character_version,
            self.progression_version,
            self.inventory_version,
            self.oath_bargain_version,
            self.life_metrics_version,
        ]
        .contains(&0)
        {
            return Err(RestorePointError::ZeroAggregateVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerEntrySnapshotV1 {
    pub character_id: [u8; ID_BYTES],
    pub content_revision: WorldFlowContentRevisionV1,
    pub progression: ProgressionRestoreV1,
    pub inventory: InventorySecurityRestoreV1,
    pub oath_bargains: OathBargainRestoreV1,
    pub versions: SafeAggregateVersionsV1,
}

impl DangerEntrySnapshotV1 {
    pub fn validate(&self) -> Result<(), RestorePointError> {
        if all_zero(&self.character_id) {
            return Err(RestorePointError::ZeroCharacterId);
        }
        self.progression.validate()?;
        self.inventory.validate()?;
        self.oath_bargains.validate()?;
        self.versions.validate()?;
        if self.progression.progression_version != self.versions.progression_version
            || self.inventory.inventory_version != self.versions.inventory_version
            || self.oath_bargains.oath_bargain_version != self.versions.oath_bargain_version
        {
            return Err(RestorePointError::AggregateVersionMismatch);
        }
        Ok(())
    }

    pub fn composite_digest(&self) -> Result<[u8; 32], RestorePointError> {
        self.validate()?;
        let bytes = postcard::to_stdvec(self).map_err(|_| RestorePointError::Encoding)?;
        Ok(*blake3::hash(&bytes).as_bytes())
    }
}

/// Complete `TECH-023` danger-entry restore contract.
///
/// V2 adds the mandatory life-metrics component and binds its version into the safe aggregate
/// envelope. Its canonical digest is domain-separated from V1 and covers every serialized field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerEntrySnapshotV2 {
    pub character_id: [u8; ID_BYTES],
    pub content_revision: WorldFlowContentRevisionV1,
    pub progression: ProgressionRestoreV1,
    pub inventory: InventorySecurityRestoreV1,
    pub oath_bargains: OathBargainRestoreV1,
    pub life_metrics: LifeMetricsRestoreV2,
    pub versions: SafeAggregateVersionsV2,
}

impl DangerEntrySnapshotV2 {
    pub fn validate(&self) -> Result<(), RestorePointError> {
        if all_zero(&self.character_id) {
            return Err(RestorePointError::ZeroCharacterId);
        }
        self.progression.validate()?;
        self.inventory.validate()?;
        self.oath_bargains.validate()?;
        self.life_metrics.validate()?;
        self.versions.validate()?;
        if self.progression.progression_version != self.versions.progression_version
            || self.inventory.inventory_version != self.versions.inventory_version
            || self.oath_bargains.oath_bargain_version != self.versions.oath_bargain_version
            || self.life_metrics.life_metrics_version != self.versions.life_metrics_version
        {
            return Err(RestorePointError::AggregateVersionMismatch);
        }
        Ok(())
    }

    pub fn composite_digest(&self) -> Result<[u8; 32], RestorePointError> {
        self.validate()?;
        let bytes = postcard::to_stdvec(self).map_err(|_| RestorePointError::Encoding)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(V2_DIGEST_DOMAIN);
        hasher.update(&bytes);
        Ok(*hasher.finalize().as_bytes())
    }
}

/// Exact entry location for one V3 inventory identity.
///
/// The discriminants intentionally mirror the durable schema contract. `PersonalGround` is not a
/// valid entry baseline: it can only be a post-entry gain that crash recovery revokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum EntryInventoryLocationV3 {
    Equipment = 0,
    Belt = 1,
    RunBackpack = 2,
}

impl TryFrom<i16> for EntryInventoryLocationV3 {
    type Error = RestorePointError;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Equipment),
            1 => Ok(Self::Belt),
            2 => Ok(Self::RunBackpack),
            _ => Err(RestorePointError::InvalidInventory),
        }
    }
}

/// Security state captured after the atomic danger transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum EntryInventorySecurityV3 {
    AtRiskEquipped = 1,
    AtRiskPending = 2,
}

impl TryFrom<i16> for EntryInventorySecurityV3 {
    type Error = RestorePointError;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::AtRiskEquipped),
            2 => Ok(Self::AtRiskPending),
            _ => Err(RestorePointError::InvalidInventory),
        }
    }
}

/// Complete immutable provenance for one item present at danger entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryBaselineItemV3 {
    pub item_uid: ItemUid,
    pub template_id: WireText<CONTENT_ID_BYTES>,
    pub content_revision: WireText<CONTENT_ID_BYTES>,
    pub creation_kind: u8,
    pub creation_request_id: [u8; ID_BYTES],
    pub roll_index: u16,
    pub unit_ordinal: u16,
    pub provenance_kind: u8,
    pub location: EntryInventoryLocationV3,
    pub slot_index: u8,
    pub item_version: u64,
    pub security: EntryInventorySecurityV3,
}

impl InventoryBaselineItemV3 {
    fn validate(&self) -> Result<(), RestorePointError> {
        let valid_location = match self.location {
            EntryInventoryLocationV3::Equipment => {
                usize::from(self.slot_index) < EQUIPMENT_SLOT_COUNT
                    && self.security == EntryInventorySecurityV3::AtRiskEquipped
            }
            EntryInventoryLocationV3::Belt => {
                usize::from(self.slot_index) < BELT_SLOT_COUNT
                    && self.security == EntryInventorySecurityV3::AtRiskEquipped
            }
            EntryInventoryLocationV3::RunBackpack => {
                self.slot_index < 8 && self.security == EntryInventorySecurityV3::AtRiskPending
            }
        };
        if !valid_location
            || self.item_version == 0
            || all_zero(&self.creation_request_id)
            || self.creation_kind > 3
            || self.provenance_kind > 7
            || !valid_item_content_revision(self.content_revision.as_str())
        {
            return Err(RestorePointError::InvalidInventory);
        }
        Ok(())
    }
}

/// Full V3 inventory baseline, including deliberately risked entry Backpack property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventorySecurityRestoreV3 {
    /// Canonical order is location, slot, then item UID.
    pub baseline_items: Vec<InventoryBaselineItemV3>,
    pub pre_inventory_version: u64,
    pub inventory_version: u64,
    pub safe_placement_count: u16,
}

impl InventorySecurityRestoreV3 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if self.baseline_items.len() > MAX_ENTRY_INVENTORY_ITEMS
            || self.pre_inventory_version == 0
            || self.inventory_version < self.pre_inventory_version
            || self.inventory_version > self.pre_inventory_version.saturating_add(1)
            || self.safe_placement_count > 48
        {
            return Err(RestorePointError::InvalidInventory);
        }
        for item in &self.baseline_items {
            item.validate()?;
        }
        let mut equipment_slots = [false; EQUIPMENT_SLOT_COUNT];
        let mut belt_counts = [0_u8; BELT_SLOT_COUNT];
        let mut belt_templates: [Option<&str>; BELT_SLOT_COUNT] = [None; BELT_SLOT_COUNT];
        let mut backpack_counts = [0_u8; 8];
        let mut backpack_templates: [Option<&str>; 8] = [None; 8];
        for item in &self.baseline_items {
            let slot = usize::from(item.slot_index);
            match item.location {
                EntryInventoryLocationV3::Equipment => {
                    if equipment_slots[slot] {
                        return Err(RestorePointError::InvalidInventory);
                    }
                    equipment_slots[slot] = true;
                }
                EntryInventoryLocationV3::Belt => {
                    belt_counts[slot] = belt_counts[slot].saturating_add(1);
                    if usize::from(belt_counts[slot]) > MAX_BELT_UNITS
                        || belt_templates[slot]
                            .is_some_and(|template| template != item.template_id.as_str())
                    {
                        return Err(RestorePointError::InvalidInventory);
                    }
                    belt_templates[slot] = Some(item.template_id.as_str());
                }
                EntryInventoryLocationV3::RunBackpack => {
                    backpack_counts[slot] = backpack_counts[slot].saturating_add(1);
                    if usize::from(backpack_counts[slot]) > MAX_BELT_UNITS
                        || backpack_templates[slot]
                            .is_some_and(|template| template != item.template_id.as_str())
                    {
                        return Err(RestorePointError::InvalidInventory);
                    }
                    backpack_templates[slot] = Some(item.template_id.as_str());
                }
            }
        }
        if self
            .baseline_items
            .windows(2)
            .any(|pair| inventory_item_sort_key(&pair[0]) >= inventory_item_sort_key(&pair[1]))
        {
            return Err(RestorePointError::InvalidInventory);
        }
        if self.baseline_items.iter().enumerate().any(|(index, item)| {
            self.baseline_items[index + 1..]
                .iter()
                .any(|other| item.item_uid == other.item_uid)
        }) {
            return Err(RestorePointError::DuplicateItemUid);
        }
        Ok(())
    }
}

/// Entry provenance for one active Bargain, in acquisition order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveBargainRestoreV3 {
    pub acquisition_ordinal: u8,
    pub bargain_id: WireText<CONTENT_ID_BYTES>,
    pub acquired_by_offer_id: [u8; ID_BYTES],
    pub source_reward_event_id: [u8; ID_BYTES],
    pub content_version: WireText<CONTENT_ID_BYTES>,
    pub content_revision: WorldFlowContentRevisionV1,
}

impl ActiveBargainRestoreV3 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if self.acquisition_ordinal == 0
            || usize::from(self.acquisition_ordinal) > MAX_ACTIVE_BARGAINS
            || all_zero(&self.acquired_by_offer_id)
            || all_zero(&self.source_reward_event_id)
        {
            return Err(RestorePointError::InvalidOathBargains);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OathBargainRestoreV3 {
    pub oath_id: Option<WireText<CONTENT_ID_BYTES>>,
    pub active_bargains: Vec<ActiveBargainRestoreV3>,
    pub earned_bargain_slots: u8,
    pub oath_bargain_version: u64,
}

impl OathBargainRestoreV3 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if self.active_bargains.len() > MAX_ACTIVE_BARGAINS
            || usize::from(self.earned_bargain_slots) > MAX_ACTIVE_BARGAINS
            || self.active_bargains.len() > usize::from(self.earned_bargain_slots)
            || self.oath_bargain_version == 0
        {
            return Err(RestorePointError::InvalidOathBargains);
        }
        for (index, bargain) in self.active_bargains.iter().enumerate() {
            bargain.validate()?;
            if usize::from(bargain.acquisition_ordinal) != index + 1 {
                return Err(RestorePointError::InvalidOathBargains);
            }
        }
        if self
            .active_bargains
            .iter()
            .enumerate()
            .any(|(index, bargain)| {
                self.active_bargains[index + 1..].iter().any(|other| {
                    bargain.bargain_id == other.bargain_id
                        || bargain.acquired_by_offer_id == other.acquired_by_offer_id
                })
            })
        {
            return Err(RestorePointError::InvalidOathBargains);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifeMetricsRestoreV3 {
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub life_metrics_version: u64,
}

impl LifeMetricsRestoreV3 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if self.life_metrics_version == 0 || self.permadeath_combat_ticks > self.lifetime_ticks {
            return Err(RestorePointError::InvalidLifeMetrics);
        }
        Ok(())
    }
}

/// V3 deliberately captures only wallet authority, not a balance to overwrite during recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AshWalletRestoreV3 {
    pub ash_wallet_version: u64,
}

impl AshWalletRestoreV3 {
    fn validate(self) -> Result<(), RestorePointError> {
        if self.ash_wallet_version == 0 {
            return Err(RestorePointError::ZeroAggregateVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeAggregateVersionsV3 {
    pub account_version: u64,
    pub character_version: u64,
    pub progression_version: u64,
    pub inventory_version: u64,
    pub oath_bargain_version: u64,
    pub life_metrics_version: u64,
    pub ash_wallet_version: u64,
}

impl SafeAggregateVersionsV3 {
    fn validate(&self) -> Result<(), RestorePointError> {
        if [
            self.account_version,
            self.character_version,
            self.progression_version,
            self.inventory_version,
            self.oath_bargain_version,
            self.life_metrics_version,
            self.ash_wallet_version,
        ]
        .contains(&0)
        {
            return Err(RestorePointError::ZeroAggregateVersion);
        }
        Ok(())
    }
}

/// Component-complete danger-entry snapshot accepted by `SPEC-CONFLICT-027`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerEntrySnapshotV3 {
    pub character_id: [u8; ID_BYTES],
    pub content_revision: WorldFlowContentRevisionV1,
    pub progression: ProgressionRestoreV1,
    pub inventory: InventorySecurityRestoreV3,
    pub oath_bargains: OathBargainRestoreV3,
    pub life_metrics: LifeMetricsRestoreV3,
    pub ash_wallet: AshWalletRestoreV3,
    pub versions: SafeAggregateVersionsV3,
}

impl DangerEntrySnapshotV3 {
    pub fn validate(&self) -> Result<(), RestorePointError> {
        if all_zero(&self.character_id) {
            return Err(RestorePointError::ZeroCharacterId);
        }
        self.progression.validate()?;
        self.inventory.validate()?;
        self.oath_bargains.validate()?;
        self.life_metrics.validate()?;
        self.ash_wallet.validate()?;
        self.versions.validate()?;
        if self.progression.progression_version != self.versions.progression_version
            || self.inventory.inventory_version != self.versions.inventory_version
            || self.oath_bargains.oath_bargain_version != self.versions.oath_bargain_version
            || self.life_metrics.life_metrics_version != self.versions.life_metrics_version
            || self.ash_wallet.ash_wallet_version != self.versions.ash_wallet_version
        {
            return Err(RestorePointError::AggregateVersionMismatch);
        }
        Ok(())
    }

    pub fn composite_digest(&self) -> Result<[u8; 32], RestorePointError> {
        self.validate()?;
        let bytes = postcard::to_stdvec(self).map_err(|_| RestorePointError::Encoding)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(V3_DIGEST_DOMAIN);
        hasher.update(&bytes);
        Ok(*hasher.finalize().as_bytes())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryCaptureContext {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub restore_point_id: [u8; ID_BYTES],
    /// World-transfer mutation that owns deterministic entry item ledgers.
    pub mutation_id: [u8; ID_BYTES],
    /// Item units already moved by the atomic `CharacterSafe` preflight.
    pub safe_placement_count: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashRestoreContext {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub restore_point_id: [u8; ID_BYTES],
}

pub trait EntryRestoreProvider: Send + Sync {
    type Snapshot: Send;

    fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> impl Future<Output = Result<Self::Snapshot, RestorePointError>> + Send + 'a;

    fn restore_and_revoke_post_entry<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: CrashRestoreContext,
    ) -> impl Future<Output = Result<(), RestorePointError>> + Send + 'a;
}

/// Transaction-bound `PostgreSQL` inventory capture for the V3 exact-entry baseline.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDangerEntryInventoryProviderV3;

impl EntryRestoreProvider for PostgresDangerEntryInventoryProviderV3 {
    type Snapshot = InventorySecurityRestoreV3;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let stored = stage_danger_entry_inventory_restore_v3(
            transaction,
            context.account_id,
            context.character_id,
            context.restore_point_id,
            context.mutation_id,
            context.safe_placement_count,
        )
        .await
        .map_err(|_| RestorePointError::Persistence)?;
        let baseline_items = stored
            .items
            .into_iter()
            .map(|item| {
                Ok(InventoryBaselineItemV3 {
                    item_uid: ItemUid::new(item.item_uid)?,
                    template_id: WireText::new(item.template_id)
                        .map_err(|_| RestorePointError::InvalidInventory)?,
                    content_revision: WireText::new(item.content_revision)
                        .map_err(|_| RestorePointError::InvalidInventory)?,
                    creation_kind: u8::try_from(item.creation_kind)
                        .map_err(|_| RestorePointError::InvalidInventory)?,
                    creation_request_id: item.creation_request_id,
                    roll_index: u16::try_from(item.roll_index)
                        .map_err(|_| RestorePointError::InvalidInventory)?,
                    unit_ordinal: u16::try_from(item.unit_ordinal)
                        .map_err(|_| RestorePointError::InvalidInventory)?,
                    provenance_kind: u8::try_from(item.provenance_kind)
                        .map_err(|_| RestorePointError::InvalidInventory)?,
                    location: EntryInventoryLocationV3::try_from(item.location_kind)?,
                    slot_index: u8::try_from(item.slot_index)
                        .map_err(|_| RestorePointError::InvalidInventory)?,
                    item_version: item.entry_item_version,
                    security: EntryInventorySecurityV3::try_from(item.entry_security_state)?,
                })
            })
            .collect::<Result<Vec<_>, RestorePointError>>()?;
        let snapshot = InventorySecurityRestoreV3 {
            baseline_items,
            pre_inventory_version: stored.pre_inventory_version,
            inventory_version: stored.post_inventory_version,
            safe_placement_count: stored.safe_placement_count,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Err(RestorePointError::IncompleteRestorePoint)
    }
}

/// Transaction-bound `PostgreSQL` Oath/Bargain capture with acquisition provenance.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDangerEntryOathBargainProviderV3;

impl EntryRestoreProvider for PostgresDangerEntryOathBargainProviderV3 {
    type Snapshot = OathBargainRestoreV3;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let stored = stage_danger_entry_oath_bargain_restore_v3(
            transaction,
            context.account_id,
            context.character_id,
            context.restore_point_id,
        )
        .await
        .map_err(|_| RestorePointError::Persistence)?;
        let active_bargains = stored
            .active_bargains
            .into_iter()
            .map(|bargain| {
                Ok(ActiveBargainRestoreV3 {
                    acquisition_ordinal: bargain.acquisition_ordinal,
                    bargain_id: WireText::new(bargain.bargain_id)
                        .map_err(|_| RestorePointError::InvalidOathBargains)?,
                    acquired_by_offer_id: bargain.acquired_by_offer_id,
                    source_reward_event_id: bargain.source_reward_event_id,
                    content_version: WireText::new(bargain.content_version)
                        .map_err(|_| RestorePointError::InvalidOathBargains)?,
                    content_revision: WorldFlowContentRevisionV1 {
                        records_blake3: protocol::ManifestHash::new(bargain.records_blake3)
                            .map_err(|_| RestorePointError::InvalidOathBargains)?,
                        assets_blake3: protocol::ManifestHash::new(bargain.assets_blake3)
                            .map_err(|_| RestorePointError::InvalidOathBargains)?,
                        localization_blake3: protocol::ManifestHash::new(
                            bargain.localization_blake3,
                        )
                        .map_err(|_| RestorePointError::InvalidOathBargains)?,
                    },
                })
            })
            .collect::<Result<Vec<_>, RestorePointError>>()?;
        let snapshot = OathBargainRestoreV3 {
            oath_id: stored
                .oath_id
                .map(|value| {
                    WireText::new(value).map_err(|_| RestorePointError::InvalidOathBargains)
                })
                .transpose()?,
            active_bargains,
            earned_bargain_slots: stored.earned_bargain_slots,
            oath_bargain_version: stored.oath_bargain_version,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Err(RestorePointError::IncompleteRestorePoint)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDangerEntryLifeMetricsProviderV3;

impl EntryRestoreProvider for PostgresDangerEntryLifeMetricsProviderV3 {
    type Snapshot = LifeMetricsRestoreV3;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let stored = stage_danger_entry_life_metrics_restore_v3(
            transaction,
            context.account_id,
            context.character_id,
            context.restore_point_id,
        )
        .await
        .map_err(|_| RestorePointError::Persistence)?;
        let snapshot = LifeMetricsRestoreV3 {
            lifetime_ticks: stored.captured_lifetime_ticks,
            permadeath_combat_ticks: stored.rollback_permadeath_combat_ticks,
            life_metrics_version: stored.life_metrics_version,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Err(RestorePointError::IncompleteRestorePoint)
    }
}

/// Captures only wallet version authority; recovery later compensates bound danger earns.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDangerEntryAshWalletProviderV3;

impl EntryRestoreProvider for PostgresDangerEntryAshWalletProviderV3 {
    type Snapshot = AshWalletRestoreV3;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let stored = stage_danger_entry_ash_wallet_restore_v3(
            transaction,
            context.account_id,
            context.character_id,
            context.restore_point_id,
        )
        .await
        .map_err(|_| RestorePointError::Persistence)?;
        let snapshot = AshWalletRestoreV3 {
            ash_wallet_version: stored.ash_wallet_version,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Err(RestorePointError::IncompleteRestorePoint)
    }
}

#[derive(Debug, Clone)]
pub struct RestorePointProviders<Progression, Inventory, OathBargains> {
    progression: Progression,
    inventory: Inventory,
    oath_bargains: OathBargains,
}

impl<Progression, Inventory, OathBargains>
    RestorePointProviders<Progression, Inventory, OathBargains>
{
    pub const fn new(
        progression: Progression,
        inventory: Inventory,
        oath_bargains: OathBargains,
    ) -> Self {
        Self {
            progression,
            inventory,
            oath_bargains,
        }
    }
}

impl<Progression, Inventory, OathBargains>
    RestorePointProviders<Progression, Inventory, OathBargains>
where
    Progression: EntryRestoreProvider<Snapshot = ProgressionRestoreV1>,
    Inventory: EntryRestoreProvider<Snapshot = InventorySecurityRestoreV1>,
    OathBargains: EntryRestoreProvider<Snapshot = OathBargainRestoreV1>,
{
    pub async fn capture_v1(
        &self,
        transaction: &mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
        content_revision: WorldFlowContentRevisionV1,
        account_version: u64,
        character_version: u64,
    ) -> Result<DangerEntrySnapshotV1, RestorePointError> {
        validate_context(
            &context.account_id,
            &context.character_id,
            &context.restore_point_id,
        )?;
        if all_zero(&context.mutation_id) || context.safe_placement_count > 48 {
            return Err(RestorePointError::InvalidInventory);
        }
        let progression = self.progression.capture(transaction, context).await?;
        let inventory = self.inventory.capture(transaction, context).await?;
        let oath_bargains = self.oath_bargains.capture(transaction, context).await?;
        let snapshot = DangerEntrySnapshotV1 {
            character_id: context.character_id,
            content_revision,
            versions: SafeAggregateVersionsV1 {
                account_version,
                character_version,
                progression_version: progression.progression_version,
                inventory_version: inventory.inventory_version,
                oath_bargain_version: oath_bargains.oath_bargain_version,
            },
            progression,
            inventory,
            oath_bargains,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub async fn restore_v1(
        &self,
        transaction: &mut PersistenceTransaction<'_>,
        context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        validate_context(
            &context.account_id,
            &context.character_id,
            &context.restore_point_id,
        )?;
        self.progression
            .restore_and_revoke_post_entry(transaction, context)
            .await?;
        self.inventory
            .restore_and_revoke_post_entry(transaction, context)
            .await?;
        self.oath_bargains
            .restore_and_revoke_post_entry(transaction, context)
            .await
    }
}

/// Mandatory four-provider composition for V2 entry capture and crash restoration.
///
/// This is additive to [`RestorePointProviders`] so stored V1 restore points remain readable while
/// new danger entries can require all V2 components at the type boundary.
#[derive(Debug, Clone)]
pub struct RestorePointProvidersV2<Progression, Inventory, OathBargains, LifeMetrics> {
    progression: Progression,
    inventory: Inventory,
    oath_bargains: OathBargains,
    life_metrics: LifeMetrics,
}

impl<Progression, Inventory, OathBargains, LifeMetrics>
    RestorePointProvidersV2<Progression, Inventory, OathBargains, LifeMetrics>
{
    pub const fn new(
        progression: Progression,
        inventory: Inventory,
        oath_bargains: OathBargains,
        life_metrics: LifeMetrics,
    ) -> Self {
        Self {
            progression,
            inventory,
            oath_bargains,
            life_metrics,
        }
    }
}

impl<Progression, Inventory, OathBargains, LifeMetrics>
    RestorePointProvidersV2<Progression, Inventory, OathBargains, LifeMetrics>
where
    Progression: EntryRestoreProvider<Snapshot = ProgressionRestoreV1>,
    Inventory: EntryRestoreProvider<Snapshot = InventorySecurityRestoreV1>,
    OathBargains: EntryRestoreProvider<Snapshot = OathBargainRestoreV1>,
    LifeMetrics: EntryRestoreProvider<Snapshot = LifeMetricsRestoreV2>,
{
    pub async fn capture_v2(
        &self,
        transaction: &mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
        content_revision: WorldFlowContentRevisionV1,
        account_version: u64,
        character_version: u64,
    ) -> Result<DangerEntrySnapshotV2, RestorePointError> {
        validate_context(
            &context.account_id,
            &context.character_id,
            &context.restore_point_id,
        )?;
        if all_zero(&context.mutation_id) || context.safe_placement_count > 48 {
            return Err(RestorePointError::InvalidInventory);
        }
        let progression = self.progression.capture(transaction, context).await?;
        let inventory = self.inventory.capture(transaction, context).await?;
        let oath_bargains = self.oath_bargains.capture(transaction, context).await?;
        let life_metrics = self.life_metrics.capture(transaction, context).await?;
        let snapshot = DangerEntrySnapshotV2 {
            character_id: context.character_id,
            content_revision,
            versions: SafeAggregateVersionsV2 {
                account_version,
                character_version,
                progression_version: progression.progression_version,
                inventory_version: inventory.inventory_version,
                oath_bargain_version: oath_bargains.oath_bargain_version,
                life_metrics_version: life_metrics.life_metrics_version,
            },
            progression,
            inventory,
            oath_bargains,
            life_metrics,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Restores all components in [`CRASH_RESTORE_ORDER_V2`] within the caller's transaction.
    pub async fn restore_v2(
        &self,
        transaction: &mut PersistenceTransaction<'_>,
        context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        validate_context(
            &context.account_id,
            &context.character_id,
            &context.restore_point_id,
        )?;

        for component in CRASH_RESTORE_ORDER_V2 {
            match component {
                CrashRestoreComponentV2::Progression => {
                    self.progression
                        .restore_and_revoke_post_entry(transaction, context)
                        .await?;
                }
                CrashRestoreComponentV2::Inventory => {
                    self.inventory
                        .restore_and_revoke_post_entry(transaction, context)
                        .await?;
                }
                CrashRestoreComponentV2::OathBargains => {
                    self.oath_bargains
                        .restore_and_revoke_post_entry(transaction, context)
                        .await?;
                }
                CrashRestoreComponentV2::LifeMetrics => {
                    self.life_metrics
                        .restore_and_revoke_post_entry(transaction, context)
                        .await?;
                }
            }
        }
        Ok(())
    }
}

/// Mandatory five-provider composition for all newly admitted danger entries.
///
/// V1/V2 remain available only to decode historical fixtures. Production V3 capture cannot be
/// constructed without the Ash authority or the full entry inventory/Bargain provenance types.
#[derive(Debug, Clone)]
pub struct RestorePointProvidersV3<Progression, Inventory, OathBargains, LifeMetrics, AshWallet> {
    progression: Progression,
    inventory: Inventory,
    oath_bargains: OathBargains,
    life_metrics: LifeMetrics,
    ash_wallet: AshWallet,
}

impl<Progression, Inventory, OathBargains, LifeMetrics, AshWallet>
    RestorePointProvidersV3<Progression, Inventory, OathBargains, LifeMetrics, AshWallet>
{
    pub const fn new(
        progression: Progression,
        inventory: Inventory,
        oath_bargains: OathBargains,
        life_metrics: LifeMetrics,
        ash_wallet: AshWallet,
    ) -> Self {
        Self {
            progression,
            inventory,
            oath_bargains,
            life_metrics,
            ash_wallet,
        }
    }
}

impl<Progression, Inventory, OathBargains, LifeMetrics, AshWallet>
    RestorePointProvidersV3<Progression, Inventory, OathBargains, LifeMetrics, AshWallet>
where
    Progression: EntryRestoreProvider<Snapshot = ProgressionRestoreV1>,
    Inventory: EntryRestoreProvider<Snapshot = InventorySecurityRestoreV3>,
    OathBargains: EntryRestoreProvider<Snapshot = OathBargainRestoreV3>,
    LifeMetrics: EntryRestoreProvider<Snapshot = LifeMetricsRestoreV3>,
    AshWallet: EntryRestoreProvider<Snapshot = AshWalletRestoreV3>,
{
    pub async fn capture_v3(
        &self,
        transaction: &mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
        content_revision: WorldFlowContentRevisionV1,
        account_version: u64,
        character_version: u64,
    ) -> Result<DangerEntrySnapshotV3, RestorePointError> {
        validate_context(
            &context.account_id,
            &context.character_id,
            &context.restore_point_id,
        )?;
        if all_zero(&context.mutation_id) || context.safe_placement_count > 48 {
            return Err(RestorePointError::InvalidInventory);
        }
        let progression = self.progression.capture(transaction, context).await?;
        let inventory = self.inventory.capture(transaction, context).await?;
        if inventory.safe_placement_count != context.safe_placement_count {
            return Err(RestorePointError::InvalidInventory);
        }
        let oath_bargains = self.oath_bargains.capture(transaction, context).await?;
        let life_metrics = self.life_metrics.capture(transaction, context).await?;
        let ash_wallet = self.ash_wallet.capture(transaction, context).await?;
        let snapshot = DangerEntrySnapshotV3 {
            character_id: context.character_id,
            content_revision,
            versions: SafeAggregateVersionsV3 {
                account_version,
                character_version,
                progression_version: progression.progression_version,
                inventory_version: inventory.inventory_version,
                oath_bargain_version: oath_bargains.oath_bargain_version,
                life_metrics_version: life_metrics.life_metrics_version,
                ash_wallet_version: ash_wallet.ash_wallet_version,
            },
            progression,
            inventory,
            oath_bargains,
            life_metrics,
            ash_wallet,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Stages all recovery components in the permanent V3 lock/order contract.
    pub async fn restore_v3(
        &self,
        transaction: &mut PersistenceTransaction<'_>,
        context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        validate_context(
            &context.account_id,
            &context.character_id,
            &context.restore_point_id,
        )?;
        for component in CRASH_RESTORE_ORDER_V3 {
            match component {
                CrashRestoreComponentV3::Progression => {
                    self.progression
                        .restore_and_revoke_post_entry(transaction, context)
                        .await?;
                }
                CrashRestoreComponentV3::Inventory => {
                    self.inventory
                        .restore_and_revoke_post_entry(transaction, context)
                        .await?;
                }
                CrashRestoreComponentV3::OathBargains => {
                    self.oath_bargains
                        .restore_and_revoke_post_entry(transaction, context)
                        .await?;
                }
                CrashRestoreComponentV3::LifeMetrics => {
                    self.life_metrics
                        .restore_and_revoke_post_entry(transaction, context)
                        .await?;
                }
                CrashRestoreComponentV3::AshWallet => {
                    self.ash_wallet
                        .restore_and_revoke_post_entry(transaction, context)
                        .await?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RestorePointError {
    #[error("item UID must be nonzero")]
    ZeroItemUid,
    #[error("character ID must be nonzero")]
    ZeroCharacterId,
    #[error("capture or restore context contains a zero identity")]
    ZeroContextIdentity,
    #[error("progression restore component is invalid")]
    InvalidProgression,
    #[error("inventory restore component is invalid")]
    InvalidInventory,
    #[error("Belt restore stack is invalid")]
    InvalidBeltStack,
    #[error("one item UID appears in more than one restored location")]
    DuplicateItemUid,
    #[error("Oath/Bargain restore component is invalid")]
    InvalidOathBargains,
    #[error("life-metrics restore component is invalid")]
    InvalidLifeMetrics,
    #[error("safe aggregate version must be nonzero")]
    ZeroAggregateVersion,
    #[error("component version disagrees with the root version envelope")]
    AggregateVersionMismatch,
    #[error("restore snapshot encoding failed")]
    Encoding,
    #[error("a required restore-point provider is unavailable")]
    IncompleteRestorePoint,
    #[error("restore-point persistence failed")]
    Persistence,
    #[error("a committed death or extraction superseded crash restoration")]
    RestoreSuperseded,
}

fn inventory_item_sort_key(
    item: &InventoryBaselineItemV3,
) -> (EntryInventoryLocationV3, u8, [u8; ID_BYTES]) {
    (item.location, item.slot_index, item.item_uid.into_bytes())
}

fn valid_item_content_revision(value: &str) -> bool {
    const PREFIX: &str = "core-dev.blake3.";
    value.strip_prefix(PREFIX).is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn validate_context(
    first: &[u8; ID_BYTES],
    second: &[u8; ID_BYTES],
    third: &[u8; ID_BYTES],
) -> Result<(), RestorePointError> {
    if [first, second, third].into_iter().any(all_zero) {
        Err(RestorePointError::ZeroContextIdentity)
    } else {
        Ok(())
    }
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

    fn uid(value: u8) -> ItemUid {
        ItemUid::new([value; ID_BYTES]).unwrap()
    }

    fn snapshot() -> DangerEntrySnapshotV1 {
        DangerEntrySnapshotV1 {
            character_id: [1; ID_BYTES],
            content_revision: WorldFlowContentRevisionV1 {
                records_blake3: protocol::ManifestHash::new("a".repeat(64)).unwrap(),
                assets_blake3: protocol::ManifestHash::new("b".repeat(64)).unwrap(),
                localization_blake3: protocol::ManifestHash::new("c".repeat(64)).unwrap(),
            },
            progression: ProgressionRestoreV1 {
                level: 1,
                xp: 0,
                current_health: 120,
                progression_version: 1,
            },
            inventory: InventorySecurityRestoreV1 {
                equipment: [Some(uid(2)), None, None, None],
                belt: [
                    BeltStackV1 {
                        consumable_id: Some(WireText::new("consumable.red_tonic").unwrap()),
                        unit_uids: vec![uid(3), uid(4)],
                    },
                    BeltStackV1 {
                        consumable_id: None,
                        unit_uids: vec![],
                    },
                ],
                inventory_version: 1,
            },
            oath_bargains: OathBargainRestoreV1 {
                oath_id: None,
                active_bargain_ids: vec![],
                earned_bargain_slots: 0,
                oath_bargain_version: 1,
            },
            versions: SafeAggregateVersionsV1 {
                account_version: 2,
                character_version: 1,
                progression_version: 1,
                inventory_version: 1,
                oath_bargain_version: 1,
            },
        }
    }

    fn snapshot_v2() -> DangerEntrySnapshotV2 {
        DangerEntrySnapshotV2 {
            character_id: [1; ID_BYTES],
            content_revision: WorldFlowContentRevisionV1 {
                records_blake3: protocol::ManifestHash::new("a".repeat(64)).unwrap(),
                assets_blake3: protocol::ManifestHash::new("b".repeat(64)).unwrap(),
                localization_blake3: protocol::ManifestHash::new("c".repeat(64)).unwrap(),
            },
            progression: ProgressionRestoreV1 {
                level: 10,
                xp: 4_200,
                current_health: 120,
                progression_version: 5,
            },
            inventory: InventorySecurityRestoreV1 {
                equipment: [Some(uid(2)), None, None, None],
                belt: [
                    BeltStackV1 {
                        consumable_id: Some(WireText::new("consumable.red_tonic").unwrap()),
                        unit_uids: vec![uid(3), uid(4)],
                    },
                    BeltStackV1 {
                        consumable_id: None,
                        unit_uids: vec![],
                    },
                ],
                inventory_version: 7,
            },
            oath_bargains: OathBargainRestoreV1 {
                oath_id: Some(WireText::new("oath.arbalist.long_vigil").unwrap()),
                active_bargain_ids: vec![WireText::new("bargain.cinder_hunger").unwrap()],
                earned_bargain_slots: 1,
                oath_bargain_version: 9,
            },
            life_metrics: LifeMetricsRestoreV2 {
                lifetime_ticks: 36_000,
                permadeath_combat_ticks: 900,
                life_metrics_version: 3,
            },
            versions: SafeAggregateVersionsV2 {
                account_version: 2,
                character_version: 11,
                progression_version: 5,
                inventory_version: 7,
                oath_bargain_version: 9,
                life_metrics_version: 3,
            },
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the complete V3 authority fixture stays visible in one canonical constructor"
    )]
    fn snapshot_v3() -> DangerEntrySnapshotV3 {
        let revision = WorldFlowContentRevisionV1 {
            records_blake3: protocol::ManifestHash::new("a".repeat(64)).unwrap(),
            assets_blake3: protocol::ManifestHash::new("b".repeat(64)).unwrap(),
            localization_blake3: protocol::ManifestHash::new("c".repeat(64)).unwrap(),
        };
        DangerEntrySnapshotV3 {
            character_id: [1; ID_BYTES],
            content_revision: revision.clone(),
            progression: ProgressionRestoreV1 {
                level: 10,
                xp: 4_200,
                current_health: 120,
                progression_version: 5,
            },
            inventory: InventorySecurityRestoreV3 {
                baseline_items: vec![
                    InventoryBaselineItemV3 {
                        item_uid: uid(2),
                        template_id: WireText::new("weapon.iron_arbalest").unwrap(),
                        content_revision: WireText::new(format!(
                            "core-dev.blake3.{}",
                            "d".repeat(64)
                        ))
                        .unwrap(),
                        creation_kind: 0,
                        creation_request_id: [10; ID_BYTES],
                        roll_index: 0,
                        unit_ordinal: 0,
                        provenance_kind: 0,
                        location: EntryInventoryLocationV3::Equipment,
                        slot_index: 0,
                        item_version: 2,
                        security: EntryInventorySecurityV3::AtRiskEquipped,
                    },
                    InventoryBaselineItemV3 {
                        item_uid: uid(3),
                        template_id: WireText::new("consumable.red_tonic").unwrap(),
                        content_revision: WireText::new(format!(
                            "core-dev.blake3.{}",
                            "d".repeat(64)
                        ))
                        .unwrap(),
                        creation_kind: 1,
                        creation_request_id: [11; ID_BYTES],
                        roll_index: 2,
                        unit_ordinal: 1,
                        provenance_kind: 1,
                        location: EntryInventoryLocationV3::Belt,
                        slot_index: 0,
                        item_version: 4,
                        security: EntryInventorySecurityV3::AtRiskEquipped,
                    },
                    InventoryBaselineItemV3 {
                        item_uid: uid(4),
                        template_id: WireText::new("relic.ember_glass").unwrap(),
                        content_revision: WireText::new(format!(
                            "core-dev.blake3.{}",
                            "d".repeat(64)
                        ))
                        .unwrap(),
                        creation_kind: 2,
                        creation_request_id: [12; ID_BYTES],
                        roll_index: 3,
                        unit_ordinal: 0,
                        provenance_kind: 2,
                        location: EntryInventoryLocationV3::RunBackpack,
                        slot_index: 5,
                        item_version: 7,
                        security: EntryInventorySecurityV3::AtRiskPending,
                    },
                ],
                pre_inventory_version: 6,
                inventory_version: 7,
                safe_placement_count: 1,
            },
            oath_bargains: OathBargainRestoreV3 {
                oath_id: Some(WireText::new("oath.arbalist.long_vigil").unwrap()),
                active_bargains: vec![ActiveBargainRestoreV3 {
                    acquisition_ordinal: 1,
                    bargain_id: WireText::new("bargain.cinder_hunger").unwrap(),
                    acquired_by_offer_id: [20; ID_BYTES],
                    source_reward_event_id: [21; ID_BYTES],
                    content_version: WireText::new("core-dev").unwrap(),
                    content_revision: revision,
                }],
                earned_bargain_slots: 1,
                oath_bargain_version: 9,
            },
            life_metrics: LifeMetricsRestoreV3 {
                lifetime_ticks: 36_000,
                permadeath_combat_ticks: 900,
                life_metrics_version: 3,
            },
            ash_wallet: AshWalletRestoreV3 {
                ash_wallet_version: 8,
            },
            versions: SafeAggregateVersionsV3 {
                account_version: 2,
                character_version: 11,
                progression_version: 5,
                inventory_version: 7,
                oath_bargain_version: 9,
                life_metrics_version: 3,
                ash_wallet_version: 8,
            },
        }
    }

    fn assert_v2_digest_changes(mut change: impl FnMut(&mut DangerEntrySnapshotV2)) {
        let original = snapshot_v2();
        let original_digest = original.composite_digest().unwrap();
        let mut changed = original;
        change(&mut changed);
        assert_eq!(changed.validate(), Ok(()));
        assert_ne!(changed.composite_digest().unwrap(), original_digest);
    }

    fn assert_v3_digest_changes(mut change: impl FnMut(&mut DangerEntrySnapshotV3)) {
        let original = snapshot_v3();
        let original_digest = original.composite_digest().unwrap();
        let mut changed = original;
        change(&mut changed);
        assert_eq!(changed.validate(), Ok(()));
        assert_ne!(changed.composite_digest().unwrap(), original_digest);
    }

    #[test]
    fn exact_v1_snapshot_is_valid_and_hashes_deterministically() {
        let snapshot = snapshot();
        assert_eq!(snapshot.validate(), Ok(()));
        assert_eq!(snapshot.composite_digest(), snapshot.composite_digest());
    }

    #[test]
    fn duplicate_item_identity_and_unsorted_belt_fail_closed() {
        let mut duplicate = snapshot();
        duplicate.inventory.equipment[0] = Some(uid(3));
        assert_eq!(
            duplicate.validate(),
            Err(RestorePointError::DuplicateItemUid)
        );
        let mut unsorted = snapshot();
        unsorted.inventory.belt[0].unit_uids.swap(0, 1);
        assert_eq!(
            unsorted.validate(),
            Err(RestorePointError::InvalidBeltStack)
        );
    }

    #[test]
    fn missing_or_mismatched_component_versions_fail_closed() {
        let mut mismatch = snapshot();
        mismatch.versions.inventory_version = 2;
        assert_eq!(
            mismatch.validate(),
            Err(RestorePointError::AggregateVersionMismatch)
        );
        let mut missing = snapshot();
        missing.oath_bargains.oath_bargain_version = 0;
        assert_eq!(
            missing.validate(),
            Err(RestorePointError::InvalidOathBargains)
        );
    }

    #[test]
    fn exact_v2_snapshot_is_valid_and_hashes_deterministically() {
        let snapshot = snapshot_v2();
        assert_eq!(snapshot.validate(), Ok(()));
        assert_eq!(snapshot.composite_digest(), snapshot.composite_digest());
    }

    #[test]
    fn v2_digest_binds_every_snapshot_field() {
        assert_v2_digest_changes(|value| value.character_id = [8; ID_BYTES]);
        assert_v2_digest_changes(|value| {
            value.content_revision.records_blake3 =
                protocol::ManifestHash::new("d".repeat(64)).unwrap();
        });
        assert_v2_digest_changes(|value| {
            value.content_revision.assets_blake3 =
                protocol::ManifestHash::new("e".repeat(64)).unwrap();
        });
        assert_v2_digest_changes(|value| {
            value.content_revision.localization_blake3 =
                protocol::ManifestHash::new("f".repeat(64)).unwrap();
        });

        assert_v2_digest_changes(|value| value.progression.level = 11);
        assert_v2_digest_changes(|value| value.progression.xp += 1);
        assert_v2_digest_changes(|value| value.progression.current_health += 1);
        assert_v2_digest_changes(|value| {
            value.progression.progression_version += 1;
            value.versions.progression_version += 1;
        });

        assert_v2_digest_changes(|value| value.inventory.equipment[1] = Some(uid(5)));
        assert_v2_digest_changes(|value| {
            value.inventory.belt[0].consumable_id =
                Some(WireText::new("consumable.black_tonic").unwrap());
        });
        assert_v2_digest_changes(|value| value.inventory.belt[0].unit_uids.push(uid(5)));
        assert_v2_digest_changes(|value| {
            value.inventory.inventory_version += 1;
            value.versions.inventory_version += 1;
        });

        assert_v2_digest_changes(|value| {
            value.oath_bargains.oath_id = Some(WireText::new("oath.arbalist.nailkeeper").unwrap());
        });
        assert_v2_digest_changes(|value| {
            value.oath_bargains.active_bargain_ids[0] = WireText::new("bargain.bell_debt").unwrap();
        });
        assert_v2_digest_changes(|value| value.oath_bargains.earned_bargain_slots = 2);
        assert_v2_digest_changes(|value| {
            value.oath_bargains.oath_bargain_version += 1;
            value.versions.oath_bargain_version += 1;
        });

        assert_v2_digest_changes(|value| value.life_metrics.lifetime_ticks += 1);
        assert_v2_digest_changes(|value| value.life_metrics.permadeath_combat_ticks += 1);
        assert_v2_digest_changes(|value| {
            value.life_metrics.life_metrics_version += 1;
            value.versions.life_metrics_version += 1;
        });
        assert_v2_digest_changes(|value| value.versions.account_version += 1);
        assert_v2_digest_changes(|value| value.versions.character_version += 1);
    }

    #[test]
    fn invalid_or_mismatched_life_metrics_fail_closed() {
        let mut zero_version = snapshot_v2();
        zero_version.life_metrics.life_metrics_version = 0;
        assert_eq!(
            zero_version.validate(),
            Err(RestorePointError::InvalidLifeMetrics)
        );

        let mut impossible_clock = snapshot_v2();
        impossible_clock.life_metrics.permadeath_combat_ticks =
            impossible_clock.life_metrics.lifetime_ticks + 1;
        assert_eq!(
            impossible_clock.validate(),
            Err(RestorePointError::InvalidLifeMetrics)
        );

        let mut mismatched_version = snapshot_v2();
        mismatched_version.versions.life_metrics_version += 1;
        assert_eq!(
            mismatched_version.validate(),
            Err(RestorePointError::AggregateVersionMismatch)
        );
    }

    #[test]
    fn v2_restore_order_is_stable_and_life_metrics_are_last() {
        assert_eq!(
            CRASH_RESTORE_ORDER_V2,
            [
                CrashRestoreComponentV2::Progression,
                CrashRestoreComponentV2::Inventory,
                CrashRestoreComponentV2::OathBargains,
                CrashRestoreComponentV2::LifeMetrics,
            ]
        );
    }

    #[test]
    fn exact_v3_snapshot_is_valid_and_hashes_deterministically() {
        let snapshot = snapshot_v3();
        assert_eq!(snapshot.validate(), Ok(()));
        assert_eq!(snapshot.composite_digest(), snapshot.composite_digest());
    }

    #[test]
    fn v3_digest_binds_backpack_provenance_bargain_authority_and_ash() {
        assert_eq!(
            snapshot_v3().composite_digest().unwrap(),
            [
                62, 92, 98, 12, 238, 99, 91, 222, 86, 244, 12, 154, 10, 49, 205, 162, 82, 102, 107,
                128, 16, 210, 21, 158, 193, 103, 238, 190, 128, 111, 24, 66,
            ]
        );
        assert_v3_digest_changes(|value| {
            value.inventory.baseline_items[2].slot_index = 6;
        });
        assert_v3_digest_changes(|value| {
            value.inventory.baseline_items[2].creation_request_id = [13; ID_BYTES];
        });
        assert_v3_digest_changes(|value| {
            value.oath_bargains.active_bargains[0].acquired_by_offer_id = [22; ID_BYTES];
        });
        assert_v3_digest_changes(|value| {
            value.oath_bargains.active_bargains[0]
                .content_revision
                .records_blake3 = protocol::ManifestHash::new("e".repeat(64)).unwrap();
        });
        assert_v3_digest_changes(|value| {
            value.ash_wallet.ash_wallet_version += 1;
            value.versions.ash_wallet_version += 1;
        });
    }

    #[test]
    fn v3_inventory_rejects_noncanonical_or_inexact_entry_authority() {
        let mut out_of_order = snapshot_v3();
        out_of_order.inventory.baseline_items.swap(0, 2);
        assert_eq!(
            out_of_order.validate(),
            Err(RestorePointError::InvalidInventory)
        );

        let mut wrong_security = snapshot_v3();
        wrong_security.inventory.baseline_items[2].security =
            EntryInventorySecurityV3::AtRiskEquipped;
        assert_eq!(
            wrong_security.validate(),
            Err(RestorePointError::InvalidInventory)
        );

        let mut invalid_content_revision = snapshot_v3();
        invalid_content_revision.inventory.baseline_items[0].content_revision =
            WireText::new("core-dev.blake3.not-a-hash").unwrap();
        assert_eq!(
            invalid_content_revision.validate(),
            Err(RestorePointError::InvalidInventory)
        );
    }

    #[test]
    fn v3_bargain_provenance_and_component_versions_fail_closed() {
        let mut missing_offer = snapshot_v3();
        missing_offer.oath_bargains.active_bargains[0].acquired_by_offer_id = [0; ID_BYTES];
        assert_eq!(
            missing_offer.validate(),
            Err(RestorePointError::InvalidOathBargains)
        );

        let mut wrong_ordinal = snapshot_v3();
        wrong_ordinal.oath_bargains.active_bargains[0].acquisition_ordinal = 2;
        assert_eq!(
            wrong_ordinal.validate(),
            Err(RestorePointError::InvalidOathBargains)
        );

        let mut mismatched_ash = snapshot_v3();
        mismatched_ash.versions.ash_wallet_version += 1;
        assert_eq!(
            mismatched_ash.validate(),
            Err(RestorePointError::AggregateVersionMismatch)
        );
    }

    #[test]
    fn v3_restore_order_is_stable_and_ash_is_last() {
        assert_eq!(
            CRASH_RESTORE_ORDER_V3,
            [
                CrashRestoreComponentV3::Progression,
                CrashRestoreComponentV3::Inventory,
                CrashRestoreComponentV3::OathBargains,
                CrashRestoreComponentV3::LifeMetrics,
                CrashRestoreComponentV3::AshWallet,
            ]
        );
    }
}
