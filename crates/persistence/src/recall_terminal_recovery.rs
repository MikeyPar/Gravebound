//! Strict read-only recovery for one committed production Emergency Recall.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-010`, `LOOT-002`,
//! `LOOT-033`, and `TECH-015`/`021`-`023`;
//! `Gravebound_Content_Production_Spec_v1.md` `CONT-HUB-001`/`002`, the Core
//! microrealm/dungeon/boss Recall contract, and `CONT-VALID-001`; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08`, plus accepted
//! `SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md`.
//!
//! Recovery proves the immutable result, normalized loss projection, item ledgers,
//! destroyed custody, danger-root closure, audit, and outbox. Historical results do
//! not depend on mutable current aggregate heads. `owns_current_hall` separately
//! identifies the narrow post-commit state that may reconstruct a live terminal actor.

use sqlx::{PgConnection, Row};

use crate::{
    PersistenceError, PostgresPersistence, ProductionRecallTriggerV1, StoredProductionRecallItemV1,
    StoredProductionRecallMaterialDestructionV1, StoredProductionRecallResultV1,
    StoredRecallLocationV1, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};

pub const PRODUCTION_RECALL_RECOVERY_SCHEMA_VERSION: u16 = 1;

const RESTORE_RECALL_COMMITTED: i16 = 3;
const LINEAGE_CLOSED_SUCCESS: i16 = 2;

const TERMINAL_ROOT_SQL: &str =
    "SELECT account_id,character_id,mutation_id,terminal_id,contract_version,
            terminal_kind,trigger_kind,explicit_request_sequence,explicit_client_tick,
            canonical_request_hash,canonical_plan_hash,result_hash,result_payload,
            instance_lineage_id,entry_restore_point_id,source_content_id,
            destination_content_id,records_blake3,assets_blake3,localization_blake3,
            floor(extract(epoch FROM issued_at)*1000)::bigint AS issued_at_ms,
            trigger_started_tick,completion_tick,
            floor(extract(epoch FROM committed_at)*1000)::bigint AS committed_at_ms,
            pre_account_version,post_account_version,
            pre_character_version,post_character_version,
            pre_world_version,post_world_version,
            pre_inventory_version,post_inventory_version,
            pre_life_metrics_version,post_life_metrics_version,
            pre_lifetime_ticks,post_lifetime_ticks,
            pre_permadeath_combat_ticks,post_permadeath_combat_ticks,
            preserved_progression_version,preserved_oath_bargain_version,
            preserved_ash_wallet_version,stabilized_item_count,
            destroyed_item_count,destroyed_material_stack_count,result_code
     FROM character_recall_terminal_results_v1
     WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
       AND ($4::bytea IS NULL OR mutation_id=$4)
       AND ($5::bytea IS NULL OR terminal_id=$5)
     ORDER BY committed_at DESC,terminal_id DESC
     LIMIT 1";

const IMMUTABLE_GRAPH_SQL: &str = "SELECT root.restore_state,root.recall_terminal_id,
            (root.consumed_at=terminal.committed_at) AS root_time_exact,
            root.records_blake3 AS root_records_blake3,
            root.assets_blake3 AS root_assets_blake3,
            root.localization_blake3 AS root_localization_blake3,
            lineage.lineage_state,lineage.content_id,
            (lineage.closed_at=terminal.committed_at) AS lineage_time_exact,
            lineage.records_blake3 AS lineage_records_blake3,
            lineage.assets_blake3 AS lineage_assets_blake3,
            lineage.localization_blake3 AS lineage_localization_blake3,
            audit.event_digest AS audit_digest,
            (audit.created_at=terminal.committed_at) AS audit_time_exact,
            outbox.event_payload AS outbox_payload,
            (outbox.created_at=terminal.committed_at) AS outbox_time_exact
     FROM character_recall_terminal_results_v1 AS terminal
     JOIN character_entry_restore_points AS root
       ON root.namespace_id=terminal.namespace_id
      AND root.account_id=terminal.account_id
      AND root.character_id=terminal.character_id
      AND root.restore_point_id=terminal.entry_restore_point_id
      AND root.lineage_id=terminal.instance_lineage_id
     JOIN character_instance_lineages AS lineage
       ON lineage.namespace_id=terminal.namespace_id
      AND lineage.account_id=terminal.account_id
      AND lineage.character_id=terminal.character_id
      AND lineage.lineage_id=terminal.instance_lineage_id
     LEFT JOIN recall_terminal_audit_events_v1 AS audit
       ON audit.namespace_id=terminal.namespace_id
      AND audit.account_id=terminal.account_id
      AND audit.character_id=terminal.character_id
      AND audit.terminal_id=terminal.terminal_id
      AND audit.event_type=$5
     LEFT JOIN recall_terminal_outbox_events_v1 AS outbox
       ON outbox.namespace_id=terminal.namespace_id
      AND outbox.account_id=terminal.account_id
      AND outbox.character_id=terminal.character_id
      AND outbox.terminal_id=terminal.terminal_id
      AND outbox.event_type=$5
     WHERE terminal.namespace_id=$1 AND terminal.account_id=$2
       AND terminal.character_id=$3 AND terminal.terminal_id=$4
     LIMIT 2";

const STABILIZATIONS_SQL: &str = "SELECT account_id,character_id,mutation_id,stabilization_ordinal,
            item_uid,template_id,content_revision,item_kind,source_kind,
            source_slot_index,pre_item_version,post_item_version,ledger_event_id
     FROM recall_terminal_item_stabilizations_v1
     WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND terminal_id=$4
     ORDER BY stabilization_ordinal";

