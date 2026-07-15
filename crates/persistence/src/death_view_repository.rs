//! Authenticated, read-only `PostgreSQL` projections for committed deaths and memorials.
//!
//! This repository reads only the immutable death graph authored under GDD `DTH-001`,
//! `DTH-020`, and `TECH-020` through `TECH-023`; Content `CONT-HUB-002` and
//! `CONT-ECHO-009`; and Roadmap `GB-M03-06`. Every query is pinned to the wipeable Core
//! namespace and receives account authority from the authenticated server session.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgPool, Row, postgres::PgRow};
use thiserror::Error;

use crate::{
    DURABLE_DEATH_SUMMARY_REVISION, DURABLE_DEATH_TRACE_WINDOW_TICKS, DurableCombatTraceEntryV1,
    DurableDamageTypeV1, DurableDeathCauseV1, DurableEchoOutcomeV1, DurableNetworkStateV1,
    DurableOrderedContentIdV1, DurableRecallStateV1, DurableSummaryProjectionEntryV1,
    DurableSummaryProjectionKindV1, DurableTraceStatusV1, MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES,
    MAX_DURABLE_DEATH_STATUSES_PER_ENTRY, MAX_DURABLE_DEATH_TRACE_ENTRIES, PostgresPersistence,
    StoredCommittedDeathResultV1, WIPEABLE_CORE_NAMESPACE,
};

pub const MAX_DEATH_VIEW_LOST_PER_PAGE: u16 = 32;
pub const MAX_DEATH_VIEW_MEMORIALS_PER_PAGE: u8 = 32;
pub const MAX_DEATH_VIEW_TRACE_PER_PAGE: u8 = 8;

