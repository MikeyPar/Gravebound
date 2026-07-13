use std::collections::BTreeSet;

use sqlx::Row;

use crate::{PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

const MAX_REWARD_ITEMS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRewardRequest {
    pub reward_request_id: [u8; 16],
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub source_instance_id: [u8; 16],
    pub reward_table_id: String,
    pub content_revision: String,
    pub epoch_id: String,
    pub canonical_request_hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPendingItem {
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub item_kind: i16,
    pub slot_index: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardPlanningState {
    pub inventory_version: i64,
    pub pending_items: Vec<StoredPendingItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRewardEntry {
    pub roll_index: i32,
    pub template_id: String,
    pub item_kind: i16,
    pub quantity: i16,
    pub item_level: Option<i16>,
    pub rarity: Option<i16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRewardItem {
    pub item_uid: [u8; 16],
    pub ledger_event_id: [u8; 16],
    pub roll_index: i32,
    pub unit_ordinal: i32,
    pub template_id: String,
    pub item_kind: i16,
    pub item_level: Option<i16>,
    pub rarity: Option<i16>,
    pub location_kind: i16,
    pub slot_index: Option<i16>,
    pub instance_id: Option<[u8; 16]>,
    pub pickup_id: Option<[u8; 16]>,
    pub expires_at_tick: Option<i64>,
    pub provenance_kind: i16,
    pub salvage_band: i16,
    pub salvage_value: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRewardCommit {
    pub plan_hash: [u8; 32],
    pub result_hash: [u8; 32],
    pub audit_digest: [u8; 32],
    pub entries: Vec<StoredRewardEntry>,
    pub items: Vec<StoredRewardItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRewardOutcome {
    pub replayed: bool,
    pub reward_request_id: [u8; 16],
    pub epoch_id: String,
    pub pre_inventory_version: i64,
    pub post_inventory_version: i64,
    pub plan_hash: [u8; 32],
    pub result_hash: [u8; 32],
    pub audit_digest: [u8; 32],
    pub entries: Vec<StoredRewardEntry>,
    pub items: Vec<StoredRewardItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewardTransaction<T> {
    Fresh {
        service_result: T,
        outcome: StoredRewardOutcome,
    },
    Replay(StoredRewardOutcome),
}

impl PostgresPersistence {
    pub async fn transact_reward<T>(
        &self,
        request: StoredRewardRequest,
        planner: impl FnOnce(&RewardPlanningState) -> Result<(T, StoredRewardCommit), PersistenceError>,
    ) -> Result<RewardTransaction<T>, PersistenceError>
    where
        T: Send,
    {
        validate_request(&request)?;
        let mut transaction = self.begin_transaction().await?;
        let inventory_version = super::items::lock_or_create_inventory(
            transaction.connection(),
            request.account_id,
            request.character_id,
        )
        .await?;

        if let Some(outcome) = load_replay(transaction.connection(), &request).await? {
            transaction.rollback().await?;
            return Ok(RewardTransaction::Replay(outcome));
        }

        reserve_request(transaction.connection(), &request, inventory_version).await?;
        let planning_state = RewardPlanningState {
            inventory_version,
            pending_items: load_pending_items(
                transaction.connection(),
                request.account_id,
                request.character_id,
            )
            .await?,
        };
        let (service_result, commit) = planner(&planning_state)?;
        validate_commit(&request, &commit)?;
        persist_commit(
            transaction.connection(),
            &request,
            inventory_version,
            &commit,
        )
        .await?;
        let post_inventory_version = inventory_version + i64::from(!commit.items.is_empty());
        transaction.commit().await?;
        Ok(RewardTransaction::Fresh {
            service_result,
            outcome: StoredRewardOutcome {
                replayed: false,
                reward_request_id: request.reward_request_id,
                epoch_id: request.epoch_id,
                pre_inventory_version: inventory_version,
                post_inventory_version,
                plan_hash: commit.plan_hash,
                result_hash: commit.result_hash,
                audit_digest: commit.audit_digest,
                entries: commit.entries,
                items: commit.items,
            },
        })
    }

    pub async fn reward_replay(
        &self,
        request: &StoredRewardRequest,
    ) -> Result<Option<StoredRewardOutcome>, PersistenceError> {
        validate_request(request)?;
        let mut transaction = self.begin_transaction().await?;
        let outcome = load_replay(transaction.connection(), request).await?;
        transaction.rollback().await?;
        Ok(outcome)
    }
}

async fn reserve_request(
    connection: &mut sqlx::PgConnection,
    request: &StoredRewardRequest,
    inventory_version: i64,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO reward_requests \
         (namespace_id, reward_request_id, account_id, character_id, source_instance_id, \
          reward_table_id, content_revision, epoch_id, canonical_request_hash, \
          pre_inventory_version, request_state) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.reward_request_id.as_slice())
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.source_instance_id.as_slice())
    .bind(&request.reward_table_id)
    .bind(&request.content_revision)
    .bind(&request.epoch_id)
    .bind(request.canonical_request_hash.as_slice())
    .bind(inventory_version)
    .execute(connection)
    .await?;
    Ok(())
}

async fn load_replay(
    connection: &mut sqlx::PgConnection,
    request: &StoredRewardRequest,
) -> Result<Option<StoredRewardOutcome>, PersistenceError> {
    let Some(row) = sqlx::query(
        "SELECT account_id, character_id, source_instance_id, reward_table_id, content_revision, \
         epoch_id, canonical_request_hash, request_state, pre_inventory_version, \
         post_inventory_version, plan_hash, result_hash, audit_digest, reward_item_count \
         FROM reward_requests WHERE namespace_id = $1 AND reward_request_id = $2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.reward_request_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    if fixed_bytes::<16>(row.try_get("account_id")?)? != request.account_id
        || fixed_bytes::<16>(row.try_get("character_id")?)? != request.character_id
        || fixed_bytes::<16>(row.try_get("source_instance_id")?)? != request.source_instance_id
        || row.try_get::<String, _>("reward_table_id")? != request.reward_table_id
        || row.try_get::<String, _>("content_revision")? != request.content_revision
        || fixed_bytes::<32>(row.try_get("canonical_request_hash")?)?
            != request.canonical_request_hash
    {
        return Err(PersistenceError::ItemIdempotencyConflict);
    }
    if row.try_get::<i16, _>("request_state")? != 1 {
        return Err(PersistenceError::CorruptStoredItems);
    }
    let outcome = StoredRewardOutcome {
        replayed: true,
        reward_request_id: request.reward_request_id,
        epoch_id: row.try_get("epoch_id")?,
        pre_inventory_version: row.try_get("pre_inventory_version")?,
        post_inventory_version: row.try_get("post_inventory_version")?,
        plan_hash: fixed_bytes(row.try_get("plan_hash")?)?,
        result_hash: fixed_bytes(row.try_get("result_hash")?)?,
        audit_digest: fixed_bytes(row.try_get("audit_digest")?)?,
        entries: load_entries(&mut *connection, request.reward_request_id).await?,
        items: load_reward_items(&mut *connection, request.reward_request_id).await?,
    };
    if usize::try_from(row.try_get::<i16, _>("reward_item_count")?)
        .map_err(|_| PersistenceError::CorruptStoredItems)?
        != outcome.items.len()
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    validate_commit(
        request,
        &StoredRewardCommit {
            plan_hash: outcome.plan_hash,
            result_hash: outcome.result_hash,
            audit_digest: outcome.audit_digest,
            entries: outcome.entries.clone(),
            items: outcome.items.clone(),
        },
    )?;
    validate_outcome(&outcome)?;
    Ok(Some(outcome))
}

async fn load_pending_items(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<StoredPendingItem>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT item_uid, template_id, item_kind, slot_index FROM item_instances \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 \
         AND location_kind = 2 ORDER BY slot_index, item_uid FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredPendingItem {
                item_uid: fixed_bytes(row.try_get("item_uid")?)?,
                template_id: row.try_get("template_id")?,
                item_kind: row.try_get("item_kind")?,
                slot_index: row.try_get("slot_index")?,
            })
        })
        .collect()
}

async fn persist_commit(
    connection: &mut sqlx::PgConnection,
    request: &StoredRewardRequest,
    inventory_version: i64,
    commit: &StoredRewardCommit,
) -> Result<(), PersistenceError> {
    for entry in &commit.entries {
        sqlx::query(
            "INSERT INTO reward_result_entries \
             (namespace_id, reward_request_id, roll_index, template_id, item_kind, quantity, \
              item_level, rarity) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.reward_request_id.as_slice())
        .bind(entry.roll_index)
        .bind(&entry.template_id)
        .bind(entry.item_kind)
        .bind(entry.quantity)
        .bind(entry.item_level)
        .bind(entry.rarity)
        .execute(&mut *connection)
        .await?;
    }
    for item in &commit.items {
        insert_reward_item(&mut *connection, request, item).await?;
    }
    let item_count =
        i16::try_from(commit.items.len()).map_err(|_| PersistenceError::CorruptStoredItems)?;
    let post_inventory_version = inventory_version + i64::from(item_count > 0);
    if item_count > 0 {
        sqlx::query(
            "UPDATE character_inventories SET inventory_version = $1, \
             updated_at = transaction_timestamp() WHERE namespace_id = $2 \
             AND account_id = $3 AND character_id = $4",
        )
        .bind(post_inventory_version)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .execute(&mut *connection)
        .await?;
    }
    sqlx::query(
        "UPDATE reward_requests SET request_state = 1, plan_hash = $1, result_hash = $2, \
         audit_digest = $3, post_inventory_version = $4, reward_item_count = $5 \
         WHERE namespace_id = $6 AND reward_request_id = $7 AND request_state = 0",
    )
    .bind(commit.plan_hash.as_slice())
    .bind(commit.result_hash.as_slice())
    .bind(commit.audit_digest.as_slice())
    .bind(post_inventory_version)
    .bind(item_count)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.reward_request_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_reward_item(
    connection: &mut sqlx::PgConnection,
    request: &StoredRewardRequest,
    item: &StoredRewardItem,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO item_instances \
         (namespace_id, item_uid, account_id, character_id, template_id, content_revision, \
          item_kind, item_level, rarity, creation_kind, creation_request_id, roll_index, \
          unit_ordinal, item_version, security_state, location_kind, slot_index, instance_id, \
          pickup_id, expires_at_tick, provenance_kind, salvage_band, salvage_value) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 1, $10, $11, $12, 1, 2, $13, \
          $14, $15, $16, $17, $18, $19, $20)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item.item_uid.as_slice())
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(&item.template_id)
    .bind(&request.content_revision)
    .bind(item.item_kind)
    .bind(item.item_level)
    .bind(item.rarity)
    .bind(request.reward_request_id.as_slice())
    .bind(item.roll_index)
    .bind(item.unit_ordinal)
    .bind(item.location_kind)
    .bind(item.slot_index)
    .bind(item.instance_id.map(|value| value.to_vec()))
    .bind(item.pickup_id.map(|value| value.to_vec()))
    .bind(item.expires_at_tick)
    .bind(item.provenance_kind)
    .bind(item.salvage_band)
    .bind(item.salvage_value)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO item_ledger_events \
         (namespace_id, ledger_event_id, item_uid, account_id, character_id, mutation_id, \
          event_kind, source_kind, pre_item_version, post_item_version, post_security_state, \
          post_location_kind) VALUES ($1, $2, $3, $4, $5, $6, 0, 1, 0, 1, 2, $7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item.ledger_event_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.reward_request_id.as_slice())
    .bind(item.location_kind)
    .execute(connection)
    .await?;
    Ok(())
}

async fn load_entries(
    connection: &mut sqlx::PgConnection,
    request_id: [u8; 16],
) -> Result<Vec<StoredRewardEntry>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT roll_index, template_id, item_kind, quantity, item_level, rarity \
         FROM reward_result_entries WHERE namespace_id = $1 AND reward_request_id = $2 \
         ORDER BY roll_index",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredRewardEntry {
                roll_index: row.try_get("roll_index")?,
                template_id: row.try_get("template_id")?,
                item_kind: row.try_get("item_kind")?,
                quantity: row.try_get("quantity")?,
                item_level: row.try_get("item_level")?,
                rarity: row.try_get("rarity")?,
            })
        })
        .collect()
}

