use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgConnection, Row, postgres::PgRow};

use crate::{
    PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
    items::CORE_ITEM_CONTENT_REVISION,
};

const CHARACTER_SAFE_CAPACITY: u16 = 8;
const VAULT_CAPACITY: u16 = 160;
const OVERFLOW_CAPACITY: u16 = 20;
const RUN_BACKPACK_CAPACITY: u16 = 8;
const CONSUMABLE_STACK_CAPACITY: usize = 6;
const MAX_TRANSACTION_ATTEMPTS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredSafeInventoryCommandKind {
    CharacterSafeToVault,
    VaultToCharacterSafe,
    CharacterSafeToRunBackpack,
    OverflowToCharacterSafe,
}

impl StoredSafeInventoryCommandKind {
    const fn database_value(self) -> i16 {
        match self {
            Self::CharacterSafeToVault => 0,
            Self::VaultToCharacterSafe => 1,
            Self::CharacterSafeToRunBackpack => 2,
            Self::OverflowToCharacterSafe => 3,
        }
    }

    const fn touches_vault(self) -> bool {
        !matches!(self, Self::CharacterSafeToRunBackpack)
    }

    fn decode(value: i16) -> Result<Self, PersistenceError> {
        match value {
            0 => Ok(Self::CharacterSafeToVault),
            1 => Ok(Self::VaultToCharacterSafe),
            2 => Ok(Self::CharacterSafeToRunBackpack),
            3 => Ok(Self::OverflowToCharacterSafe),
            _ => Err(PersistenceError::CorruptStoredSafeInventory),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StoredSafeInventoryLocation {
    RunBackpack(u8),
    CharacterSafe(u8),
    Vault(u16),
    Overflow(u8),
}

impl StoredSafeInventoryLocation {
    fn database_values(self) -> Result<(i16, i16), PersistenceError> {
        match self {
            Self::RunBackpack(slot) => Ok((2, i16::from(slot))),
            Self::CharacterSafe(slot) => Ok((5, i16::from(slot))),
            Self::Vault(slot) => Ok((
                6,
                i16::try_from(slot).map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
            )),
            Self::Overflow(slot) => Ok((8, i16::from(slot))),
        }
    }

    fn decode(kind: i16, slot: i16) -> Result<Self, PersistenceError> {
        match kind {
            2 => u8::try_from(slot)
                .ok()
                .filter(|slot| u16::from(*slot) < RUN_BACKPACK_CAPACITY)
                .map(Self::RunBackpack),
            5 => u8::try_from(slot)
                .ok()
                .filter(|slot| u16::from(*slot) < CHARACTER_SAFE_CAPACITY)
                .map(Self::CharacterSafe),
            6 => u16::try_from(slot)
                .ok()
                .filter(|slot| *slot < VAULT_CAPACITY)
                .map(Self::Vault),
            8 => u8::try_from(slot)
                .ok()
                .filter(|slot| u16::from(*slot) < OVERFLOW_CAPACITY)
                .map(Self::Overflow),
            _ => None,
        }
        .ok_or(PersistenceError::CorruptStoredSafeInventory)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSafeInventoryItem {
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub item_kind: i16,
    pub item_version: u64,
    pub security_state: i16,
    pub location: StoredSafeInventoryLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSafeInventorySnapshot {
    pub account_version: u64,
    pub inventory_version: u64,
    pub character_safe: Vec<StoredSafeInventoryItem>,
    pub vault: Vec<StoredSafeInventoryItem>,
    pub run_backpack: Vec<StoredSafeInventoryItem>,
    pub overflow: Vec<StoredSafeInventoryItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredSafeInventoryPlacement {
    pub item_uid: [u8; 16],
    pub source: StoredSafeInventoryLocation,
    pub destination: StoredSafeInventoryLocation,
    pub expected_item_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSafeInventoryCommand {
    pub mutation_id: [u8; 16],
    pub canonical_request_hash: [u8; 32],
    pub result_hash: [u8; 32],
    pub kind: StoredSafeInventoryCommandKind,
    pub source_slot_index: u16,
    pub expected_account_version: u64,
    pub expected_inventory_version: u64,
    pub placements: Vec<StoredSafeInventoryPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSafeInventoryResult {
    pub replayed: bool,
    pub mutation_id: [u8; 16],
    pub result_hash: [u8; 32],
    pub pre_account_version: u64,
    pub post_account_version: u64,
    pub pre_inventory_version: u64,
    pub post_inventory_version: u64,
    pub placements: Vec<StoredSafeInventoryPlacement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredSafeInventoryPreflightResult {
    pub moved_item_count: usize,
    pub account_version: u64,
    pub inventory_version: u64,
}

/// Locks and projects the safe-storage state inside an already account/character-locked
/// world-flow transaction. This deliberately does not relock the account or Hall location.
pub async fn load_world_flow_safe_inventory(
    transaction: &mut crate::PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    locked_account_version: u64,
) -> Result<StoredSafeInventorySnapshot, PersistenceError> {
    ensure_no_unresolved_inventory_mutation(transaction.connection(), account_id, character_id)
        .await?;
    let inventory_version =
        lock_inventory(transaction.connection(), account_id, character_id).await?;
    let items = lock_storage_items(transaction.connection(), account_id, character_id).await?;
    decode_snapshot(locked_account_version, inventory_version, items)
}

/// Applies a complete server-planned CharacterSafe-to-Vault preflight inside the caller's
/// world-flow transaction. The durable planner independently derives and verifies every
/// destination before the first write, keeping manual transfers and entry preflight aligned.
pub async fn stage_world_flow_safe_inventory_preflight(
    transaction: &mut crate::PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    mutation_id: [u8; 16],
    snapshot: &StoredSafeInventorySnapshot,
    placements: &[StoredSafeInventoryPlacement],
) -> Result<StoredSafeInventoryPreflightResult, PersistenceError> {
    if mutation_id == [0; 16] {
        return Err(PersistenceError::CorruptStoredSafeInventory);
    }
    // Re-read the complete storage aggregate under the caller's existing transaction before
    // planning. Besides preventing a caller-supplied snapshot from becoming authority, decoding
    // this locked state enforces the exact Core item content revision before the first write.
    let current_inventory_version =
        lock_inventory(transaction.connection(), account_id, character_id).await?;
    let current_items =
        lock_storage_items(transaction.connection(), account_id, character_id).await?;
    let current_snapshot = decode_snapshot(
        snapshot.account_version,
        current_inventory_version,
        current_items,
    )?;
    if current_snapshot != *snapshot {
        return Err(PersistenceError::SafeInventoryBindingMismatch);
    }
    let expected = derive_preflight_placements(snapshot)?;
    if placements != expected {
        return Err(PersistenceError::SafeInventoryBindingMismatch);
    }
    if placements.is_empty() {
        return Ok(StoredSafeInventoryPreflightResult {
            moved_item_count: 0,
            account_version: snapshot.account_version,
            inventory_version: snapshot.inventory_version,
        });
    }
    for placement in placements {
        transition_item(
            transaction.connection(),
            account_id,
            character_id,
            mutation_id,
            placement,
        )
        .await?;
    }
    let account_version = snapshot
        .account_version
        .checked_add(1)
        .ok_or(PersistenceError::CorruptStoredSafeInventory)?;
    let inventory_version = snapshot
        .inventory_version
        .checked_add(1)
        .ok_or(PersistenceError::CorruptStoredSafeInventory)?;
    update_account_version(transaction.connection(), account_id, account_version).await?;
    update_inventory_version(
        transaction.connection(),
        account_id,
        character_id,
        inventory_version,
    )
    .await?;
    Ok(StoredSafeInventoryPreflightResult {
        moved_item_count: placements.len(),
        account_version,
        inventory_version,
    })
}

impl PostgresPersistence {
    pub async fn load_safe_inventory_replay(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        mutation_id: [u8; 16],
        canonical_request_hash: [u8; 32],
    ) -> Result<Option<StoredSafeInventoryResult>, PersistenceError> {
        if mutation_id == [0; 16] || canonical_request_hash == [0; 32] {
            return Err(PersistenceError::CorruptStoredSafeInventory);
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .load_safe_inventory_replay_once(
                    account_id,
                    character_id,
                    mutation_id,
                    canonical_request_hash,
                )
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && crate::is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded safe-inventory replay read always returns")
    }

    async fn load_safe_inventory_replay_once(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        mutation_id: [u8; 16],
        canonical_request_hash: [u8; 32],
    ) -> Result<Option<StoredSafeInventoryResult>, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        lock_account(transaction.connection(), account_id).await?;
        let replay = load_replay(
            transaction.connection(),
            account_id,
            character_id,
            mutation_id,
            canonical_request_hash,
        )
        .await?;
        transaction.rollback().await?;
        Ok(replay)
    }

    pub async fn load_safe_inventory_snapshot(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<StoredSafeInventorySnapshot, PersistenceError> {
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .load_safe_inventory_snapshot_once(account_id, character_id)
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && crate::is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded safe-inventory snapshot read always returns")
    }

    async fn load_safe_inventory_snapshot_once(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<StoredSafeInventorySnapshot, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let (account_version, selected_character_id) =
            lock_account(transaction.connection(), account_id).await?;
        if selected_character_id != Some(character_id) {
            return Err(PersistenceError::SafeInventoryHallBindingMismatch);
        }
        lock_living_hall_character(transaction.connection(), account_id, character_id).await?;
        ensure_no_unresolved_inventory_mutation(transaction.connection(), account_id, character_id)
            .await?;
        let inventory_version =
            lock_inventory(transaction.connection(), account_id, character_id).await?;
        let items = lock_storage_items(transaction.connection(), account_id, character_id).await?;
        let snapshot = decode_snapshot(account_version, inventory_version, items)?;
        transaction.rollback().await?;
        Ok(snapshot)
    }

    pub async fn commit_safe_inventory_transfer(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        command: &StoredSafeInventoryCommand,
    ) -> Result<StoredSafeInventoryResult, PersistenceError> {
        validate_command(command)?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .commit_safe_inventory_transfer_once(account_id, character_id, command)
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && crate::is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded safe-inventory transaction always returns")
    }

    async fn commit_safe_inventory_transfer_once(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        command: &StoredSafeInventoryCommand,
    ) -> Result<StoredSafeInventoryResult, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;

        // Account is the outer lock for both Vault and selected-character storage. Replay is
        // deliberately checked immediately afterward, before any current-state validation.
        let (account_version, selected_character_id) =
            lock_account(transaction.connection(), account_id).await?;
        if let Some(result) = load_replay(
            transaction.connection(),
            account_id,
            character_id,
            command.mutation_id,
            command.canonical_request_hash,
        )
        .await?
        {
            transaction.rollback().await?;
            return Ok(result);
        }

        if account_version != command.expected_account_version {
            return Err(PersistenceError::SafeInventoryVersionMismatch);
        }
        if selected_character_id != Some(character_id) {
            return Err(PersistenceError::SafeInventoryHallBindingMismatch);
        }
        lock_living_hall_character(transaction.connection(), account_id, character_id).await?;
        ensure_no_unresolved_inventory_mutation(transaction.connection(), account_id, character_id)
            .await?;
        let inventory_version =
            lock_inventory(transaction.connection(), account_id, character_id).await?;
        if inventory_version != command.expected_inventory_version {
            return Err(PersistenceError::SafeInventoryVersionMismatch);
        }

        // This single ordered query locks selected-character storage and account Vault units in
        // unsigned UID byte order. The complete plan is verified before the first write.
        let items = lock_storage_items(transaction.connection(), account_id, character_id).await?;
        let snapshot = decode_snapshot(account_version, inventory_version, items)?;
        validate_placements(&snapshot, command)?;

        for placement in &command.placements {
            transition_item(
                transaction.connection(),
                account_id,
                character_id,
                command.mutation_id,
                placement,
            )
            .await?;
        }

        let post_account_version = account_version
            .checked_add(u64::from(command.kind.touches_vault()))
            .ok_or(PersistenceError::CorruptStoredSafeInventory)?;
        let post_inventory_version = inventory_version
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredSafeInventory)?;
        if command.kind.touches_vault() {
            update_account_version(transaction.connection(), account_id, post_account_version)
                .await?;
        }
        update_inventory_version(
            transaction.connection(),
            account_id,
            character_id,
            post_inventory_version,
        )
        .await?;
        insert_receipt(
            transaction.connection(),
            account_id,
            character_id,
            command,
            account_version,
            post_account_version,
            inventory_version,
            post_inventory_version,
        )
        .await?;
        transaction.commit().await?;

        Ok(result_from_command(
            command,
            false,
            account_version,
            post_account_version,
            inventory_version,
            post_inventory_version,
        ))
    }
}

async fn lock_account(
    connection: &mut PgConnection,
    account_id: [u8; 16],
) -> Result<(u64, Option<[u8; 16]>), PersistenceError> {
    let row = sqlx::query(
        "SELECT state_version, selected_character_id FROM accounts WHERE namespace_id = $1 \
         AND account_id = $2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::SafeInventoryAccountNotFound)?;
    Ok((
        positive_u64(row.try_get("state_version")?)?,
        optional_bytes(row.try_get("selected_character_id")?)?,
    ))
}

async fn lock_living_hall_character(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<(), PersistenceError> {
    let accepted = sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM characters c JOIN character_world_locations w ON \
         w.namespace_id = c.namespace_id AND w.account_id = c.account_id \
         AND w.character_id = c.character_id WHERE c.namespace_id = $1 AND c.account_id = $2 \
         AND c.character_id = $3 AND c.life_state = 0 AND c.security_state = 0 \
         AND w.location_kind = 1 AND w.location_content_id = 'hub.lantern_halls_01' \
         FOR UPDATE OF c, w",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    if accepted.is_none() {
        return Err(PersistenceError::SafeInventoryHallBindingMismatch);
    }
    Ok(())
}

async fn ensure_no_unresolved_inventory_mutation(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<(), PersistenceError> {
    let unresolved = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM reward_requests WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 AND request_state = 0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_one(connection)
    .await
    .map_err(PersistenceError::Database)?;
    if unresolved {
        return Err(PersistenceError::SafeInventoryUnresolvedMutation);
    }
    Ok(())
}

async fn lock_inventory(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<u64, PersistenceError> {
    let value = sqlx::query_scalar::<_, i64>(
        "SELECT inventory_version FROM character_inventories WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::SafeInventoryHallBindingMismatch)?;
    positive_u64(value)
}

async fn lock_storage_items(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<PgRow>, PersistenceError> {
    sqlx::query(
        "SELECT item_uid, template_id, content_revision, item_kind, item_version, security_state, \
         location_kind, slot_index, character_id FROM item_instances WHERE namespace_id = $1 AND account_id = $2 \
         AND ((character_id = $3 AND location_kind IN (2, 5)) OR \
         (character_id IS NULL AND location_kind IN (6, 8))) ORDER BY item_uid FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await
    .map_err(PersistenceError::Database)
}

fn decode_snapshot(
    account_version: u64,
    inventory_version: u64,
    rows: Vec<PgRow>,
) -> Result<StoredSafeInventorySnapshot, PersistenceError> {
    if account_version == 0 || inventory_version == 0 {
        return Err(PersistenceError::CorruptStoredSafeInventory);
    }
    let mut snapshot = StoredSafeInventorySnapshot {
        account_version,
        inventory_version,
        character_safe: Vec::new(),
        vault: Vec::new(),
        run_backpack: Vec::new(),
        overflow: Vec::new(),
    };
    let mut identities = BTreeSet::new();
    for row in rows {
        let item_uid = fixed_bytes(row.try_get("item_uid")?)?;
        if item_uid == [0; 16] || !identities.insert(item_uid) {
            return Err(PersistenceError::CorruptStoredSafeInventory);
        }
        let content_revision: String = row.try_get("content_revision")?;
        require_core_item_content_revision(&content_revision)?;
        let location = StoredSafeInventoryLocation::decode(
            row.try_get("location_kind")?,
            row.try_get("slot_index")?,
        )?;
        let character_custody = row.try_get::<Option<Vec<u8>>, _>("character_id")?;
        let security_state = row.try_get("security_state")?;
        if matches!(
            location,
            StoredSafeInventoryLocation::Vault(_) | StoredSafeInventoryLocation::Overflow(_)
        ) != character_custody.is_none()
            || (matches!(location, StoredSafeInventoryLocation::RunBackpack(_))
                && security_state != 2)
            || (!matches!(location, StoredSafeInventoryLocation::RunBackpack(_))
                && security_state != 0)
        {
            return Err(PersistenceError::CorruptStoredSafeInventory);
        }
        let item = StoredSafeInventoryItem {
            item_uid,
            template_id: row.try_get("template_id")?,
            item_kind: row.try_get("item_kind")?,
            item_version: positive_u64(row.try_get("item_version")?)?,
            security_state,
            location,
        };
        match location {
            StoredSafeInventoryLocation::RunBackpack(_) => snapshot.run_backpack.push(item),
            StoredSafeInventoryLocation::CharacterSafe(_) => snapshot.character_safe.push(item),
            StoredSafeInventoryLocation::Vault(_) => snapshot.vault.push(item),
            StoredSafeInventoryLocation::Overflow(_) => snapshot.overflow.push(item),
        }
    }
    validate_stored_slots(&snapshot)?;
    Ok(snapshot)
}

fn require_core_item_content_revision(content_revision: &str) -> Result<(), PersistenceError> {
    if content_revision != CORE_ITEM_CONTENT_REVISION {
        return Err(PersistenceError::CorruptStoredSafeInventory);
    }
    Ok(())
}

fn validate_stored_slots(snapshot: &StoredSafeInventorySnapshot) -> Result<(), PersistenceError> {
    let mut slots: BTreeMap<StoredSafeInventoryLocation, Vec<&StoredSafeInventoryItem>> =
        BTreeMap::new();
    for item in snapshot
        .character_safe
        .iter()
        .chain(&snapshot.vault)
        .chain(&snapshot.run_backpack)
        .chain(&snapshot.overflow)
    {
        if item.template_id.is_empty() || !matches!(item.item_kind, 0 | 1) {
            return Err(PersistenceError::CorruptStoredSafeInventory);
        }
        slots.entry(item.location).or_default().push(item);
    }
    for items in slots.values() {
        if items[0].item_kind == 0 {
            if items.len() != 1 {
                return Err(PersistenceError::CorruptStoredSafeInventory);
            }
        } else if items.len() > CONSUMABLE_STACK_CAPACITY
            || items
                .iter()
                .any(|item| item.item_kind != 1 || item.template_id != items[0].template_id)
            || !items
                .windows(2)
                .all(|pair| pair[0].item_uid < pair[1].item_uid)
        {
            return Err(PersistenceError::CorruptStoredSafeInventory);
        }
    }
    Ok(())
}

fn validate_command(command: &StoredSafeInventoryCommand) -> Result<(), PersistenceError> {
    let source_valid = match command.kind {
        StoredSafeInventoryCommandKind::CharacterSafeToVault
        | StoredSafeInventoryCommandKind::CharacterSafeToRunBackpack => {
            command.source_slot_index < CHARACTER_SAFE_CAPACITY
        }
        StoredSafeInventoryCommandKind::VaultToCharacterSafe => {
            command.source_slot_index < VAULT_CAPACITY
        }
        StoredSafeInventoryCommandKind::OverflowToCharacterSafe => {
            command.source_slot_index < OVERFLOW_CAPACITY
        }
    };
    if command.mutation_id == [0; 16]
        || command.canonical_request_hash == [0; 32]
        || command.result_hash == [0; 32]
        || command.expected_account_version == 0
        || command.expected_inventory_version == 0
        || !source_valid
        || !(1..=CONSUMABLE_STACK_CAPACITY).contains(&command.placements.len())
    {
        return Err(PersistenceError::CorruptStoredSafeInventory);
    }
    let mut identities = BTreeSet::new();
    for placement in &command.placements {
        if placement.item_uid == [0; 16]
            || placement.expected_item_version == 0
            || !identities.insert(placement.item_uid)
            || !location_in_bounds(placement.source)
            || !location_in_bounds(placement.destination)
        {
            return Err(PersistenceError::CorruptStoredSafeInventory);
        }
    }
    Ok(())
}

const fn location_in_bounds(location: StoredSafeInventoryLocation) -> bool {
    match location {
        StoredSafeInventoryLocation::RunBackpack(slot) => (slot as u16) < RUN_BACKPACK_CAPACITY,
        StoredSafeInventoryLocation::CharacterSafe(slot) => (slot as u16) < CHARACTER_SAFE_CAPACITY,
        StoredSafeInventoryLocation::Vault(slot) => slot < VAULT_CAPACITY,
        StoredSafeInventoryLocation::Overflow(slot) => (slot as u16) < OVERFLOW_CAPACITY,
    }
}

fn validate_placements(
    snapshot: &StoredSafeInventorySnapshot,
    command: &StoredSafeInventoryCommand,
) -> Result<(), PersistenceError> {
    if snapshot.account_version != command.expected_account_version
        || snapshot.inventory_version != command.expected_inventory_version
    {
        return Err(PersistenceError::SafeInventoryVersionMismatch);
    }
    let source = match command.kind {
        StoredSafeInventoryCommandKind::CharacterSafeToVault
        | StoredSafeInventoryCommandKind::CharacterSafeToRunBackpack => {
            StoredSafeInventoryLocation::CharacterSafe(
                u8::try_from(command.source_slot_index)
                    .map_err(|_| PersistenceError::SafeInventoryBindingMismatch)?,
            )
        }
        StoredSafeInventoryCommandKind::VaultToCharacterSafe => {
            StoredSafeInventoryLocation::Vault(command.source_slot_index)
        }
        StoredSafeInventoryCommandKind::OverflowToCharacterSafe => {
            StoredSafeInventoryLocation::Overflow(
                u8::try_from(command.source_slot_index)
                    .map_err(|_| PersistenceError::SafeInventoryBindingMismatch)?,
            )
        }
    };
    let destination_kind = match command.kind {
        StoredSafeInventoryCommandKind::CharacterSafeToVault => 6,
        StoredSafeInventoryCommandKind::CharacterSafeToRunBackpack => 2,
        StoredSafeInventoryCommandKind::VaultToCharacterSafe
        | StoredSafeInventoryCommandKind::OverflowToCharacterSafe => 5,
    };
    let mut source_items = items_at(snapshot, source);
    if source_items.is_empty() {
        return Err(PersistenceError::SafeInventoryBindingMismatch);
    }
    source_items.sort_by_key(|item| item.item_uid);
    let expected = derive_placements(snapshot, &source_items, source, destination_kind)?;
    if expected.len() != command.placements.len() {
        return Err(PersistenceError::SafeInventoryBindingMismatch);
    }
    for (actual, (item, destination)) in command.placements.iter().zip(expected) {
        if actual.item_uid != item.item_uid
            || actual.source != source
            || actual.destination != destination
            || actual.expected_item_version != item.item_version
        {
            return Err(PersistenceError::SafeInventoryBindingMismatch);
        }
    }
    Ok(())
}

fn derive_placements<'a>(
    snapshot: &'a StoredSafeInventorySnapshot,
    source_items: &[&'a StoredSafeInventoryItem],
    source: StoredSafeInventoryLocation,
    destination_kind: i16,
) -> Result<Vec<(&'a StoredSafeInventoryItem, StoredSafeInventoryLocation)>, PersistenceError> {
    let capacity = match destination_kind {
        2 => RUN_BACKPACK_CAPACITY,
        5 => CHARACTER_SAFE_CAPACITY,
        6 => VAULT_CAPACITY,
        _ => return Err(PersistenceError::CorruptStoredSafeInventory),
    };
    let destination_items = match destination_kind {
        2 => &snapshot.run_backpack,
        5 => &snapshot.character_safe,
        6 => &snapshot.vault,
        _ => unreachable!(),
    };
    let mut occupied: BTreeMap<u16, Vec<&StoredSafeInventoryItem>> = BTreeMap::new();
    for item in destination_items {
        occupied
            .entry(location_slot(item.location))
            .or_default()
            .push(item);
    }
    let mut result = Vec::with_capacity(source_items.len());
    if source_items[0].item_kind == 0 {
        let slot = (0..capacity)
            .find(|slot| !occupied.contains_key(slot))
            .ok_or(PersistenceError::SafeInventoryStorageFull)?;
        result.push((source_items[0], make_location(destination_kind, slot)?));
        return Ok(result);
    }

    let template = &source_items[0].template_id;
    let mut remaining = source_items.iter().copied().peekable();
    for slot in 0..capacity {
        let Some(items) = occupied.get_mut(&slot) else {
            continue;
        };
        if items[0].item_kind != 1 || items[0].template_id != *template {
            continue;
        }
        while items.len() < CONSUMABLE_STACK_CAPACITY {
            let Some(item) = remaining.next() else { break };
            items.push(item);
            result.push((item, make_location(destination_kind, slot)?));
        }
        if remaining.peek().is_none() {
            return Ok(result);
        }
    }
    while remaining.peek().is_some() {
        let slot = (0..capacity)
            .find(|slot| !occupied.contains_key(slot))
            .ok_or(PersistenceError::SafeInventoryStorageFull)?;
        let mut stack = Vec::new();
        while stack.len() < CONSUMABLE_STACK_CAPACITY {
            let Some(item) = remaining.next() else { break };
            stack.push(item);
            result.push((item, make_location(destination_kind, slot)?));
        }
        occupied.insert(slot, stack);
    }
    if result.iter().any(|(_, destination)| *destination == source) {
        return Err(PersistenceError::SafeInventoryBindingMismatch);
    }
    Ok(result)
}

fn derive_preflight_placements(
    snapshot: &StoredSafeInventorySnapshot,
) -> Result<Vec<StoredSafeInventoryPlacement>, PersistenceError> {
    let mut virtual_snapshot = snapshot.clone();
    let mut placements = Vec::new();
    for source_slot in 0..CHARACTER_SAFE_CAPACITY {
        let source = StoredSafeInventoryLocation::CharacterSafe(
            u8::try_from(source_slot).map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
        );
        let (derived, moved_items) = {
            let mut source_items = items_at(&virtual_snapshot, source);
            if source_items.is_empty() {
                continue;
            }
            source_items.sort_by_key(|item| item.item_uid);
            let derived = derive_placements(&virtual_snapshot, &source_items, source, 6)?
                .into_iter()
                .map(|(item, destination)| StoredSafeInventoryPlacement {
                    item_uid: item.item_uid,
                    source,
                    destination,
                    expected_item_version: item.item_version,
                })
                .collect::<Vec<_>>();
            let moved_items = source_items.into_iter().cloned().collect::<Vec<_>>();
            (derived, moved_items)
        };
        virtual_snapshot
            .character_safe
            .retain(|item| item.location != source);
        for (mut item, placement) in moved_items.into_iter().zip(&derived) {
            item.location = placement.destination;
            virtual_snapshot.vault.push(item);
        }
        placements.extend(derived);
    }
    Ok(placements)
}

fn items_at(
    snapshot: &StoredSafeInventorySnapshot,
    location: StoredSafeInventoryLocation,
) -> Vec<&StoredSafeInventoryItem> {
    snapshot
        .character_safe
        .iter()
        .chain(&snapshot.vault)
        .chain(&snapshot.run_backpack)
        .chain(&snapshot.overflow)
        .filter(|item| item.location == location)
        .collect()
}

const fn location_slot(location: StoredSafeInventoryLocation) -> u16 {
    match location {
        StoredSafeInventoryLocation::RunBackpack(slot)
        | StoredSafeInventoryLocation::CharacterSafe(slot)
        | StoredSafeInventoryLocation::Overflow(slot) => slot as u16,
        StoredSafeInventoryLocation::Vault(slot) => slot,
    }
}

fn make_location(kind: i16, slot: u16) -> Result<StoredSafeInventoryLocation, PersistenceError> {
    StoredSafeInventoryLocation::decode(
        kind,
        i16::try_from(slot).map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
    )
}

async fn transition_item(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    mutation_id: [u8; 16],
    placement: &StoredSafeInventoryPlacement,
) -> Result<(), PersistenceError> {
    let (source_kind, source_slot) = placement.source.database_values()?;
    let (destination_kind, destination_slot) = placement.destination.database_values()?;
    let destination_character = if destination_kind == 6 {
        None
    } else {
        Some(character_id.as_slice())
    };
    let post_security = if destination_kind == 2 { 2_i16 } else { 0_i16 };
    let post_version = placement
        .expected_item_version
        .checked_add(1)
        .ok_or(PersistenceError::CorruptStoredSafeInventory)?;
    let changed = sqlx::query(
        "UPDATE item_instances SET character_id = $1, item_version = $2, security_state = $3, \
         location_kind = $4, slot_index = $5, instance_id = NULL, pickup_id = NULL, \
         expires_at_tick = NULL, destruction_reason = NULL, overflow_expires_at = NULL, \
         updated_at = transaction_timestamp() \
         WHERE namespace_id = $6 AND account_id = $7 AND item_uid = $8 AND item_version = $9 \
         AND location_kind = $10 AND slot_index = $11",
    )
    .bind(destination_character)
    .bind(i64::try_from(post_version).map_err(|_| PersistenceError::CorruptStoredSafeInventory)?)
    .bind(post_security)
    .bind(destination_kind)
    .bind(destination_slot)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(placement.item_uid.as_slice())
    .bind(
        i64::try_from(placement.expected_item_version)
            .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
    )
    .bind(source_kind)
    .bind(source_slot)
    .execute(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?
    .rows_affected();
    if changed != 1 {
        return Err(PersistenceError::SafeInventoryVersionMismatch);
    }
    let ledger_event_id = transition_event_id(mutation_id, placement.item_uid);
    let pre_security = if source_kind == 2 { 2_i16 } else { 0_i16 };
    sqlx::query(
        "INSERT INTO item_ledger_events (namespace_id, ledger_event_id, item_uid, account_id, \
         character_id, mutation_id, event_kind, source_kind, pre_item_version, post_item_version, \
         pre_security_state, post_security_state, pre_location_kind, post_location_kind) \
         VALUES ($1,$2,$3,$4,$5,$6,1,2,$7,$8,$9,$10,$11,$12)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ledger_event_id.as_slice())
    .bind(placement.item_uid.as_slice())
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(mutation_id.as_slice())
    .bind(
        i64::try_from(placement.expected_item_version)
            .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
    )
    .bind(i64::try_from(post_version).map_err(|_| PersistenceError::CorruptStoredSafeInventory)?)
    .bind(pre_security)
    .bind(post_security)
    .bind(source_kind)
    .bind(destination_kind)
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

fn transition_event_id(mutation_id: [u8; 16], item_uid: [u8; 16]) -> [u8; 16] {
    let mut material = [0_u8; 32];
    material[..16].copy_from_slice(&mutation_id);
    material[16..].copy_from_slice(&item_uid);
    let hash = blake3::derive_key("gravebound.safe-inventory-ledger.v1", &material);
    let mut value = [0; 16];
    value.copy_from_slice(&hash[..16]);
    value
}

async fn update_account_version(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    post_version: u64,
) -> Result<(), PersistenceError> {
    let changed = sqlx::query(
        "UPDATE accounts SET state_version = $1, updated_at = transaction_timestamp() \
         WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(i64::try_from(post_version).map_err(|_| PersistenceError::CorruptStoredSafeInventory)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?
    .rows_affected();
    if changed != 1 {
        return Err(PersistenceError::SafeInventoryVersionMismatch);
    }
    Ok(())
}

async fn update_inventory_version(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    post_version: u64,
) -> Result<(), PersistenceError> {
    let changed = sqlx::query(
        "UPDATE character_inventories SET inventory_version = $1, \
         updated_at = transaction_timestamp() WHERE namespace_id = $2 AND account_id = $3 \
         AND character_id = $4",
    )
    .bind(i64::try_from(post_version).map_err(|_| PersistenceError::CorruptStoredSafeInventory)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?
    .rows_affected();
    if changed != 1 {
        return Err(PersistenceError::SafeInventoryVersionMismatch);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_receipt(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    command: &StoredSafeInventoryCommand,
    pre_account_version: u64,
    post_account_version: u64,
    pre_inventory_version: u64,
    post_inventory_version: u64,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO safe_inventory_mutations (namespace_id, account_id, mutation_id, \
         character_id, command_kind, source_slot_index, canonical_request_hash, \
         pre_account_version, post_account_version, pre_inventory_version, \
         post_inventory_version, placement_count, result_code, result_hash) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,1,$13)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(command.mutation_id.as_slice())
    .bind(character_id.as_slice())
    .bind(command.kind.database_value())
    .bind(
        i16::try_from(command.source_slot_index)
            .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
    )
    .bind(command.canonical_request_hash.as_slice())
    .bind(
        i64::try_from(pre_account_version)
            .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
    )
    .bind(
        i64::try_from(post_account_version)
            .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
    )
    .bind(
        i64::try_from(pre_inventory_version)
            .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
    )
    .bind(
        i64::try_from(post_inventory_version)
            .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
    )
    .bind(
        i16::try_from(command.placements.len())
            .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
    )
    .bind(command.result_hash.as_slice())
    .execute(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    for (ordinal, placement) in command.placements.iter().enumerate() {
        let (destination_kind, destination_slot_index) = placement.destination.database_values()?;
        sqlx::query(
            "INSERT INTO safe_inventory_placements (namespace_id, account_id, mutation_id, \
             placement_ordinal, item_uid, destination_kind, destination_slot_index, \
             pre_item_version, post_item_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(command.mutation_id.as_slice())
        .bind(i16::try_from(ordinal).map_err(|_| PersistenceError::CorruptStoredSafeInventory)?)
        .bind(placement.item_uid.as_slice())
        .bind(destination_kind)
        .bind(destination_slot_index)
        .bind(
            i64::try_from(placement.expected_item_version)
                .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
        )
        .bind(
            i64::try_from(
                placement
                    .expected_item_version
                    .checked_add(1)
                    .ok_or(PersistenceError::CorruptStoredSafeInventory)?,
            )
            .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
        )
        .execute(&mut *connection)
        .await
        .map_err(PersistenceError::Database)?;
    }
    Ok(())
}

async fn load_replay(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    mutation_id: [u8; 16],
    request_hash: [u8; 32],
) -> Result<Option<StoredSafeInventoryResult>, PersistenceError> {
    let row = sqlx::query(
        "SELECT character_id, canonical_request_hash, result_hash, command_kind, source_slot_index, \
         pre_account_version, post_account_version, pre_inventory_version, \
         post_inventory_version, placement_count, result_code FROM safe_inventory_mutations \
         WHERE namespace_id = $1 AND account_id = $2 AND mutation_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(mutation_id.as_slice())
    .fetch_optional(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    let Some(row) = row else { return Ok(None) };
    if fixed_bytes::<32>(row.try_get("canonical_request_hash")?)? != request_hash {
        return Err(PersistenceError::SafeInventoryIdempotencyConflict);
    }
    if fixed_bytes::<16>(row.try_get("character_id")?)? != character_id {
        return Err(PersistenceError::SafeInventoryIdempotencyConflict);
    }
    if row.try_get::<i16, _>("result_code")? != 1 {
        return Err(PersistenceError::CorruptStoredSafeInventory);
    }
    let kind = StoredSafeInventoryCommandKind::decode(row.try_get("command_kind")?)?;
    let source_slot = u16::try_from(row.try_get::<i16, _>("source_slot_index")?)
        .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?;
    let source = match kind {
        StoredSafeInventoryCommandKind::CharacterSafeToVault
        | StoredSafeInventoryCommandKind::CharacterSafeToRunBackpack => {
            StoredSafeInventoryLocation::CharacterSafe(
                u8::try_from(source_slot)
                    .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
            )
        }
        StoredSafeInventoryCommandKind::VaultToCharacterSafe => {
            StoredSafeInventoryLocation::Vault(source_slot)
        }
        StoredSafeInventoryCommandKind::OverflowToCharacterSafe => {
            StoredSafeInventoryLocation::Overflow(
                u8::try_from(source_slot)
                    .map_err(|_| PersistenceError::CorruptStoredSafeInventory)?,
            )
        }
    };
    let placement_rows = sqlx::query(
        "SELECT item_uid, destination_kind, destination_slot_index, pre_item_version, \
         post_item_version FROM safe_inventory_placements WHERE namespace_id = $1 \
         AND account_id = $2 AND mutation_id = $3 ORDER BY placement_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(mutation_id.as_slice())
    .fetch_all(connection)
    .await
    .map_err(PersistenceError::Database)?;
    if usize::try_from(row.try_get::<i16, _>("placement_count")?).ok() != Some(placement_rows.len())
    {
        return Err(PersistenceError::CorruptStoredSafeInventory);
    }
    let mut placements = Vec::with_capacity(placement_rows.len());
    for placement in placement_rows {
        let pre = positive_u64(placement.try_get("pre_item_version")?)?;
        if positive_u64(placement.try_get("post_item_version")?)?
            != pre
                .checked_add(1)
                .ok_or(PersistenceError::CorruptStoredSafeInventory)?
        {
            return Err(PersistenceError::CorruptStoredSafeInventory);
        }
        placements.push(StoredSafeInventoryPlacement {
            item_uid: fixed_bytes(placement.try_get("item_uid")?)?,
            source,
            destination: StoredSafeInventoryLocation::decode(
                placement.try_get("destination_kind")?,
                placement.try_get("destination_slot_index")?,
            )?,
            expected_item_version: pre,
        });
    }
    let result = StoredSafeInventoryResult {
        replayed: true,
        mutation_id,
        result_hash: fixed_bytes(row.try_get("result_hash")?)?,
        pre_account_version: positive_u64(row.try_get("pre_account_version")?)?,
        post_account_version: positive_u64(row.try_get("post_account_version")?)?,
        pre_inventory_version: positive_u64(row.try_get("pre_inventory_version")?)?,
        post_inventory_version: positive_u64(row.try_get("post_inventory_version")?)?,
        placements,
    };
    validate_result(&result, kind)?;
    Ok(Some(result))
}

fn validate_result(
    result: &StoredSafeInventoryResult,
    kind: StoredSafeInventoryCommandKind,
) -> Result<(), PersistenceError> {
    if result.mutation_id == [0; 16]
        || result.result_hash == [0; 32]
        || !(1..=CONSUMABLE_STACK_CAPACITY).contains(&result.placements.len())
        || result.post_inventory_version != result.pre_inventory_version + 1
        || result.post_account_version
            != result.pre_account_version + u64::from(kind.touches_vault())
    {
        return Err(PersistenceError::CorruptStoredSafeInventory);
    }
    Ok(())
}

fn result_from_command(
    command: &StoredSafeInventoryCommand,
    replayed: bool,
    pre_account_version: u64,
    post_account_version: u64,
    pre_inventory_version: u64,
    post_inventory_version: u64,
) -> StoredSafeInventoryResult {
    StoredSafeInventoryResult {
        replayed,
        mutation_id: command.mutation_id,
        result_hash: command.result_hash,
        pre_account_version,
        post_account_version,
        pre_inventory_version,
        post_inventory_version,
        placements: command.placements.clone(),
    }
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredSafeInventory)
}

fn fixed_bytes<const N: usize>(value: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredSafeInventory)
}

fn optional_bytes(value: Option<Vec<u8>>) -> Result<Option<[u8; 16]>, PersistenceError> {
    value.map(fixed_bytes).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        uid: u8,
        template: &str,
        kind: i16,
        location: StoredSafeInventoryLocation,
    ) -> StoredSafeInventoryItem {
        StoredSafeInventoryItem {
            item_uid: [uid; 16],
            template_id: template.to_owned(),
            item_kind: kind,
            item_version: u64::from(uid),
            security_state: if matches!(location, StoredSafeInventoryLocation::RunBackpack(_)) {
                2
            } else {
                0
            },
            location,
        }
    }

    fn snapshot(items: Vec<StoredSafeInventoryItem>) -> StoredSafeInventorySnapshot {
        StoredSafeInventorySnapshot {
            account_version: 4,
            inventory_version: 7,
            character_safe: items
                .iter()
                .filter(|item| {
                    matches!(item.location, StoredSafeInventoryLocation::CharacterSafe(_))
                })
                .cloned()
                .collect(),
            vault: items
                .iter()
                .filter(|item| matches!(item.location, StoredSafeInventoryLocation::Vault(_)))
                .cloned()
                .collect(),
            overflow: items
                .iter()
                .filter(|item| matches!(item.location, StoredSafeInventoryLocation::Overflow(_)))
                .cloned()
                .collect(),
            run_backpack: items
                .into_iter()
                .filter(|item| matches!(item.location, StoredSafeInventoryLocation::RunBackpack(_)))
                .collect(),
        }
    }

    fn command(
        kind: StoredSafeInventoryCommandKind,
        source_slot_index: u16,
        placements: Vec<StoredSafeInventoryPlacement>,
    ) -> StoredSafeInventoryCommand {
        StoredSafeInventoryCommand {
            mutation_id: [1; 16],
            canonical_request_hash: [2; 32],
            result_hash: [3; 32],
            kind,
            source_slot_index,
            expected_account_version: 4,
            expected_inventory_version: 7,
            placements,
        }
    }

    #[test]
    fn verifies_lowest_equipment_destination_and_item_version() {
        let snapshot = snapshot(vec![
            item(
                1,
                "equipment.one",
                0,
                StoredSafeInventoryLocation::CharacterSafe(2),
            ),
            item(2, "equipment.two", 0, StoredSafeInventoryLocation::Vault(0)),
            item(
                3,
                "equipment.three",
                0,
                StoredSafeInventoryLocation::Vault(2),
            ),
        ]);
        let valid = command(
            StoredSafeInventoryCommandKind::CharacterSafeToVault,
            2,
            vec![StoredSafeInventoryPlacement {
                item_uid: [1; 16],
                source: StoredSafeInventoryLocation::CharacterSafe(2),
                destination: StoredSafeInventoryLocation::Vault(1),
                expected_item_version: 1,
            }],
        );
        assert!(validate_placements(&snapshot, &valid).is_ok());
        let mut stale = valid.clone();
        stale.placements[0].expected_item_version = 2;
        assert!(matches!(
            validate_placements(&snapshot, &stale),
            Err(PersistenceError::SafeInventoryBindingMismatch)
        ));
    }

    #[test]
    fn verifies_consumable_merge_split_and_unsigned_uid_order() {
        let snapshot = snapshot(vec![
            item(
                4,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::CharacterSafe(0),
            ),
            item(
                5,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::CharacterSafe(0),
            ),
            item(
                6,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::CharacterSafe(0),
            ),
            item(
                1,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::Vault(1),
            ),
            item(
                2,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::Vault(1),
            ),
            item(
                3,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::Vault(1),
            ),
            item(
                7,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::Vault(1),
            ),
            item(
                8,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::Vault(1),
            ),
            item(
                9,
                "consumable.other",
                1,
                StoredSafeInventoryLocation::Vault(0),
            ),
        ]);
        let placements = [
            (4, StoredSafeInventoryLocation::Vault(1)),
            (5, StoredSafeInventoryLocation::Vault(2)),
            (6, StoredSafeInventoryLocation::Vault(2)),
        ]
        .into_iter()
        .map(|(uid, destination)| StoredSafeInventoryPlacement {
            item_uid: [uid; 16],
            source: StoredSafeInventoryLocation::CharacterSafe(0),
            destination,
            expected_item_version: u64::from(uid),
        })
        .collect();
        assert!(
            validate_placements(
                &snapshot,
                &command(
                    StoredSafeInventoryCommandKind::CharacterSafeToVault,
                    0,
                    placements
                )
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_partial_source_wrong_destination_and_stale_aggregate() {
        let snapshot = snapshot(vec![
            item(
                1,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::CharacterSafe(0),
            ),
            item(
                2,
                "consumable.red",
                1,
                StoredSafeInventoryLocation::CharacterSafe(0),
            ),
        ]);
        let one = StoredSafeInventoryPlacement {
            item_uid: [1; 16],
            source: StoredSafeInventoryLocation::CharacterSafe(0),
            destination: StoredSafeInventoryLocation::Vault(1),
            expected_item_version: 1,
        };
        let partial = command(
            StoredSafeInventoryCommandKind::CharacterSafeToVault,
            0,
            vec![one],
        );
        assert!(matches!(
            validate_placements(&snapshot, &partial),
            Err(PersistenceError::SafeInventoryBindingMismatch)
        ));
        let mut stale = partial;
        stale.expected_inventory_version = 8;
        assert!(matches!(
            validate_placements(&snapshot, &stale),
            Err(PersistenceError::SafeInventoryVersionMismatch)
        ));
    }

    #[test]
    fn full_destinations_reject_before_any_write() {
        let mut items = vec![item(
            200,
            "equipment.source",
            0,
            StoredSafeInventoryLocation::CharacterSafe(0),
        )];
        for slot in 0..VAULT_CAPACITY {
            items.push(StoredSafeInventoryItem {
                item_uid: (u128::from(slot) + 1).to_be_bytes(),
                template_id: "equipment.full".to_owned(),
                item_kind: 0,
                item_version: 1,
                security_state: 0,
                location: StoredSafeInventoryLocation::Vault(slot),
            });
        }
        let snapshot = snapshot(items);
        let source = items_at(&snapshot, StoredSafeInventoryLocation::CharacterSafe(0));
        assert!(matches!(
            derive_placements(
                &snapshot,
                &source,
                StoredSafeInventoryLocation::CharacterSafe(0),
                6
            ),
            Err(PersistenceError::SafeInventoryStorageFull)
        ));
    }

    #[test]
    fn entry_preflight_derives_all_sources_against_one_virtual_vault() {
        let storage = snapshot(vec![
            item(
                1,
                "equipment.one",
                0,
                StoredSafeInventoryLocation::CharacterSafe(1),
            ),
            item(
                2,
                "equipment.two",
                0,
                StoredSafeInventoryLocation::CharacterSafe(7),
            ),
            item(
                3,
                "equipment.vault",
                0,
                StoredSafeInventoryLocation::Vault(0),
            ),
        ]);
        assert_eq!(
            derive_preflight_placements(&storage).unwrap(),
            vec![
                StoredSafeInventoryPlacement {
                    item_uid: [1; 16],
                    source: StoredSafeInventoryLocation::CharacterSafe(1),
                    destination: StoredSafeInventoryLocation::Vault(1),
                    expected_item_version: 1,
                },
                StoredSafeInventoryPlacement {
                    item_uid: [2; 16],
                    source: StoredSafeInventoryLocation::CharacterSafe(7),
                    destination: StoredSafeInventoryLocation::Vault(2),
                    expected_item_version: 2,
                },
            ]
        );
        assert!(
            derive_preflight_placements(&snapshot(Vec::new()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn withdrawal_and_deliberate_risk_enforce_exact_eight_slot_boundaries() {
        let mut safe_full = vec![item(
            20,
            "equipment.vault",
            0,
            StoredSafeInventoryLocation::Vault(159),
        )];
        for slot in 0..CHARACTER_SAFE_CAPACITY {
            safe_full.push(item(
                u8::try_from(slot + 1).unwrap(),
                "equipment.safe",
                0,
                StoredSafeInventoryLocation::CharacterSafe(u8::try_from(slot).unwrap()),
            ));
        }
        let safe_full = snapshot(safe_full);
        let vault_source = items_at(&safe_full, StoredSafeInventoryLocation::Vault(159));
        assert!(matches!(
            derive_placements(
                &safe_full,
                &vault_source,
                StoredSafeInventoryLocation::Vault(159),
                5
            ),
            Err(PersistenceError::SafeInventoryStorageFull)
        ));

        let mut backpack_full = vec![item(
            20,
            "equipment.safe",
            0,
            StoredSafeInventoryLocation::CharacterSafe(7),
        )];
        for slot in 0..RUN_BACKPACK_CAPACITY {
            backpack_full.push(item(
                u8::try_from(slot + 1).unwrap(),
                "equipment.pending",
                0,
                StoredSafeInventoryLocation::RunBackpack(u8::try_from(slot).unwrap()),
            ));
        }
        let backpack_full = snapshot(backpack_full);
        let safe_source = items_at(
            &backpack_full,
            StoredSafeInventoryLocation::CharacterSafe(7),
        );
        assert!(matches!(
            derive_placements(
                &backpack_full,
                &safe_source,
                StoredSafeInventoryLocation::CharacterSafe(7),
                2
            ),
            Err(PersistenceError::SafeInventoryStorageFull)
        ));
    }

    #[test]
    fn corrupt_mixed_stack_and_cross_axis_security_fail_closed() {
        let mixed = StoredSafeInventorySnapshot {
            account_version: 1,
            inventory_version: 1,
            character_safe: vec![
                item(
                    1,
                    "consumable.red",
                    1,
                    StoredSafeInventoryLocation::CharacterSafe(0),
                ),
                item(
                    2,
                    "consumable.blue",
                    1,
                    StoredSafeInventoryLocation::CharacterSafe(0),
                ),
            ],
            vault: Vec::new(),
            run_backpack: Vec::new(),
            overflow: Vec::new(),
        };
        assert!(matches!(
            validate_stored_slots(&mixed),
            Err(PersistenceError::CorruptStoredSafeInventory)
        ));

        let mut invalid_command = command(
            StoredSafeInventoryCommandKind::CharacterSafeToRunBackpack,
            0,
            vec![StoredSafeInventoryPlacement {
                item_uid: [1; 16],
                source: StoredSafeInventoryLocation::CharacterSafe(0),
                destination: StoredSafeInventoryLocation::Vault(160),
                expected_item_version: 1,
            }],
        );
        assert!(matches!(
            validate_command(&invalid_command),
            Err(PersistenceError::CorruptStoredSafeInventory)
        ));
        invalid_command.placements[0].destination = StoredSafeInventoryLocation::RunBackpack(7);
        assert!(validate_command(&invalid_command).is_ok());
    }

    #[test]
    fn command_bounds_and_replay_result_versions_fail_closed() {
        let placement = StoredSafeInventoryPlacement {
            item_uid: [1; 16],
            source: StoredSafeInventoryLocation::Vault(0),
            destination: StoredSafeInventoryLocation::CharacterSafe(0),
            expected_item_version: 1,
        };
        let mut invalid = command(
            StoredSafeInventoryCommandKind::VaultToCharacterSafe,
            160,
            vec![placement],
        );
        assert!(matches!(
            validate_command(&invalid),
            Err(PersistenceError::CorruptStoredSafeInventory)
        ));
        invalid.source_slot_index = 0;
        assert!(validate_command(&invalid).is_ok());

        let result = result_from_command(&invalid, true, 4, 5, 7, 8);
        assert!(validate_result(&result, invalid.kind).is_ok());
        let mut corrupt = result;
        corrupt.post_account_version = 4;
        assert!(matches!(
            validate_result(&corrupt, invalid.kind),
            Err(PersistenceError::CorruptStoredSafeInventory)
        ));
    }

    #[test]
    fn deliberate_risk_does_not_advance_account_version() {
        let command = command(
            StoredSafeInventoryCommandKind::CharacterSafeToRunBackpack,
            0,
            vec![StoredSafeInventoryPlacement {
                item_uid: [1; 16],
                source: StoredSafeInventoryLocation::CharacterSafe(0),
                destination: StoredSafeInventoryLocation::RunBackpack(0),
                expected_item_version: 1,
            }],
        );
        let result = result_from_command(&command, false, 4, 4, 7, 8);
        assert!(validate_result(&result, command.kind).is_ok());
    }

    #[test]
    fn item_content_authority_requires_the_exact_core_revision() {
        assert!(require_core_item_content_revision(CORE_ITEM_CONTENT_REVISION).is_ok());

        let alternate_constraint_valid_revision = format!("core-dev.blake3.{}", "f".repeat(64));
        assert_ne!(
            alternate_constraint_valid_revision,
            CORE_ITEM_CONTENT_REVISION
        );
        assert!(matches!(
            require_core_item_content_revision(&alternate_constraint_valid_revision),
            Err(PersistenceError::CorruptStoredSafeInventory)
        ));
    }
}
