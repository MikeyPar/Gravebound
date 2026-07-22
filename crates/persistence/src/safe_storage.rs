use sqlx::{PgConnection, Row, postgres::PgRow};

use crate::{
    CORE_ITEM_CONTENT_REVISION, PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
};

const MAX_PAGE_STACKS: usize = 32;
const MAX_STACK_ITEMS: usize = 6;
const CHARACTER_SAFE_LOCATION: i16 = 5;
const VAULT_LOCATION: i16 = 6;
const OVERFLOW_LOCATION: i16 = 8;
const MAX_TRANSACTION_ATTEMPTS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredSafeStorageSurface {
    Vault,
    Overflow,
}

impl StoredSafeStorageSurface {
    const fn location_kind(self) -> i16 {
        match self {
            Self::Vault => VAULT_LOCATION,
            Self::Overflow => OVERFLOW_LOCATION,
        }
    }

    const fn capacity(self) -> u16 {
        match self {
            Self::Vault => 160,
            Self::Overflow => 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSafeStorageItem {
    pub item_uid: [u8; 16],
    pub item_version: u64,
    pub item_level: Option<u8>,
    pub rarity: Option<u8>,
    pub security_state: u8,
    pub provenance_kind: u8,
    pub salvage_band: u8,
    pub salvage_value: u32,
    pub overflow_expires_at_unix_millis: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSafeStorageStack {
    pub slot_index: u16,
    pub template_id: String,
    pub item_kind: u8,
    pub items: Vec<StoredSafeStorageItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSafeStoragePage {
    pub surface: StoredSafeStorageSurface,
    pub account_version: u64,
    pub inventory_version: u64,
    pub content_revision: String,
    pub character_safe: Vec<StoredSafeStorageStack>,
    pub stacks: Vec<StoredSafeStorageStack>,
    pub next_after_slot: Option<u16>,
}

impl PostgresPersistence {
    /// Reads one bounded safe-storage page plus the complete eight-slot CharacterSafe companion.
    /// The exact selected living Hall binding and both version roots are checked before and after
    /// the page query so a concurrent mutation forces the caller to restart pagination.
    pub async fn load_safe_storage_page(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        surface: StoredSafeStorageSurface,
        after_slot: Option<u16>,
        expected_versions: Option<(u64, u64)>,
    ) -> Result<StoredSafeStoragePage, PersistenceError> {
        if account_id == [0; 16]
            || character_id == [0; 16]
            || after_slot.is_some_and(|slot| slot >= surface.capacity())
            || expected_versions.is_some_and(|(account, inventory)| account == 0 || inventory == 0)
            || (after_slot.is_some() != expected_versions.is_some())
        {
            return Err(PersistenceError::CorruptStoredSafeStorage);
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .load_safe_storage_page_once(
                    account_id,
                    character_id,
                    surface,
                    after_slot,
                    expected_versions,
                )
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && crate::is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded safe-storage read always returns")
    }

    async fn load_safe_storage_page_once(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        surface: StoredSafeStorageSurface,
        after_slot: Option<u16>,
        expected_versions: Option<(u64, u64)>,
    ) -> Result<StoredSafeStoragePage, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let initial =
            load_authority_versions(transaction.connection(), account_id, character_id).await?;
        require_expected_versions(initial, expected_versions)?;
        let character_safe_rows =
            fetch_character_safe_rows(transaction.connection(), account_id, character_id).await?;
        let surface_rows = fetch_surface_rows(
            transaction.connection(),
            account_id,
            character_id,
            surface,
            after_slot,
        )
        .await?;
        let final_versions =
            load_versions_only(transaction.connection(), account_id, character_id).await?;
        if final_versions != initial {
            return Err(PersistenceError::SafeStorageVersionMismatch);
        }
        let character_safe = decode_stacks(character_safe_rows, CHARACTER_SAFE_LOCATION, 8)?;
        let mut all_surface = decode_stacks(
            surface_rows,
            surface.location_kind(),
            usize::from(surface.capacity()),
        )?;
        let has_more = all_surface.len() > MAX_PAGE_STACKS;
        if has_more {
            all_surface.truncate(MAX_PAGE_STACKS);
        }
        let next_after_slot = has_more.then(|| {
            all_surface
                .last()
                .expect("has-more page retains exactly 32 stacks")
                .slot_index
        });
        transaction.rollback().await?;
        Ok(StoredSafeStoragePage {
            surface,
            account_version: initial.0,
            inventory_version: initial.1,
            content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
            character_safe,
            stacks: all_surface,
            next_after_slot,
        })
    }
}

fn require_expected_versions(
    actual: (u64, u64),
    expected: Option<(u64, u64)>,
) -> Result<(), PersistenceError> {
    if expected.is_some_and(|expected| expected != actual) {
        return Err(PersistenceError::SafeStorageVersionMismatch);
    }
    Ok(())
}

async fn load_authority_versions(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<(u64, u64), PersistenceError> {
    let selected = sqlx::query_scalar::<_, Option<Vec<u8>>>(
        "SELECT selected_character_id FROM accounts WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::SafeStorageHallBindingMismatch)?;
    if selected
        .map(|value| value.as_slice() != character_id)
        .unwrap_or(true)
    {
        return Err(PersistenceError::SafeStorageForeignAuthority);
    }
    let row = sqlx::query(
        "SELECT a.state_version AS account_version, i.inventory_version \
         FROM accounts a JOIN characters c ON c.namespace_id = a.namespace_id \
         AND c.account_id = a.account_id AND c.character_id = $3 \
         JOIN character_world_locations w ON w.namespace_id = c.namespace_id \
         AND w.account_id = c.account_id AND w.character_id = c.character_id \
         JOIN character_inventories i ON i.namespace_id = c.namespace_id \
         AND i.account_id = c.account_id AND i.character_id = c.character_id \
         WHERE a.namespace_id = $1 AND a.account_id = $2 \
         AND a.selected_character_id = c.character_id AND c.life_state = 0 \
         AND c.security_state = 0 AND w.location_kind = 1 \
         AND w.location_content_id = 'hub.lantern_halls_01'",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::SafeStorageHallBindingMismatch)?;
    Ok((
        positive(row.try_get("account_version")?)?,
        positive(row.try_get("inventory_version")?)?,
    ))
}

async fn load_versions_only(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<(u64, u64), PersistenceError> {
    let row = sqlx::query(
        "SELECT a.state_version AS account_version, i.inventory_version \
         FROM accounts a JOIN character_inventories i ON i.namespace_id = a.namespace_id \
         AND i.account_id = a.account_id AND i.character_id = $3 \
         WHERE a.namespace_id = $1 AND a.account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::SafeStorageHallBindingMismatch)?;
    Ok((
        positive(row.try_get("account_version")?)?,
        positive(row.try_get("inventory_version")?)?,
    ))
}

async fn fetch_character_safe_rows(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<PgRow>, PersistenceError> {
    sqlx::query(STORAGE_ROW_SELECT)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(CHARACTER_SAFE_LOCATION)
        .bind(-1_i16)
        .bind(9_i64)
        .fetch_all(connection)
        .await
        .map_err(PersistenceError::Database)
}

async fn fetch_surface_rows(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    surface: StoredSafeStorageSurface,
    after_slot: Option<u16>,
) -> Result<Vec<PgRow>, PersistenceError> {
    let cursor = after_slot.map_or(-1_i16, |slot| i16::try_from(slot).unwrap_or(i16::MAX));
    sqlx::query(STORAGE_ROW_SELECT)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(surface.location_kind())
        .bind(cursor)
        .bind(i64::try_from(MAX_PAGE_STACKS + 1).expect("page limit fits i64"))
        .fetch_all(connection)
        .await
        .map_err(PersistenceError::Database)
}

const STORAGE_ROW_SELECT: &str = "WITH bounded_slots AS (\
       SELECT DISTINCT slot_index FROM item_instances \
       WHERE namespace_id = $1 AND account_id = $2 AND location_kind = $4 \
       AND slot_index > $5 AND (\
         ($4 = 5 AND character_id = $3) OR ($4 IN (6,8) AND character_id IS NULL)\
       ) ORDER BY slot_index LIMIT $6\
     ) SELECT i.slot_index, i.item_uid, i.template_id, i.content_revision, i.item_kind, \
       i.item_level, i.rarity, i.item_version, i.security_state, i.provenance_kind, \
       i.salvage_band, i.salvage_value, \
       CASE WHEN i.overflow_expires_at IS NULL THEN NULL ELSE \
         floor(extract(epoch FROM i.overflow_expires_at) * 1000)::bigint END \
         AS overflow_expires_at_unix_millis \
     FROM item_instances i JOIN bounded_slots s ON s.slot_index = i.slot_index \
     WHERE i.namespace_id = $1 AND i.account_id = $2 AND i.location_kind = $4 AND (\
       ($4 = 5 AND i.character_id = $3) OR ($4 IN (6,8) AND i.character_id IS NULL)\
     ) ORDER BY i.slot_index, i.item_uid";

fn decode_stacks(
    rows: Vec<PgRow>,
    location_kind: i16,
    capacity: usize,
) -> Result<Vec<StoredSafeStorageStack>, PersistenceError> {
    let mut stacks: Vec<StoredSafeStorageStack> = Vec::new();
    let mut prior_uid: Option<[u8; 16]> = None;
    for row in rows {
        let slot_index = u16::try_from(row.try_get::<i16, _>("slot_index")?)
            .map_err(|_| PersistenceError::CorruptStoredSafeStorage)?;
        if usize::from(slot_index) >= capacity
            || row.try_get::<String, _>("content_revision")? != CORE_ITEM_CONTENT_REVISION
        {
            return Err(PersistenceError::CorruptStoredSafeStorage);
        }
        let item_uid = exact_id(row.try_get("item_uid")?)?;
        if prior_uid.is_some_and(|prior| {
            stacks
                .last()
                .is_some_and(|stack| stack.slot_index == slot_index)
                && prior >= item_uid
        }) {
            return Err(PersistenceError::CorruptStoredSafeStorage);
        }
        let item_kind = exact_u8(row.try_get("item_kind")?)?;
        if item_kind > 1 {
            return Err(PersistenceError::CorruptStoredSafeStorage);
        }
        let template_id: String = row.try_get("template_id")?;
        if template_id.is_empty() || template_id.len() > 96 {
            return Err(PersistenceError::CorruptStoredSafeStorage);
        }
        let item = StoredSafeStorageItem {
            item_uid,
            item_version: positive(row.try_get("item_version")?)?,
            item_level: optional_u8(row.try_get("item_level")?)?,
            rarity: optional_u8(row.try_get("rarity")?)?,
            security_state: exact_u8(row.try_get("security_state")?)?,
            provenance_kind: exact_u8(row.try_get("provenance_kind")?)?,
            salvage_band: exact_u8(row.try_get("salvage_band")?)?,
            salvage_value: u32::try_from(row.try_get::<i32, _>("salvage_value")?)
                .map_err(|_| PersistenceError::CorruptStoredSafeStorage)?,
            overflow_expires_at_unix_millis: row
                .try_get::<Option<i64>, _>("overflow_expires_at_unix_millis")?
                .map(positive)
                .transpose()?,
        };
        if item.security_state != 0
            || item.provenance_kind > 5
            || item.salvage_band > 5
            || (location_kind == OVERFLOW_LOCATION)
                != item.overflow_expires_at_unix_millis.is_some()
            || (item_kind == 0
                && (item.item_level.is_none() || item.rarity.is_none_or(|value| value > 4)))
            || (item_kind == 1
                && (item.item_level.is_some()
                    || item.rarity.is_some()
                    || item.salvage_band != 0
                    || item.salvage_value != 0))
        {
            return Err(PersistenceError::CorruptStoredSafeStorage);
        }
        if let Some(stack) = stacks
            .last_mut()
            .filter(|stack| stack.slot_index == slot_index)
        {
            if stack.template_id != template_id
                || stack.item_kind != item_kind
                || stack.items.len() >= MAX_STACK_ITEMS
            {
                return Err(PersistenceError::CorruptStoredSafeStorage);
            }
            stack.items.push(item);
        } else {
            if stacks
                .last()
                .is_some_and(|stack| stack.slot_index >= slot_index)
            {
                return Err(PersistenceError::CorruptStoredSafeStorage);
            }
            stacks.push(StoredSafeStorageStack {
                slot_index,
                template_id,
                item_kind,
                items: vec![item],
            });
        }
        prior_uid = Some(item_uid);
    }
    Ok(stacks)
}

fn positive(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredSafeStorage)
}

fn exact_u8(value: i16) -> Result<u8, PersistenceError> {
    u8::try_from(value).map_err(|_| PersistenceError::CorruptStoredSafeStorage)
}

fn optional_u8(value: Option<i16>) -> Result<Option<u8>, PersistenceError> {
    value.map(exact_u8).transpose()
}

fn exact_id(value: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    let id = value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredSafeStorage)?;
    if id == [0; 16] {
        return Err(PersistenceError::CorruptStoredSafeStorage);
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_capacity_and_location_are_exact() {
        assert_eq!(StoredSafeStorageSurface::Vault.capacity(), 160);
        assert_eq!(StoredSafeStorageSurface::Vault.location_kind(), 6);
        assert_eq!(StoredSafeStorageSurface::Overflow.capacity(), 20);
        assert_eq!(StoredSafeStorageSurface::Overflow.location_kind(), 8);
    }

    #[test]
    fn continuation_restarts_when_either_aggregate_version_changes() {
        require_expected_versions((4, 7), Some((4, 7))).unwrap();
        assert!(matches!(
            require_expected_versions((5, 7), Some((4, 7))),
            Err(PersistenceError::SafeStorageVersionMismatch)
        ));
        assert!(matches!(
            require_expected_versions((4, 8), Some((4, 7))),
            Err(PersistenceError::SafeStorageVersionMismatch)
        ));
    }
}
