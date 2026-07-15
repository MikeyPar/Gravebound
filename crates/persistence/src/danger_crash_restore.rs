//! Canonical request and result contract for authoritative danger crash restoration.
//!
//! The `PostgreSQL` coordinator is the only writer for this contract. These types deliberately
//! contain normalized, replay-safe state rather than provider-specific intermediate values.

use serde::{Deserialize, Serialize};

use crate::PersistenceError;

pub const DANGER_CRASH_RESTORE_CONTRACT: &str = "gravebound.danger-crash-restore.v1";
pub const MAX_CRASH_ITEM_CHANGES: usize = 4095;
pub const MAX_CRASH_COMPONENT_CHANGES: usize = 4095;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerCrashRestoreRequest {
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub mutation_id: [u8; 16],
    pub request_hash: [u8; 32],
}

impl DangerCrashRestoreRequest {
    #[must_use]
    pub fn canonical_request_hash(
        account_id: [u8; 16],
        character_id: [u8; 16],
        restore_point_id: [u8; 16],
        mutation_id: [u8; 16],
    ) -> [u8; 32] {
        #[derive(Serialize)]
        struct Material {
            contract: &'static str,
            account_id: [u8; 16],
            character_id: [u8; 16],
            restore_point_id: [u8; 16],
            mutation_id: [u8; 16],
        }

        canonical_hash(&Material {
            contract: DANGER_CRASH_RESTORE_CONTRACT,
            account_id,
            character_id,
            restore_point_id,
            mutation_id,
        })
    }

    #[must_use]
    pub fn expected_request_hash(&self) -> [u8; 32] {
        Self::canonical_request_hash(
            self.account_id,
            self.character_id,
            self.restore_point_id,
            self.mutation_id,
        )
    }

    pub fn validate(&self) -> Result<(), PersistenceError> {
        if [
            self.account_id,
            self.character_id,
            self.restore_point_id,
            self.mutation_id,
        ]
        .contains(&[0; 16])
            || self.request_hash != self.expected_request_hash()
        {
            return Err(PersistenceError::CorruptStoredDangerCrashRestore);
        }
        Ok(())
    }

    #[must_use]
    pub fn conflict_audit_id(&self, stored_request_hash: [u8; 32]) -> [u8; 16] {
        derived_identity(
            "gravebound.danger-crash-conflict-audit.v1",
            &[
                &self.account_id,
                &self.mutation_id,
                &stored_request_hash,
                &self.request_hash,
            ],
        )
    }

    #[must_use]
    pub fn item_ledger_event_id(&self, item_uid: [u8; 16]) -> [u8; 16] {
        derived_identity(
            "gravebound.danger-crash-item-ledger.v1",
            &[&self.mutation_id, &item_uid],
        )
    }

    #[must_use]
    pub fn ash_compensation_mutation_id(&self, original_mutation_id: [u8; 16]) -> [u8; 16] {
        derived_identity(
            "gravebound.danger-crash-ash-compensation.v1",
            &[&self.mutation_id, &original_mutation_id],
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i16)]
pub enum DangerCrashRestoreCode {
    Restored = 0,
    ExtractionCommitted = 1,
    DeathCommitted = 2,
    RecallCommitted = 3,
    AlreadyCrashRestored = 4,
}

impl DangerCrashRestoreCode {
    pub const fn from_code(code: i16) -> Option<Self> {
        match code {
            0 => Some(Self::Restored),
            1 => Some(Self::ExtractionCommitted),
            2 => Some(Self::DeathCommitted),
            3 => Some(Self::RecallCommitted),
            4 => Some(Self::AlreadyCrashRestored),
            _ => None,
        }
    }