const DESTRUCTIONS_SQL: &str = "SELECT account_id,character_id,mutation_id,destruction_ordinal,
            item_uid,template_id,content_revision,item_kind,source_kind,
            source_slot_index,source_instance_id,source_pickup_id,
            source_expires_at_tick,pre_item_version,post_item_version,ledger_event_id
     FROM recall_terminal_item_destructions_v1
     WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND terminal_id=$4
     ORDER BY destruction_ordinal";

const MATERIALS_SQL: &str = "SELECT account_id,character_id,mutation_id,destruction_ordinal,
            material_id,destroyed_quantity,pre_pouch_version,post_pouch_version,
            destruction_event_id
     FROM recall_terminal_material_destructions_v1
     WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND terminal_id=$4
     ORDER BY destruction_ordinal";

const LEDGER_AND_CUSTODY_SQL: &str = "SELECT
       (SELECT count(*) FROM item_ledger_events
        WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
          AND terminal_recall_id=$4) AS item_ledgers,
       (SELECT count(*) FROM item_instances
        WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
          AND terminal_recall_id=$4 AND security_state=3 AND location_kind=4
          AND destruction_reason='recall') AS destroyed_items,
       (SELECT count(*) FROM character_run_material_stacks
        WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
          AND terminal_recall_id=$4 AND quantity=0 AND security_state=3
          AND terminal_reason='recall') AS destroyed_materials,
       EXISTS (
         SELECT 1
         FROM recall_terminal_item_stabilizations_v1 AS projection
         JOIN character_recall_terminal_results_v1 AS terminal
           ON terminal.namespace_id=projection.namespace_id
          AND terminal.terminal_id=projection.terminal_id
         LEFT JOIN item_ledger_events AS ledger
           ON ledger.namespace_id=projection.namespace_id
          AND ledger.ledger_event_id=projection.ledger_event_id
         WHERE projection.namespace_id=$1 AND projection.account_id=$2
           AND projection.character_id=$3 AND projection.terminal_id=$4
           AND (
             ledger.ledger_event_id IS NULL
             OR ledger.item_uid IS DISTINCT FROM projection.item_uid
             OR ledger.account_id IS DISTINCT FROM projection.account_id
             OR ledger.character_id IS DISTINCT FROM projection.character_id
             OR ledger.mutation_id IS DISTINCT FROM projection.mutation_id
             OR ledger.event_kind IS DISTINCT FROM 1
             OR ledger.source_kind IS DISTINCT FROM 6
             OR ledger.pre_item_version IS DISTINCT FROM projection.pre_item_version
             OR ledger.post_item_version IS DISTINCT FROM projection.post_item_version
             OR ledger.pre_security_state IS DISTINCT FROM 1
             OR ledger.post_security_state IS DISTINCT FROM 0
             OR ledger.pre_location_kind IS DISTINCT FROM projection.source_kind
             OR ledger.post_location_kind IS DISTINCT FROM projection.source_kind
             OR ledger.reason IS NOT NULL
             OR ledger.terminal_recall_id IS DISTINCT FROM projection.terminal_id
             OR ledger.committed_at IS DISTINCT FROM terminal.committed_at
           )
       ) AS stabilization_mismatch,
       EXISTS (
         SELECT 1
         FROM recall_terminal_item_destructions_v1 AS projection
         JOIN character_recall_terminal_results_v1 AS terminal
           ON terminal.namespace_id=projection.namespace_id
          AND terminal.terminal_id=projection.terminal_id
         LEFT JOIN item_ledger_events AS ledger
           ON ledger.namespace_id=projection.namespace_id
          AND ledger.ledger_event_id=projection.ledger_event_id
         LEFT JOIN item_instances AS item
           ON item.namespace_id=projection.namespace_id
          AND item.item_uid=projection.item_uid
         WHERE projection.namespace_id=$1 AND projection.account_id=$2
           AND projection.character_id=$3 AND projection.terminal_id=$4
           AND (
             ledger.ledger_event_id IS NULL
             OR ledger.item_uid IS DISTINCT FROM projection.item_uid
             OR ledger.account_id IS DISTINCT FROM projection.account_id
             OR ledger.character_id IS DISTINCT FROM projection.character_id
             OR ledger.mutation_id IS DISTINCT FROM projection.mutation_id
             OR ledger.event_kind IS DISTINCT FROM 2
             OR ledger.source_kind IS DISTINCT FROM 6
             OR ledger.pre_item_version IS DISTINCT FROM projection.pre_item_version
             OR ledger.post_item_version IS DISTINCT FROM projection.post_item_version
             OR ledger.pre_security_state IS DISTINCT FROM 2
             OR ledger.post_security_state IS DISTINCT FROM 3
             OR ledger.pre_location_kind IS DISTINCT FROM projection.source_kind
             OR ledger.post_location_kind IS DISTINCT FROM 4
             OR ledger.reason IS DISTINCT FROM 'recall'
             OR ledger.terminal_recall_id IS DISTINCT FROM projection.terminal_id
             OR ledger.committed_at IS DISTINCT FROM terminal.committed_at
             OR item.item_uid IS NULL
             OR item.account_id IS DISTINCT FROM projection.account_id
             OR item.character_id IS DISTINCT FROM projection.character_id
             OR item.template_id IS DISTINCT FROM projection.template_id
             OR item.content_revision IS DISTINCT FROM projection.content_revision
             OR item.item_kind IS DISTINCT FROM projection.item_kind
             OR item.item_version IS DISTINCT FROM projection.post_item_version
             OR item.security_state IS DISTINCT FROM 3
             OR item.location_kind IS DISTINCT FROM 4
             OR item.destruction_reason IS DISTINCT FROM 'recall'
             OR item.terminal_recall_id IS DISTINCT FROM projection.terminal_id
             OR item.recalled_at IS DISTINCT FROM terminal.committed_at
           )
       ) AS destruction_mismatch,
       EXISTS (
         SELECT 1
         FROM recall_terminal_material_destructions_v1 AS projection
         JOIN character_recall_terminal_results_v1 AS terminal
           ON terminal.namespace_id=projection.namespace_id
          AND terminal.terminal_id=projection.terminal_id
         LEFT JOIN character_run_material_stacks AS pouch
           ON pouch.namespace_id=projection.namespace_id
          AND pouch.account_id=projection.account_id
          AND pouch.character_id=projection.character_id
          AND pouch.material_id=projection.material_id
         WHERE projection.namespace_id=$1 AND projection.account_id=$2
           AND projection.character_id=$3 AND projection.terminal_id=$4
           AND (
             pouch.material_id IS NULL
             OR pouch.quantity IS DISTINCT FROM 0
             OR pouch.material_version IS DISTINCT FROM projection.post_pouch_version
             OR pouch.security_state IS DISTINCT FROM 3
             OR pouch.terminal_reason IS DISTINCT FROM 'recall'
             OR pouch.terminal_restore_point_id IS NOT NULL
             OR pouch.terminal_death_id IS NOT NULL
             OR pouch.terminal_extraction_id IS NOT NULL
             OR pouch.terminal_recall_id IS DISTINCT FROM projection.terminal_id
             OR pouch.recalled_at IS DISTINCT FROM terminal.committed_at
           )
       ) AS material_mismatch";

