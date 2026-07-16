//! Strict read-only recovery for one committed production extraction.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-011`, `LOOT-002`,
//! and `TECH-015`/`021`-`023`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-BOSS-001` and `CONT-HUB-001`/`002`; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08`, plus accepted
//! `SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md`.
//!
//! Recovery reads the immutable terminal result and proves its current Hall projection, closed
//! danger authority, schema-26 compatibility projection, audit, outbox, placements, and material
//! ledgers. It never repairs state or becomes a second gameplay writer.

use sqlx::{PgConnection, Row};

use crate::{
    PersistenceError, PostgresPersistence, StoredProductionExtractionResultV1,
    WIPEABLE_CORE_NAMESPACE,
};

pub const PRODUCTION_EXTRACTION_RECOVERY_SCHEMA_VERSION: u16 = 1;

const LOCATION_SAFE: i16 = 1;
const RESTORE_EXTRACTION_COMMITTED: i16 = 1;
const LINEAGE_CLOSED_SUCCESS: i16 = 2;
const EXTRACTION_COMMITTED: i16 = 1;
const PRODUCTION_AUTHORITY: i16 = 1;

const TERMINAL_ROOT_SQL: &str =
    "SELECT account_id,character_id,mutation_id,terminal_id,extraction_request_id,
            extraction_receipt_id,result_hash,result_payload,encounter_id,
            instance_lineage_id,entry_restore_point_id,exit_instance_id,
            placement_count,material_credit_count
     FROM character_extraction_terminal_results_v1
     WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
       AND ($4::bytea IS NULL OR extraction_request_id=$4)
       AND ($5::bytea IS NULL OR extraction_receipt_id=$5)
     ORDER BY committed_at DESC,terminal_id DESC
     LIMIT 1";

const TERMINAL_GRAPH_SQL: &str = "SELECT account.state_version,
            character.life_state,character.security_state,
            character.character_state_version,
            world.character_version AS world_version,
            world.location_kind,world.location_content_id,world.safe_arrival_kind,
            world.instance_lineage_id AS world_lineage_id,
            world.entry_restore_point_id AS world_restore_point_id,
            inventory.inventory_version,life.life_metrics_version,
            root.restore_state,root.extraction_terminal_id,
            lineage.lineage_state,
            seam.extraction_receipt_id AS seam_receipt_id,
            seam.receipt_payload_hash AS seam_result_hash,
            seam.extraction_state,seam.authority_kind,seam.destination_content_id,
            seam.safe_arrival_kind AS seam_safe_arrival_kind,
            seam.transfer_mutation_id,seam.post_character_version,
            seam.production_mutation_id,
            (seam.transferred_at=seam.committed_at) AS seam_transfer_time_exact,
            audit.event_digest AS audit_digest,
            outbox.event_payload AS outbox_payload
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
     JOIN character_entry_restore_points AS root
       ON root.namespace_id=character.namespace_id
      AND root.account_id=character.account_id
      AND root.character_id=character.character_id
      AND root.restore_point_id=$4
      AND root.lineage_id=$5
     JOIN character_instance_lineages AS lineage
       ON lineage.namespace_id=character.namespace_id
      AND lineage.account_id=character.account_id
      AND lineage.character_id=character.character_id
      AND lineage.lineage_id=$5
     JOIN character_extraction_results AS seam
       ON seam.namespace_id=character.namespace_id
      AND seam.account_id=character.account_id
      AND seam.character_id=character.character_id
      AND seam.extraction_request_id=$6
     LEFT JOIN extraction_terminal_audit_events_v1 AS audit
       ON audit.namespace_id=character.namespace_id
      AND audit.account_id=character.account_id
      AND audit.character_id=character.character_id
      AND audit.terminal_id=$7
      AND audit.event_type='extraction_committed'
     LEFT JOIN extraction_terminal_outbox_events_v1 AS outbox
       ON outbox.namespace_id=character.namespace_id
      AND outbox.account_id=character.account_id
      AND outbox.character_id=character.character_id
      AND outbox.terminal_id=$7
      AND outbox.event_type='extraction_committed'
     WHERE account.namespace_id=$1 AND account.account_id=$2
       AND account.selected_character_id=$3
       AND character.character_id=$3
     LIMIT 2";