    pub const fn restore_state(self) -> i16 {
        match self {
            Self::Restored | Self::AlreadyCrashRestored => 4,
            Self::ExtractionCommitted => 1,
            Self::DeathCommitted => 2,
            Self::RecallCommitted => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerCrashRestoreVersions {
    pub account: u64,
    pub character: u64,
    pub progression: u64,
    pub inventory: u64,
    pub oath_bargain: u64,
    pub life_metrics: u64,
    pub ash_wallet: u64,
}

impl DangerCrashRestoreVersions {
    fn valid(&self) -> bool {
        self.account > 0
            && self.character > 0
            && self.progression > 0
            && self.inventory > 0
            && self.oath_bargain > 0
            && self.life_metrics > 0
            && self.ash_wallet > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DangerCrashItemChangeKind {
    Restored,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerCrashItemChange {
    pub kind: DangerCrashItemChangeKind,
    pub item_uid: [u8; 16],
    pub ledger_event_id: [u8; 16],
    pub pre_item_version: u64,
    pub post_item_version: u64,
    pub pre_security_state: i16,
    pub post_security_state: i16,
    pub pre_location_kind: i16,
    pub post_location_kind: i16,
    pub post_slot_index: Option<i16>,
}

impl DangerCrashItemChange {
    fn valid(&self) -> bool {
        self.item_uid != [0; 16]
            && self.ledger_event_id != [0; 16]
            && self.pre_item_version > 0
            && self.post_item_version == self.pre_item_version.saturating_add(1)
            && match self.kind {
                DangerCrashItemChangeKind::Restored => matches!(
                    (
                        self.post_security_state,
                        self.post_location_kind,
                        self.post_slot_index,
                    ),
                    (0, 0, Some(0..=3)) | (0, 1, Some(0..=1)) | (2, 2, Some(0..=7))
                ),
                DangerCrashItemChangeKind::Revoked => {
                    self.post_security_state == 3
                        && self.post_location_kind == 4
                        && self.post_slot_index.is_none()
                }
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerCrashMaterialChange {
    pub material_id: String,
    pub pre_quantity: u32,
    pub pre_material_version: u64,
    pub post_material_version: u64,
}

impl DangerCrashMaterialChange {
    fn valid(&self) -> bool {
        (3..=96).contains(&self.material_id.len())
            && self.pre_quantity > 0
            && self.pre_material_version > 0
            && self.post_material_version == self.pre_material_version.saturating_add(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DangerCrashBargainRecordKind {
    Offer,
    Milestone,
    Decision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerCrashBargainChange {
    pub kind: DangerCrashBargainRecordKind,
    pub record_id: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerCrashAshChange {
    pub original_mutation_id: [u8; 16],
    pub compensation_mutation_id: [u8; 16],
    pub amount: u32,
    pub pre_wallet_version: u64,
    pub post_wallet_version: u64,
}

impl DangerCrashAshChange {
    fn valid(&self) -> bool {
        self.original_mutation_id != [0; 16]
            && self.compensation_mutation_id != [0; 16]
            && self.original_mutation_id != self.compensation_mutation_id
            && (1..=99_999).contains(&self.amount)
            && self.pre_wallet_version > 0
            && self.post_wallet_version == self.pre_wallet_version.saturating_add(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerCrashRestoreReceipt {
    pub contract: String,
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub request_mutation_id: [u8; 16],
    pub request_hash: [u8; 32],
    pub code: DangerCrashRestoreCode,
    pub committed_mutation_id: Option<[u8; 16]>,
    pub versions: Option<DangerCrashRestoreVersions>,
    pub item_changes: Vec<DangerCrashItemChange>,
    pub material_changes: Vec<DangerCrashMaterialChange>,
    pub bargain_changes: Vec<DangerCrashBargainChange>,
    pub ash_changes: Vec<DangerCrashAshChange>,
}

impl DangerCrashRestoreReceipt {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.contract != DANGER_CRASH_RESTORE_CONTRACT
            || [
                self.account_id,
                self.character_id,
                self.restore_point_id,
                self.request_mutation_id,
            ]
            .contains(&[0; 16])
            || self.request_hash == [0; 32]
            || self.item_changes.len() > MAX_CRASH_ITEM_CHANGES
            || self.material_changes.len() > MAX_CRASH_COMPONENT_CHANGES
            || self.bargain_changes.len() > MAX_CRASH_COMPONENT_CHANGES
            || self.ash_changes.len() > MAX_CRASH_COMPONENT_CHANGES
            || !unique_item_changes(&self.item_changes)
            || !unique_bargain_changes(&self.bargain_changes)
            || !unique_ash_changes(&self.ash_changes)
            || self.item_changes.iter().any(|value| !value.valid())
            || self.material_changes.iter().any(|value| !value.valid())
            || self
                .bargain_changes
                .iter()
                .any(|value| value.record_id == [0; 16])
            || self.ash_changes.iter().any(|value| !value.valid())
        {
            return Err(PersistenceError::CorruptStoredDangerCrashRestore);
        }

        let committed = self.committed_mutation_id;
        match self.code {
            DangerCrashRestoreCode::Restored => {
                if committed != Some(self.request_mutation_id)
                    || self.versions.as_ref().is_none_or(|value| !value.valid())
                {
                    return Err(PersistenceError::CorruptStoredDangerCrashRestore);
                }
            }
            DangerCrashRestoreCode::AlreadyCrashRestored => {
                if committed.is_none_or(|value| value == [0; 16])
                    || self.versions.is_some()
                    || self.has_normalized_changes()
                {
                    return Err(PersistenceError::CorruptStoredDangerCrashRestore);
                }
            }
            DangerCrashRestoreCode::ExtractionCommitted
            | DangerCrashRestoreCode::DeathCommitted
            | DangerCrashRestoreCode::RecallCommitted => {
                if committed.is_some() || self.versions.is_some() || self.has_normalized_changes() {
                    return Err(PersistenceError::CorruptStoredDangerCrashRestore);
                }
            }
        }
        Ok(())
    }

    fn has_normalized_changes(&self) -> bool {
        !self.item_changes.is_empty()
            || !self.material_changes.is_empty()
            || !self.bargain_changes.is_empty()
            || !self.ash_changes.is_empty()
    }

    pub fn payload(&self) -> Result<Vec<u8>, PersistenceError> {
        self.validate()?;
        postcard::to_stdvec(self).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)
    }

    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        canonical_hash(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DangerCrashRestoreTransaction {
    Fresh(DangerCrashRestoreReceipt),
    Replayed(DangerCrashRestoreReceipt),
    Conflict {
        mutation_id: [u8; 16],
        stored_request_hash: [u8; 32],
        attempted_request_hash: [u8; 32],
        audit_id: [u8; 16],
    },
}

pub(crate) fn derived_identity(context: &str, parts: &[&[u8]]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new_derive_key(context);
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    let mut value = [0; 16];
    value.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    value
}

fn canonical_hash<T: Serialize>(value: &T) -> [u8; 32] {
    let bytes =
        postcard::to_stdvec(value).expect("bounded canonical crash restore value serializes");
    *blake3::hash(&bytes).as_bytes()
}

fn unique_item_changes(values: &[DangerCrashItemChange]) -> bool {
    !values.iter().enumerate().any(|(index, value)| {
        values[index + 1..].iter().any(|other| {
            other.item_uid == value.item_uid || other.ledger_event_id == value.ledger_event_id
        })
    })
}

fn unique_bargain_changes(values: &[DangerCrashBargainChange]) -> bool {
    !values.iter().enumerate().any(|(index, value)| {
        values[index + 1..]
            .iter()
            .any(|other| other.kind == value.kind && other.record_id == value.record_id)
    })
}

fn unique_ash_changes(values: &[DangerCrashAshChange]) -> bool {
    !values.iter().enumerate().any(|(index, value)| {
        values[index + 1..].iter().any(|other| {
            other.original_mutation_id == value.original_mutation_id
                || other.compensation_mutation_id == value.compensation_mutation_id
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> DangerCrashRestoreRequest {
        let mut value = DangerCrashRestoreRequest {
            account_id: [1; 16],
            character_id: [2; 16],
            restore_point_id: [3; 16],
            mutation_id: [4; 16],
            request_hash: [0; 32],
        };
        value.request_hash = value.expected_request_hash();
        value
    }

    fn restored_receipt() -> DangerCrashRestoreReceipt {
        let request = request();
        DangerCrashRestoreReceipt {
            contract: DANGER_CRASH_RESTORE_CONTRACT.into(),
            account_id: request.account_id,
            character_id: request.character_id,
            restore_point_id: request.restore_point_id,
            request_mutation_id: request.mutation_id,
            request_hash: request.request_hash,
            code: DangerCrashRestoreCode::Restored,
            committed_mutation_id: Some(request.mutation_id),
            versions: Some(DangerCrashRestoreVersions {
                account: 2,
                character: 2,
                progression: 2,
                inventory: 2,
                oath_bargain: 2,
                life_metrics: 2,
                ash_wallet: 2,
            }),
            item_changes: Vec::new(),
            material_changes: Vec::new(),
            bargain_changes: Vec::new(),
            ash_changes: Vec::new(),
        }
    }

    #[test]
    fn request_hash_binds_every_authoritative_identity() {
        let request = request();
        assert!(request.validate().is_ok());
        let mut altered = request.clone();
        altered.restore_point_id = [9; 16];
        assert!(matches!(
            altered.validate(),
            Err(PersistenceError::CorruptStoredDangerCrashRestore)
        ));

        let stored_hash = request.request_hash;
        altered.request_hash = altered.expected_request_hash();
        assert_eq!(
            altered.conflict_audit_id(stored_hash),
            altered.conflict_audit_id(stored_hash)
        );
        assert_ne!(
            altered.conflict_audit_id(stored_hash),
            altered.conflict_audit_id([8; 32])
        );
    }

    #[test]
    fn terminal_receipt_shapes_are_disjoint_and_canonical() {
        let restored = restored_receipt();
        assert!(restored.validate().is_ok());
        assert_eq!(restored.digest(), restored.digest());
        assert!(!restored.payload().unwrap().is_empty());

        let mut superseded = restored.clone();
        superseded.code = DangerCrashRestoreCode::DeathCommitted;
        superseded.committed_mutation_id = None;
        superseded.versions = None;
        assert!(superseded.validate().is_ok());

        superseded.item_changes.push(DangerCrashItemChange {
            kind: DangerCrashItemChangeKind::Revoked,
            item_uid: [5; 16],
            ledger_event_id: [6; 16],
            pre_item_version: 1,
            post_item_version: 2,
            pre_security_state: 2,
            post_security_state: 3,
            pre_location_kind: 2,
            post_location_kind: 4,
            post_slot_index: None,
        });
        assert!(superseded.validate().is_err());
    }

    #[test]
    fn normalized_change_validation_rejects_duplicates_and_bad_arithmetic() {
        let mut receipt = restored_receipt();
        let item = DangerCrashItemChange {
            kind: DangerCrashItemChangeKind::Restored,
            item_uid: [5; 16],
            ledger_event_id: [6; 16],
            pre_item_version: 4,
            post_item_version: 5,
            pre_security_state: 4,
            post_security_state: 0,
            pre_location_kind: 7,
            post_location_kind: 1,
            post_slot_index: Some(0),
        };
        receipt.item_changes.push(item.clone());
        assert!(receipt.validate().is_ok());
        receipt.item_changes.push(item);
        assert!(receipt.validate().is_err());
    }

    #[test]
    fn derived_identities_are_domain_separated_and_stable() {
        let first = derived_identity("gravebound.test.a", &[&[1; 16], &[2; 16]]);
        assert_eq!(
            first,
            derived_identity("gravebound.test.a", &[&[1; 16], &[2; 16]])
        );
        assert_ne!(
            first,
            derived_identity("gravebound.test.b", &[&[1; 16], &[2; 16]])
        );
        assert_ne!(first, [0; 16]);
    }
}