async fn load_reward_items(
    connection: &mut sqlx::PgConnection,
    request_id: [u8; 16],
) -> Result<Vec<StoredRewardItem>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT i.item_uid, l.ledger_event_id, i.roll_index, i.unit_ordinal, i.template_id, \
         i.item_kind, i.item_level, i.rarity, i.location_kind, i.slot_index, i.instance_id, \
         i.pickup_id, i.expires_at_tick, i.provenance_kind, i.salvage_band, i.salvage_value \
         FROM item_instances i JOIN item_ledger_events l ON l.namespace_id = i.namespace_id \
         AND l.item_uid = i.item_uid AND l.post_item_version = 1 \
         WHERE i.namespace_id = $1 AND i.creation_kind = 1 AND i.creation_request_id = $2 \
         ORDER BY i.roll_index, i.unit_ordinal, i.item_uid",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.iter().map(stored_item_from_row).collect()
}

fn stored_item_from_row(row: &sqlx::postgres::PgRow) -> Result<StoredRewardItem, PersistenceError> {
    Ok(StoredRewardItem {
        item_uid: fixed_bytes(row.try_get("item_uid")?)?,
        ledger_event_id: fixed_bytes(row.try_get("ledger_event_id")?)?,
        roll_index: row.try_get("roll_index")?,
        unit_ordinal: row.try_get("unit_ordinal")?,
        template_id: row.try_get("template_id")?,
        item_kind: row.try_get("item_kind")?,
        item_level: row.try_get("item_level")?,
        rarity: row.try_get("rarity")?,
        location_kind: row.try_get("location_kind")?,
        slot_index: row.try_get("slot_index")?,
        instance_id: optional_fixed_bytes(row.try_get("instance_id")?)?,
        pickup_id: optional_fixed_bytes(row.try_get("pickup_id")?)?,
        expires_at_tick: row.try_get("expires_at_tick")?,
        provenance_kind: row.try_get("provenance_kind")?,
        salvage_band: row.try_get("salvage_band")?,
        salvage_value: row.try_get("salvage_value")?,
    })
}

