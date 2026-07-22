//! Reconnect-safe client model for the authoritative Hall Vault and Overflow surfaces.
//!
//! Queries are bounded and version-rooted. Mutations retain their exact frame across transport
//! loss, while all destinations remain server planned.

use protocol::{
    CHARACTER_ID_BYTES, MUTATION_ID_BYTES, SAFE_STORAGE_SCHEMA_VERSION, SafeInventoryDestinationV1,
    SafeInventoryResultCodeV1, SafeInventoryTransferFrameV1, SafeInventoryTransferKindV1,
    SafeInventoryTransferPayloadV1, SafeInventoryTransferResultV1, SafeStorageQueryCodeV1,
    SafeStorageQueryFrameV1, SafeStorageQueryResultV1, SafeStorageStackV1, SafeStorageSurfaceV1,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SafeStorageClientPhase {
    Dormant,
    Loading,
    Ready,
    Mutating,
    Reconnecting,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SafeStorageSelectionPane {
    CharacterSafe,
    Surface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SafeStorageApplyOutcome {
    Continue,
    Ready,
    Restart,
    QueryRejected(SafeStorageQueryCodeV1),
    MutationStored,
    MutationRejected(SafeInventoryResultCodeV1),
}

#[derive(Debug, Clone)]
struct PendingQuery {
    sequence: u32,
    after_slot: Option<u16>,
}

#[derive(Debug, Clone)]
pub(crate) struct SafeStorageClientModel {
    phase: SafeStorageClientPhase,
    surface: Option<SafeStorageSurfaceV1>,
    character_id: Option<[u8; CHARACTER_ID_BYTES]>,
    account_version: Option<u64>,
    inventory_version: Option<u64>,
    content_revision: Option<String>,
    character_safe: Vec<SafeStorageStackV1>,
    stacks: Vec<SafeStorageStackV1>,
    next_after_slot: Option<u16>,
    pending_query: Option<PendingQuery>,
    pending_mutation: Option<SafeInventoryTransferFrameV1>,
    selected_pane: SafeStorageSelectionPane,
    selected_index: usize,
    last_query_code: Option<SafeStorageQueryCodeV1>,
    last_mutation_code: Option<SafeInventoryResultCodeV1>,
}

impl Default for SafeStorageClientModel {
    fn default() -> Self {
        Self {
            phase: SafeStorageClientPhase::Dormant,
            surface: None,
            character_id: None,
            account_version: None,
            inventory_version: None,
            content_revision: None,
            character_safe: Vec::new(),
            stacks: Vec::new(),
            next_after_slot: None,
            pending_query: None,
            pending_mutation: None,
            selected_pane: SafeStorageSelectionPane::Surface,
            selected_index: 0,
            last_query_code: None,
            last_mutation_code: None,
        }
    }
}

impl SafeStorageClientModel {
    pub(crate) fn open(
        &mut self,
        surface: SafeStorageSurfaceV1,
        sequence: u32,
        character_id: [u8; CHARACTER_ID_BYTES],
    ) -> SafeStorageQueryFrameV1 {
        self.phase = SafeStorageClientPhase::Loading;
        self.surface = Some(surface);
        self.character_id = Some(character_id);
        self.account_version = None;
        self.inventory_version = None;
        self.content_revision = None;
        self.character_safe.clear();
        self.stacks.clear();
        self.next_after_slot = None;
        self.pending_query = Some(PendingQuery {
            sequence,
            after_slot: None,
        });
        self.selected_pane = SafeStorageSelectionPane::Surface;
        self.selected_index = 0;
        self.last_query_code = None;
        SafeStorageQueryFrameV1 {
            schema_version: SAFE_STORAGE_SCHEMA_VERSION,
            sequence,
            character_id,
            surface,
            after_slot: None,
            expected_account_version: None,
            expected_inventory_version: None,
        }
    }

    pub(crate) fn continue_query(
        &mut self,
        sequence: u32,
    ) -> Result<SafeStorageQueryFrameV1, SafeStorageClientError> {
        if self.phase != SafeStorageClientPhase::Loading || self.pending_query.is_some() {
            return Err(SafeStorageClientError::ActionUnavailable);
        }
        let after_slot = self
            .next_after_slot
            .ok_or(SafeStorageClientError::ActionUnavailable)?;
        let frame = SafeStorageQueryFrameV1 {
            schema_version: SAFE_STORAGE_SCHEMA_VERSION,
            sequence,
            character_id: self
                .character_id
                .ok_or(SafeStorageClientError::AuthorityMismatch)?,
            surface: self
                .surface
                .ok_or(SafeStorageClientError::AuthorityMismatch)?,
            after_slot: Some(after_slot),
            expected_account_version: self.account_version,
            expected_inventory_version: self.inventory_version,
        };
        frame
            .validate()
            .map_err(|_| SafeStorageClientError::AuthorityMismatch)?;
        self.pending_query = Some(PendingQuery {
            sequence,
            after_slot: Some(after_slot),
        });
        Ok(frame)
    }

    pub(crate) fn apply_query_result(
        &mut self,
        result: &SafeStorageQueryResultV1,
        expected_content_revision: &str,
    ) -> Result<SafeStorageApplyOutcome, SafeStorageClientError> {
        result
            .validate()
            .map_err(|_| SafeStorageClientError::AuthorityMismatch)?;
        let pending = self
            .pending_query
            .take()
            .ok_or(SafeStorageClientError::UnexpectedResult)?;
        match result {
            SafeStorageQueryResultV1::Rejected { sequence, code, .. } => {
                if *sequence != pending.sequence {
                    return Err(SafeStorageClientError::UnexpectedResult);
                }
                if *code == SafeStorageQueryCodeV1::StaleVersions {
                    return Ok(SafeStorageApplyOutcome::Restart);
                }
                self.last_query_code = Some(*code);
                self.phase = SafeStorageClientPhase::Failed;
                Ok(SafeStorageApplyOutcome::QueryRejected(*code))
            }
            SafeStorageQueryResultV1::Stored {
                sequence,
                character_id,
                surface,
                account_version,
                inventory_version,
                content_revision,
                character_safe,
                stacks,
                next_after_slot,
                ..
            } => {
                if *sequence != pending.sequence
                    || Some(*character_id) != self.character_id
                    || Some(*surface) != self.surface
                    || content_revision.as_str() != expected_content_revision
                {
                    return Err(SafeStorageClientError::AuthorityMismatch);
                }
                if pending.after_slot.is_none() {
                    self.account_version = Some(*account_version);
                    self.inventory_version = Some(*inventory_version);
                    self.content_revision = Some(content_revision.as_str().to_owned());
                    self.character_safe.clone_from(character_safe);
                    self.stacks.clone_from(stacks);
                } else {
                    if self.account_version != Some(*account_version)
                        || self.inventory_version != Some(*inventory_version)
                        || self.content_revision.as_deref() != Some(content_revision.as_str())
                        || self.character_safe != *character_safe
                    {
                        return Err(SafeStorageClientError::AuthorityMismatch);
                    }
                    self.stacks.extend(stacks.iter().cloned());
                }
                self.next_after_slot = *next_after_slot;
                if self.next_after_slot.is_some() {
                    Ok(SafeStorageApplyOutcome::Continue)
                } else {
                    self.phase = SafeStorageClientPhase::Ready;
                    self.clamp_selection();
                    Ok(SafeStorageApplyOutcome::Ready)
                }
            }
        }
    }

    pub(crate) fn begin_selected_transfer(
        &mut self,
        mutation_id: [u8; MUTATION_ID_BYTES],
        issued_at_unix_millis: u64,
    ) -> Result<SafeInventoryTransferFrameV1, SafeStorageClientError> {
        if self.phase != SafeStorageClientPhase::Ready || self.pending_mutation.is_some() {
            return Err(SafeStorageClientError::ActionUnavailable);
        }
        let surface = self
            .surface
            .ok_or(SafeStorageClientError::AuthorityMismatch)?;
        let stack = self
            .selected_stack()
            .ok_or(SafeStorageClientError::ActionUnavailable)?;
        let kind = match (surface, self.selected_pane) {
            (SafeStorageSurfaceV1::Vault, SafeStorageSelectionPane::CharacterSafe) => {
                SafeInventoryTransferKindV1::CharacterSafeToVault
            }
            (SafeStorageSurfaceV1::Vault, SafeStorageSelectionPane::Surface) => {
                SafeInventoryTransferKindV1::VaultToCharacterSafe
            }
            (SafeStorageSurfaceV1::Overflow, SafeStorageSelectionPane::Surface) => {
                SafeInventoryTransferKindV1::OverflowToCharacterSafe
            }
            (SafeStorageSurfaceV1::Overflow, SafeStorageSelectionPane::CharacterSafe) => {
                return Err(SafeStorageClientError::ActionUnavailable);
            }
        };
        let payload = SafeInventoryTransferPayloadV1 {
            kind,
            source_slot_index: stack.slot_index,
            expected_account_version: self
                .account_version
                .ok_or(SafeStorageClientError::AuthorityMismatch)?,
            expected_inventory_version: self
                .inventory_version
                .ok_or(SafeStorageClientError::AuthorityMismatch)?,
        };
        let frame = SafeInventoryTransferFrameV1 {
            mutation_id,
            character_id: self
                .character_id
                .ok_or(SafeStorageClientError::AuthorityMismatch)?,
            issued_at_unix_millis,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        frame
            .validate()
            .map_err(|_| SafeStorageClientError::AuthorityMismatch)?;
        self.pending_mutation = Some(frame);
        self.phase = SafeStorageClientPhase::Mutating;
        Ok(frame)
    }

    pub(crate) fn apply_transfer_result(
        &mut self,
        result: &SafeInventoryTransferResultV1,
    ) -> Result<SafeStorageApplyOutcome, SafeStorageClientError> {
        result
            .validate()
            .map_err(|_| SafeStorageClientError::AuthorityMismatch)?;
        let pending = self
            .pending_mutation
            .as_ref()
            .ok_or(SafeStorageClientError::UnexpectedResult)?;
        if result.mutation_id != pending.mutation_id
            || Some(result.character_id) != self.character_id
        {
            return Err(SafeStorageClientError::UnexpectedResult);
        }
        if result.code == SafeInventoryResultCodeV1::Accepted
            && (pending.payload.expected_account_version.checked_add(1)
                != Some(result.account_version)
                || pending.payload.expected_inventory_version.checked_add(1)
                    != Some(result.inventory_version))
        {
            return Err(SafeStorageClientError::AuthorityMismatch);
        }
        if result.code == SafeInventoryResultCodeV1::Accepted
            && !result.placements.iter().all(|placement| {
                matches!(
                    (pending.payload.kind, placement.destination),
                    (
                        SafeInventoryTransferKindV1::CharacterSafeToVault,
                        SafeInventoryDestinationV1::Vault { .. }
                    ) | (
                        SafeInventoryTransferKindV1::VaultToCharacterSafe
                            | SafeInventoryTransferKindV1::OverflowToCharacterSafe,
                        SafeInventoryDestinationV1::CharacterSafe { .. }
                    )
                )
            })
        {
            return Err(SafeStorageClientError::AuthorityMismatch);
        }
        self.last_mutation_code = Some(result.code);
        if result.code == SafeInventoryResultCodeV1::ServiceUnavailable {
            self.phase = SafeStorageClientPhase::Reconnecting;
            return Ok(SafeStorageApplyOutcome::MutationRejected(result.code));
        }
        self.pending_mutation = None;
        self.phase = SafeStorageClientPhase::Loading;
        Ok(if result.code == SafeInventoryResultCodeV1::Accepted {
            SafeStorageApplyOutcome::MutationStored
        } else {
            SafeStorageApplyOutcome::MutationRejected(result.code)
        })
    }

    pub(crate) fn transport_lost(&mut self) {
        self.pending_query = None;
        self.close();
    }

    pub(crate) fn exact_mutation_retry(&self) -> Option<SafeInventoryTransferFrameV1> {
        self.pending_mutation
    }

    pub(crate) fn close(&mut self) {
        let Some(pending_mutation) = self.pending_mutation else {
            *self = Self::default();
            return;
        };
        let character_id = self.character_id;
        *self = Self::default();
        self.pending_mutation = Some(pending_mutation);
        self.character_id = character_id;
        self.phase = SafeStorageClientPhase::Reconnecting;
    }

    pub(crate) fn select_previous(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    pub(crate) fn select_next(&mut self) {
        let length = self.selected_stacks().len();
        if self.selected_index + 1 < length {
            self.selected_index += 1;
        }
    }

    pub(crate) fn toggle_pane(&mut self) {
        self.selected_pane = match self.selected_pane {
            SafeStorageSelectionPane::CharacterSafe if !self.stacks.is_empty() => {
                SafeStorageSelectionPane::Surface
            }
            SafeStorageSelectionPane::Surface if !self.character_safe.is_empty() => {
                SafeStorageSelectionPane::CharacterSafe
            }
            pane => pane,
        };
        self.selected_index = 0;
    }

    pub(crate) const fn phase(&self) -> SafeStorageClientPhase {
        self.phase
    }

    pub(crate) const fn surface(&self) -> Option<SafeStorageSurfaceV1> {
        self.surface
    }

    pub(crate) const fn selected_pane(&self) -> SafeStorageSelectionPane {
        self.selected_pane
    }

    pub(crate) const fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub(crate) fn character_safe(&self) -> &[SafeStorageStackV1] {
        &self.character_safe
    }

    pub(crate) fn stacks(&self) -> &[SafeStorageStackV1] {
        &self.stacks
    }

    pub(crate) const fn versions(&self) -> Option<(u64, u64)> {
        match (self.account_version, self.inventory_version) {
            (Some(account), Some(inventory)) => Some((account, inventory)),
            _ => None,
        }
    }

    pub(crate) const fn last_mutation_code(&self) -> Option<SafeInventoryResultCodeV1> {
        self.last_mutation_code
    }

    pub(crate) const fn last_query_code(&self) -> Option<SafeStorageQueryCodeV1> {
        self.last_query_code
    }

    pub(crate) fn captures_input(&self) -> bool {
        self.surface.is_some()
    }

    fn selected_stacks(&self) -> &[SafeStorageStackV1] {
        match self.selected_pane {
            SafeStorageSelectionPane::CharacterSafe => &self.character_safe,
            SafeStorageSelectionPane::Surface => &self.stacks,
        }
    }

    fn selected_stack(&self) -> Option<&SafeStorageStackV1> {
        self.selected_stacks().get(self.selected_index)
    }

    fn clamp_selection(&mut self) {
        if self.selected_stacks().is_empty() {
            self.toggle_pane();
        }
        self.selected_index = self
            .selected_index
            .min(self.selected_stacks().len().saturating_sub(1));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum SafeStorageClientError {
    #[error("safe-storage action is unavailable")]
    ActionUnavailable,
    #[error("safe-storage authority does not match the selected Hall aggregate")]
    AuthorityMismatch,
    #[error("safe-storage response does not match the pending request")]
    UnexpectedResult,
}