const CURRENT_HALL_SQL: &str = "SELECT EXISTS (
       SELECT 1
       FROM accounts AS account
       JOIN characters AS character
         ON character.namespace_id=account.namespace_id
        AND character.account_id=account.account_id
       JOIN character_world_locations AS world
         ON world.namespace_id=character.namespace_id
        AND world.account_id=character.account_id
        AND world.character_id=character.character_id
       JOIN character_inventories AS inventory
         ON inventory.namespace_id=character.namespace_id
        AND inventory.account_id=character.account_id
        AND inventory.character_id=character.character_id
       JOIN character_life_metrics AS life
         ON life.namespace_id=character.namespace_id
        AND life.account_id=character.account_id
        AND life.character_id=character.character_id
       JOIN character_progression AS progression
         ON progression.namespace_id=character.namespace_id
        AND progression.account_id=character.account_id
        AND progression.character_id=character.character_id
       JOIN character_oath_bargain_state AS oath
         ON oath.namespace_id=character.namespace_id
        AND oath.account_id=character.account_id
        AND oath.character_id=character.character_id
       JOIN ash_wallets AS ash
         ON ash.namespace_id=account.namespace_id
        AND ash.account_id=account.account_id
       WHERE account.namespace_id=$1 AND account.account_id=$2
         AND account.selected_character_id=$3 AND account.state_version=$4
         AND character.character_id=$3 AND character.life_state=0
         AND character.security_state=0 AND character.character_state_version=$5
         AND world.character_version=$6 AND world.location_kind=1
         AND world.location_content_id=$7 AND world.safe_arrival_kind=0
         AND world.safe_spawn_id IS NULL AND world.instance_lineage_id IS NULL
         AND world.entry_restore_point_id IS NULL
         AND inventory.inventory_version=$8
         AND life.life_metrics_version=$9 AND life.lifetime_ticks=$10
         AND life.permadeath_combat_ticks=$11
         AND progression.progression_version=$12
         AND oath.oath_bargain_version=$13 AND ash.wallet_version=$14
         AND NOT EXISTS (
           SELECT 1 FROM character_danger_checkpoints AS checkpoint
           WHERE checkpoint.namespace_id=character.namespace_id
             AND checkpoint.account_id=character.account_id
             AND checkpoint.character_id=character.character_id
         )
         AND NOT EXISTS (
           SELECT 1 FROM item_instances AS item
           WHERE item.namespace_id=character.namespace_id
             AND item.account_id=character.account_id
             AND item.character_id=character.character_id
             AND item.security_state IN (1,2)
         )
         AND NOT EXISTS (
           SELECT 1 FROM character_run_material_stacks AS pouch
           WHERE pouch.namespace_id=character.namespace_id
             AND pouch.account_id=character.account_id
             AND pouch.character_id=character.character_id
             AND pouch.security_state=2 AND pouch.quantity>0
         )
     ) AS owns_current_hall";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredCommittedRecallTerminalV1 {
    pub schema_version: u16,
    pub result: StoredProductionRecallResultV1,
    pub result_hash: [u8; 32],
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub content_revision: StoredWorldFlowRevisionV1,
    pub owns_current_hall: bool,
}

impl StoredCommittedRecallTerminalV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.schema_version != PRODUCTION_RECALL_RECOVERY_SCHEMA_VERSION
            || self.result_hash == [0; 32]
            || self.lineage_id == [0; 16]
            || self.restore_point_id == [0; 16]
            || !valid_revision(&self.content_revision)
        {
            return Err(corrupt());
        }
        self.result.validate()?;
        if self.result.digest()? != self.result_hash {
            return Err(corrupt());
        }
        Ok(())
    }
}

