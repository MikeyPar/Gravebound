use persistence::{
    PersistenceError, PostgresPersistence, StoredSafeStorageItem, StoredSafeStoragePage,
    StoredSafeStorageStack, StoredSafeStorageSurface,
};
use protocol::{
    SAFE_STORAGE_SCHEMA_VERSION, SafeStorageItemKindV1, SafeStorageItemV1, SafeStorageLocationV1,
    SafeStorageProvenanceV1, SafeStorageQueryCodeV1, SafeStorageQueryFrameV1,
    SafeStorageQueryResultV1, SafeStorageRarityV1, SafeStorageSecurityV1, SafeStorageStackV1,
    SafeStorageSurfaceV1, WireText,
};

use crate::{AuthenticatedAccount, AuthenticatedNamespace};

#[derive(Debug, Clone)]
pub struct PostgresSafeStorageService {
    persistence: PostgresPersistence,
}

impl PostgresSafeStorageService {
    #[must_use]
    pub const fn new(persistence: PostgresPersistence) -> Self {
        Self { persistence }
    }

    pub async fn query(
        &self,
        account_id: [u8; 16],
        frame: &SafeStorageQueryFrameV1,
    ) -> Result<SafeStorageQueryResultV1, SafeStorageServiceError> {
        frame
            .validate()
            .map_err(|_| SafeStorageServiceError::InvalidRequest)?;
        let expected_versions = frame
            .expected_account_version
            .zip(frame.expected_inventory_version);
        let page = self
            .persistence
            .load_safe_storage_page(
                account_id,
                frame.character_id,
                stored_surface(frame.surface),
                frame.after_slot,
                expected_versions,
            )
            .await
            .map_err(map_persistence)?;
        project_page(frame, page)
    }
}

#[derive(Debug, Clone)]
pub enum CoreSafeStorageAuthority {
    Disabled,
    Persistent(PostgresSafeStorageService),
}

impl CoreSafeStorageAuthority {
    #[must_use]
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    #[must_use]
    pub const fn persistent(service: PostgresSafeStorageService) -> Self {
        Self::Persistent(service)
    }

