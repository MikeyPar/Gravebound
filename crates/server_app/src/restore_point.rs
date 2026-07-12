//! Typed `TECH-023` danger-entry snapshots.
//!
//! `GB-M03-03B` owns only the composition boundary. The normal route remains disabled until
//! progression/inventory and Oath/Bargain packages provide all three transactional providers.

use std::future::Future;

use persistence::PersistenceTransaction;
use protocol::{ManifestHash, WireText};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const ID_BYTES: usize = 16;
const CONTENT_ID_BYTES: usize = 96;
const BELT_SLOT_COUNT: usize = 2;
const EQUIPMENT_SLOT_COUNT: usize = 4;
const MAX_BELT_UNITS: usize = 6;
const MAX_ACTIVE_BARGAINS: usize = 3;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerEntrySnapshotV1 {
    pub character_id: [u8; ID_BYTES],
    pub content_revision: ManifestHash,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryCaptureContext {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub transfer_id: [u8; ID_BYTES],
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
        content_revision: ManifestHash,
        account_version: u64,
        character_version: u64,
    ) -> Result<DangerEntrySnapshotV1, RestorePointError> {
        validate_context(
            &context.account_id,
            &context.character_id,
            &context.transfer_id,
        )?;
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
            content_revision: ManifestHash::new("a".repeat(64)).unwrap(),
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
}
