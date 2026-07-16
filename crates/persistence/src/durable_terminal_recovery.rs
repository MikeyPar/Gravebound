//! Read-only recovery projection for one committed durable-death terminal.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-001`, `TECH-015`,
//! `TECH-021`, `TECH-022`, and `TECH-023`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-ECHO-009`, `CONT-BOSS-005`, and `CONT-HUB-002`; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-06`, `GB-M03-08`, `GB-M03-13`,
//! plus the M03 restart, atomicity, and nonduplication gates.
//!
//! The projection derives restart authority from the existing committed graph. It does not add
//! another receipt writer or permit repair: the immutable mutation result, death event, and
//! terminal danger restore root must agree exactly or the read fails closed.

use sqlx::{PgConnection, Row};

use crate::durable_death_repository::load_strict_stored_trace_promotion_v1;
use crate::{
    DURABLE_DEATH_CONTRACT, PersistenceError, PostgresPersistence, StoredCommittedDeathResultV1,
    WIPEABLE_CORE_NAMESPACE, canonical_death_terminal_payload_hash_v1,
};

pub const DURABLE_TERMINAL_RECOVERY_SCHEMA_VERSION: u16 = 1;

const DEATH_RESULT_COMMITTED: i16 = 1;
const RESTORE_STATE_DEATH_COMMITTED: i16 = 2;

const COMMITTED_DEATH_TERMINAL_SQL: &str = "SELECT mutation.account_id AS mutation_account_id, \
            mutation.character_id AS mutation_character_id, \
            mutation.mutation_id AS mutation_mutation_id, \
            mutation.death_id AS mutation_death_id, \
            mutation.contract_kind AS mutation_contract_kind, \
            mutation.canonical_request_hash AS mutation_request_hash, \
            mutation.result_code AS mutation_result_code, \
            mutation.result_payload AS mutation_result_payload, \
            mutation.result_hash AS mutation_result_hash, \
            floor(extract(epoch FROM mutation.issued_at) * 1000)::bigint \
                AS mutation_issued_at_unix_ms, \
            floor(extract(epoch FROM mutation.committed_at) * 1000)::bigint \
                AS mutation_committed_at_unix_ms, \
            death.account_id AS death_account_id, \
            death.character_id AS death_character_id, \
            death.mutation_id AS death_mutation_id, \
            death.death_id AS death_death_id, \
            death.contract_kind AS death_contract_kind, \
            death.canonical_request_hash AS death_request_hash, \
            death.lineage_id AS death_lineage_id, \
            death.restore_point_id AS death_restore_point_id, \
            death.death_tick AS death_tick, \
            death.pre_account_version AS death_pre_account_version, \
            death.post_account_version AS death_post_account_version, \
            floor(extract(epoch FROM death.committed_at) * 1000)::bigint \
                AS death_committed_at_unix_ms, \
            root.account_id AS root_account_id, \
            root.character_id AS root_character_id, \
            root.lineage_id AS root_lineage_id, \
            root.restore_point_id AS root_restore_point_id, \
            root.restore_state AS root_restore_state, \
            root.death_mutation_id AS root_death_mutation_id, \
            promotion.account_id AS promotion_account_id, \
            promotion.character_id AS promotion_character_id, \
            promotion.promotion_digest AS promotion_digest, \
            promotion.terminal_payload_hash AS promotion_terminal_payload_hash \
     FROM death_mutation_results AS mutation \
     FULL OUTER JOIN death_events AS death \
       ON death.namespace_id=mutation.namespace_id \
      AND death.account_id=mutation.account_id \
      AND death.character_id=mutation.character_id \
      AND death.death_id=mutation.death_id \
     LEFT JOIN character_entry_restore_points AS root \
       ON root.namespace_id=death.namespace_id \
      AND root.account_id=death.account_id \
      AND root.character_id=death.character_id \
      AND root.lineage_id=death.lineage_id \
      AND root.restore_point_id=death.restore_point_id \
     LEFT JOIN death_live_trace_sets_v1 AS promotion \
       ON promotion.namespace_id=death.namespace_id \
      AND promotion.death_id=death.death_id \
     WHERE coalesce(mutation.namespace_id,death.namespace_id)=$1 \
       AND coalesce(mutation.account_id,death.account_id)=$2 \
       AND coalesce(mutation.character_id,death.character_id)=$3 \
       AND coalesce(mutation.contract_kind,death.contract_kind)=$4 \
     LIMIT 2";

