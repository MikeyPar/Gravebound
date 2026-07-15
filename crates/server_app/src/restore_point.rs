//! Typed `TECH-023` danger-entry snapshots.
//!
//! `GB-M03-03B` owns only the composition boundary. The normal route remains disabled until
//! progression/inventory and Oath/Bargain packages provide all three transactional providers.

use std::future::Future;

use persistence::{
    PersistenceTransaction, stage_danger_entry_inventory_restore_v2,
    stage_danger_entry_life_metrics_restore_v2, stage_danger_entry_oath_bargain_restore_v2,
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
const V2_DIGEST_DOMAIN: &[u8] = b"gravebound.danger-entry-restore.v2\0";

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

/// Transaction-bound `PostgreSQL` inventory capture for restore contract V2.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDangerEntryInventoryProviderV2;

impl EntryRestoreProvider for PostgresDangerEntryInventoryProviderV2 {
    type Snapshot = InventorySecurityRestoreV1;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let stored = stage_danger_entry_inventory_restore_v2(
            transaction,
            context.account_id,
            context.character_id,
            context.restore_point_id,
            context.mutation_id,
            context.safe_placement_count,
        )
        .await
        .map_err(|_| RestorePointError::Persistence)?;
        let inventory_version = stored.post_inventory_version;
        let mut equipment = [None; EQUIPMENT_SLOT_COUNT];
        let mut belt_ids = [Vec::new(), Vec::new()];
        let mut belt_templates = [None, None];
        for item in stored.items {
            let uid = ItemUid::new(item.item_uid)?;
            let slot = usize::try_from(item.slot_index)
                .map_err(|_| RestorePointError::InvalidInventory)?;
            match item.location_kind {
                0 if slot < EQUIPMENT_SLOT_COUNT => equipment[slot] = Some(uid),
                1 if slot < BELT_SLOT_COUNT => {
                    belt_ids[slot].push(uid);
                    belt_templates[slot] = Some(
                        WireText::new(item.template_id)
                            .map_err(|_| RestorePointError::InvalidInventory)?,
                    );
                }
                _ => return Err(RestorePointError::InvalidInventory),
            }
        }
        Ok(InventorySecurityRestoreV1 {
            equipment,
            belt: [
                BeltStackV1 {
                    consumable_id: belt_templates[0].take(),
                    unit_uids: std::mem::take(&mut belt_ids[0]),
                },
                BeltStackV1 {
                    consumable_id: belt_templates[1].take(),
                    unit_uids: std::mem::take(&mut belt_ids[1]),
                },
            ],
            inventory_version,
        })
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Err(RestorePointError::IncompleteRestorePoint)
    }
}

/// Transaction-bound `PostgreSQL` Oath/Bargain capture for restore contract V2.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDangerEntryOathBargainProviderV2;

impl EntryRestoreProvider for PostgresDangerEntryOathBargainProviderV2 {
    type Snapshot = OathBargainRestoreV1;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let stored = stage_danger_entry_oath_bargain_restore_v2(
            transaction,
            context.account_id,
            context.character_id,
            context.restore_point_id,
        )
        .await
        .map_err(|_| RestorePointError::Persistence)?;
        Ok(OathBargainRestoreV1 {
            oath_id: stored
                .oath_id
                .map(|value| {
                    WireText::new(value).map_err(|_| RestorePointError::InvalidOathBargains)
                })
                .transpose()?,
            active_bargain_ids: stored
                .active_bargain_ids
                .into_iter()
                .map(|value| {
                    WireText::new(value).map_err(|_| RestorePointError::InvalidOathBargains)
                })
                .collect::<Result<Vec<_>, _>>()?,
            earned_bargain_slots: stored.earned_bargain_slots,
            oath_bargain_version: stored.oath_bargain_version,
        })
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Err(RestorePointError::IncompleteRestorePoint)
    }
}

/// Transaction-bound `PostgreSQL` lifetime/combat-clock capture for restore contract V2.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDangerEntryLifeMetricsProviderV2;

impl EntryRestoreProvider for PostgresDangerEntryLifeMetricsProviderV2 {
    type Snapshot = LifeMetricsRestoreV2;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let stored = stage_danger_entry_life_metrics_restore_v2(
            transaction,
            context.account_id,
            context.character_id,
            context.restore_point_id,
        )
        .await
        .map_err(|_| RestorePointError::Persistence)?;
        Ok(LifeMetricsRestoreV2 {
            lifetime_ticks: stored.captured_lifetime_ticks,
            permadeath_combat_ticks: stored.rollback_permadeath_combat_ticks,
            life_metrics_version: stored.life_metrics_version,
        })
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

    fn assert_v2_digest_changes(mut change: impl FnMut(&mut DangerEntrySnapshotV2)) {
        let original = snapshot_v2();
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
}