fn validate_request(request: &StoredRewardRequest) -> Result<(), PersistenceError> {
    if request.reward_request_id == [0; 16]
        || request.account_id == [0; 16]
        || request.character_id == [0; 16]
        || request.source_instance_id == [0; 16]
        || request.canonical_request_hash == [0; 32]
        || !(3..=96).contains(&request.reward_table_id.len())
        || !(1..=64).contains(&request.epoch_id.len())
        || !request.content_revision.starts_with("core-dev.blake3.")
        || request.content_revision.len() != 80
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(())
}

fn validate_commit(
    request: &StoredRewardRequest,
    commit: &StoredRewardCommit,
) -> Result<(), PersistenceError> {
    if commit.plan_hash == [0; 32]
        || commit.result_hash == [0; 32]
        || commit.audit_digest == [0; 32]
        || commit.items.len() > MAX_REWARD_ITEMS
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    let mut rolls = BTreeSet::new();
    for entry in &commit.entries {
        if !rolls.insert(entry.roll_index) || !valid_entry(entry) {
            return Err(PersistenceError::CorruptStoredItems);
        }
        let matching = commit
            .items
            .iter()
            .filter(|item| item.roll_index == entry.roll_index)
            .collect::<Vec<_>>();
        if matching.len() != usize::try_from(entry.quantity).unwrap_or_default() {
            return Err(PersistenceError::CorruptStoredItems);
        }
        for (ordinal, item) in matching.into_iter().enumerate() {
            if item.template_id != entry.template_id
                || item.item_kind != entry.item_kind
                || item.item_level != entry.item_level
                || item.rarity != entry.rarity
                || item.unit_ordinal != i32::try_from(ordinal).unwrap_or(-1)
                || !valid_reward_item(request, item)
            {
                return Err(PersistenceError::CorruptStoredItems);
            }
        }
    }
    if commit
        .items
        .iter()
        .any(|item| !rolls.contains(&item.roll_index))
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    for (index, item) in commit.items.iter().enumerate() {
        if commit.items[..index].iter().any(|other| {
            other.item_uid == item.item_uid || other.ledger_event_id == item.ledger_event_id
        }) {
            return Err(PersistenceError::CorruptStoredItems);
        }
    }
    Ok(())
}