/// Strict server reconstruction material. `server_app` can map it to its versioned terminal
/// receipt without depending on database rows or accepting client-authored authority.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredCommittedDeathTerminalV1 {
    pub schema_version: u16,
    pub result: StoredCommittedDeathResultV1,
    pub result_hash: [u8; 32],
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub death_tick: u64,
    pub promotion_digest: [u8; 32],
    pub terminal_payload_hash: [u8; 32],
}

impl StoredCommittedDeathTerminalV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.schema_version != DURABLE_TERMINAL_RECOVERY_SCHEMA_VERSION
            || self.lineage_id == [0; 16]
            || self.restore_point_id == [0; 16]
            || self.death_tick == 0
            || self.promotion_digest == [0; 32]
            || self.terminal_payload_hash == [0; 32]
        {
            return Err(corrupt());
        }
        self.result.validate()?;
        if self.result.digest()? != self.result_hash {
            return Err(corrupt());
        }
        if canonical_death_terminal_payload_hash_v1(
            self.result.canonical_request_hash,
            self.promotion_digest,
        )? != self.terminal_payload_hash
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

impl PostgresPersistence {
    /// Loads final death authority for lazy process-restart reconstruction.
    ///
    /// `Ok(None)` means the authenticated character has no committed permadeath. A partial,
    /// mismatched, unknown-version, or corrupt graph returns an error rather than masquerading as
    /// an absent outcome.
    pub async fn load_committed_death_terminal_v1(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<Option<StoredCommittedDeathTerminalV1>, PersistenceError> {
        if account_id == [0; 16] || character_id == [0; 16] {
            return Err(PersistenceError::DurableDeathBindingMismatch);
        }
        let mut transaction = self.begin_read_transaction().await?;
        let terminal =
            load_committed_death_terminal_v1_on(transaction.connection(), account_id, character_id)
                .await?;
        transaction.rollback().await?;
        Ok(terminal)
    }
}

/// Connection-scoped form used by composite audit readers that must retain one serializable
/// snapshot across terminal recovery and every dependent row family.
pub(crate) async fn load_committed_death_terminal_v1_on(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Option<StoredCommittedDeathTerminalV1>, PersistenceError> {
    let rows = sqlx::query(COMMITTED_DEATH_TERMINAL_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(DURABLE_DEATH_CONTRACT)
        .fetch_all(&mut *connection)
        .await?;
    let [row] = rows.as_slice() else {
        return if rows.is_empty() {
            Ok(None)
        } else {
            Err(corrupt())
        };
    };
    let terminal = terminal_from_row(row, account_id, character_id)?;
    let promotion = load_strict_stored_trace_promotion_v1(
        connection,
        WIPEABLE_CORE_NAMESPACE,
        terminal.result.death_id,
        terminal.result.canonical_request_hash,
    )
    .await?
    .ok_or_else(corrupt)?;
    if promotion.account_id != account_id
        || promotion.character_id != character_id
        || promotion.promotion_digest != terminal.promotion_digest
        || promotion.terminal_payload_hash != terminal.terminal_payload_hash
    {
        return Err(corrupt());
    }
    Ok(Some(terminal))
}

fn terminal_from_row(
    row: &sqlx::postgres::PgRow,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<StoredCommittedDeathTerminalV1, PersistenceError> {
    let mutation_account = required_id(row, "mutation_account_id")?;
    let mutation_character = required_id(row, "mutation_character_id")?;
    let mutation_id = required_id(row, "mutation_mutation_id")?;
    let mutation_death_id = required_id(row, "mutation_death_id")?;
    let mutation_contract = required_string(row, "mutation_contract_kind")?;
    let mutation_request_hash = required_hash(row, "mutation_request_hash")?;
    let mutation_result_code = required_i16(row, "mutation_result_code")?;
    let result_payload = required_bytes(row, "mutation_result_payload")?;
    let result_hash = required_hash(row, "mutation_result_hash")?;
    let mutation_issued_at = required_positive_u64(row, "mutation_issued_at_unix_ms")?;
    let mutation_committed_at = required_positive_u64(row, "mutation_committed_at_unix_ms")?;

    let death_account = required_id(row, "death_account_id")?;
    let death_character = required_id(row, "death_character_id")?;
    let death_mutation_id = required_id(row, "death_mutation_id")?;
    let death_id = required_id(row, "death_death_id")?;
    let death_contract = required_string(row, "death_contract_kind")?;
    let death_request_hash = required_hash(row, "death_request_hash")?;
    let lineage_id = required_id(row, "death_lineage_id")?;
    let restore_point_id = required_id(row, "death_restore_point_id")?;
    let death_tick = required_positive_u64(row, "death_tick")?;
    let pre_account_version = required_positive_u64(row, "death_pre_account_version")?;
    let post_account_version = required_positive_u64(row, "death_post_account_version")?;
    let death_committed_at = required_positive_u64(row, "death_committed_at_unix_ms")?;

    let root_account = required_id(row, "root_account_id")?;
    let root_character = required_id(row, "root_character_id")?;
    let root_lineage = required_id(row, "root_lineage_id")?;
    let root_restore_point = required_id(row, "root_restore_point_id")?;
    let root_state = required_i16(row, "root_restore_state")?;
    let root_death_mutation = required_id(row, "root_death_mutation_id")?;
    let promotion_account = required_id(row, "promotion_account_id")?;
    let promotion_character = required_id(row, "promotion_character_id")?;
    let promotion_digest = required_hash(row, "promotion_digest")?;
    let terminal_payload_hash = required_hash(row, "promotion_terminal_payload_hash")?;

    let result = StoredCommittedDeathResultV1::decode(&result_payload)?;
    result.validate()?;
    if mutation_account != account_id
        || mutation_character != character_id
        || mutation_contract != DURABLE_DEATH_CONTRACT
        || mutation_result_code != DEATH_RESULT_COMMITTED
        || result.account_id != mutation_account
        || result.character_id != mutation_character
        || result.mutation_id != mutation_id
        || result.death_id != mutation_death_id
        || result.contract != mutation_contract
        || result.canonical_request_hash != mutation_request_hash
        || result.issued_at_unix_ms != mutation_issued_at
        || result.committed_at_unix_ms != mutation_committed_at
        || result.digest()? != result_hash
        || death_account != mutation_account
        || death_character != mutation_character
        || death_mutation_id != mutation_id
        || death_id != mutation_death_id
        || death_contract != mutation_contract
        || death_request_hash != mutation_request_hash
        || result.versions.account.pre != pre_account_version
        || result.versions.account.post != post_account_version
        || result.committed_at_unix_ms != death_committed_at
        || root_account != mutation_account
        || root_character != mutation_character
        || root_lineage != lineage_id
        || root_restore_point != restore_point_id
        || root_state != RESTORE_STATE_DEATH_COMMITTED
        || root_death_mutation != mutation_id
        || promotion_account != mutation_account
        || promotion_character != mutation_character
        || canonical_death_terminal_payload_hash_v1(mutation_request_hash, promotion_digest)?
            != terminal_payload_hash
    {
        return Err(corrupt());
    }

    let terminal = StoredCommittedDeathTerminalV1 {
        schema_version: DURABLE_TERMINAL_RECOVERY_SCHEMA_VERSION,
        result,
        result_hash,
        lineage_id,
        restore_point_id,
        death_tick,
        promotion_digest,
        terminal_payload_hash,
    };
    terminal.validate()?;
    Ok(terminal)
}

fn required_id(row: &sqlx::postgres::PgRow, column: &str) -> Result<[u8; 16], PersistenceError> {
    exact_id(
        row.try_get::<Option<Vec<u8>>, _>(column)?
            .ok_or_else(corrupt)?,
    )
}

fn required_hash(row: &sqlx::postgres::PgRow, column: &str) -> Result<[u8; 32], PersistenceError> {
    exact_hash(
        row.try_get::<Option<Vec<u8>>, _>(column)?
            .ok_or_else(corrupt)?,
    )
}

fn required_string(row: &sqlx::postgres::PgRow, column: &str) -> Result<String, PersistenceError> {
    row.try_get::<Option<String>, _>(column)?
        .ok_or_else(corrupt)
}

fn required_bytes(row: &sqlx::postgres::PgRow, column: &str) -> Result<Vec<u8>, PersistenceError> {
    row.try_get::<Option<Vec<u8>>, _>(column)?
        .ok_or_else(corrupt)
}

fn required_i16(row: &sqlx::postgres::PgRow, column: &str) -> Result<i16, PersistenceError> {
    row.try_get::<Option<i16>, _>(column)?.ok_or_else(corrupt)
}

fn required_positive_u64(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<u64, PersistenceError> {
    positive_u64(row.try_get::<Option<i64>, _>(column)?.ok_or_else(corrupt)?)
}

fn exact_id(value: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    value.try_into().map_err(|_| corrupt())
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; 32], PersistenceError> {
    value.try_into().map_err(|_| corrupt())
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(corrupt)
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredDurableDeath
}

#[cfg(test)]
mod tests {
    use crate::{
        DeathAggregateVersionsV1, DeathVersionAdvanceV1, DurableDeathResultCodeV1,
        DurableEchoOutcomeV1,
    };

    use super::*;

    fn uuid_v7(seed: u8) -> [u8; 16] {
        let mut value = [seed; 16];
        value[6] = 0x70 | (seed & 0x0f);
        value[8] = 0x80 | (seed & 0x3f);
        value
    }

    const fn advance(pre: u64) -> DeathVersionAdvanceV1 {
        DeathVersionAdvanceV1 { pre, post: pre + 1 }
    }

    fn result() -> StoredCommittedDeathResultV1 {
        StoredCommittedDeathResultV1 {
            schema_version: crate::DURABLE_DEATH_SCHEMA_VERSION,
            contract: DURABLE_DEATH_CONTRACT.into(),
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            death_id: uuid_v7(4),
            canonical_request_hash: [5; 32],
            canonical_plan_hash: [6; 32],
            result_code: DurableDeathResultCodeV1::Committed,
            issued_at_unix_ms: 10,
            committed_at_unix_ms: 20,
            versions: DeathAggregateVersionsV1 {
                account: advance(1),
                character: advance(2),
                progression: advance(3),
                inventory: advance(4),
                oath_bargain: advance(5),
                life_metrics: advance(6),
            },
            trace_digest: [7; 32],
            destruction_digest: [8; 32],
            summary_digest: [9; 32],
            memorial_digest: [10; 32],
            echo_outcome: DurableEchoOutcomeV1::NotEligible,
            created_echo_id: None,
            promoted_echo_id: None,
        }
    }

    fn terminal() -> StoredCommittedDeathTerminalV1 {
        let result = result();
        let promotion_digest = [13; 32];
        let terminal_payload_hash = canonical_death_terminal_payload_hash_v1(
            result.canonical_request_hash,
            promotion_digest,
        )
        .unwrap();
        StoredCommittedDeathTerminalV1 {
            schema_version: DURABLE_TERMINAL_RECOVERY_SCHEMA_VERSION,
            result_hash: result.digest().unwrap(),
            result,
            lineage_id: [11; 16],
            restore_point_id: [12; 16],
            death_tick: 30,
            promotion_digest,
            terminal_payload_hash,
        }
    }

    #[test]
    fn strict_projection_accepts_only_the_versioned_hash_bound_shape() {
        let terminal = terminal();
        terminal.validate().unwrap();

        let mut wrong_schema = terminal.clone();
        wrong_schema.schema_version += 1;
        assert!(matches!(
            wrong_schema.validate(),
            Err(PersistenceError::CorruptStoredDurableDeath)
        ));

        let mut wrong_hash = terminal.clone();
        wrong_hash.result_hash = [99; 32];
        assert!(matches!(
            wrong_hash.validate(),
            Err(PersistenceError::CorruptStoredDurableDeath)
        ));

        let mut wrong_promotion = terminal.clone();
        wrong_promotion.promotion_digest[0] ^= 1;
        assert!(matches!(
            wrong_promotion.validate(),
            Err(PersistenceError::CorruptStoredDurableDeath)
        ));

        let mut no_lineage = terminal.clone();
        no_lineage.lineage_id = [0; 16];
        assert!(matches!(
            no_lineage.validate(),
            Err(PersistenceError::CorruptStoredDurableDeath)
        ));

        let mut no_restore = terminal.clone();
        no_restore.restore_point_id = [0; 16];
        assert!(matches!(
            no_restore.validate(),
            Err(PersistenceError::CorruptStoredDurableDeath)
        ));

        let mut no_tick = terminal;
        no_tick.death_tick = 0;
        assert!(matches!(
            no_tick.validate(),
            Err(PersistenceError::CorruptStoredDurableDeath)
        ));
    }

    #[test]
    fn recovery_query_requires_all_three_committed_authorities() {
        for required in [
            "FROM death_mutation_results AS mutation",
            "FULL OUTER JOIN death_events AS death",
            "LEFT JOIN character_entry_restore_points AS root",
            "coalesce(mutation.contract_kind,death.contract_kind)=$4",
            "root.lineage_id=death.lineage_id",
            "root.restore_point_id=death.restore_point_id",
            "LIMIT 2",
        ] {
            assert!(
                COMMITTED_DEATH_TERMINAL_SQL.contains(required),
                "{required}"
            );
        }
    }
}