const TERMINAL_COUNTS_SQL: &str = "SELECT
       (SELECT count(*) FROM extraction_terminal_item_placements_v1
        WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND terminal_id=$4)
            AS placements,
       (SELECT count(*) FROM item_ledger_events
        WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
          AND terminal_extraction_id=$4) AS item_ledgers,
       (SELECT count(*) FROM extraction_terminal_material_credits_v1
        WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND terminal_id=$4)
            AS material_credits,
       (SELECT count(*) FROM account_material_ledger_events_v1
        WHERE namespace_id=$1 AND account_id=$2 AND terminal_id=$4) AS material_ledgers";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredCommittedExtractionTerminalV1 {
    pub schema_version: u16,
    pub result: StoredProductionExtractionResultV1,
    pub result_hash: [u8; 32],
    pub encounter_id: [u8; 16],
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub exit_instance_id: [u8; 16],
}

impl StoredCommittedExtractionTerminalV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.schema_version != PRODUCTION_EXTRACTION_RECOVERY_SCHEMA_VERSION
            || [
                self.encounter_id,
                self.lineage_id,
                self.restore_point_id,
                self.exit_instance_id,
            ]
            .contains(&[0; 16])
            || self.result_hash == [0; 32]
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
    /// Loads the latest committed extraction that still owns this selected character's Hall state.
    pub async fn load_committed_extraction_terminal_v1(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<Option<StoredCommittedExtractionTerminalV1>, PersistenceError> {
        load_public(self, account_id, character_id, None, None).await
    }

    /// Loads one exact production result for compatibility transfer replay.
    pub async fn load_committed_extraction_terminal_by_identity_v1(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        extraction_request_id: [u8; 16],
        extraction_receipt_id: [u8; 16],
    ) -> Result<Option<StoredCommittedExtractionTerminalV1>, PersistenceError> {
        if extraction_request_id == [0; 16]
            || extraction_receipt_id == [0; 16]
            || extraction_request_id == extraction_receipt_id
        {
            return Err(corrupt());
        }
        load_public(
            self,
            account_id,
            character_id,
            Some(extraction_request_id),
            Some(extraction_receipt_id),
        )
        .await
    }
}

async fn load_public(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
    extraction_request_id: Option<[u8; 16]>,
    extraction_receipt_id: Option<[u8; 16]>,
) -> Result<Option<StoredCommittedExtractionTerminalV1>, PersistenceError> {
    if account_id == [0; 16]
        || character_id == [0; 16]
        || extraction_request_id.is_some() != extraction_receipt_id.is_some()
    {
        return Err(corrupt());
    }
    let mut transaction = persistence.begin_read_transaction().await?;
    let terminal = load_committed_extraction_terminal_v1_on(
        transaction.connection(),
        account_id,
        character_id,
        extraction_request_id,
        extraction_receipt_id,
    )
    .await?;
    transaction.rollback().await?;
    Ok(terminal)
}