fn valid_entry(entry: &StoredRewardEntry) -> bool {
    (0..=i32::from(u16::MAX)).contains(&entry.roll_index)
        && (1..=6).contains(&entry.quantity)
        && (entry.item_kind == 0
            && entry.quantity == 1
            && entry
                .item_level
                .is_some_and(|level| (1..=10).contains(&level))
            && entry.rarity.is_some_and(|rarity| (0..=4).contains(&rarity))
            || entry.item_kind == 1 && entry.item_level.is_none() && entry.rarity.is_none())
}

fn valid_reward_item(request: &StoredRewardRequest, item: &StoredRewardItem) -> bool {
    if item.item_uid == [0; 16]
        || item.ledger_event_id == [0; 16]
        || item.provenance_kind != 1
        || !(0..=5).contains(&item.salvage_band)
        || item.salvage_value < 0
    {
        return false;
    }
    match item.location_kind {
        2 => {
            item.slot_index.is_some_and(|slot| (0..=7).contains(&slot))
                && item.instance_id.is_none()
                && item.pickup_id.is_none()
                && item.expires_at_tick.is_none()
        }
        3 => {
            item.slot_index.is_none()
                && item.instance_id == Some(request.source_instance_id)
                && item.pickup_id.is_some_and(|id| id != [0; 16])
                && item.expires_at_tick.is_some_and(|tick| tick > 0)
        }
        _ => false,
    }
}

