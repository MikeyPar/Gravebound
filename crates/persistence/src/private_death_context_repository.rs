//! Coherent `PostgreSQL` planning snapshot for the live Core permanent-death owner.
//!
//! The three authorities are the canonical GDD `DTH-001`, `ECH-001`, and `TECH-021..023`;
//! the Content Production Spec `CONT-ECHO-001` and Core route records; and roadmap
//! `GB-M03-03`/`06`/`13`. This read model never commits death. It captures every mutable
//! persistence input under one repeatable-read snapshot; the later durable-death transaction
//! locks and revalidates the exact versions before permanent loss can commit.

use sqlx::Row;

use crate::{
    CORE_ITEM_CONTENT_REVISION, DeathAggregateVersionsV1, DeathVersionAdvanceV1,
    DurableDestructionLocationV1, DurableEquipmentSlotV1, PersistenceError, PostgresPersistence,
    WIPEABLE_CORE_NAMESPACE,
};

const LIVING: i16 = 0;
const DANGER_LOCATION: i16 = 2;
const OPEN_LINEAGE_MAX: i16 = 1;
const ACTIVE_RESTORE: i16 = 0;
const AT_RISK_EQUIPPED: i16 = 1;
const AT_RISK_PENDING: i16 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateDeathPlanningRequestV1 {
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub expected_character_version: u64,
    pub death_tick: u64,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateDeathPlanningSnapshotV1 {
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub former_roster_ordinal: u8,
    pub class_id: String,
    pub level: u8,
    pub oath_id: Option<String>,
    pub active_bargain_ids: Vec<String>,
    pub location_content_id: String,
    pub lineage_content_id: String,
    pub layout_id: Option<String>,
    pub content_revision: String,
    pub versions: DeathAggregateVersionsV1,
    pub clock: StoredPrivateDeathClockV1,
    pub custody_items: Vec<StoredPrivateDeathCustodyItemV1>,
    pub run_materials: Vec<StoredPrivateDeathRunMaterialV1>,
    pub deeds: StoredPrivateDeathDeedsV1,
    pub echo_queue: StoredPrivateDeathEchoQueueV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateDeathCustodyItemV1 {
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub content_revision: String,
    pub item_level: Option<u8>,
    pub rarity: Option<u8>,
    pub item_version: u64,
    pub location: DurableDestructionLocationV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateDeathRunMaterialV1 {
    pub material_id: String,
    pub quantity: u32,
    pub material_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateDeathClockV1 {
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub authoritative_tick: u64,
    pub link_lost_ticks: u32,
    pub danger_entry_life_metrics_version: u64,
    pub danger_entry_permadeath_combat_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredPrivateDeathDeedKindV1 {
    DungeonBoss,
    MajorRealmEvent,
    FinalDeedOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateDeathDeedV1 {
    pub completion_id: [u8; 16],
    pub deed_id: String,
    pub achieved_tick: u64,
    pub kind: StoredPrivateDeathDeedKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateDeathDeedsV1 {
    pub completions: Vec<StoredPrivateDeathDeedV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredPrivateDeathEchoQueueV1 {
    ExistingAvailable {
        echo_id: [u8; 16],
    },
    PromoteOldestDormant {
        echo_id: [u8; 16],
        death_id: [u8; 16],
        next_transition_ordinal: u16,
    },
    PromoteNewEcho,
}

impl PostgresPersistence {
    /// Loads one coherent server planning snapshot without holding locks across simulation work.
    /// The committing repository treats every returned pre-version as a compare-and-swap guard.
    pub async fn load_private_death_planning_snapshot_v1(
        &self,
        request: &PrivateDeathPlanningRequestV1,
    ) -> Result<StoredPrivateDeathPlanningSnapshotV1, PersistenceError> {
        validate_request(request)?;
        let mut transaction = self.begin_read_transaction().await?;
        let root = load_root(transaction.connection(), request).await?;
        let active_bargain_ids = load_bargains(
            transaction.connection(),
            request.account_id,
            request.character_id,
        )
        .await?;
        let custody_items = load_custody_items(
            transaction.connection(),
            request.account_id,
            request.character_id,
            &root.content_revision,
        )
        .await?;
        let run_materials = load_run_materials(
            transaction.connection(),
            request.account_id,
            request.character_id,
        )
        .await?;
        let deeds = load_deeds(transaction.connection(), request).await?;
        let echo_queue = load_echo_queue(transaction.connection(), request.account_id).await?;
        transaction.rollback().await?;
        Ok(StoredPrivateDeathPlanningSnapshotV1 {
            account_id: request.account_id,
            character_id: request.character_id,
            former_roster_ordinal: root.former_roster_ordinal,
            class_id: root.class_id,
            level: root.level,
            oath_id: root.oath_id,
            active_bargain_ids,
            location_content_id: root.location_content_id,
            lineage_content_id: root.lineage_content_id,
            layout_id: root.layout_id,
            content_revision: root.content_revision,
            versions: root.versions,
            clock: root.clock,
            custody_items,
            run_materials,
            deeds,
            echo_queue,
        })
    }
}

#[derive(Debug)]
struct RootSnapshot {
    former_roster_ordinal: u8,
    class_id: String,
    level: u8,
    oath_id: Option<String>,
    location_content_id: String,
    lineage_content_id: String,
    layout_id: Option<String>,
    content_revision: String,
    versions: DeathAggregateVersionsV1,
    clock: StoredPrivateDeathClockV1,
}

#[allow(
    clippy::too_many_lines,
    reason = "one joined row keeps all mutable aggregate heads in the same database snapshot"
)]
async fn load_root(
    connection: &mut sqlx::PgConnection,
    request: &PrivateDeathPlanningRequestV1,
) -> Result<RootSnapshot, PersistenceError> {
    let row = sqlx::query(
        "SELECT account.state_version AS account_version, account.selected_character_id, \
                character.roster_ordinal, character.class_id, character.level AS identity_level, \
                character.oath_id, character.life_state, character.security_state AS character_security_state, \
                character.character_state_version, \
                progression.level AS progression_level, progression.progression_version, \
                inventory.inventory_version, oath.oath_bargain_version, \
                life.lifetime_ticks, life.permadeath_combat_ticks, life.life_metrics_version, \
                world.character_version AS world_character_version, world.location_kind, \
                world.location_content_id, world.instance_lineage_id, \
                world.entry_restore_point_id, lineage.content_id AS lineage_content_id, \
                lineage.layout_id, lineage.lineage_state, \
                lineage.records_blake3 AS lineage_records_blake3, \
                lineage.assets_blake3 AS lineage_assets_blake3, \
                lineage.localization_blake3 AS lineage_localization_blake3, \
                root.restore_state, root.account_version AS root_account_version, \
                root.character_version AS root_character_version, \
                root.progression_version AS root_progression_version, \
                root.inventory_version AS root_inventory_version, \
                root.oath_bargain_version AS root_oath_bargain_version, \
                root.life_metrics_version AS root_life_metrics_version, \
                root.records_blake3, root.assets_blake3, root.localization_blake3, \
                entry.life_metrics_version AS entry_life_metrics_version, \
                entry.rollback_permadeath_combat_ticks, \
                clock.authoritative_tick AS clock_authoritative_tick, clock.clock_state, \
                clock.lineage_id AS clock_lineage_id, \
                clock.restore_point_id AS clock_restore_point_id, \
                clock.danger_entry_life_metrics_version, \
                clock.danger_entry_permadeath_combat_ticks, \
                clock.post_lifetime_ticks, clock.post_permadeath_combat_ticks, \
                clock.post_link_lost_ticks::bigint AS post_link_lost_ticks, \
                clock.post_life_metrics_version \
         FROM accounts AS account \
         JOIN characters AS character USING (namespace_id, account_id) \
         JOIN character_progression AS progression USING (namespace_id, account_id, character_id) \
         JOIN character_inventories AS inventory USING (namespace_id, account_id, character_id) \
         JOIN character_oath_bargain_state AS oath USING (namespace_id, account_id, character_id) \
         JOIN character_life_metrics AS life USING (namespace_id, account_id, character_id) \
         JOIN character_world_locations AS world USING (namespace_id, account_id, character_id) \
         JOIN character_instance_lineages AS lineage \
           ON lineage.namespace_id=world.namespace_id AND lineage.account_id=world.account_id \
          AND lineage.character_id=world.character_id \
          AND lineage.lineage_id=world.instance_lineage_id \
         JOIN character_entry_restore_points AS root \
           ON root.namespace_id=world.namespace_id AND root.account_id=world.account_id \
          AND root.character_id=world.character_id \
          AND root.restore_point_id=world.entry_restore_point_id \
         JOIN entry_restore_life_metrics_v3 AS entry \
           ON entry.namespace_id=root.namespace_id AND entry.account_id=root.account_id \
          AND entry.character_id=root.character_id \
          AND entry.restore_point_id=root.restore_point_id \
         JOIN LATERAL ( \
           SELECT receipt.authoritative_tick, receipt.clock_state, receipt.lineage_id, \
                  receipt.restore_point_id, receipt.danger_entry_life_metrics_version, \
                  receipt.danger_entry_permadeath_combat_ticks, receipt.post_lifetime_ticks, \
                  receipt.post_permadeath_combat_ticks, receipt.post_link_lost_ticks, \
                  receipt.post_life_metrics_version \
           FROM character_life_clock_checkpoint_receipts_v1 AS receipt \
           WHERE receipt.namespace_id=account.namespace_id \
             AND receipt.account_id=account.account_id \
             AND receipt.character_id=character.character_id \
           ORDER BY receipt.authoritative_tick DESC, receipt.committed_at DESC LIMIT 1 \
         ) AS clock ON TRUE \
         WHERE account.namespace_id=$1 AND account.account_id=$2 AND character.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::DurableDeathOwnerNotFound)?;

    let selected = optional_id(row.try_get("selected_character_id")?)?;
    let lineage_id = optional_id(row.try_get("instance_lineage_id")?)?;
    let restore_point_id = optional_id(row.try_get("entry_restore_point_id")?)?;
    let character_version = positive(row.try_get("character_state_version")?)?;
    let progression_level = u8_value(row.try_get("progression_level")?)?;
    let identity_level = u8_value(row.try_get("identity_level")?)?;
    if selected != Some(request.character_id)
        || row.try_get::<i16, _>("life_state")? != LIVING
        || row.try_get::<i16, _>("character_security_state")? != 0
        || row.try_get::<i16, _>("location_kind")? != DANGER_LOCATION
        || lineage_id != Some(request.lineage_id)
        || restore_point_id != Some(request.restore_point_id)
        || row.try_get::<i16, _>("lineage_state")? > OPEN_LINEAGE_MAX
        || row.try_get::<i16, _>("restore_state")? != ACTIVE_RESTORE
        || character_version != request.expected_character_version
        || positive(row.try_get("world_character_version")?)? != character_version
        || identity_level != progression_level
    {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    if row.try_get::<String, _>("lineage_records_blake3")? != request.records_blake3
        || row.try_get::<String, _>("lineage_assets_blake3")? != request.assets_blake3
        || row.try_get::<String, _>("lineage_localization_blake3")? != request.localization_blake3
        || row.try_get::<String, _>("records_blake3")? != request.records_blake3
        || row.try_get::<String, _>("assets_blake3")? != request.assets_blake3
        || row.try_get::<String, _>("localization_blake3")? != request.localization_blake3
    {
        return Err(PersistenceError::DurableDeathContentMismatch);
    }

    let account = positive(row.try_get("account_version")?)?;
    let progression = positive(row.try_get("progression_version")?)?;
    let inventory = positive(row.try_get("inventory_version")?)?;
    let oath_bargain = positive(row.try_get("oath_bargain_version")?)?;
    let life_metrics = positive(row.try_get("life_metrics_version")?)?;
    let clock_state: i16 = row.try_get("clock_state")?;
    let clock_authoritative_tick = nonnegative(row.try_get("clock_authoritative_tick")?)?;
    let clock_lineage_id = optional_id(row.try_get("clock_lineage_id")?)?;
    let clock_restore_point_id = optional_id(row.try_get("clock_restore_point_id")?)?;
    let entry_life_metrics_version = positive(row.try_get("entry_life_metrics_version")?)?;
    let danger_entry_life_metrics_version =
        positive(row.try_get("danger_entry_life_metrics_version")?)?;
    let rollback_permadeath_combat_ticks =
        nonnegative(row.try_get("rollback_permadeath_combat_ticks")?)?;
    let danger_entry_permadeath_combat_ticks =
        nonnegative(row.try_get("danger_entry_permadeath_combat_ticks")?)?;
    let lifetime_ticks = nonnegative(row.try_get("lifetime_ticks")?)?;
    let permadeath_combat_ticks = nonnegative(row.try_get("permadeath_combat_ticks")?)?;
    let post_life_metrics_version = positive(row.try_get("post_life_metrics_version")?)?;
    let post_link_lost_ticks = u32_value(row.try_get("post_link_lost_ticks")?)?;
    if !matches!(clock_state, 6 | 7)
        || clock_authoritative_tick != request.death_tick
        || clock_lineage_id != Some(request.lineage_id)
        || clock_restore_point_id != Some(request.restore_point_id)
        || danger_entry_life_metrics_version != entry_life_metrics_version
        || danger_entry_permadeath_combat_ticks != rollback_permadeath_combat_ticks
        || nonnegative(row.try_get("post_lifetime_ticks")?)? != lifetime_ticks
        || nonnegative(row.try_get("post_permadeath_combat_ticks")?)? != permadeath_combat_ticks
        || post_life_metrics_version != life_metrics
        || (clock_state == 6 && post_link_lost_ticks != 0)
        || (clock_state == 7 && !(1..=90).contains(&post_link_lost_ticks))
    {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    for (root_column, live) in [
        ("root_account_version", account),
        ("root_character_version", character_version),
        ("root_progression_version", progression),
        ("root_inventory_version", inventory),
        ("root_oath_bargain_version", oath_bargain),
        ("root_life_metrics_version", life_metrics),
    ] {
        if positive(row.try_get(root_column)?)? > live {
            return Err(PersistenceError::CorruptStoredDurableDeath);
        }
    }
    Ok(RootSnapshot {
        former_roster_ordinal: u8_value(row.try_get("roster_ordinal")?)?,
        class_id: row.try_get("class_id")?,
        level: progression_level,
        oath_id: row.try_get("oath_id")?,
        location_content_id: row.try_get("location_content_id")?,
        lineage_content_id: row.try_get("lineage_content_id")?,
        layout_id: row.try_get("layout_id")?,
        content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
        versions: DeathAggregateVersionsV1 {
            account: advance(account)?,
            character: advance(character_version)?,
            progression: advance(progression)?,
            inventory: advance(inventory)?,
            oath_bargain: advance(oath_bargain)?,
            life_metrics: advance(life_metrics)?,
        },
        clock: StoredPrivateDeathClockV1 {
            lifetime_ticks,
            permadeath_combat_ticks,
            authoritative_tick: clock_authoritative_tick,
            link_lost_ticks: post_link_lost_ticks,
            danger_entry_life_metrics_version,
            danger_entry_permadeath_combat_ticks,
        },
    })
}

async fn load_bargains(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<String>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT bargain_id, acquisition_ordinal FROM character_active_bargains \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
         ORDER BY acquisition_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let ordinal = i16::try_from(index + 1)
                .map_err(|_| PersistenceError::CorruptStoredDurableDeath)?;
            if row.try_get::<i16, _>("acquisition_ordinal")? != ordinal {
                return Err(PersistenceError::CorruptStoredDurableDeath);
            }
            row.try_get("bargain_id")
                .map_err(PersistenceError::Database)
        })
        .collect()
}

async fn load_custody_items(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    content_revision: &str,
) -> Result<Vec<StoredPrivateDeathCustodyItemV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT item_uid, template_id, content_revision, item_kind, item_level, rarity, item_version, \
                security_state, location_kind, slot_index, instance_id, pickup_id \
         FROM item_instances WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
           AND location_kind IN (0,1,2,3) ORDER BY item_uid",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| decode_custody_item(&row, content_revision))
        .collect()
}

fn decode_custody_item(
    row: &sqlx::postgres::PgRow,
    expected_content_revision: &str,
) -> Result<StoredPrivateDeathCustodyItemV1, PersistenceError> {
    let location_kind: i16 = row.try_get("location_kind")?;
    let security_state: i16 = row.try_get("security_state")?;
    let slot = row.try_get::<Option<i16>, _>("slot_index")?;
    let instance_id = optional_id(row.try_get("instance_id")?)?;
    let pickup_id = optional_id(row.try_get("pickup_id")?)?;
    let location = match (location_kind, security_state, slot, instance_id, pickup_id) {
        (0, AT_RISK_EQUIPPED, Some(index), None, None) => DurableDestructionLocationV1::Equipment {
            slot: equipment_slot(index)?,
        },
        (1, AT_RISK_EQUIPPED, Some(index @ 0..=1), None, None) => {
            DurableDestructionLocationV1::Belt {
                index: u8_value(index)?,
            }
        }
        (2, AT_RISK_PENDING, Some(index @ 0..=7), None, None) => {
            DurableDestructionLocationV1::RunBackpack {
                index: u8_value(index)?,
            }
        }
        (3, AT_RISK_PENDING, None, Some(instance_id), Some(pickup_id)) => {
            DurableDestructionLocationV1::PersonalGround {
                instance_id,
                pickup_id,
            }
        }
        _ => return Err(PersistenceError::CorruptStoredDurableDeath),
    };
    let content_revision: String = row.try_get("content_revision")?;
    if content_revision != expected_content_revision {
        return Err(PersistenceError::DurableDeathContentMismatch);
    }
    let item_kind: i16 = row.try_get("item_kind")?;
    let item_level = row
        .try_get::<Option<i16>, _>("item_level")?
        .map(u8_value)
        .transpose()?;
    let rarity = row
        .try_get::<Option<i16>, _>("rarity")?
        .map(u8_value)
        .transpose()?;
    if !matches!(
        (item_kind, item_level, rarity),
        (0, Some(_), Some(_)) | (1, None, None)
    ) {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    Ok(StoredPrivateDeathCustodyItemV1 {
        item_uid: exact_id(row.try_get("item_uid")?)?,
        template_id: row.try_get("template_id")?,
        content_revision,
        item_level,
        rarity,
        item_version: positive(row.try_get("item_version")?)?,
        location,
    })
}

async fn load_run_materials(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<StoredPrivateDeathRunMaterialV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT material_id, quantity, material_version FROM character_run_material_stacks \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
           AND security_state=2 AND quantity>0 ORDER BY material_id COLLATE \"C\"",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredPrivateDeathRunMaterialV1 {
                material_id: row.try_get("material_id")?,
                quantity: u32_value(row.try_get("quantity")?)?,
                material_version: positive(row.try_get("material_version")?)?,
            })
        })
        .collect()
}

async fn load_deeds(
    connection: &mut sqlx::PgConnection,
    request: &PrivateDeathPlanningRequestV1,
) -> Result<StoredPrivateDeathDeedsV1, PersistenceError> {
    let rows = sqlx::query(
        "SELECT reward_event_id, deed_id, deed_kind, achieved_tick FROM character_life_deeds \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
           AND achieved_tick<=$4 AND content_revision=$5 \
         ORDER BY achieved_tick DESC, deed_id COLLATE \"C\" DESC",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(as_i64(request.death_tick)?)
    .bind(CORE_ITEM_CONTENT_REVISION)
    .fetch_all(connection)
    .await?;
    let completions = rows
        .into_iter()
        .map(|row| {
            let kind = match row.try_get::<i16, _>("deed_kind")? {
                0 => StoredPrivateDeathDeedKindV1::DungeonBoss,
                1 => StoredPrivateDeathDeedKindV1::MajorRealmEvent,
                2 => StoredPrivateDeathDeedKindV1::FinalDeedOnly,
                _ => return Err(PersistenceError::CorruptStoredDurableDeath),
            };
            Ok(StoredPrivateDeathDeedV1 {
                completion_id: exact_id(row.try_get("reward_event_id")?)?,
                deed_id: row.try_get("deed_id")?,
                achieved_tick: positive(row.try_get("achieved_tick")?)?,
                kind,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    Ok(StoredPrivateDeathDeedsV1 { completions })
}

async fn load_echo_queue(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
) -> Result<StoredPrivateDeathEchoQueueV1, PersistenceError> {
    let rows = sqlx::query(
        "SELECT echo.echo_id, echo.death_id, echo.state, \
                COALESCE((SELECT max(transition.transition_ordinal) \
                  FROM echo_state_transitions AS transition \
                  WHERE transition.namespace_id=echo.namespace_id \
                    AND transition.echo_id=echo.echo_id), CAST(-1 AS SMALLINT)) AS tail_ordinal \
         FROM echo_records AS echo WHERE echo.namespace_id=$1 AND echo.account_id=$2 \
         ORDER BY echo.created_at, echo.echo_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_all(connection)
    .await?;
    let mut available = None;
    let mut oldest_dormant = None;
    for row in rows {
        let state: i16 = row.try_get("state")?;
        if !(0..=4).contains(&state) {
            return Err(PersistenceError::CorruptStoredDurableDeath);
        }
        let echo_id = exact_id(row.try_get("echo_id")?)?;
        let death_id = exact_id(row.try_get("death_id")?)?;
        let tail: i16 = row.try_get("tail_ordinal")?;
        if tail < 0 {
            return Err(PersistenceError::CorruptStoredDurableDeath);
        }
        if state == 1 && available.replace(echo_id).is_some() {
            return Err(PersistenceError::CorruptStoredDurableDeath);
        }
        if state == 0 && oldest_dormant.is_none() {
            oldest_dormant = Some((
                echo_id,
                death_id,
                u16::try_from(tail).map_err(|_| PersistenceError::CorruptStoredDurableDeath)?,
            ));
        }
    }
    if let Some(echo_id) = available {
        return Ok(StoredPrivateDeathEchoQueueV1::ExistingAvailable { echo_id });
    }
    if let Some((echo_id, death_id, tail)) = oldest_dormant {
        return Ok(StoredPrivateDeathEchoQueueV1::PromoteOldestDormant {
            echo_id,
            death_id,
            next_transition_ordinal: tail
                .checked_add(1)
                .ok_or(PersistenceError::CorruptStoredDurableDeath)?,
        });
    }
    Ok(StoredPrivateDeathEchoQueueV1::PromoteNewEcho)
}

fn validate_request(request: &PrivateDeathPlanningRequestV1) -> Result<(), PersistenceError> {
    if [
        request.account_id,
        request.character_id,
        request.lineage_id,
        request.restore_point_id,
    ]
    .contains(&[0; 16])
        || request.expected_character_version == 0
        || request.death_tick == 0
        || i64::try_from(request.expected_character_version).is_err()
        || i64::try_from(request.death_tick).is_err()
        || !valid_hash(&request.records_blake3)
        || !valid_hash(&request.assets_blake3)
        || !valid_hash(&request.localization_blake3)
    {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    Ok(())
}

fn equipment_slot(index: i16) -> Result<DurableEquipmentSlotV1, PersistenceError> {
    match index {
        0 => Ok(DurableEquipmentSlotV1::Weapon),
        1 => Ok(DurableEquipmentSlotV1::Relic),
        2 => Ok(DurableEquipmentSlotV1::Armor),
        3 => Ok(DurableEquipmentSlotV1::Charm),
        _ => Err(PersistenceError::CorruptStoredDurableDeath),
    }
}

fn advance(pre: u64) -> Result<DeathVersionAdvanceV1, PersistenceError> {
    Ok(DeathVersionAdvanceV1 {
        pre,
        post: pre
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredDurableDeath)?,
    })
}

fn optional_id(value: Option<Vec<u8>>) -> Result<Option<[u8; 16]>, PersistenceError> {
    value.map(exact_id).transpose()
}

fn exact_id(value: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    let value =
        <[u8; 16]>::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)?;
    if value == [0; 16] {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    Ok(value)
}

fn positive(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredDurableDeath)
}

fn nonnegative(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn u8_value(value: i16) -> Result<u8, PersistenceError> {
    u8::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn u32_value(value: i64) -> Result<u32, PersistenceError> {
    u32::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn as_i64(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::DurableDeathBindingMismatch)
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PrivateDeathPlanningRequestV1 {
        PrivateDeathPlanningRequestV1 {
            account_id: [1; 16],
            character_id: [2; 16],
            lineage_id: [3; 16],
            restore_point_id: [4; 16],
            expected_character_version: 5,
            death_tick: 6,
            records_blake3: "a".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
        }
    }

    #[test]
    fn request_authority_is_exact_and_bounded() {
        assert!(validate_request(&request()).is_ok());
        let mut invalid = request();
        invalid.character_id = [0; 16];
        assert!(validate_request(&invalid).is_err());
        let mut invalid = request();
        invalid.records_blake3.replace_range(..1, "A");
        assert!(validate_request(&invalid).is_err());
    }

    #[test]
    fn aggregate_versions_advance_exactly_once() {
        assert_eq!(
            advance(8).unwrap(),
            DeathVersionAdvanceV1 { pre: 8, post: 9 }
        );
        assert!(advance(u64::MAX).is_err());
    }

    #[test]
    fn equipment_slots_follow_canonical_destruction_order() {
        assert_eq!(equipment_slot(0).unwrap(), DurableEquipmentSlotV1::Weapon);
        assert_eq!(equipment_slot(1).unwrap(), DurableEquipmentSlotV1::Relic);
        assert_eq!(equipment_slot(2).unwrap(), DurableEquipmentSlotV1::Armor);
        assert_eq!(equipment_slot(3).unwrap(), DurableEquipmentSlotV1::Charm);
        assert!(equipment_slot(4).is_err());
    }
}