pub(crate) async fn load_committed_extraction_terminal_v1_on(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    extraction_request_id: Option<[u8; 16]>,
    extraction_receipt_id: Option<[u8; 16]>,
) -> Result<Option<StoredCommittedExtractionTerminalV1>, PersistenceError> {
    let row = sqlx::query(TERMINAL_ROOT_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(extraction_request_id.as_ref().map(<[u8; 16]>::as_slice))
        .bind(extraction_receipt_id.as_ref().map(<[u8; 16]>::as_slice))
        .fetch_optional(&mut *connection)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let terminal = terminal_from_root(&row, account_id, character_id)?;
    validate_committed_graph(connection, &terminal).await?;
    Ok(Some(terminal))
}

fn terminal_from_root(
    row: &sqlx::postgres::PgRow,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<StoredCommittedExtractionTerminalV1, PersistenceError> {
    let result =
        StoredProductionExtractionResultV1::decode(&required_bytes(row, "result_payload")?)?;
    let result_hash = required_hash(row, "result_hash")?;
    if required_id(row, "account_id")? != account_id
        || required_id(row, "character_id")? != character_id
        || result.account_id != account_id
        || result.character_id != character_id
        || result.mutation_id != required_id(row, "mutation_id")?
        || result.terminal_id != required_id(row, "terminal_id")?
        || result.extraction_request_id != required_id(row, "extraction_request_id")?
        || result.extraction_receipt_id != required_id(row, "extraction_receipt_id")?
        || result.digest()? != result_hash
        || i64_from_usize(result.placements.len())?
            != i64::from(required_i16(row, "placement_count")?)
        || i64_from_usize(result.material_credits.len())?
            != i64::from(required_i16(row, "material_credit_count")?)
    {
        return Err(corrupt());
    }
    let terminal = StoredCommittedExtractionTerminalV1 {
        schema_version: PRODUCTION_EXTRACTION_RECOVERY_SCHEMA_VERSION,
        result,
        result_hash,
        encounter_id: required_id(row, "encounter_id")?,
        lineage_id: required_id(row, "instance_lineage_id")?,
        restore_point_id: required_id(row, "entry_restore_point_id")?,
        exit_instance_id: required_id(row, "exit_instance_id")?,
    };
    terminal.validate()?;
    Ok(terminal)
}

async fn validate_committed_graph(
    connection: &mut PgConnection,
    terminal: &StoredCommittedExtractionTerminalV1,
) -> Result<(), PersistenceError> {
    let result = &terminal.result;
    let rows = sqlx::query(TERMINAL_GRAPH_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(result.account_id.as_slice())
        .bind(result.character_id.as_slice())
        .bind(terminal.restore_point_id.as_slice())
        .bind(terminal.lineage_id.as_slice())
        .bind(result.extraction_request_id.as_slice())
        .bind(result.terminal_id.as_slice())
        .fetch_all(&mut *connection)
        .await?;
    let [row] = rows.as_slice() else {
        return Err(corrupt());
    };
    let outbox_result =
        StoredProductionExtractionResultV1::decode(&required_bytes(row, "outbox_payload")?)?;
    if positive_u64(row.try_get("state_version")?)? != result.versions.account.post
        || required_i16(row, "life_state")? != 0
        || required_i16(row, "security_state")? != i16::from(result.storage_resolution_required)
        || positive_u64(row.try_get("character_state_version")?)? != result.versions.character.post
        || positive_u64(row.try_get("world_version")?)? != result.versions.world.post
        || required_i16(row, "location_kind")? != LOCATION_SAFE
        || required_string(row, "location_content_id")? != crate::PRODUCTION_EXTRACTION_HALL_ID
        || required_i16(row, "safe_arrival_kind")? != 0
        || optional_bytes(row, "world_lineage_id")?.is_some()
        || optional_bytes(row, "world_restore_point_id")?.is_some()
        || positive_u64(row.try_get("inventory_version")?)? != result.versions.inventory.post
        || positive_u64(row.try_get("life_metrics_version")?)? != result.versions.life_metrics.post
        || required_i16(row, "restore_state")? != RESTORE_EXTRACTION_COMMITTED
        || required_id(row, "extraction_terminal_id")? != result.terminal_id
        || required_i16(row, "lineage_state")? != LINEAGE_CLOSED_SUCCESS
        || required_id(row, "seam_receipt_id")? != result.extraction_receipt_id
        || required_hash(row, "seam_result_hash")? != terminal.result_hash
        || required_i16(row, "extraction_state")? != EXTRACTION_COMMITTED
        || required_i16(row, "authority_kind")? != PRODUCTION_AUTHORITY
        || required_string(row, "destination_content_id")? != crate::PRODUCTION_EXTRACTION_HALL_ID
        || required_i16(row, "seam_safe_arrival_kind")? != 0
        || required_id(row, "transfer_mutation_id")? != result.mutation_id
        || positive_u64(row.try_get("post_character_version")?)? != result.versions.character.post
        || required_id(row, "production_mutation_id")? != result.mutation_id
        || !required_bool(row, "seam_transfer_time_exact")?
        || required_hash(row, "audit_digest")? != terminal.result_hash
        || outbox_result != *result
    {
        return Err(corrupt());
    }

    let counts = sqlx::query(TERMINAL_COUNTS_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(result.account_id.as_slice())
        .bind(result.character_id.as_slice())
        .bind(result.terminal_id.as_slice())
        .fetch_one(connection)
        .await?;
    let placements = i64_from_usize(result.placements.len())?;
    let material_credits = i64_from_usize(result.material_credits.len())?;
    if required_i64(&counts, "placements")? != placements
        || required_i64(&counts, "item_ledgers")? != placements
        || required_i64(&counts, "material_credits")? != material_credits
        || required_i64(&counts, "material_ledgers")? != material_credits
    {
        return Err(corrupt());
    }
    Ok(())
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

fn required_string(row: &sqlx::postgres::PgRow, column: &str) -> Result<String, PersistenceError> {
    row.try_get::<Option<String>, _>(column)?
        .ok_or_else(corrupt)
}

fn required_i16(row: &sqlx::postgres::PgRow, column: &str) -> Result<i16, PersistenceError> {
    row.try_get::<Option<i16>, _>(column)?.ok_or_else(corrupt)
}

fn required_i64(row: &sqlx::postgres::PgRow, column: &str) -> Result<i64, PersistenceError> {
    row.try_get::<Option<i64>, _>(column)?.ok_or_else(corrupt)
}

fn required_bool(row: &sqlx::postgres::PgRow, column: &str) -> Result<bool, PersistenceError> {
    row.try_get::<Option<bool>, _>(column)?.ok_or_else(corrupt)
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(corrupt)
}

fn i64_from_usize(value: usize) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| corrupt())
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredExtraction
}

#[cfg(test)]
mod tests {
    use crate::{
        ProductionExtractionVersionAdvanceV1, ProductionExtractionVersionsV1,
        StoredExtractionLocationV1, StoredProductionExtractionPlacementV1,
    };

    use super::*;

    fn result() -> StoredProductionExtractionResultV1 {
        let placements = vec![StoredProductionExtractionPlacementV1 {
            ordinal: 0,
            item_uid: [11; 16],
            template_id: "item.weapon.test".into(),
            item_kind: 0,
            source: StoredExtractionLocationV1::Equipped(0),
            destination: StoredExtractionLocationV1::Equipped(0),
            pre_item_version: 1,
            post_item_version: 2,
            ledger_event_id: [12; 16],
        }];
        let canonical_plan_hash =
            crate::canonical_production_extraction_plan_hash_v1(&placements, &[]).unwrap();
        StoredProductionExtractionResultV1 {
            contract_version: crate::PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            terminal_id: [4; 16],
            extraction_request_id: [5; 16],
            extraction_receipt_id: [6; 16],
            canonical_request_hash: [7; 32],
            canonical_plan_hash,
            result_code: 1,
            issued_at_unix_ms: 10,
            observed_tick: 20,
            committed_at_unix_ms: 30,
            destination_content_id: crate::PRODUCTION_EXTRACTION_HALL_ID.into(),
            versions: ProductionExtractionVersionsV1 {
                account: ProductionExtractionVersionAdvanceV1 { pre: 1, post: 1 },
                character: ProductionExtractionVersionAdvanceV1 { pre: 2, post: 3 },
                world: ProductionExtractionVersionAdvanceV1 { pre: 2, post: 3 },
                inventory: ProductionExtractionVersionAdvanceV1 { pre: 3, post: 4 },
                life_metrics: ProductionExtractionVersionAdvanceV1 { pre: 4, post: 5 },
            },
            placements,
            material_credits: Vec::new(),
            storage_resolution_required: false,
        }
    }

    fn terminal() -> StoredCommittedExtractionTerminalV1 {
        let result = result();
        StoredCommittedExtractionTerminalV1 {
            schema_version: PRODUCTION_EXTRACTION_RECOVERY_SCHEMA_VERSION,
            result_hash: result.digest().unwrap(),
            result,
            encounter_id: [8; 16],
            lineage_id: [9; 16],
            restore_point_id: [10; 16],
            exit_instance_id: [13; 16],
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
            Err(PersistenceError::CorruptStoredExtraction)
        ));

        let mut wrong_hash = terminal.clone();
        wrong_hash.result_hash = [99; 32];
        assert!(matches!(
            wrong_hash.validate(),
            Err(PersistenceError::CorruptStoredExtraction)
        ));

        let mut no_lineage = terminal;
        no_lineage.lineage_id = [0; 16];
        assert!(matches!(
            no_lineage.validate(),
            Err(PersistenceError::CorruptStoredExtraction)
        ));
    }

    #[test]
    fn recovery_queries_cover_terminal_hall_and_evidence_graphs() {
        for required in [
            "FROM character_extraction_terminal_results_v1",
            "JOIN character_world_locations AS world",
            "JOIN character_entry_restore_points AS root",
            "JOIN character_instance_lineages AS lineage",
            "JOIN character_extraction_results AS seam",
            "extraction_terminal_audit_events_v1",
            "extraction_terminal_outbox_events_v1",
            "extraction_terminal_item_placements_v1",
            "account_material_ledger_events_v1",
        ] {
            assert!(
                TERMINAL_ROOT_SQL.contains(required)
                    || TERMINAL_GRAPH_SQL.contains(required)
                    || TERMINAL_COUNTS_SQL.contains(required),
                "{required}"
            );
        }
    }
}