fn validate_outcome(outcome: &StoredRewardOutcome) -> Result<(), PersistenceError> {
    if !(1..=64).contains(&outcome.epoch_id.len())
        || outcome.pre_inventory_version <= 0
        || outcome.post_inventory_version
            != outcome.pre_inventory_version + i64::from(!outcome.items.is_empty())
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(())
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredItems)
}

fn optional_fixed_bytes<const N: usize>(
    bytes: Option<Vec<u8>>,
) -> Result<Option<[u8; N]>, PersistenceError> {
    bytes.map(fixed_bytes).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> StoredRewardRequest {
        StoredRewardRequest {
            reward_request_id: [1; 16],
            account_id: [2; 16],
            character_id: [3; 16],
            source_instance_id: [4; 16],
            reward_table_id: "reward.normal_outer".to_owned(),
            content_revision: format!("core-dev.blake3.{}", "a".repeat(64)),
            epoch_id: "test-epoch".to_owned(),
            canonical_request_hash: [5; 32],
        }
    }

    fn equipment_item() -> StoredRewardItem {
        StoredRewardItem {
            item_uid: [6; 16],
            ledger_event_id: [7; 16],
            roll_index: 0,
            unit_ordinal: 0,
            template_id: "item.armor.pilgrim.t1".to_owned(),
            item_kind: 0,
            item_level: Some(1),
            rarity: Some(1),
            location_kind: 2,
            slot_index: Some(0),
            instance_id: None,
            pickup_id: None,
            expires_at_tick: None,
            provenance_kind: 1,
            salvage_band: 1,
            salvage_value: 5,
        }
    }

    #[test]
    fn normalized_commit_requires_exact_units_and_locations() {
        let entry = StoredRewardEntry {
            roll_index: 0,
            template_id: "item.armor.pilgrim.t1".to_owned(),
            item_kind: 0,
            quantity: 1,
            item_level: Some(1),
            rarity: Some(1),
        };
        let mut commit = StoredRewardCommit {
            plan_hash: [8; 32],
            result_hash: [9; 32],
            audit_digest: [10; 32],
            entries: vec![entry],
            items: vec![equipment_item()],
        };
        assert!(validate_commit(&request(), &commit).is_ok());
        commit.items[0].slot_index = Some(8);
        assert!(validate_commit(&request(), &commit).is_err());
        commit.items[0] = equipment_item();
        commit.items[0].unit_ordinal = 1;
        assert!(validate_commit(&request(), &commit).is_err());
    }

    #[test]
    fn empty_commit_is_legal_and_does_not_imply_an_inventory_change() {
        let commit = StoredRewardCommit {
            plan_hash: [8; 32],
            result_hash: [9; 32],
            audit_digest: [10; 32],
            entries: Vec::new(),
            items: Vec::new(),
        };
        assert!(validate_commit(&request(), &commit).is_ok());
        assert!(
            validate_outcome(&StoredRewardOutcome {
                replayed: false,
                reward_request_id: [1; 16],
                epoch_id: "test-epoch".to_owned(),
                pre_inventory_version: 3,
                post_inventory_version: 3,
                plan_hash: commit.plan_hash,
                result_hash: commit.result_hash,
                audit_digest: commit.audit_digest,
                entries: commit.entries,
                items: commit.items,
            })
            .is_ok()
        );
    }
}