const PRESERVED_PROJECTIONS: [(DurableSummaryProjectionKindV1, &str); 5] = [
    (
        DurableSummaryProjectionKindV1::PreservedAccountRecords,
        "projection.preserved.account_records",
    ),
    (
        DurableSummaryProjectionKindV1::PreservedCurrency,
        "projection.preserved.currency",
    ),
    (
        DurableSummaryProjectionKindV1::PreservedVault,
        "projection.preserved.vault",
    ),
    (
        DurableSummaryProjectionKindV1::PreservedCosmetics,
        "projection.preserved.cosmetics",
    ),
    (
        DurableSummaryProjectionKindV1::PreservedRecipes,
        "projection.preserved.recipes",
    ),
];
const CREATED_PROJECTIONS: [(DurableSummaryProjectionKindV1, &str); 2] = [
    (
        DurableSummaryProjectionKindV1::CreatedMemorial,
        "projection.created.memorial",
    ),
    (
        DurableSummaryProjectionKindV1::CreatedEcho,
        "projection.created.echo",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DeathViewReadError {
    #[error("committed death does not exist")]
    DeathNotFound,
    #[error("committed death is owned by another account")]
    DeathNotOwned,
    #[error("requested death-view page is outside the stored projection")]
    PageOutOfRange,
    #[error("stored death-view graph violates the durable contract")]
    CorruptStoredRecord,
    #[error("death-view storage is unavailable")]
    ServiceUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredDeathMemorialCursorV1 {
    pub death_at_unix_ms: u64,
    pub death_id: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLatestCommittedDeathV1 {
    pub death_id: [u8; 16],
    pub character_id: [u8; 16],
    pub death_at_unix_ms: u64,
    pub death_tick: u64,
    pub cause: DurableDeathCauseV1,
    pub killer_content_id: String,
    pub killer_pattern_id: Option<String>,
    pub network_state: DurableNetworkStateV1,
    pub recall_state: DurableRecallStateV1,
    pub trace_entry_count: u16,
    pub trace_digest: [u8; 32],
    pub destruction_entry_count: u16,
    pub destruction_digest: [u8; 32],
    pub summary_snapshot_digest: [u8; 32],
    pub content_revision: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDeathSummaryViewV1 {
    pub death_id: [u8; 16],
    pub summary_revision: u16,
    pub hero_label_key: String,
    pub character_name_snapshot: String,
    pub class_id: String,
    pub level: u8,
    pub oath_id: Option<String>,
    pub bargains: Vec<DurableOrderedContentIdV1>,
    pub lifetime_ms: u64,
    pub final_deed_id: String,
    pub lethal_trace_ordinal: u16,
    pub last_five_damage: Vec<DurableCombatTraceEntryV1>,
    pub lost_total_count: u16,
    pub lost_start_ordinal: u16,
    pub lost: Vec<DurableSummaryProjectionEntryV1>,
    pub next_lost_ordinal: Option<u16>,
    pub preserved: Vec<DurableSummaryProjectionEntryV1>,
    pub created: Vec<DurableSummaryProjectionEntryV1>,
    pub echo_outcome: DurableEchoOutcomeV1,
    pub death_tick: u64,
    pub content_revision: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
    pub snapshot_digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDeathMemorialEntryV1 {
    pub cursor: StoredDeathMemorialCursorV1,
    pub summary_revision: u16,
    pub summary_snapshot_digest: [u8; 32],
    pub presentation_key: String,
    pub presentation_digest: [u8; 32],
    pub character_name_snapshot: String,
    pub class_id: String,
    pub level: u8,
    pub echo_outcome: DurableEchoOutcomeV1,
    pub content_revision: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDeathMemorialPageV1 {
    pub entries: Vec<StoredDeathMemorialEntryV1>,
    pub next_cursor: Option<StoredDeathMemorialCursorV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDeathTracePageV1 {
    pub death_id: [u8; 16],
    pub death_tick: u64,
    pub total_entry_count: u16,
    pub trace_digest: [u8; 32],
    pub start_ordinal: u16,
    pub entries: Vec<DurableCombatTraceEntryV1>,
    pub next_ordinal: Option<u16>,
}

const LATEST_SQL: &str = "SELECT death.death_id, death.character_id, \
        floor(extract(epoch FROM death.committed_at) * 1000)::bigint AS death_at_ms, \
        death.death_tick, death.cause_kind, death.killer_content_id, death.killer_pattern_id, \
        death.network_state, death.recall_state, death.trace_digest, death.content_revision, \
        death.world_records_blake3, death.world_assets_blake3, death.world_localization_blake3, \
        summary.snapshot_digest, result.result_payload, result.result_hash, \
        (SELECT count(*)::bigint FROM death_combat_trace_entries AS trace \
          WHERE trace.namespace_id=death.namespace_id AND trace.death_id=death.death_id) AS trace_count, \
        (SELECT count(*)::bigint FROM death_destruction_entries AS destroyed \
          WHERE destroyed.namespace_id=death.namespace_id AND destroyed.death_id=death.death_id) AS destruction_count \
     FROM death_events AS death \
     JOIN death_summary_snapshots AS summary USING (namespace_id, death_id) \
     JOIN death_mutation_results AS result USING (namespace_id, account_id, character_id, death_id) \
     WHERE death.namespace_id=$1 AND death.account_id=$2 \
     ORDER BY death.committed_at DESC, death.death_id ASC LIMIT 1";

const SUMMARY_SQL: &str = "SELECT death.death_id, death.character_id, death.death_tick, death.trace_digest, \
        summary.summary_revision, summary.hero_label_key, summary.character_name_snapshot, \
        summary.class_id, summary.level, summary.oath_id, summary.lifetime_ms, \
        summary.final_deed_id, summary.echo_outcome, summary.content_revision, \
        summary.snapshot_digest, death.world_records_blake3, death.world_assets_blake3, \
        death.world_localization_blake3, result.result_payload, result.result_hash, \
        (SELECT count(*)::bigint FROM death_combat_trace_entries AS trace \
          WHERE trace.namespace_id=death.namespace_id AND trace.death_id=death.death_id) AS trace_count, \
        (SELECT count(*)::bigint FROM death_summary_projection_entries AS projection \
          WHERE projection.namespace_id=death.namespace_id AND projection.death_id=death.death_id \
            AND projection.section_kind=0) AS lost_count, \
        (SELECT count(*)::bigint FROM death_summary_projection_entries AS projection \
          WHERE projection.namespace_id=death.namespace_id AND projection.death_id=death.death_id \
            AND projection.section_kind=1) AS preserved_count, \
        (SELECT count(*)::bigint FROM death_summary_projection_entries AS projection \
          WHERE projection.namespace_id=death.namespace_id AND projection.death_id=death.death_id \
            AND projection.section_kind=2) AS created_count \
     FROM death_events AS death \
     JOIN death_summary_snapshots AS summary USING (namespace_id, death_id) \
     JOIN death_mutation_results AS result USING (namespace_id, account_id, character_id, death_id) \
     WHERE death.namespace_id=$1 AND death.account_id=$2 AND death.death_id=$3";

const MEMORIAL_SQL: &str = "SELECT memorial.death_id, \
        floor(extract(epoch FROM memorial.death_at) * 1000)::bigint AS death_at_ms, \
        memorial.summary_revision, memorial.presentation_key, memorial.presentation_digest, \
        summary.snapshot_digest, summary.character_name_snapshot, summary.class_id, \
        summary.level, summary.echo_outcome, summary.content_revision, \
        death.world_records_blake3, death.world_assets_blake3, death.world_localization_blake3, \
        result.result_payload, result.result_hash \
     FROM memorial_records AS memorial \
     JOIN death_summary_snapshots AS summary USING (namespace_id, death_id) \
     JOIN death_events AS death USING (namespace_id, account_id, death_id) \
     JOIN death_mutation_results AS result USING (namespace_id, account_id, character_id, death_id) \
     WHERE memorial.namespace_id=$1 AND memorial.account_id=$2 \
       AND ($3::bigint IS NULL \
         OR floor(extract(epoch FROM memorial.death_at) * 1000)::bigint < $3 \
         OR (floor(extract(epoch FROM memorial.death_at) * 1000)::bigint = $3 \
             AND memorial.death_id > $4::bytea)) \
     ORDER BY floor(extract(epoch FROM memorial.death_at) * 1000)::bigint DESC, \
              memorial.death_id ASC LIMIT $5";

impl PostgresPersistence {
    pub async fn load_latest_committed_death_view(
        &self,
        account_id: [u8; 16],
    ) -> Result<Option<StoredLatestCommittedDeathV1>, DeathViewReadError> {
        require_account(account_id)?;
        let row = sqlx::query(LATEST_SQL)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        row.map(|row| latest_from_row(&row, account_id)).transpose()
    }

    pub async fn load_owned_death_summary_view(
        &self,
        account_id: [u8; 16],
        death_id: [u8; 16],
        lost_start_ordinal: u16,
        lost_limit: u16,
    ) -> Result<StoredDeathSummaryViewV1, DeathViewReadError> {
        require_account(account_id)?;
        require_death_id(death_id)?;
        if !(1..=MAX_DEATH_VIEW_LOST_PER_PAGE).contains(&lost_limit) {
            return Err(DeathViewReadError::PageOutOfRange);
        }
        let row = sqlx::query(SUMMARY_SQL)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .bind(death_id.as_slice())
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        let Some(row) = row else {
            return Err(classify_missing_death(&self.pool, account_id, death_id).await?);
        };
        summary_from_row(
            &self.pool,
            &row,
            account_id,
            death_id,
            lost_start_ordinal,
            lost_limit,
        )
        .await
    }

    pub async fn load_death_memorial_page(
        &self,
        account_id: [u8; 16],
        after: Option<StoredDeathMemorialCursorV1>,
        limit: u8,
    ) -> Result<StoredDeathMemorialPageV1, DeathViewReadError> {
        require_account(account_id)?;
        if !(1..=MAX_DEATH_VIEW_MEMORIALS_PER_PAGE).contains(&limit)
            || after
                .is_some_and(|cursor| cursor.death_at_unix_ms == 0 || !is_uuid_v7(cursor.death_id))
        {
            return Err(DeathViewReadError::PageOutOfRange);
        }
        let after_ms = after
            .map(|cursor| i64_from_u64(cursor.death_at_unix_ms))
            .transpose()?;
        let after_id = after.map(|cursor| cursor.death_id.to_vec());
        let rows = sqlx::query(MEMORIAL_SQL)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .bind(after_ms)
            .bind(after_id)
            .bind(i64::from(limit) + 1)
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)?;
        let has_more = rows.len() > usize::from(limit);
        let mut entries = rows
            .iter()
            .take(usize::from(limit))
            .map(|row| memorial_from_row(row, account_id))
            .collect::<Result<Vec<_>, _>>()?;
        validate_memorial_order(&entries)?;
        let next_cursor = has_more.then(|| entries.last().expect("nonzero limit").cursor);
        Ok(StoredDeathMemorialPageV1 {
            entries: std::mem::take(&mut entries),
            next_cursor,
        })
    }

    pub async fn load_owned_death_trace_page(
        &self,
        account_id: [u8; 16],
        death_id: [u8; 16],
        start_ordinal: u16,
        limit: u8,
    ) -> Result<StoredDeathTracePageV1, DeathViewReadError> {
        require_account(account_id)?;
        require_death_id(death_id)?;
        if !(1..=MAX_DEATH_VIEW_TRACE_PER_PAGE).contains(&limit) {
            return Err(DeathViewReadError::PageOutOfRange);
        }
        let row = sqlx::query(
            "SELECT death.character_id, death.death_tick, death.trace_digest, \
                result.result_payload, result.result_hash, \
                (SELECT count(*)::bigint FROM death_combat_trace_entries AS trace \
                  WHERE trace.namespace_id=death.namespace_id \
                    AND trace.death_id=death.death_id) AS trace_count \
             FROM death_events AS death \
             JOIN death_mutation_results AS result \
               USING (namespace_id, account_id, character_id, death_id) \
             WHERE death.namespace_id=$1 AND death.account_id=$2 AND death.death_id=$3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(death_id.as_slice())
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?;
        let Some(row) = row else {
            return Err(classify_missing_death(&self.pool, account_id, death_id).await?);
        };
        let character_id = exact_nonzero_id(value(&row, "character_id")?)?;
        let death_tick = positive_u64(value(&row, "death_tick")?)?;
        let trace_count =
            bounded_count(value(&row, "trace_count")?, MAX_DURABLE_DEATH_TRACE_ENTRIES)?;
        if start_ordinal >= trace_count {
            return Err(DeathViewReadError::PageOutOfRange);
        }
        let trace_digest = exact_nonzero_hash(value(&row, "trace_digest")?)?;
        let result = result_from_row(&row, account_id, character_id, death_id)?;
        if result.trace_digest != trace_digest {
            return corrupt();
        }
        let count = usize::from(u16::from(limit).min(trace_count - start_ordinal));
        let entries =
            load_trace_slice(&self.pool, death_id, start_ordinal, count, death_tick).await?;
        validate_lethal_position(&entries, trace_count)?;
        let next_ordinal = start_ordinal
            .checked_add(u16::try_from(entries.len()).map_err(|_| corrupt_error())?)
            .filter(|next| *next < trace_count);
        Ok(StoredDeathTracePageV1 {
            death_id,
            death_tick,
            total_entry_count: trace_count,
            trace_digest,
            start_ordinal,
            entries,
            next_ordinal,
        })
    }
}

fn latest_from_row(
    row: &PgRow,
    account_id: [u8; 16],
) -> Result<StoredLatestCommittedDeathV1, DeathViewReadError> {
    let death_id = exact_uuid_v7(value(row, "death_id")?)?;
    let character_id = exact_nonzero_id(value(row, "character_id")?)?;
    let result = result_from_row(row, account_id, character_id, death_id)?;
    let trace_digest = exact_nonzero_hash(value(row, "trace_digest")?)?;
    let summary_digest = exact_nonzero_hash(value(row, "snapshot_digest")?)?;
    let trace_entry_count =
        bounded_count(value(row, "trace_count")?, MAX_DURABLE_DEATH_TRACE_ENTRIES)?;
    let destruction_entry_count = bounded_count(
        value(row, "destruction_count")?,
        MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES,
    )?;
    if trace_entry_count == 0
        || result.trace_digest != trace_digest
        || result.summary_digest != summary_digest
    {
        return corrupt();
    }
    Ok(StoredLatestCommittedDeathV1 {
        death_id,
        character_id,
        death_at_unix_ms: positive_u64(value(row, "death_at_ms")?)?,
        death_tick: positive_u64(value(row, "death_tick")?)?,
        cause: cause(value(row, "cause_kind")?)?,
        killer_content_id: stable_id(required_text(row, "killer_content_id")?)?,
        killer_pattern_id: optional_stable_id(value(row, "killer_pattern_id")?)?,
        network_state: network_state(value(row, "network_state")?)?,
        recall_state: recall_state(value(row, "recall_state")?)?,
        trace_entry_count,
        trace_digest,
        destruction_entry_count,
        destruction_digest: result.destruction_digest,
        summary_snapshot_digest: summary_digest,
        content_revision: content_revision(value(row, "content_revision")?)?,
        records_blake3: lower_blake3(value(row, "world_records_blake3")?)?,
        assets_blake3: lower_blake3(value(row, "world_assets_blake3")?)?,
        localization_blake3: lower_blake3(value(row, "world_localization_blake3")?)?,
    })
}

async fn summary_from_row(
    pool: &PgPool,
    row: &PgRow,
    account_id: [u8; 16],
    death_id: [u8; 16],
    lost_start_ordinal: u16,
    lost_limit: u16,
) -> Result<StoredDeathSummaryViewV1, DeathViewReadError> {
    if exact_uuid_v7(value(row, "death_id")?)? != death_id {
        return corrupt();
    }
    let character_id = exact_nonzero_id(value(row, "character_id")?)?;
    let result = result_from_row(row, account_id, character_id, death_id)?;
    let death_tick = positive_u64(value(row, "death_tick")?)?;
    let trace_count = bounded_count(value(row, "trace_count")?, MAX_DURABLE_DEATH_TRACE_ENTRIES)?;
    let lost_total_count = bounded_count(
        value(row, "lost_count")?,
        MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES,
    )?;
    let preserved_count =
        bounded_count(value(row, "preserved_count")?, PRESERVED_PROJECTIONS.len())?;
    let created_count = bounded_count(value(row, "created_count")?, CREATED_PROJECTIONS.len())?;
    if trace_count == 0
        || preserved_count
            != u16::try_from(PRESERVED_PROJECTIONS.len()).map_err(|_| corrupt_error())?
        || created_count != u16::try_from(CREATED_PROJECTIONS.len()).map_err(|_| corrupt_error())?
    {
        return corrupt();
    }
    if lost_start_ordinal > lost_total_count {
        return Err(DeathViewReadError::PageOutOfRange);
    }
    let summary_revision = positive_u16(value(row, "summary_revision")?)?;
    let snapshot_digest = exact_nonzero_hash(value(row, "snapshot_digest")?)?;
    if summary_revision != DURABLE_DEATH_SUMMARY_REVISION
        || result.summary_digest != snapshot_digest
        || result.trace_digest != exact_nonzero_hash(value(row, "trace_digest")?)?
    {
        return corrupt();
    }
    let bargains = load_bargains(pool, death_id).await?;
    let lethal_trace_ordinal = trace_count - 1;
    let last_start = trace_count.saturating_sub(5);
    let last_five_damage = load_trace_slice(
        pool,
        death_id,
        last_start,
        usize::from(trace_count - last_start),
        death_tick,
    )
    .await?;
    validate_lethal_position(&last_five_damage, trace_count)?;
    validate_summary_damage_refs(pool, death_id, &last_five_damage).await?;
    let lost_count = usize::from(lost_limit.min(lost_total_count - lost_start_ordinal));
    let lost = load_projections(pool, death_id, 0, lost_start_ordinal, lost_count).await?;
    validate_unique_losses(&lost)?;
    let preserved = load_projections(pool, death_id, 1, 0, PRESERVED_PROJECTIONS.len()).await?;
    let created = load_projections(pool, death_id, 2, 0, CREATED_PROJECTIONS.len()).await?;
    validate_fixed_projections(&preserved, &PRESERVED_PROJECTIONS)?;
    validate_fixed_projections(&created, &CREATED_PROJECTIONS)?;
    let next_lost_ordinal = lost_start_ordinal
        .checked_add(u16::try_from(lost.len()).map_err(|_| corrupt_error())?)
        .filter(|next| *next < lost_total_count);
    let echo_outcome = echo_outcome(value(row, "echo_outcome")?)?;
    if echo_outcome != result.echo_outcome {
        return corrupt();
    }
    Ok(StoredDeathSummaryViewV1 {
        death_id,
        summary_revision,
        hero_label_key: stable_id(value(row, "hero_label_key")?)?,
        character_name_snapshot: character_name(value(row, "character_name_snapshot")?)?,
        class_id: stable_id(value(row, "class_id")?)?,
        level: core_level(value(row, "level")?)?,
        oath_id: optional_stable_id(value(row, "oath_id")?)?,
        bargains,
        lifetime_ms: nonnegative_u64(value(row, "lifetime_ms")?)?,
        final_deed_id: stable_id(value(row, "final_deed_id")?)?,
        lethal_trace_ordinal,
        last_five_damage,
        lost_total_count,
        lost_start_ordinal,
        lost,
        next_lost_ordinal,
        preserved,
        created,
        echo_outcome,
        death_tick,
        content_revision: content_revision(value(row, "content_revision")?)?,
        records_blake3: lower_blake3(value(row, "world_records_blake3")?)?,
        assets_blake3: lower_blake3(value(row, "world_assets_blake3")?)?,
        localization_blake3: lower_blake3(value(row, "world_localization_blake3")?)?,
        snapshot_digest,
    })
}

fn memorial_from_row(
    row: &PgRow,
    account_id: [u8; 16],
) -> Result<StoredDeathMemorialEntryV1, DeathViewReadError> {
    let death_id = exact_uuid_v7(value(row, "death_id")?)?;
    let result = result_from_row(row, account_id, result_character_id(row)?, death_id)?;
    let summary_snapshot_digest = exact_nonzero_hash(value(row, "snapshot_digest")?)?;
    let presentation_digest = exact_nonzero_hash(value(row, "presentation_digest")?)?;
    let echo_outcome = echo_outcome(value(row, "echo_outcome")?)?;
    if result.summary_digest != summary_snapshot_digest
        || result.memorial_digest != presentation_digest
        || result.echo_outcome != echo_outcome
    {
        return corrupt();
    }
    let death_at_unix_ms = positive_u64(value(row, "death_at_ms")?)?;
    let summary_revision = positive_u16(value(row, "summary_revision")?)?;
    if result.committed_at_unix_ms != death_at_unix_ms
        || summary_revision != DURABLE_DEATH_SUMMARY_REVISION
    {
        return corrupt();
    }
    Ok(StoredDeathMemorialEntryV1 {
        cursor: StoredDeathMemorialCursorV1 {
            death_at_unix_ms,
            death_id,
        },
        summary_revision,
        summary_snapshot_digest,
        presentation_key: stable_id(value(row, "presentation_key")?)?,
        presentation_digest,
        character_name_snapshot: character_name(value(row, "character_name_snapshot")?)?,
        class_id: stable_id(value(row, "class_id")?)?,
        level: core_level(value(row, "level")?)?,
        echo_outcome,
        content_revision: content_revision(value(row, "content_revision")?)?,
        records_blake3: lower_blake3(value(row, "world_records_blake3")?)?,
        assets_blake3: lower_blake3(value(row, "world_assets_blake3")?)?,
        localization_blake3: lower_blake3(value(row, "world_localization_blake3")?)?,
    })
}

fn result_character_id(row: &PgRow) -> Result<[u8; 16], DeathViewReadError> {
    let payload: Vec<u8> = value(row, "result_payload")?;
    StoredCommittedDeathResultV1::decode(&payload)
        .map(|result| result.character_id)
        .map_err(|_| corrupt_error())
}

fn result_from_row(
    row: &PgRow,
    account_id: [u8; 16],
    character_id: [u8; 16],
    death_id: [u8; 16],
) -> Result<StoredCommittedDeathResultV1, DeathViewReadError> {
    let payload: Vec<u8> = value(row, "result_payload")?;
    let digest = exact_nonzero_hash(value(row, "result_hash")?)?;
    let result = StoredCommittedDeathResultV1::decode(&payload).map_err(|_| corrupt_error())?;
    if result.digest().map_err(|_| corrupt_error())? != digest
        || result.namespace_id != WIPEABLE_CORE_NAMESPACE
        || result.account_id != account_id
        || result.character_id != character_id
        || result.death_id != death_id
    {
        return corrupt();
    }
    Ok(result)
}

async fn load_bargains(
    pool: &PgPool,
    death_id: [u8; 16],
) -> Result<Vec<DurableOrderedContentIdV1>, DeathViewReadError> {
    let rows = sqlx::query(
        "SELECT bargain_ordinal, bargain_id FROM death_summary_bargains \
         WHERE namespace_id=$1 AND death_id=$2 ORDER BY bargain_ordinal ASC",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    if rows.len() > 3 {
        return corrupt();
    }
    let mut ids = BTreeSet::new();
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let ordinal = nonnegative_u16(value(row, "bargain_ordinal")?)?;
            let content_id = stable_id(value(row, "bargain_id")?)?;
            if ordinal != u16::try_from(index).map_err(|_| corrupt_error())?
                || !ids.insert(content_id.clone())
            {
                return corrupt();
            }
            Ok(DurableOrderedContentIdV1 {
                ordinal,
                content_id,
            })
        })
        .collect()
}

async fn load_trace_slice(
    pool: &PgPool,
    death_id: [u8; 16],
    start: u16,
    count: usize,
    death_tick: u64,
) -> Result<Vec<DurableCombatTraceEntryV1>, DeathViewReadError> {
    let rows = sqlx::query(
        "SELECT trace_ordinal, event_tick, event_ordinal, source_content_id, source_entity_id, \
            pattern_id, attack_id, raw_damage, final_damage, damage_type, pre_health, post_health, \
            source_x_milli_tiles, source_y_milli_tiles, network_state, recall_state, lethal \
         FROM death_combat_trace_entries WHERE namespace_id=$1 AND death_id=$2 \
           AND trace_ordinal >= $3 ORDER BY trace_ordinal ASC LIMIT $4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .bind(i16::try_from(start).map_err(|_| corrupt_error())?)
    .bind(i64::try_from(count).map_err(|_| corrupt_error())?)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    if rows.len() != count {
        return corrupt();
    }
    let end = start
        .checked_add(u16::try_from(count).map_err(|_| corrupt_error())?)
        .ok_or_else(corrupt_error)?;
    let status_rows = sqlx::query(
        "SELECT trace_ordinal, status_ordinal, status_id, remaining_ticks, stack_count \
         FROM death_combat_trace_statuses WHERE namespace_id=$1 AND death_id=$2 \
           AND trace_ordinal >= $3 AND trace_ordinal < $4 \
         ORDER BY trace_ordinal ASC, status_ordinal ASC",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .bind(i16::try_from(start).map_err(|_| corrupt_error())?)
    .bind(i16::try_from(end).map_err(|_| corrupt_error())?)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    let mut statuses: BTreeMap<u16, Vec<DurableTraceStatusV1>> = BTreeMap::new();
    for row in &status_rows {
        let trace_ordinal = nonnegative_u16(value(row, "trace_ordinal")?)?;
        let entries = statuses.entry(trace_ordinal).or_default();
        if entries.len() >= MAX_DURABLE_DEATH_STATUSES_PER_ENTRY {
            return corrupt();
        }
        let ordinal = nonnegative_u8(value(row, "status_ordinal")?)?;
        let status_id = stable_id(value(row, "status_id")?)?;
        if ordinal != u8::try_from(entries.len()).map_err(|_| corrupt_error())?
            || entries.iter().any(|entry| entry.status_id == status_id)
        {
            return corrupt();
        }
        entries.push(DurableTraceStatusV1 {
            ordinal,
            status_id,
            remaining_ticks: bounded_u32(value(row, "remaining_ticks")?, 108_000)?,
            stack_count: positive_bounded_u16(value(row, "stack_count")?, 255)?,
        });
    }
    let entries = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let expected = start
                .checked_add(u16::try_from(index).map_err(|_| corrupt_error())?)
                .ok_or_else(corrupt_error)?;
            trace_entry_from_row(
                row,
                expected,
                death_tick,
                statuses.remove(&expected).unwrap_or_default(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !statuses.is_empty()
        || entries.windows(2).any(|pair| {
            (pair[0].event_tick, pair[0].event_ordinal)
                >= (pair[1].event_tick, pair[1].event_ordinal)
        })
    {
        return corrupt();
    }
    Ok(entries)
}

fn trace_entry_from_row(
    row: &PgRow,
    expected_ordinal: u16,
    death_tick: u64,
    statuses: Vec<DurableTraceStatusV1>,
) -> Result<DurableCombatTraceEntryV1, DeathViewReadError> {
    let ordinal = nonnegative_u16(value(row, "trace_ordinal")?)?;
    let event_tick = positive_u64(value(row, "event_tick")?)?;
    let pre_health = positive_u32(value(row, "pre_health")?)?;
    let final_damage = nonnegative_u32(value(row, "final_damage")?)?;
    let post_health = nonnegative_u32(value(row, "post_health")?)?;
    let lethal: bool = value(row, "lethal")?;
    if ordinal != expected_ordinal
        || event_tick > death_tick
        || death_tick.saturating_sub(event_tick) > DURABLE_DEATH_TRACE_WINDOW_TICKS
        || post_health != pre_health.saturating_sub(final_damage)
        || lethal != (post_health == 0)
    {
        return corrupt();
    }
    Ok(DurableCombatTraceEntryV1 {
        ordinal,
        event_tick,
        event_ordinal: nonnegative_u32(value(row, "event_ordinal")?)?,
        source_content_id: stable_id(required_text(row, "source_content_id")?)?,
        source_entity_id: optional_nonzero_id(value(row, "source_entity_id")?)?,
        pattern_id: optional_stable_id(value(row, "pattern_id")?)?,
        attack_id: stable_id(required_text(row, "attack_id")?)?,
        raw_damage: nonnegative_u32(value(row, "raw_damage")?)?,
        final_damage,
        damage_type: damage_type(value(row, "damage_type")?)?,
        pre_health,
        post_health,
        source_x_milli_tiles: value(row, "source_x_milli_tiles")?,
        source_y_milli_tiles: value(row, "source_y_milli_tiles")?,
        network_state: network_state(value(row, "network_state")?)?,
        recall_state: recall_state(value(row, "recall_state")?)?,
        lethal,
        statuses,
    })
}

async fn validate_summary_damage_refs(
    pool: &PgPool,
    death_id: [u8; 16],
    entries: &[DurableCombatTraceEntryV1],
) -> Result<(), DeathViewReadError> {
    let rows = sqlx::query(
        "SELECT summary_ordinal, trace_ordinal FROM death_summary_damage_entries \
         WHERE namespace_id=$1 AND death_id=$2 ORDER BY summary_ordinal ASC",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    if rows.len() != entries.len() {
        return corrupt();
    }
    for (index, (row, entry)) in rows.iter().zip(entries).enumerate() {
        if nonnegative_u16(value(row, "summary_ordinal")?)?
            != u16::try_from(index).map_err(|_| corrupt_error())?
            || nonnegative_u16(value(row, "trace_ordinal")?)? != entry.ordinal
        {
            return corrupt();
        }
    }
    Ok(())
}

async fn load_projections(
    pool: &PgPool,
    death_id: [u8; 16],
    section: i16,
    start: u16,
    count: usize,
) -> Result<Vec<DurableSummaryProjectionEntryV1>, DeathViewReadError> {
    let rows = sqlx::query(
        "SELECT entry_ordinal, projection_kind, content_id, quantity, item_uid \
         FROM death_summary_projection_entries WHERE namespace_id=$1 AND death_id=$2 \
           AND section_kind=$3 AND entry_ordinal >= $4 \
         ORDER BY entry_ordinal ASC LIMIT $5",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .bind(section)
    .bind(i16::try_from(start).map_err(|_| corrupt_error())?)
    .bind(i64::try_from(count).map_err(|_| corrupt_error())?)
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    if rows.len() != count {
        return corrupt();
    }
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let ordinal = nonnegative_u16(value(row, "entry_ordinal")?)?;
            if ordinal
                != start
                    .checked_add(u16::try_from(index).map_err(|_| corrupt_error())?)
                    .ok_or_else(corrupt_error)?
            {
                return corrupt();
            }
            let kind = projection_kind(value(row, "projection_kind")?)?;
            let quantity = positive_u32(value(row, "quantity")?)?;
            let item_uid = optional_nonzero_id(value(row, "item_uid")?)?;
            let section_kind_valid = match section {
                0 => matches!(
                    kind,
                    DurableSummaryProjectionKindV1::LostItem
                        | DurableSummaryProjectionKindV1::LostRunMaterial
                ),
                1 => matches!(
                    kind,
                    DurableSummaryProjectionKindV1::PreservedAccountRecords
                        | DurableSummaryProjectionKindV1::PreservedCurrency
                        | DurableSummaryProjectionKindV1::PreservedVault
                        | DurableSummaryProjectionKindV1::PreservedCosmetics
                        | DurableSummaryProjectionKindV1::PreservedRecipes
                ),
                2 => matches!(
                    kind,
                    DurableSummaryProjectionKindV1::CreatedMemorial
                        | DurableSummaryProjectionKindV1::CreatedEcho
                ),
                _ => false,
            };
            if !section_kind_valid
                || matches!(kind, DurableSummaryProjectionKindV1::LostItem)
                    != (quantity == 1 && item_uid.is_some())
                || matches!(kind, DurableSummaryProjectionKindV1::LostRunMaterial)
                    && item_uid.is_some()
                || !matches!(
                    kind,
                    DurableSummaryProjectionKindV1::LostItem
                        | DurableSummaryProjectionKindV1::LostRunMaterial
                ) && (quantity != 1 || item_uid.is_some())
            {
                return corrupt();
            }
            Ok(DurableSummaryProjectionEntryV1 {
                ordinal,
                kind,
                content_id: stable_id(value(row, "content_id")?)?,
                quantity,
                item_uid,
            })
        })
        .collect()
}

fn validate_unique_losses(
    entries: &[DurableSummaryProjectionEntryV1],
) -> Result<(), DeathViewReadError> {
    let mut item_uids = BTreeSet::new();
    let mut material_ids = BTreeSet::new();
    if entries.iter().any(|entry| match entry.kind {
        DurableSummaryProjectionKindV1::LostItem => {
            entry.item_uid.is_none_or(|uid| !item_uids.insert(uid))
        }
        DurableSummaryProjectionKindV1::LostRunMaterial => {
            !material_ids.insert(entry.content_id.as_bytes())
        }
        _ => true,
    }) {
        return corrupt();
    }
    Ok(())
}

fn validate_lethal_position(
    entries: &[DurableCombatTraceEntryV1],
    total_count: u16,
) -> Result<(), DeathViewReadError> {
    if entries
        .iter()
        .any(|entry| entry.lethal != (entry.ordinal.checked_add(1) == Some(total_count)))
    {
        return corrupt();
    }
    Ok(())
}

fn validate_fixed_projections<const N: usize>(
    entries: &[DurableSummaryProjectionEntryV1],
    expected: &[(DurableSummaryProjectionKindV1, &str); N],
) -> Result<(), DeathViewReadError> {
    if entries.len() != N
        || entries
            .iter()
            .zip(expected)
            .enumerate()
            .any(|(index, (entry, (kind, content_id)))| {
                entry.ordinal != u16::try_from(index).unwrap_or(u16::MAX)
                    || entry.kind != *kind
                    || entry.content_id != *content_id
                    || entry.quantity != 1
                    || entry.item_uid.is_some()
            })
    {
        return corrupt();
    }
    Ok(())
}

fn validate_memorial_order(
    entries: &[StoredDeathMemorialEntryV1],
) -> Result<(), DeathViewReadError> {
    if entries.windows(2).any(|pair| {
        pair[0].cursor.death_at_unix_ms < pair[1].cursor.death_at_unix_ms
            || (pair[0].cursor.death_at_unix_ms == pair[1].cursor.death_at_unix_ms
                && pair[0].cursor.death_id >= pair[1].cursor.death_id)
    }) {
        return corrupt();
    }
    Ok(())
}

async fn classify_missing_death(
    pool: &PgPool,
    account_id: [u8; 16],
    death_id: [u8; 16],
) -> Result<DeathViewReadError, DeathViewReadError> {
    let owner: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT account_id FROM death_events WHERE namespace_id=$1 AND death_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    owner.map_or(Ok(DeathViewReadError::DeathNotFound), |owner| {
        let owner = exact_nonzero_id(owner)?;
        if owner == account_id {
            Ok(DeathViewReadError::CorruptStoredRecord)
        } else {
            Ok(DeathViewReadError::DeathNotOwned)
        }
    })
}

fn cause(value: i16) -> Result<DurableDeathCauseV1, DeathViewReadError> {
    match value {
        0 => Ok(DurableDeathCauseV1::DirectHit),
        1 => Ok(DurableDeathCauseV1::DamageOverTime),
        2 => Ok(DurableDeathCauseV1::Environment),
        3 => Ok(DurableDeathCauseV1::Disconnect),
        _ => corrupt(),
    }
}

fn damage_type(value: i16) -> Result<DurableDamageTypeV1, DeathViewReadError> {
    match value {
        0 => Ok(DurableDamageTypeV1::Physical),
        1 => Ok(DurableDamageTypeV1::Veil),
        _ => corrupt(),
    }
}

fn network_state(value: i16) -> Result<DurableNetworkStateV1, DeathViewReadError> {
    match value {
        0 => Ok(DurableNetworkStateV1::Connected),
        1 => Ok(DurableNetworkStateV1::Degraded),
        2 => Ok(DurableNetworkStateV1::LinkLost),
        3 => Ok(DurableNetworkStateV1::Reattached),
        _ => corrupt(),
    }
}

fn recall_state(value: i16) -> Result<DurableRecallStateV1, DeathViewReadError> {
    match value {
        0 => Ok(DurableRecallStateV1::Inactive),
        1 => Ok(DurableRecallStateV1::Channeling),
        2 => Ok(DurableRecallStateV1::CompletionPending),
        _ => corrupt(),
    }
}

fn echo_outcome(value: i16) -> Result<DurableEchoOutcomeV1, DeathViewReadError> {
    match value {
        0 => Ok(DurableEchoOutcomeV1::NotEligible),
        1 => Ok(DurableEchoOutcomeV1::Dormant),
        2 => Ok(DurableEchoOutcomeV1::Available),
        _ => corrupt(),
    }
}

fn projection_kind(value: i16) -> Result<DurableSummaryProjectionKindV1, DeathViewReadError> {
    match value {
        0 => Ok(DurableSummaryProjectionKindV1::LostItem),
        1 => Ok(DurableSummaryProjectionKindV1::LostRunMaterial),
        2 => Ok(DurableSummaryProjectionKindV1::PreservedAccountRecords),
        3 => Ok(DurableSummaryProjectionKindV1::PreservedCurrency),
        4 => Ok(DurableSummaryProjectionKindV1::PreservedVault),
        5 => Ok(DurableSummaryProjectionKindV1::PreservedCosmetics),
        6 => Ok(DurableSummaryProjectionKindV1::PreservedRecipes),
        7 => Ok(DurableSummaryProjectionKindV1::CreatedMemorial),
        8 => Ok(DurableSummaryProjectionKindV1::CreatedEcho),
        _ => corrupt(),
    }
}

fn value<T>(row: &PgRow, column: &str) -> Result<T, DeathViewReadError>
where
    for<'a> T: sqlx::Decode<'a, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(column).map_err(|_| corrupt_error())
}

fn required_text(row: &PgRow, column: &str) -> Result<String, DeathViewReadError> {
    value::<Option<String>>(row, column)?.ok_or_else(corrupt_error)
}

fn stable_id(value: String) -> Result<String, DeathViewReadError> {
    if (3..=96).contains(&value.len())
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-')
                })
        })
    {
        Ok(value)
    } else {
        corrupt()
    }
}

fn optional_stable_id(value: Option<String>) -> Result<Option<String>, DeathViewReadError> {
    value.map(stable_id).transpose()
}

fn character_name(value: String) -> Result<String, DeathViewReadError> {
    if (1..=24).contains(&value.len()) && !value.chars().any(char::is_control) {
        Ok(value)
    } else {
        corrupt()
    }
}

fn content_revision(value: String) -> Result<String, DeathViewReadError> {
    const PREFIX: &str = "core-dev.blake3.";
    if value.len() == PREFIX.len() + 64
        && value.starts_with(PREFIX)
        && value[PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value)
    } else {
        corrupt()
    }
}

fn lower_blake3(value: String) -> Result<String, DeathViewReadError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value)
    } else {
        corrupt()
    }
}

fn require_account(value: [u8; 16]) -> Result<(), DeathViewReadError> {
    if value == [0; 16] {
        Err(DeathViewReadError::DeathNotOwned)
    } else {
        Ok(())
    }
}

fn require_death_id(value: [u8; 16]) -> Result<(), DeathViewReadError> {
    if is_uuid_v7(value) {
        Ok(())
    } else {
        Err(DeathViewReadError::DeathNotFound)
    }
}

fn exact_uuid_v7(value: Vec<u8>) -> Result<[u8; 16], DeathViewReadError> {
    let value = exact_nonzero_id(value)?;
    if is_uuid_v7(value) {
        Ok(value)
    } else {
        corrupt()
    }
}

fn exact_nonzero_id(value: Vec<u8>) -> Result<[u8; 16], DeathViewReadError> {
    let value: [u8; 16] = value.try_into().map_err(|_| corrupt_error())?;
    if value == [0; 16] {
        corrupt()
    } else {
        Ok(value)
    }
}

fn optional_nonzero_id(value: Option<Vec<u8>>) -> Result<Option<[u8; 16]>, DeathViewReadError> {
    value.map(exact_nonzero_id).transpose()
}

fn exact_nonzero_hash(value: Vec<u8>) -> Result<[u8; 32], DeathViewReadError> {
    let value: [u8; 32] = value.try_into().map_err(|_| corrupt_error())?;
    if value == [0; 32] {
        corrupt()
    } else {
        Ok(value)
    }
}

fn bounded_count(value: i64, max: usize) -> Result<u16, DeathViewReadError> {
    let value = usize::try_from(value).map_err(|_| corrupt_error())?;
    if value > max {
        return corrupt();
    }
    u16::try_from(value).map_err(|_| corrupt_error())
}

fn positive_u64(value: i64) -> Result<u64, DeathViewReadError> {
    let value = u64::try_from(value).map_err(|_| corrupt_error())?;
    if value == 0 { corrupt() } else { Ok(value) }
}

fn nonnegative_u64(value: i64) -> Result<u64, DeathViewReadError> {
    u64::try_from(value).map_err(|_| corrupt_error())
}

fn positive_u32(value: i32) -> Result<u32, DeathViewReadError> {
    let value = u32::try_from(value).map_err(|_| corrupt_error())?;
    if value == 0 { corrupt() } else { Ok(value) }
}

fn nonnegative_u32(value: i32) -> Result<u32, DeathViewReadError> {
    u32::try_from(value).map_err(|_| corrupt_error())
}

fn bounded_u32(value: i32, max: u32) -> Result<u32, DeathViewReadError> {
    let value = nonnegative_u32(value)?;
    if value > max { corrupt() } else { Ok(value) }
}

fn positive_u16(value: i16) -> Result<u16, DeathViewReadError> {
    let value = u16::try_from(value).map_err(|_| corrupt_error())?;
    if value == 0 { corrupt() } else { Ok(value) }
}

fn nonnegative_u16(value: i16) -> Result<u16, DeathViewReadError> {
    u16::try_from(value).map_err(|_| corrupt_error())
}

fn positive_bounded_u16(value: i16, max: u16) -> Result<u16, DeathViewReadError> {
    let value = positive_u16(value)?;
    if value > max { corrupt() } else { Ok(value) }
}

fn nonnegative_u8(value: i16) -> Result<u8, DeathViewReadError> {
    u8::try_from(value).map_err(|_| corrupt_error())
}

fn core_level(value: i16) -> Result<u8, DeathViewReadError> {
    let value = u8::try_from(value).map_err(|_| corrupt_error())?;
    if (1..=10).contains(&value) {
        Ok(value)
    } else {
        corrupt()
    }
}

fn i64_from_u64(value: u64) -> Result<i64, DeathViewReadError> {
    i64::try_from(value).map_err(|_| DeathViewReadError::PageOutOfRange)
}

fn is_uuid_v7(value: [u8; 16]) -> bool {
    value != [0; 16] && value[6] >> 4 == 7 && value[8] & 0b1100_0000 == 0b1000_0000
}

fn unavailable(_: sqlx::Error) -> DeathViewReadError {
    DeathViewReadError::ServiceUnavailable
}

const fn corrupt_error() -> DeathViewReadError {
    DeathViewReadError::CorruptStoredRecord
}

fn corrupt<T>() -> Result<T, DeathViewReadError> {
    Err(corrupt_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_query_is_account_bound_and_newest_first() {
        assert!(LATEST_SQL.contains("death.namespace_id=$1 AND death.account_id=$2"));
        assert!(LATEST_SQL.contains("ORDER BY death.committed_at DESC, death.death_id ASC"));
        assert!(!LATEST_SQL.contains("FOR UPDATE"));
    }

    #[test]
    fn summary_query_uses_normalized_snapshot_and_stored_receipt() {
        for required in [
            "death_summary_snapshots",
            "death_mutation_results",
            "death_summary_projection_entries",
            "death_combat_trace_entries",
        ] {
            assert!(SUMMARY_SQL.contains(required));
        }
        assert!(SUMMARY_SQL.contains("death.account_id=$2 AND death.death_id=$3"));
    }

    #[test]
    fn memorial_query_is_account_bound_keyset_pagination() {
        assert!(MEMORIAL_SQL.contains("memorial.account_id=$2"));
        assert!(MEMORIAL_SQL.contains("memorial.death_id > $4::bytea"));
        assert!(MEMORIAL_SQL.contains("death_at) * 1000)::bigint DESC"));
        assert!(MEMORIAL_SQL.contains("memorial.death_id ASC LIMIT $5"));
    }

    #[test]
    fn strict_conversions_reject_malformed_storage() {
        assert_eq!(exact_nonzero_id(vec![1; 15]), corrupt());
        assert_eq!(exact_nonzero_id(vec![0; 16]), corrupt());
        assert_eq!(exact_nonzero_hash(vec![0; 32]), corrupt());
        assert_eq!(stable_id("Enemy.Invalid".into()), corrupt());
        assert_eq!(
            content_revision("core-dev.blake3.not-a-hash".into()),
            corrupt()
        );
        assert_eq!(core_level(11), corrupt());
    }

    #[test]
    fn memorial_order_is_newest_then_utf8_identity() {
        let entry = |millis, suffix| StoredDeathMemorialEntryV1 {
            cursor: StoredDeathMemorialCursorV1 {
                death_at_unix_ms: millis,
                death_id: uuid_v7(suffix),
            },
            summary_revision: 1,
            summary_snapshot_digest: [1; 32],
            presentation_key: "memorial.hero".into(),
            presentation_digest: [2; 32],
            character_name_snapshot: "Hero".into(),
            class_id: "class.arbalist".into(),
            level: 10,
            echo_outcome: DurableEchoOutcomeV1::Dormant,
            content_revision: format!("core-dev.blake3.{}", "a".repeat(64)),
            records_blake3: "a".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
        };
        assert!(validate_memorial_order(&[entry(2, 1), entry(1, 1)]).is_ok());
        assert!(validate_memorial_order(&[entry(2, 1), entry(2, 2)]).is_ok());
        assert_eq!(
            validate_memorial_order(&[entry(1, 1), entry(2, 1)]),
            corrupt()
        );
    }

    #[test]
    fn fixed_projection_contract_is_exact() {
        let entries = PRESERVED_PROJECTIONS
            .iter()
            .enumerate()
            .map(
                |(ordinal, (kind, content_id))| DurableSummaryProjectionEntryV1 {
                    ordinal: u16::try_from(ordinal).unwrap(),
                    kind: *kind,
                    content_id: (*content_id).into(),
                    quantity: 1,
                    item_uid: None,
                },
            )
            .collect::<Vec<_>>();
        assert!(validate_fixed_projections(&entries, &PRESERVED_PROJECTIONS).is_ok());
        let mut altered = entries;
        altered[0].content_id = "projection.preserved.fake".into();
        assert_eq!(
            validate_fixed_projections(&altered, &PRESERVED_PROJECTIONS),
            corrupt()
        );
    }

    fn uuid_v7(suffix: u8) -> [u8; 16] {
        let mut value = [0; 16];
        value[6] = 0x70;
        value[8] = 0x80;
        value[15] = suffix;
        value
    }
}
