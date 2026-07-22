//! Atomic durable Belt-consumable mutation for the ordinary Core danger route.
//!
//! Authority: canonical GDD `INP-001`, `LOOT-032`, `TECH-040`; Content Production
//! Specification `CONT-FP-007`, `CONT-CATALOG-003`; and the `GB-M03` roadmap gates.

use sqlx::Row;

use crate::{
    PersistenceError, PostgresPersistence, StoredActiveDangerAuthorityV1, WIPEABLE_CORE_NAMESPACE,
    active_danger_authority::{
        lock_active_danger_account, validate_active_danger_after_account_lock,
    },
    is_retryable_transaction_failure,
};

const MAX_TRANSACTION_ATTEMPTS: u8 = 8;
const RED_TONIC_TEMPLATE_ID: &str = "consumable.red_tonic";
const BELT_LOCATION: i16 = 1;
const CONSUMED_LOCATION: i16 = 7;
const AT_RISK_SECURITY: i16 = 1;
const CONSUMED_SECURITY: i16 = 4;
const CONSUMED_EVENT_KIND: i16 = 3;
const FIELD_SOURCE_KIND: i16 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreConsumableUseCommandV1 {
    pub authority: StoredActiveDangerAuthorityV1,
    pub mutation_id: [u8; 16],
    pub payload_hash: [u8; 32],
    pub actor_generation: u64,
    pub content_revision: String,
    pub expected_inventory_version: u64,
    pub slot_index: u8,
    pub preflight: CoreConsumablePreflightV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreConsumablePreflightV1 {
    Attempt,
    RejectFullHealth,
    RejectSharedCooldown,
    RejectInactiveSlot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCoreConsumableStateV1 {
    pub character_id: [u8; 16],
    pub actor_generation: u64,
    pub instance_lineage_id: [u8; 16],
    pub content_revision: String,
    pub inventory_version: u64,
    pub belt_quantities: [u8; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredCoreConsumableResultCodeV1 {
    Accepted,
    EmptySlot,
    FullHealth,
    SharedCooldown,
    InactiveSlot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCoreConsumableUseResultV1 {
    pub replayed: bool,
    pub mutation_id: [u8; 16],
    pub code: StoredCoreConsumableResultCodeV1,
    pub consumed_item_uid: Option<[u8; 16]>,
    pub state: StoredCoreConsumableStateV1,
}

impl PostgresPersistence {
    pub async fn load_core_consumable_replay_v1(
        &self,
        command: &CoreConsumableUseCommandV1,
    ) -> Result<Option<StoredCoreConsumableUseResultV1>, PersistenceError> {
        validate_command(command)?;
        let mut transaction = self.begin_transaction().await?;
        lock_active_danger_account(transaction.connection(), command.authority).await?;
        let replay = load_receipt(transaction.connection(), command, true).await?;
        transaction.rollback().await?;
        Ok(replay)
    }

    /// Consumes exactly one lowest-UID Red Tonic from the requested Belt slot, or stores an exact
    /// empty-slot result. Account-first locking preserves terminal-winner ordering.
    pub async fn commit_core_consumable_use_v1(
        &self,
        command: &CoreConsumableUseCommandV1,
    ) -> Result<StoredCoreConsumableUseResultV1, PersistenceError> {
        validate_command(command)?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.commit_core_consumable_use_once_v1(command).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded consumable transaction always returns")
    }

    async fn commit_core_consumable_use_once_v1(
        &self,
        command: &CoreConsumableUseCommandV1,
    ) -> Result<StoredCoreConsumableUseResultV1, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        lock_active_danger_account(transaction.connection(), command.authority).await?;
        if let Some(replay) = load_receipt(transaction.connection(), command, true).await? {
            transaction.rollback().await?;
            return Ok(replay);
        }
        validate_active_danger_after_account_lock(transaction.connection(), command.authority)
            .await?;
        lock_actor_generation(transaction.connection(), command).await?;
        let inventory_version = lock_inventory(transaction.connection(), command).await?;
        if inventory_version != command.expected_inventory_version {
            transaction.rollback().await?;
            return Err(PersistenceError::CoreConsumableInventoryVersionMismatch);
        }

        let unit = if command.preflight == CoreConsumablePreflightV1::Attempt {
            lock_lowest_unit(transaction.connection(), command).await?
        } else {
            None
        };
        let (code, consumed_item_uid, ledger_event_id, post_inventory_version) =
            if let Some(unit) = unit {
                let post_inventory_version = inventory_version
                    .checked_add(1)
                    .ok_or(PersistenceError::CorruptStoredCoreConsumable)?;
                let ledger_event_id = derive_ledger_event_id(command.mutation_id, unit.item_uid);
                transition_unit(transaction.connection(), command, &unit, ledger_event_id).await?;
                update_inventory_version(transaction.connection(), command, post_inventory_version)
                    .await?;
                (
                    StoredCoreConsumableResultCodeV1::Accepted,
                    Some(unit.item_uid),
                    Some(ledger_event_id),
                    post_inventory_version,
                )
            } else if command.preflight == CoreConsumablePreflightV1::Attempt {
                (
                    StoredCoreConsumableResultCodeV1::EmptySlot,
                    None,
                    None,
                    inventory_version,
                )
            } else {
                (
                    match command.preflight {
                        CoreConsumablePreflightV1::RejectFullHealth => {
                            StoredCoreConsumableResultCodeV1::FullHealth
                        }
                        CoreConsumablePreflightV1::RejectSharedCooldown => {
                            StoredCoreConsumableResultCodeV1::SharedCooldown
                        }
                        CoreConsumablePreflightV1::RejectInactiveSlot => {
                            StoredCoreConsumableResultCodeV1::InactiveSlot
                        }
                        CoreConsumablePreflightV1::Attempt => unreachable!(),
                    },
                    None,
                    None,
                    inventory_version,
                )
            };
        let belt_quantities = load_belt_quantities(transaction.connection(), command).await?;
        insert_receipt(
            transaction.connection(),
            command,
            code,
            consumed_item_uid,
            ledger_event_id,
            inventory_version,
            post_inventory_version,
            belt_quantities,
        )
        .await?;
        transaction.commit().await?;
        Ok(StoredCoreConsumableUseResultV1 {
            replayed: false,
            mutation_id: command.mutation_id,
            code,
            consumed_item_uid,
            state: stored_state(command, post_inventory_version, belt_quantities),
        })
    }

    /// Reads the authoritative quantities for attach/reconnect publication under the same danger
    /// and actor-generation authority used by writes.
    pub async fn core_consumable_state_v1(
        &self,
        command: &CoreConsumableUseCommandV1,
    ) -> Result<StoredCoreConsumableStateV1, PersistenceError> {
        validate_command(command)?;
        let mut transaction = self.begin_transaction().await?;
        lock_active_danger_account(transaction.connection(), command.authority).await?;
        validate_active_danger_after_account_lock(transaction.connection(), command.authority)
            .await?;
        lock_actor_generation(transaction.connection(), command).await?;
        let inventory_version = lock_inventory(transaction.connection(), command).await?;
        let belt_quantities = load_belt_quantities(transaction.connection(), command).await?;
        transaction.rollback().await?;
        Ok(stored_state(command, inventory_version, belt_quantities))
    }
}

#[derive(Debug, Clone, Copy)]
struct LockedConsumableUnit {
    item_uid: [u8; 16],
    item_version: u64,
}

fn validate_command(command: &CoreConsumableUseCommandV1) -> Result<(), PersistenceError> {
    command.authority.validate()?;
    if command.mutation_id == [0; 16]
        || command.payload_hash == [0; 32]
        || command.actor_generation == 0
        || command.expected_inventory_version == 0
        || command.slot_index > 1
        || !valid_content_revision(&command.content_revision)
    {
        return Err(PersistenceError::CorruptStoredCoreConsumable);
    }
    Ok(())
}

fn valid_content_revision(value: &str) -> bool {
    let Some(hash) = value.strip_prefix("core-dev.blake3.") else {
        return false;
    };
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

async fn lock_actor_generation(
    connection: &mut sqlx::PgConnection,
    command: &CoreConsumableUseCommandV1,
) -> Result<(), PersistenceError> {
    let generation = i64::try_from(command.actor_generation)
        .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?;
    let found: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM private_route_generation_allocations_v1
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND actor_generation=$4
         FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.authority.account_id.as_slice())
    .bind(command.authority.character_id.as_slice())
    .bind(generation)
    .fetch_optional(connection)
    .await?;
    if found.is_none() {
        return Err(PersistenceError::CoreConsumableAuthorityMismatch);
    }
    Ok(())
}

async fn lock_inventory(
    connection: &mut sqlx::PgConnection,
    command: &CoreConsumableUseCommandV1,
) -> Result<u64, PersistenceError> {
    let version: Option<i64> = sqlx::query_scalar(
        "SELECT inventory_version FROM character_inventories
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.authority.account_id.as_slice())
    .bind(command.authority.character_id.as_slice())
    .fetch_optional(connection)
    .await?;
    u64::try_from(version.ok_or(PersistenceError::CoreConsumableAuthorityMismatch)?)
        .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)
}

async fn lock_lowest_unit(
    connection: &mut sqlx::PgConnection,
    command: &CoreConsumableUseCommandV1,
) -> Result<Option<LockedConsumableUnit>, PersistenceError> {
    let row = sqlx::query(
        "SELECT item_uid,item_version,template_id,content_revision FROM item_instances
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND location_kind=$4 AND slot_index=$5 AND security_state=$6 AND item_kind=1
         ORDER BY item_uid LIMIT 1 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.authority.account_id.as_slice())
    .bind(command.authority.character_id.as_slice())
    .bind(BELT_LOCATION)
    .bind(i16::from(command.slot_index))
    .bind(AT_RISK_SECURITY)
    .fetch_optional(connection)
    .await?;
    row.map(|row| {
        let template_id: String = row.try_get("template_id")?;
        let content_revision: String = row.try_get("content_revision")?;
        if template_id != RED_TONIC_TEMPLATE_ID || content_revision != command.content_revision {
            return Err(PersistenceError::CoreConsumableContentMismatch);
        }
        Ok(LockedConsumableUnit {
            item_uid: fixed_bytes(row.try_get("item_uid")?)?,
            item_version: u64::try_from(row.try_get::<i64, _>("item_version")?)
                .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?,
        })
    })
    .transpose()
}

async fn transition_unit(
    connection: &mut sqlx::PgConnection,
    command: &CoreConsumableUseCommandV1,
    unit: &LockedConsumableUnit,
    ledger_event_id: [u8; 16],
) -> Result<(), PersistenceError> {
    let pre_item_version = i64::try_from(unit.item_version)
        .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?;
    let post_item_version = pre_item_version
        .checked_add(1)
        .ok_or(PersistenceError::CorruptStoredCoreConsumable)?;
    let updated = sqlx::query(
        "UPDATE item_instances SET item_version=$1,security_state=$2,location_kind=$3,
             destruction_reason='consumed',updated_at=transaction_timestamp()
         WHERE namespace_id=$4 AND account_id=$5 AND character_id=$6 AND item_uid=$7
           AND item_version=$8 AND security_state=$9 AND location_kind=$10 AND slot_index=$11",
    )
    .bind(post_item_version)
    .bind(CONSUMED_SECURITY)
    .bind(CONSUMED_LOCATION)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.authority.account_id.as_slice())
    .bind(command.authority.character_id.as_slice())
    .bind(unit.item_uid.as_slice())
    .bind(pre_item_version)
    .bind(AT_RISK_SECURITY)
    .bind(BELT_LOCATION)
    .bind(i16::from(command.slot_index))
    .execute(&mut *connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(PersistenceError::CoreConsumableAuthorityMismatch);
    }
    sqlx::query(
        "INSERT INTO item_ledger_events
         (namespace_id,ledger_event_id,item_uid,account_id,character_id,mutation_id,
          event_kind,source_kind,pre_item_version,post_item_version,pre_security_state,
          post_security_state,pre_location_kind,post_location_kind,reason)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,'consumed')",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ledger_event_id.as_slice())
    .bind(unit.item_uid.as_slice())
    .bind(command.authority.account_id.as_slice())
    .bind(command.authority.character_id.as_slice())
    .bind(command.mutation_id.as_slice())
    .bind(CONSUMED_EVENT_KIND)
    .bind(FIELD_SOURCE_KIND)
    .bind(pre_item_version)
    .bind(post_item_version)
    .bind(AT_RISK_SECURITY)
    .bind(CONSUMED_SECURITY)
    .bind(BELT_LOCATION)
    .bind(CONSUMED_LOCATION)
    .execute(connection)
    .await?;
    Ok(())
}

async fn update_inventory_version(
    connection: &mut sqlx::PgConnection,
    command: &CoreConsumableUseCommandV1,
    version: u64,
) -> Result<(), PersistenceError> {
    let version =
        i64::try_from(version).map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?;
    sqlx::query(
        "UPDATE character_inventories SET inventory_version=$1,updated_at=transaction_timestamp()
         WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4",
    )
    .bind(version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.authority.account_id.as_slice())
    .bind(command.authority.character_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn load_belt_quantities(
    connection: &mut sqlx::PgConnection,
    command: &CoreConsumableUseCommandV1,
) -> Result<[u8; 2], PersistenceError> {
    let rows = sqlx::query(
        "SELECT slot_index,COUNT(*) AS quantity FROM item_instances
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND location_kind=$4
           AND security_state=$5 AND item_kind=1 GROUP BY slot_index ORDER BY slot_index",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.authority.account_id.as_slice())
    .bind(command.authority.character_id.as_slice())
    .bind(BELT_LOCATION)
    .bind(AT_RISK_SECURITY)
    .fetch_all(connection)
    .await?;
    let mut quantities = [0_u8; 2];
    for row in rows {
        let slot = usize::try_from(row.try_get::<i16, _>("slot_index")?)
            .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?;
        let quantity = u8::try_from(row.try_get::<i64, _>("quantity")?)
            .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?;
        if slot >= quantities.len() || quantity > 6 {
            return Err(PersistenceError::CorruptStoredCoreConsumable);
        }
        quantities[slot] = quantity;
    }
    Ok(quantities)
}

#[allow(clippy::too_many_arguments)]
async fn insert_receipt(
    connection: &mut sqlx::PgConnection,
    command: &CoreConsumableUseCommandV1,
    code: StoredCoreConsumableResultCodeV1,
    consumed_item_uid: Option<[u8; 16]>,
    ledger_event_id: Option<[u8; 16]>,
    pre_inventory_version: u64,
    post_inventory_version: u64,
    belt_quantities: [u8; 2],
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO core_consumable_use_receipts_v1
         (namespace_id,account_id,character_id,mutation_id,payload_hash,actor_generation,
          instance_lineage_id,content_revision,slot_index,result_code,consumed_item_uid,
          ledger_event_id,pre_inventory_version,post_inventory_version,belt_one_quantity,
          belt_two_quantity) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.authority.account_id.as_slice())
    .bind(command.authority.character_id.as_slice())
    .bind(command.mutation_id.as_slice())
    .bind(command.payload_hash.as_slice())
    .bind(
        i64::try_from(command.actor_generation)
            .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?,
    )
    .bind(command.authority.instance_lineage_id.as_slice())
    .bind(&command.content_revision)
    .bind(i16::from(command.slot_index))
    .bind(match code {
        StoredCoreConsumableResultCodeV1::Accepted => 0_i16,
        StoredCoreConsumableResultCodeV1::EmptySlot => 1_i16,
        StoredCoreConsumableResultCodeV1::FullHealth => 2_i16,
        StoredCoreConsumableResultCodeV1::SharedCooldown => 3_i16,
        StoredCoreConsumableResultCodeV1::InactiveSlot => 4_i16,
    })
    .bind(consumed_item_uid.as_ref().map(<[u8; 16]>::as_slice))
    .bind(ledger_event_id.as_ref().map(<[u8; 16]>::as_slice))
    .bind(
        i64::try_from(pre_inventory_version)
            .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?,
    )
    .bind(
        i64::try_from(post_inventory_version)
            .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?,
    )
    .bind(i16::from(belt_quantities[0]))
    .bind(i16::from(belt_quantities[1]))
    .execute(connection)
    .await?;
    Ok(())
}

async fn load_receipt(
    connection: &mut sqlx::PgConnection,
    command: &CoreConsumableUseCommandV1,
    replayed: bool,
) -> Result<Option<StoredCoreConsumableUseResultV1>, PersistenceError> {
    let row = sqlx::query(
        "SELECT payload_hash,actor_generation,instance_lineage_id,content_revision,slot_index,
                result_code,consumed_item_uid,post_inventory_version,belt_one_quantity,belt_two_quantity
         FROM core_consumable_use_receipts_v1
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND mutation_id=$4 FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.authority.account_id.as_slice())
    .bind(command.authority.character_id.as_slice())
    .bind(command.mutation_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.map(|row| decode_receipt(&row, command, replayed))
        .transpose()
}

fn decode_receipt(
    row: &sqlx::postgres::PgRow,
    command: &CoreConsumableUseCommandV1,
    replayed: bool,
) -> Result<StoredCoreConsumableUseResultV1, PersistenceError> {
    if fixed_bytes::<32>(row.try_get("payload_hash")?)? != command.payload_hash {
        return Err(PersistenceError::CoreConsumableIdempotencyConflict);
    }
    if u64::try_from(row.try_get::<i64, _>("actor_generation")?).ok()
        != Some(command.actor_generation)
        || fixed_bytes::<16>(row.try_get("instance_lineage_id")?)?
            != command.authority.instance_lineage_id
        || row.try_get::<String, _>("content_revision")? != command.content_revision
        || row.try_get::<i16, _>("slot_index")? != i16::from(command.slot_index)
    {
        return Err(PersistenceError::CorruptStoredCoreConsumable);
    }
    let code = match row.try_get::<i16, _>("result_code")? {
        0 => StoredCoreConsumableResultCodeV1::Accepted,
        1 => StoredCoreConsumableResultCodeV1::EmptySlot,
        2 => StoredCoreConsumableResultCodeV1::FullHealth,
        3 => StoredCoreConsumableResultCodeV1::SharedCooldown,
        4 => StoredCoreConsumableResultCodeV1::InactiveSlot,
        _ => return Err(PersistenceError::CorruptStoredCoreConsumable),
    };
    let consumed_item_uid = row
        .try_get::<Option<Vec<u8>>, _>("consumed_item_uid")?
        .map(fixed_bytes)
        .transpose()?;
    if (code == StoredCoreConsumableResultCodeV1::Accepted) != consumed_item_uid.is_some() {
        return Err(PersistenceError::CorruptStoredCoreConsumable);
    }
    let inventory_version = u64::try_from(row.try_get::<i64, _>("post_inventory_version")?)
        .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?;
    let belt_quantities = [
        bounded_quantity(row.try_get("belt_one_quantity")?)?,
        bounded_quantity(row.try_get("belt_two_quantity")?)?,
    ];
    Ok(StoredCoreConsumableUseResultV1 {
        replayed,
        mutation_id: command.mutation_id,
        code,
        consumed_item_uid,
        state: stored_state(command, inventory_version, belt_quantities),
    })
}

fn stored_state(
    command: &CoreConsumableUseCommandV1,
    inventory_version: u64,
    belt_quantities: [u8; 2],
) -> StoredCoreConsumableStateV1 {
    StoredCoreConsumableStateV1 {
        character_id: command.authority.character_id,
        actor_generation: command.actor_generation,
        instance_lineage_id: command.authority.instance_lineage_id,
        content_revision: command.content_revision.clone(),
        inventory_version,
        belt_quantities,
    }
}

fn derive_ledger_event_id(mutation_id: [u8; 16], item_uid: [u8; 16]) -> [u8; 16] {
    let mut material = [0_u8; 32];
    material[..16].copy_from_slice(&mutation_id);
    material[16..].copy_from_slice(&item_uid);
    let digest = blake3::derive_key("gravebound.core-consumable-ledger.v1", &material);
    let mut id = [0_u8; 16];
    id.copy_from_slice(&digest[..16]);
    id
}

fn fixed_bytes<const N: usize>(value: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredCoreConsumable)
}

fn bounded_quantity(value: i16) -> Result<u8, PersistenceError> {
    let value = u8::try_from(value).map_err(|_| PersistenceError::CorruptStoredCoreConsumable)?;
    if value > 6 {
        return Err(PersistenceError::CorruptStoredCoreConsumable);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_identity_binds_mutation_and_selected_unit() {
        assert_eq!(
            derive_ledger_event_id([1; 16], [2; 16]),
            derive_ledger_event_id([1; 16], [2; 16])
        );
        assert_ne!(
            derive_ledger_event_id([1; 16], [2; 16]),
            derive_ledger_event_id([1; 16], [3; 16])
        );
    }

    #[test]
    fn content_revision_is_strictly_canonical() {
        assert!(valid_content_revision(&format!(
            "core-dev.blake3.{}",
            "a".repeat(64)
        )));
        assert!(!valid_content_revision("core-dev.blake3.test"));
    }
}