    pub async fn query(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &SafeStorageQueryFrameV1,
    ) -> SafeStorageQueryResultV1 {
        let result = match self {
            Self::Persistent(service)
                if authenticated.namespace == AuthenticatedNamespace::WipeableTest =>
            {
                service
                    .query(authenticated.account_id.as_bytes(), frame)
                    .await
            }
            Self::Disabled | Self::Persistent(_) => Err(SafeStorageServiceError::FeatureDisabled),
        };
        result.unwrap_or_else(|error| rejected(frame.sequence, error.code()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeStorageServiceError {
    InvalidRequest,
    FeatureDisabled,
    HallBinding,
    ForeignAuthority,
    StaleVersions,
    CorruptStoredAuthority,
    Persistence,
}

impl SafeStorageServiceError {
    const fn code(self) -> SafeStorageQueryCodeV1 {
        match self {
            Self::InvalidRequest => SafeStorageQueryCodeV1::InvalidRequest,
            Self::FeatureDisabled => SafeStorageQueryCodeV1::FeatureDisabled,
            Self::HallBinding => SafeStorageQueryCodeV1::HallBindingRequired,
            Self::ForeignAuthority => SafeStorageQueryCodeV1::ForeignAuthority,
            Self::StaleVersions => SafeStorageQueryCodeV1::StaleVersions,
            Self::CorruptStoredAuthority => SafeStorageQueryCodeV1::CorruptStoredAuthority,
            Self::Persistence => SafeStorageQueryCodeV1::ServiceUnavailable,
        }
    }
}

fn project_page(
    frame: &SafeStorageQueryFrameV1,
    page: StoredSafeStoragePage,
) -> Result<SafeStorageQueryResultV1, SafeStorageServiceError> {
    if stored_surface(frame.surface) != page.surface {
        return Err(SafeStorageServiceError::CorruptStoredAuthority);
    }
    let result = SafeStorageQueryResultV1::Stored {
        schema_version: SAFE_STORAGE_SCHEMA_VERSION,
        sequence: frame.sequence,
        character_id: frame.character_id,
        surface: frame.surface,
        account_version: page.account_version,
        inventory_version: page.inventory_version,
        content_revision: WireText::new(page.content_revision)
            .map_err(|_| SafeStorageServiceError::CorruptStoredAuthority)?,
        character_safe: page
            .character_safe
            .into_iter()
            .map(|stack| project_stack(SafeStorageLocationV1::CharacterSafe, stack))
            .collect::<Result<Vec<_>, _>>()?,
        stacks: page
            .stacks
            .into_iter()
            .map(|stack| project_stack(surface_location(frame.surface), stack))
            .collect::<Result<Vec<_>, _>>()?,
        next_after_slot: page.next_after_slot,
    };
    result
        .validate()
        .map_err(|_| SafeStorageServiceError::CorruptStoredAuthority)?;
    Ok(result)
}

fn project_stack(
    location: SafeStorageLocationV1,
    stack: StoredSafeStorageStack,
) -> Result<SafeStorageStackV1, SafeStorageServiceError> {
    let first = stack
        .items
        .first()
        .ok_or(SafeStorageServiceError::CorruptStoredAuthority)?;
    let item_level = first.item_level;
    let rarity = first.rarity.map(project_rarity).transpose()?;
    let security = project_security(first.security_state)?;
    let provenance = project_provenance(first.provenance_kind)?;
    let salvage_band = first.salvage_band;
    let salvage_value = first.salvage_value;
    let overflow_expires_at_unix_millis = stack
        .items
        .iter()
        .filter_map(|item| item.overflow_expires_at_unix_millis)
        .min();
    Ok(SafeStorageStackV1 {
        location,
        slot_index: stack.slot_index,
        template_id: WireText::new(stack.template_id)
            .map_err(|_| SafeStorageServiceError::CorruptStoredAuthority)?,
        item_kind: match stack.item_kind {
            0 => SafeStorageItemKindV1::Equipment,
            1 => SafeStorageItemKindV1::Consumable,
            _ => return Err(SafeStorageServiceError::CorruptStoredAuthority),
        },
        item_level,
        rarity,
        security,
        provenance,
        salvage_band,
        salvage_value,
        overflow_expires_at_unix_millis,
        items: stack
            .items
            .into_iter()
            .map(project_item)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn project_item(item: StoredSafeStorageItem) -> Result<SafeStorageItemV1, SafeStorageServiceError> {
    Ok(SafeStorageItemV1 {
        item_uid: item.item_uid,
        item_version: item.item_version,
        item_level: item.item_level,
        rarity: item.rarity.map(project_rarity).transpose()?,
        security: project_security(item.security_state)?,
        provenance: project_provenance(item.provenance_kind)?,
        salvage_band: item.salvage_band,
        salvage_value: item.salvage_value,
        overflow_expires_at_unix_millis: item.overflow_expires_at_unix_millis,
    })
}

const fn project_security(value: u8) -> Result<SafeStorageSecurityV1, SafeStorageServiceError> {
    match value {
        0 => Ok(SafeStorageSecurityV1::Safe),
        _ => Err(SafeStorageServiceError::CorruptStoredAuthority),
    }
}

const fn project_provenance(value: u8) -> Result<SafeStorageProvenanceV1, SafeStorageServiceError> {
    match value {
        0 => Ok(SafeStorageProvenanceV1::Starter),
        1 => Ok(SafeStorageProvenanceV1::Drop),
        2 => Ok(SafeStorageProvenanceV1::Craft),
        3 => Ok(SafeStorageProvenanceV1::Gift),
        4 => Ok(SafeStorageProvenanceV1::Grant),
        5 => Ok(SafeStorageProvenanceV1::Migration),
        _ => Err(SafeStorageServiceError::CorruptStoredAuthority),
    }
}

const fn project_rarity(value: u8) -> Result<SafeStorageRarityV1, SafeStorageServiceError> {
    match value {
        0 => Ok(SafeStorageRarityV1::Worn),
        1 => Ok(SafeStorageRarityV1::Forged),
        2 => Ok(SafeStorageRarityV1::Oathed),
        3 => Ok(SafeStorageRarityV1::Relic),
        4 => Ok(SafeStorageRarityV1::Sainted),
        _ => Err(SafeStorageServiceError::CorruptStoredAuthority),
    }
}

const fn stored_surface(surface: SafeStorageSurfaceV1) -> StoredSafeStorageSurface {
    match surface {
        SafeStorageSurfaceV1::Vault => StoredSafeStorageSurface::Vault,
        SafeStorageSurfaceV1::Overflow => StoredSafeStorageSurface::Overflow,
    }
}

const fn surface_location(surface: SafeStorageSurfaceV1) -> SafeStorageLocationV1 {
    match surface {
        SafeStorageSurfaceV1::Vault => SafeStorageLocationV1::Vault,
        SafeStorageSurfaceV1::Overflow => SafeStorageLocationV1::Overflow,
    }
}

fn rejected(sequence: u32, code: SafeStorageQueryCodeV1) -> SafeStorageQueryResultV1 {
    SafeStorageQueryResultV1::Rejected {
        schema_version: SAFE_STORAGE_SCHEMA_VERSION,
        sequence: sequence.max(1),
        code,
    }
}

#[must_use]
pub(crate) const fn unauthorized_panel_code(in_hall: bool) -> SafeStorageQueryCodeV1 {
    if in_hall {
        SafeStorageQueryCodeV1::WrongPanel
    } else {
        SafeStorageQueryCodeV1::HallBindingRequired
    }
}

fn map_persistence(error: PersistenceError) -> SafeStorageServiceError {
    match error {
        PersistenceError::SafeStorageHallBindingMismatch => SafeStorageServiceError::HallBinding,
        PersistenceError::SafeStorageForeignAuthority => SafeStorageServiceError::ForeignAuthority,
        PersistenceError::SafeStorageVersionMismatch => SafeStorageServiceError::StaleVersions,
        PersistenceError::CorruptStoredSafeStorage => {
            SafeStorageServiceError::CorruptStoredAuthority
        }
        _ => SafeStorageServiceError::Persistence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_versions_remain_a_typed_restart_signal() {
        assert_eq!(
            SafeStorageServiceError::StaleVersions.code(),
            SafeStorageQueryCodeV1::StaleVersions
        );
    }

    #[test]
    fn foreign_character_and_wrong_panel_fail_closed() {
        assert_eq!(
            map_persistence(PersistenceError::SafeStorageForeignAuthority),
            SafeStorageServiceError::ForeignAuthority
        );
        assert_eq!(
            unauthorized_panel_code(true),
            SafeStorageQueryCodeV1::WrongPanel
        );
        assert_eq!(
            unauthorized_panel_code(false),
            SafeStorageQueryCodeV1::HallBindingRequired
        );
    }

    #[tokio::test]
    async fn disabled_authority_is_fail_closed() {
        let frame = SafeStorageQueryFrameV1 {
            schema_version: 1,
            sequence: 7,
            character_id: [2; 16],
            surface: SafeStorageSurfaceV1::Vault,
            after_slot: None,
            expected_account_version: None,
            expected_inventory_version: None,
        };
        let result = CoreSafeStorageAuthority::disabled()
            .query(
                AuthenticatedAccount {
                    account_id: crate::AccountId::new([1; 16]).unwrap(),
                    namespace: AuthenticatedNamespace::WipeableTest,
                },
                &frame,
            )
            .await;
        assert!(matches!(
            result,
            SafeStorageQueryResultV1::Rejected {
                code: SafeStorageQueryCodeV1::FeatureDisabled,
                ..
            }
        ));
    }
}