impl PostgresPersistence {
    /// Loads the latest immutable Recall result for a character.
    ///
    /// `owns_current_hall` is true only while the exact post-terminal aggregate still
    /// owns the selected live Hall actor. Historical results remain valid after later
    /// safe mutations or another danger entry.
    pub async fn load_committed_recall_terminal_v1(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<Option<StoredCommittedRecallTerminalV1>, PersistenceError> {
        load_public(self, account_id, character_id, None, None).await
    }

    /// Loads one exact immutable Recall result for response-loss or retry recovery.
    pub async fn load_committed_recall_terminal_by_identity_v1(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        mutation_id: [u8; 16],
        terminal_id: [u8; 16],
    ) -> Result<Option<StoredCommittedRecallTerminalV1>, PersistenceError> {
        if mutation_id == [0; 16] || terminal_id == [0; 16] || mutation_id == terminal_id {
            return Err(corrupt());
        }
        load_public(
            self,
            account_id,
            character_id,
            Some(mutation_id),
            Some(terminal_id),
        )
        .await
    }
}

async fn load_public(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
    mutation_id: Option<[u8; 16]>,
    terminal_id: Option<[u8; 16]>,
) -> Result<Option<StoredCommittedRecallTerminalV1>, PersistenceError> {
    if account_id == [0; 16]
        || character_id == [0; 16]
        || mutation_id.is_some() != terminal_id.is_some()
    {
        return Err(corrupt());
    }
    let mut transaction = persistence.begin_read_transaction().await?;
    let terminal = load_committed_recall_terminal_v1_on(
        transaction.connection(),
        account_id,
        character_id,
        mutation_id,
        terminal_id,
    )
    .await?;
    transaction.rollback().await?;
    Ok(terminal)
}

pub(crate) async fn load_committed_recall_terminal_v1_on(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    mutation_id: Option<[u8; 16]>,
    terminal_id: Option<[u8; 16]>,
) -> Result<Option<StoredCommittedRecallTerminalV1>, PersistenceError> {
    let row = sqlx::query(TERMINAL_ROOT_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(mutation_id.as_ref().map(<[u8; 16]>::as_slice))
        .bind(terminal_id.as_ref().map(<[u8; 16]>::as_slice))
        .fetch_optional(&mut *connection)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let mut terminal = terminal_from_root(&row, account_id, character_id)?;
    validate_immutable_graph(connection, &terminal).await?;
    terminal.owns_current_hall = load_current_hall_ownership(connection, &terminal).await?;
    Ok(Some(terminal))
}

#[allow(
    clippy::too_many_lines,
    reason = "the terminal root deliberately rechecks every stored result binding"
)]
fn terminal_from_root(
    row: &sqlx::postgres::PgRow,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<StoredCommittedRecallTerminalV1, PersistenceError> {
    let result = StoredProductionRecallResultV1::decode(&required_bytes(row, "result_payload")?)?;
    let result_hash = required_hash(row, "result_hash")?;
    let request_sequence = optional_positive_u32(row, "explicit_request_sequence")?;
    let explicit_client_tick = optional_positive_u64(row, "explicit_client_tick")?;
    let trigger = trigger_from_code(
        required_i16(row, "terminal_kind")?,
        required_i16(row, "trigger_kind")?,
    )?;
    if required_id(row, "account_id")? != account_id
        || required_id(row, "character_id")? != character_id
        || result.account_id != account_id
        || result.character_id != character_id
        || result.mutation_id != required_id(row, "mutation_id")?
        || result.terminal_id != required_id(row, "terminal_id")?
        || result.contract_version != positive_u16(required_i16(row, "contract_version")?)?
        || result.trigger != trigger
        || result.request_sequence != request_sequence
        || result.explicit_client_tick != explicit_client_tick
        || result.canonical_request_hash != required_hash(row, "canonical_request_hash")?
        || result.canonical_plan_hash != required_hash(row, "canonical_plan_hash")?
        || result.digest()? != result_hash
        || result.source_content_id != required_string(row, "source_content_id")?
        || result.destination_content_id != required_string(row, "destination_content_id")?
        || result.issued_at_unix_ms != positive_u64(required_i64(row, "issued_at_ms")?)?
        || result.trigger_started_tick != positive_u64(required_i64(row, "trigger_started_tick")?)?
        || result.completion_tick != positive_u64(required_i64(row, "completion_tick")?)?
        || result.committed_at_unix_ms != positive_u64(required_i64(row, "committed_at_ms")?)?
        || result.versions.account.pre != positive_u64(required_i64(row, "pre_account_version")?)?
        || result.versions.account.post != positive_u64(required_i64(row, "post_account_version")?)?
        || result.versions.character.pre
            != positive_u64(required_i64(row, "pre_character_version")?)?
        || result.versions.character.post
            != positive_u64(required_i64(row, "post_character_version")?)?
        || result.versions.world.pre != positive_u64(required_i64(row, "pre_world_version")?)?
        || result.versions.world.post != positive_u64(required_i64(row, "post_world_version")?)?
        || result.versions.inventory.pre
            != positive_u64(required_i64(row, "pre_inventory_version")?)?
        || result.versions.inventory.post
            != positive_u64(required_i64(row, "post_inventory_version")?)?
        || result.versions.life_metrics.pre
            != positive_u64(required_i64(row, "pre_life_metrics_version")?)?
        || result.versions.life_metrics.post
            != positive_u64(required_i64(row, "post_life_metrics_version")?)?
        || result.pre_lifetime_ticks != nonnegative_u64(required_i64(row, "pre_lifetime_ticks")?)?
        || result.post_lifetime_ticks != nonnegative_u64(required_i64(row, "post_lifetime_ticks")?)?
        || result.pre_permadeath_combat_ticks
            != nonnegative_u64(required_i64(row, "pre_permadeath_combat_ticks")?)?
        || result.post_permadeath_combat_ticks
            != nonnegative_u64(required_i64(row, "post_permadeath_combat_ticks")?)?
        || result.versions.progression.pre
            != positive_u64(required_i64(row, "preserved_progression_version")?)?
        || result.versions.oath_bargain.pre
            != positive_u64(required_i64(row, "preserved_oath_bargain_version")?)?
        || result.versions.ash_wallet.pre
            != positive_u64(required_i64(row, "preserved_ash_wallet_version")?)?
        || i64_from_usize(result.stabilized_items.len())?
            != i64::from(required_i16(row, "stabilized_item_count")?)
        || i64_from_usize(result.destroyed_items.len())?
            != i64::from(required_i32(row, "destroyed_item_count")?)
        || i64_from_usize(result.destroyed_materials.len())?
            != i64::from(required_i16(row, "destroyed_material_stack_count")?)
        || i16::from(result.result_code) != required_i16(row, "result_code")?
    {
        return Err(corrupt());
    }
    let terminal = StoredCommittedRecallTerminalV1 {
        schema_version: PRODUCTION_RECALL_RECOVERY_SCHEMA_VERSION,
        result,
        result_hash,
        lineage_id: required_id(row, "instance_lineage_id")?,
        restore_point_id: required_id(row, "entry_restore_point_id")?,
        content_revision: StoredWorldFlowRevisionV1 {
            records_blake3: required_string(row, "records_blake3")?,
            assets_blake3: required_string(row, "assets_blake3")?,
            localization_blake3: required_string(row, "localization_blake3")?,
        },
        owns_current_hall: false,
    };
    terminal.validate()?;
    Ok(terminal)
}

async fn validate_immutable_graph(
    connection: &mut PgConnection,
    terminal: &StoredCommittedRecallTerminalV1,
) -> Result<(), PersistenceError> {
    let result = &terminal.result;
    let event_type = recall_event_type(result.trigger);
    let rows = sqlx::query(IMMUTABLE_GRAPH_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(result.account_id.as_slice())
        .bind(result.character_id.as_slice())
        .bind(result.terminal_id.as_slice())
        .bind(event_type)
        .fetch_all(&mut *connection)
        .await?;
    let [row] = rows.as_slice() else {
        return Err(corrupt());
    };
    let outbox_result =
        StoredProductionRecallResultV1::decode(&required_bytes(row, "outbox_payload")?)?;
    if required_i16(row, "restore_state")? != RESTORE_RECALL_COMMITTED
        || required_id(row, "recall_terminal_id")? != result.terminal_id
        || !required_bool(row, "root_time_exact")?
        || required_string(row, "root_records_blake3")? != terminal.content_revision.records_blake3
        || required_string(row, "root_assets_blake3")? != terminal.content_revision.assets_blake3
        || required_string(row, "root_localization_blake3")?
            != terminal.content_revision.localization_blake3
        || required_i16(row, "lineage_state")? != LINEAGE_CLOSED_SUCCESS
        || required_string(row, "content_id")? != result.source_content_id
        || !required_bool(row, "lineage_time_exact")?
        || required_string(row, "lineage_records_blake3")?
            != terminal.content_revision.records_blake3
        || required_string(row, "lineage_assets_blake3")? != terminal.content_revision.assets_blake3
        || required_string(row, "lineage_localization_blake3")?
            != terminal.content_revision.localization_blake3
        || required_hash(row, "audit_digest")? != terminal.result_hash
        || !required_bool(row, "audit_time_exact")?
        || outbox_result != *result
        || !required_bool(row, "outbox_time_exact")?
    {
        return Err(corrupt());
    }

    let stabilized = load_stabilizations(connection, terminal).await?;
    let destroyed = load_destructions(connection, terminal).await?;
    let materials = load_materials(connection, terminal).await?;
    if stabilized != result.stabilized_items
        || destroyed != result.destroyed_items
        || materials != result.destroyed_materials
    {
        return Err(corrupt());
    }

    let integrity = sqlx::query(LEDGER_AND_CUSTODY_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(result.account_id.as_slice())
        .bind(result.character_id.as_slice())
        .bind(result.terminal_id.as_slice())
        .fetch_one(connection)
        .await?;
    let item_count = i64_from_usize(result.stabilized_items.len() + result.destroyed_items.len())?;
    if required_i64(&integrity, "item_ledgers")? != item_count
        || required_i64(&integrity, "destroyed_items")?
            != i64_from_usize(result.destroyed_items.len())?
        || required_i64(&integrity, "destroyed_materials")?
            != i64_from_usize(result.destroyed_materials.len())?
        || required_bool(&integrity, "stabilization_mismatch")?
        || required_bool(&integrity, "destruction_mismatch")?
        || required_bool(&integrity, "material_mismatch")?
    {
        return Err(corrupt());
    }
    Ok(())
}

async fn load_stabilizations(
    connection: &mut PgConnection,
    terminal: &StoredCommittedRecallTerminalV1,
) -> Result<Vec<StoredProductionRecallItemV1>, PersistenceError> {
    let result = &terminal.result;
    sqlx::query(STABILIZATIONS_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(result.account_id.as_slice())
        .bind(result.character_id.as_slice())
        .bind(result.terminal_id.as_slice())
        .fetch_all(connection)
        .await?
        .iter()
        .map(|row| {
            validate_projection_binding(row, result)?;
            let source_kind = required_i16(row, "source_kind")?;
            let slot = positive_u8(required_i16(row, "source_slot_index")?, true)?;
            let source = match source_kind {
                0 => StoredRecallLocationV1::Equipped(slot),
                1 => StoredRecallLocationV1::Belt(slot),
                _ => return Err(corrupt()),
            };
            Ok(StoredProductionRecallItemV1 {
                ordinal: nonnegative_u16(required_i16(row, "stabilization_ordinal")?)?,
                item_uid: required_id(row, "item_uid")?,
                template_id: required_string(row, "template_id")?,
                content_revision: required_string(row, "content_revision")?,
                item_kind: nonnegative_u8(required_i16(row, "item_kind")?)?,
                source,
                pre_item_version: positive_u64(required_i64(row, "pre_item_version")?)?,
                post_item_version: positive_u64(required_i64(row, "post_item_version")?)?,
                ledger_event_id: required_id(row, "ledger_event_id")?,
            })
        })
        .collect()
}

async fn load_destructions(
    connection: &mut PgConnection,
    terminal: &StoredCommittedRecallTerminalV1,
) -> Result<Vec<StoredProductionRecallItemV1>, PersistenceError> {
    let result = &terminal.result;
    sqlx::query(DESTRUCTIONS_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(result.account_id.as_slice())
        .bind(result.character_id.as_slice())
        .bind(result.terminal_id.as_slice())
        .fetch_all(connection)
        .await?
        .iter()
        .map(|row| {
            validate_projection_binding(row, result)?;
            let source = match required_i16(row, "source_kind")? {
                2 => {
                    if optional_bytes(row, "source_instance_id")?.is_some()
                        || optional_bytes(row, "source_pickup_id")?.is_some()
                        || optional_i64(row, "source_expires_at_tick")?.is_some()
                    {
                        return Err(corrupt());
                    }
                    StoredRecallLocationV1::RunBackpack(positive_u8(
                        required_i16(row, "source_slot_index")?,
                        true,
                    )?)
                }
                3 => {
                    if optional_i16(row, "source_slot_index")?.is_some() {
                        return Err(corrupt());
                    }
                    StoredRecallLocationV1::PersonalGround {
                        instance_id: optional_id(row, "source_instance_id")?.ok_or_else(corrupt)?,
                        pickup_id: optional_id(row, "source_pickup_id")?.ok_or_else(corrupt)?,
                        expires_at_tick: positive_u64(
                            optional_i64(row, "source_expires_at_tick")?.ok_or_else(corrupt)?,
                        )?,
                    }
                }
                _ => return Err(corrupt()),
            };
            Ok(StoredProductionRecallItemV1 {
                ordinal: nonnegative_u16_from_i32(required_i32(row, "destruction_ordinal")?)?,
                item_uid: required_id(row, "item_uid")?,
                template_id: required_string(row, "template_id")?,
                content_revision: required_string(row, "content_revision")?,
                item_kind: nonnegative_u8(required_i16(row, "item_kind")?)?,
                source,
                pre_item_version: positive_u64(required_i64(row, "pre_item_version")?)?,
                post_item_version: positive_u64(required_i64(row, "post_item_version")?)?,
                ledger_event_id: required_id(row, "ledger_event_id")?,
            })
        })
        .collect()
}

async fn load_materials(
    connection: &mut PgConnection,
    terminal: &StoredCommittedRecallTerminalV1,
) -> Result<Vec<StoredProductionRecallMaterialDestructionV1>, PersistenceError> {
    let result = &terminal.result;
    sqlx::query(MATERIALS_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(result.account_id.as_slice())
        .bind(result.character_id.as_slice())
        .bind(result.terminal_id.as_slice())
        .fetch_all(connection)
        .await?
        .iter()
        .map(|row| {
            validate_projection_binding(row, result)?;
            Ok(StoredProductionRecallMaterialDestructionV1 {
                ordinal: nonnegative_u8(required_i16(row, "destruction_ordinal")?)?,
                material_id: required_string(row, "material_id")?,
                destroyed_quantity: positive_u8_from_i32(required_i32(row, "destroyed_quantity")?)?,
                pre_pouch_version: positive_u64(required_i64(row, "pre_pouch_version")?)?,
                post_pouch_version: positive_u64(required_i64(row, "post_pouch_version")?)?,
                destruction_event_id: required_id(row, "destruction_event_id")?,
            })
        })
        .collect()
}

async fn load_current_hall_ownership(
    connection: &mut PgConnection,
    terminal: &StoredCommittedRecallTerminalV1,
) -> Result<bool, PersistenceError> {
    let result = &terminal.result;
    let row = sqlx::query(CURRENT_HALL_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(result.account_id.as_slice())
        .bind(result.character_id.as_slice())
        .bind(i64_value(result.versions.account.post)?)
        .bind(i64_value(result.versions.character.post)?)
        .bind(i64_value(result.versions.world.post)?)
        .bind(&result.destination_content_id)
        .bind(i64_value(result.versions.inventory.post)?)
        .bind(i64_value(result.versions.life_metrics.post)?)
        .bind(i64_value(result.post_lifetime_ticks)?)
        .bind(i64_value(result.post_permadeath_combat_ticks)?)
        .bind(i64_value(result.versions.progression.post)?)
        .bind(i64_value(result.versions.oath_bargain.post)?)
        .bind(i64_value(result.versions.ash_wallet.post)?)
        .fetch_one(connection)
        .await?;
    required_bool(&row, "owns_current_hall")
}

fn validate_projection_binding(
    row: &sqlx::postgres::PgRow,
    result: &StoredProductionRecallResultV1,
) -> Result<(), PersistenceError> {
    if required_id(row, "account_id")? != result.account_id
        || required_id(row, "character_id")? != result.character_id
        || required_id(row, "mutation_id")? != result.mutation_id
    {
        return Err(corrupt());
    }
    Ok(())
}

const fn recall_event_type(trigger: ProductionRecallTriggerV1) -> &'static str {
    match trigger {
        ProductionRecallTriggerV1::Explicit => "emergency_recall_committed",
        ProductionRecallTriggerV1::LinkLost => "disconnect_recovery_committed",
    }
}

fn trigger_from_code(
    terminal_kind: i16,
    trigger_kind: i16,
) -> Result<ProductionRecallTriggerV1, PersistenceError> {
    match (terminal_kind, trigger_kind) {
        (3, 0) => Ok(ProductionRecallTriggerV1::Explicit),
        (4, 1) => Ok(ProductionRecallTriggerV1::LinkLost),
        _ => Err(corrupt()),
    }
}

fn valid_revision(revision: &StoredWorldFlowRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .into_iter()
    .all(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn required_id(row: &sqlx::postgres::PgRow, column: &str) -> Result<[u8; 16], PersistenceError> {
    required_bytes(row, column)?
        .try_into()
        .map_err(|_| corrupt())
}

fn required_hash(row: &sqlx::postgres::PgRow, column: &str) -> Result<[u8; 32], PersistenceError> {
    required_bytes(row, column)?
        .try_into()
        .map_err(|_| corrupt())
}

fn required_bytes(row: &sqlx::postgres::PgRow, column: &str) -> Result<Vec<u8>, PersistenceError> {
    row.try_get::<Option<Vec<u8>>, _>(column)?
        .ok_or_else(corrupt)
}

fn optional_bytes(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<Vec<u8>>, PersistenceError> {
    row.try_get(column).map_err(Into::into)
}

fn optional_id(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<[u8; 16]>, PersistenceError> {
    optional_bytes(row, column)?
        .map(|value| value.try_into().map_err(|_| corrupt()))
        .transpose()
}

fn required_string(row: &sqlx::postgres::PgRow, column: &str) -> Result<String, PersistenceError> {
    row.try_get::<Option<String>, _>(column)?
        .ok_or_else(corrupt)
}

fn required_i16(row: &sqlx::postgres::PgRow, column: &str) -> Result<i16, PersistenceError> {
    row.try_get::<Option<i16>, _>(column)?.ok_or_else(corrupt)
}

fn optional_i16(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<i16>, PersistenceError> {
    row.try_get(column).map_err(Into::into)
}

fn required_i32(row: &sqlx::postgres::PgRow, column: &str) -> Result<i32, PersistenceError> {
    row.try_get::<Option<i32>, _>(column)?.ok_or_else(corrupt)
}

fn required_i64(row: &sqlx::postgres::PgRow, column: &str) -> Result<i64, PersistenceError> {
    row.try_get::<Option<i64>, _>(column)?.ok_or_else(corrupt)
}

fn optional_i64(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<i64>, PersistenceError> {
    row.try_get(column).map_err(Into::into)
}

fn required_bool(row: &sqlx::postgres::PgRow, column: &str) -> Result<bool, PersistenceError> {
    row.try_get::<Option<bool>, _>(column)?.ok_or_else(corrupt)
}

fn optional_positive_u32(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<u32>, PersistenceError> {
    optional_i64(row, column)?
        .map(|value| {
            u32::try_from(value)
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(corrupt)
        })
        .transpose()
}

fn optional_positive_u64(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<u64>, PersistenceError> {
    optional_i64(row, column)?.map(positive_u64).transpose()
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(corrupt)
}

fn nonnegative_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| corrupt())
}

fn positive_u16(value: i16) -> Result<u16, PersistenceError> {
    u16::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(corrupt)
}

fn nonnegative_u16(value: i16) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| corrupt())
}

fn nonnegative_u16_from_i32(value: i32) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| corrupt())
}

fn nonnegative_u8(value: i16) -> Result<u8, PersistenceError> {
    u8::try_from(value).map_err(|_| corrupt())
}

fn positive_u8(value: i16, allow_zero: bool) -> Result<u8, PersistenceError> {
    let value = u8::try_from(value).map_err(|_| corrupt())?;
    if !allow_zero && value == 0 {
        return Err(corrupt());
    }
    Ok(value)
}

fn positive_u8_from_i32(value: i32) -> Result<u8, PersistenceError> {
    u8::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(corrupt)
}

fn i64_from_usize(value: usize) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| corrupt())
}

fn i64_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| corrupt())
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredRecall
}

#[cfg(test)]
mod tests {
    use crate::{
        ProductionRecallVersionAdvanceV1, ProductionRecallVersionsV1, StoredProductionRecallItemV1,
        StoredProductionRecallMaterialDestructionV1, canonical_production_recall_plan_hash_v1,
    };

    use super::*;

    fn result() -> StoredProductionRecallResultV1 {
        let stabilized_items = vec![StoredProductionRecallItemV1 {
            ordinal: 0,
            item_uid: [11; 16],
            template_id: "item.weapon.test".into(),
            content_revision: "core.items.v1".into(),
            item_kind: 0,
            source: StoredRecallLocationV1::Equipped(0),
            pre_item_version: 1,
            post_item_version: 2,
            ledger_event_id: [12; 16],
        }];
        let destroyed_items = vec![StoredProductionRecallItemV1 {
            ordinal: 0,
            item_uid: [13; 16],
            template_id: "item.armor.test".into(),
            content_revision: "core.items.v1".into(),
            item_kind: 0,
            source: StoredRecallLocationV1::RunBackpack(0),
            pre_item_version: 1,
            post_item_version: 2,
            ledger_event_id: [14; 16],
        }];
        let destroyed_materials = vec![StoredProductionRecallMaterialDestructionV1 {
            ordinal: 0,
            material_id: "material.bell_brass".into(),
            destroyed_quantity: 2,
            pre_pouch_version: 1,
            post_pouch_version: 2,
            destruction_event_id: [15; 16],
        }];
        let canonical_plan_hash = canonical_production_recall_plan_hash_v1(
            &stabilized_items,
            &destroyed_items,
            &destroyed_materials,
        )
        .unwrap();
        StoredProductionRecallResultV1 {
            contract_version: crate::PRODUCTION_RECALL_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            terminal_id: [4; 16],
            canonical_request_hash: [5; 32],
            canonical_plan_hash,
            result_code: 1,
            trigger: ProductionRecallTriggerV1::Explicit,
            request_sequence: Some(1),
            explicit_client_tick: Some(2),
            issued_at_unix_ms: 10,
            trigger_started_tick: 20,
            completion_tick: 32,
            committed_at_unix_ms: 40,
            source_content_id: "world.core_microrealm_01".into(),
            destination_content_id: crate::PRODUCTION_RECALL_HALL_ID.into(),
            versions: ProductionRecallVersionsV1 {
                account: ProductionRecallVersionAdvanceV1 { pre: 1, post: 1 },
                character: ProductionRecallVersionAdvanceV1 { pre: 2, post: 3 },
                world: ProductionRecallVersionAdvanceV1 { pre: 2, post: 3 },
                inventory: ProductionRecallVersionAdvanceV1 { pre: 3, post: 4 },
                life_metrics: ProductionRecallVersionAdvanceV1 { pre: 4, post: 5 },
                progression: ProductionRecallVersionAdvanceV1 { pre: 6, post: 6 },
                oath_bargain: ProductionRecallVersionAdvanceV1 { pre: 7, post: 7 },
                ash_wallet: ProductionRecallVersionAdvanceV1 { pre: 8, post: 8 },
            },
            pre_lifetime_ticks: 100,
            post_lifetime_ticks: 112,
            pre_permadeath_combat_ticks: 80,
            post_permadeath_combat_ticks: 92,
            stabilized_items,
            destroyed_items,
            destroyed_materials,
        }
    }

    fn terminal() -> StoredCommittedRecallTerminalV1 {
        let result = result();
        StoredCommittedRecallTerminalV1 {
            schema_version: PRODUCTION_RECALL_RECOVERY_SCHEMA_VERSION,
            result_hash: result.digest().unwrap(),
            result,
            lineage_id: [8; 16],
            restore_point_id: [9; 16],
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: "1".repeat(64),
                assets_blake3: "2".repeat(64),
                localization_blake3: "3".repeat(64),
            },
            owns_current_hall: true,
        }
    }

    #[test]
    fn strict_projection_accepts_only_complete_hash_bound_authority() {
        let terminal = terminal();
        terminal.validate().unwrap();

        let mut wrong_schema = terminal.clone();
        wrong_schema.schema_version += 1;
        assert!(matches!(
            wrong_schema.validate(),
            Err(PersistenceError::CorruptStoredRecall)
        ));

        let mut wrong_hash = terminal.clone();
        wrong_hash.result_hash = [99; 32];
        assert!(matches!(
            wrong_hash.validate(),
            Err(PersistenceError::CorruptStoredRecall)
        ));

        let mut no_lineage = terminal;
        no_lineage.lineage_id = [0; 16];
        assert!(matches!(
            no_lineage.validate(),
            Err(PersistenceError::CorruptStoredRecall)
        ));
    }

    #[test]
    fn recovery_queries_cover_immutable_and_current_authorities() {
        for required in [
            "FROM character_recall_terminal_results_v1",
            "explicit_client_tick",
            "JOIN character_entry_restore_points AS root",
            "JOIN character_instance_lineages AS lineage",
            "recall_terminal_audit_events_v1",
            "recall_terminal_outbox_events_v1",
            "recall_terminal_item_stabilizations_v1",
            "recall_terminal_item_destructions_v1",
            "recall_terminal_material_destructions_v1",
            "item_ledger_events",
            "character_run_material_stacks",
            "character_danger_checkpoints",
        ] {
            assert!(
                [
                    TERMINAL_ROOT_SQL,
                    IMMUTABLE_GRAPH_SQL,
                    STABILIZATIONS_SQL,
                    DESTRUCTIONS_SQL,
                    MATERIALS_SQL,
                    LEDGER_AND_CUSTODY_SQL,
                    CURRENT_HALL_SQL,
                ]
                .iter()
                .any(|query| query.contains(required)),
                "{required}"
            );
        }
    }
}
