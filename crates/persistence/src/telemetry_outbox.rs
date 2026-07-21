//! Committed `PostgreSQL` outbox adapter for `GB-M03-09`.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`TECH-123`,
//! `TEL-001`-`005`), `Gravebound_Content_Production_Spec_v1.md` (Core stable IDs and lifecycle
//! boundaries), and `Gravebound_Development_Roadmap_v1.md` (`ADR-005`, `GB-M03-09`). This module
//! reads only committed immutable rows and advances only their first `published_at` marker after
//! exact exporter acceptance.

use std::{collections::BTreeMap, fmt};

use sqlx::{PgPool, Postgres, Row, Transaction};
use telemetry::{
    CommittedOutboxError, CommittedOutboxEventV1, CommittedTelemetrySource, DamageTypeV1,
    DeathCauseV1, DeathEventV1, ExtractionEventV1, PseudonymousAccountId, RecallEventV1,
    RecallStateV1, RecallTriggerV1, StableTelemetryId, SuccessorEventV1, TelemetryContextV1,
    TelemetryEnvironmentV1, TelemetryEventError, TelemetryEventV1, TelemetryId,
    TelemetryIdentifierError, TelemetryPlatformV1, VersionedTelemetryEnvelopeV1,
};
use thiserror::Error;

use crate::{
    PersistenceError, ProductionRecallTriggerV1, StoredExtractionLocationV1,
    StoredProductionExtractionResultV1, StoredProductionRecallResultV1, StoredRecallLocationV1,
    StoredSuccessorResultV1, WIPEABLE_CORE_NAMESPACE,
};

pub const MAX_M03_TELEMETRY_POLL: usize = 256;

const POLL_SQL: &str = r"
SELECT family, account_id, character_id, event_id, event_payload,
       created_at_millis, commit_order, related_at_millis
FROM (
    SELECT 0::smallint AS family, death.account_id, death.character_id,
           outbox.event_id, outbox.event_payload,
           floor(extract(epoch FROM outbox.created_at) * 1000)::bigint AS created_at_millis,
           floor(extract(epoch FROM outbox.created_at) * 1000000)::bigint AS commit_order,
           NULL::bigint AS related_at_millis
    FROM death_outbox_events AS outbox
    JOIN death_events AS death
      ON death.namespace_id=outbox.namespace_id AND death.death_id=outbox.death_id
    WHERE outbox.namespace_id=$1 AND outbox.event_type='death_committed'
      AND outbox.published_at IS NULL
    UNION ALL
    SELECT 1::smallint, outbox.account_id, outbox.character_id,
           outbox.event_id, outbox.event_payload,
           floor(extract(epoch FROM outbox.created_at) * 1000)::bigint,
           floor(extract(epoch FROM outbox.created_at) * 1000000)::bigint,
           NULL::bigint
    FROM extraction_terminal_outbox_events_v1 AS outbox
    WHERE outbox.namespace_id=$1 AND outbox.event_type='extraction_committed'
      AND outbox.published_at IS NULL
    UNION ALL
    SELECT 2::smallint, outbox.account_id, outbox.character_id,
           outbox.event_id, outbox.event_payload,
           floor(extract(epoch FROM outbox.created_at) * 1000)::bigint,
           floor(extract(epoch FROM outbox.created_at) * 1000000)::bigint,
           NULL::bigint
    FROM recall_terminal_outbox_events_v1 AS outbox
    WHERE outbox.namespace_id=$1
      AND outbox.event_type IN ('emergency_recall_committed','disconnect_recovery_committed')
      AND outbox.published_at IS NULL
    UNION ALL
    SELECT 3::smallint, outbox.account_id, outbox.successor_id,
           outbox.event_id, outbox.event_payload,
           floor(extract(epoch FROM outbox.created_at) * 1000)::bigint,
           floor(extract(epoch FROM outbox.created_at) * 1000000)::bigint,
           floor(extract(epoch FROM death.committed_at) * 1000)::bigint
    FROM successor_mutation_outbox_events_v1 AS outbox
    JOIN death_events AS death
      ON death.namespace_id=outbox.namespace_id AND death.death_id=outbox.death_id
    WHERE outbox.namespace_id=$1 AND outbox.event_type=1 AND outbox.published_at IS NULL
) AS committed
ORDER BY commit_order, event_id, family
LIMIT $2
";

const DEATH_DETAIL_SQL: &str = r"
SELECT summary.class_id, summary.level, summary.oath_id, summary.lifetime_ms,
       death.region_id, death.room_id, death.cause_kind, death.killer_content_id,
       death.killer_pattern_id, death.raw_damage, death.final_damage, death.damage_type,
       death.pre_hit_health, death.recall_state,
       ARRAY(
           SELECT bargain.bargain_id FROM death_summary_bargains AS bargain
           WHERE bargain.namespace_id=death.namespace_id AND bargain.death_id=death.death_id
           ORDER BY bargain.bargain_id
       ) AS bargain_ids,
       ARRAY(
           SELECT DISTINCT status.status_id
           FROM death_combat_trace_entries AS trace
           JOIN death_combat_trace_statuses AS status
             ON status.namespace_id=trace.namespace_id AND status.death_id=trace.death_id
            AND status.trace_ordinal=trace.trace_ordinal
           WHERE trace.namespace_id=death.namespace_id AND trace.death_id=death.death_id
             AND trace.lethal
           ORDER BY status.status_id
       ) AS status_ids
FROM death_events AS death
JOIN death_summary_snapshots AS summary
  ON summary.namespace_id=death.namespace_id AND summary.death_id=death.death_id
WHERE death.namespace_id=$1 AND death.death_id=$2
";

#[derive(Clone, PartialEq, Eq)]
pub struct TelemetryPseudonymizationKeyV1([u8; 32]);

impl TelemetryPseudonymizationKeyV1 {
    pub fn new(bytes: [u8; 32]) -> Result<Self, M03TelemetryOutboxError> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Err(M03TelemetryOutboxError::InvalidPseudonymizationKey);
        }
        Ok(Self(bytes))
    }

    fn pseudonymize(
        &self,
        account_id: [u8; 16],
    ) -> Result<PseudonymousAccountId, M03TelemetryOutboxError> {
        let mut hasher = blake3::Hasher::new_keyed(&self.0);
        hasher.update(b"gravebound.telemetry.account-pseudonym.v1");
        hasher.update(&account_id);
        PseudonymousAccountId::new(*hasher.finalize().as_bytes()).map_err(Into::into)
    }
}

impl fmt::Debug for TelemetryPseudonymizationKeyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TelemetryPseudonymizationKeyV1([redacted])")
    }
}

impl Drop for TelemetryPseudonymizationKeyV1 {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M03TelemetryProjectionContextV1 {
    pub session_id: TelemetryId,
    pub build_id: StableTelemetryId,
    pub content_bundle_version: StableTelemetryId,
    pub platform: TelemetryPlatformV1,
    pub region_id: StableTelemetryId,
    pub environment: TelemetryEnvironmentV1,
    pub cohort_tags: Vec<StableTelemetryId>,
    pub pseudonymization_key: TelemetryPseudonymizationKeyV1,
}

impl M03TelemetryProjectionContextV1 {
    fn for_row(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<TelemetryContextV1, M03TelemetryOutboxError> {
        Ok(TelemetryContextV1 {
            pseudonymous_account_id: self.pseudonymization_key.pseudonymize(account_id)?,
            character_id: Some(TelemetryId::new(character_id)?),
            session_id: self.session_id,
            build_id: self.build_id.clone(),
            content_bundle_version: self.content_bundle_version.clone(),
            platform: self.platform,
            region_id: self.region_id.clone(),
            environment: self.environment,
            cohort_tags: self.cohort_tags.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum M03OutboxFamily {
    Death,
    Extraction,
    Recall,
    Successor,
}

impl M03OutboxFamily {
    fn decode(value: i16) -> Result<Self, M03TelemetryOutboxError> {
        match value {
            0 => Ok(Self::Death),
            1 => Ok(Self::Extraction),
            2 => Ok(Self::Recall),
            3 => Ok(Self::Successor),
            _ => Err(M03TelemetryOutboxError::CorruptSourceRow),
        }
    }
}

#[derive(Debug)]
struct RawCommittedRow {
    family: M03OutboxFamily,
    account_id: [u8; 16],
    character_id: [u8; 16],
    event_id: [u8; 16],
    payload: Vec<u8>,
    created_at_millis: u64,
    commit_order: u64,
    related_at_millis: Option<u64>,
}

#[derive(Debug)]
pub struct PostgresM03TelemetryOutboxAdapter {
    pool: PgPool,
    context: M03TelemetryProjectionContextV1,
    in_flight: BTreeMap<TelemetryId, M03OutboxFamily>,
}

impl PostgresM03TelemetryOutboxAdapter {
    #[must_use]
    pub fn new(pool: PgPool, context: M03TelemetryProjectionContextV1) -> Self {
        Self {
            pool,
            context,
            in_flight: BTreeMap::new(),
        }
    }

    async fn poll(
        &mut self,
        limit: usize,
    ) -> Result<Vec<CommittedOutboxEventV1>, M03TelemetryOutboxError> {
        if limit == 0 || limit > MAX_M03_TELEMETRY_POLL {
            return Err(M03TelemetryOutboxError::InvalidPollLimit);
        }
        let limit = i64::try_from(limit).map_err(|_| M03TelemetryOutboxError::InvalidPollLimit)?;
        let rows = sqlx::query(POLL_SQL)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        let mut projected = Vec::with_capacity(rows.len());
        for row in rows {
            let raw = RawCommittedRow {
                family: M03OutboxFamily::decode(row.try_get("family")?)?,
                account_id: exact_id(row.try_get("account_id")?)?,
                character_id: exact_id(row.try_get("character_id")?)?,
                event_id: exact_id(row.try_get("event_id")?)?,
                payload: row.try_get("event_payload")?,
                created_at_millis: positive(row.try_get("created_at_millis")?)?,
                commit_order: positive(row.try_get("commit_order")?)?,
                related_at_millis: optional_positive(row.try_get("related_at_millis")?)?,
            };
            let outbox_id = TelemetryId::new(raw.event_id)?;
            if self
                .in_flight
                .insert(outbox_id, raw.family)
                .is_some_and(|existing| existing != raw.family)
            {
                return Err(M03TelemetryOutboxError::DuplicateCrossFamilyEventId);
            }
            projected.push(self.project(raw).await?);
        }
        Ok(projected)
    }

    async fn project(
        &self,
        row: RawCommittedRow,
    ) -> Result<CommittedOutboxEventV1, M03TelemetryOutboxError> {
        let event = match row.family {
            M03OutboxFamily::Death => self.project_death(&row).await?,
            M03OutboxFamily::Extraction => {
                let result = StoredProductionExtractionResultV1::decode(&row.payload)?;
                if result.account_id != row.account_id || result.character_id != row.character_id {
                    return Err(M03TelemetryOutboxError::CorruptSourceRow);
                }
                let hold_count = result
                    .placements
                    .iter()
                    .filter(|placement| {
                        matches!(
                            placement.destination,
                            StoredExtractionLocationV1::ResolutionHold(_)
                        )
                    })
                    .count();
                TelemetryEventV1::Extraction(ExtractionEventV1 {
                    terminal_id: TelemetryId::new(result.terminal_id)?,
                    extraction_request_id: TelemetryId::new(result.extraction_request_id)?,
                    placed_item_count: u16::try_from(result.placements.len())
                        .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
                    credited_material_stack_count: u8::try_from(result.material_credits.len())
                        .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
                    resolution_hold_stack_count: u8::try_from(hold_count)
                        .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
                })
            }
            M03OutboxFamily::Recall => {
                let result = StoredProductionRecallResultV1::decode(&row.payload)?;
                if result.account_id != row.account_id || result.character_id != row.character_id {
                    return Err(M03TelemetryOutboxError::CorruptSourceRow);
                }
                let preserved_equipped = result
                    .stabilized_items
                    .iter()
                    .filter(|item| matches!(item.source, StoredRecallLocationV1::Equipped(_)))
                    .count();
                TelemetryEventV1::Recall(RecallEventV1 {
                    terminal_id: TelemetryId::new(result.terminal_id)?,
                    trigger: match result.trigger {
                        ProductionRecallTriggerV1::Explicit => RecallTriggerV1::Explicit,
                        ProductionRecallTriggerV1::LinkLost => RecallTriggerV1::LinkLost,
                    },
                    destroyed_pending_item_count: u16::try_from(result.destroyed_items.len())
                        .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
                    destroyed_material_stack_count: u8::try_from(result.destroyed_materials.len())
                        .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
                    preserved_equipped_item_count: u8::try_from(preserved_equipped)
                        .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
                })
            }
            M03OutboxFamily::Successor => {
                let result = StoredSuccessorResultV1::decode(&row.payload)?;
                if result.account_id != row.account_id || result.successor_id != row.character_id {
                    return Err(M03TelemetryOutboxError::CorruptSourceRow);
                }
                let death_at = row
                    .related_at_millis
                    .ok_or(M03TelemetryOutboxError::CorruptSourceRow)?;
                TelemetryEventV1::Successor(SuccessorEventV1::Created {
                    source_death_id: TelemetryId::new(result.death_id)?,
                    elapsed_from_summary_millis: row.created_at_millis.saturating_sub(death_at),
                })
            }
        };
        let envelope = VersionedTelemetryEnvelopeV1::new(
            TelemetryId::new(row.event_id)?,
            row.created_at_millis,
            self.context.for_row(row.account_id, row.character_id)?,
            event,
        )?;
        Ok(CommittedOutboxEventV1::from_committed_row(
            TelemetryId::new(row.event_id)?,
            row.commit_order,
            row.created_at_millis,
            envelope,
        )?)
    }

    async fn project_death(
        &self,
        source: &RawCommittedRow,
    ) -> Result<TelemetryEventV1, M03TelemetryOutboxError> {
        let stored = crate::StoredCommittedDeathResultV1::decode(&source.payload)?;
        if stored.account_id != source.account_id || stored.character_id != source.character_id {
            return Err(M03TelemetryOutboxError::CorruptSourceRow);
        }
        let row = sqlx::query(DEATH_DETAIL_SQL)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(stored.death_id.as_slice())
            .fetch_optional(&self.pool)
            .await?
            .ok_or(M03TelemetryOutboxError::CorruptSourceRow)?;
        let cause = match row.try_get::<i16, _>("cause_kind")? {
            0 => DeathCauseV1::DirectHit,
            1 => DeathCauseV1::DamageOverTime,
            2 => DeathCauseV1::Environment,
            3 => DeathCauseV1::Disconnect,
            _ => return Err(M03TelemetryOutboxError::CorruptSourceRow),
        };
        let damage_type = match row.try_get::<i16, _>("damage_type")? {
            0 => DamageTypeV1::Physical,
            1 => DamageTypeV1::Veil,
            _ => return Err(M03TelemetryOutboxError::CorruptSourceRow),
        };
        let recall_state = match row.try_get::<i16, _>("recall_state")? {
            0 => RecallStateV1::Idle,
            1 => RecallStateV1::Channeling,
            2 => RecallStateV1::LostRace,
            _ => return Err(M03TelemetryOutboxError::CorruptSourceRow),
        };
        let active_bargain_ids = stable_ids(row.try_get("bargain_ids")?)?;
        let status_ids = stable_ids(row.try_get("status_ids")?)?;
        let killer_content_id = row
            .try_get::<Option<String>, _>("killer_content_id")?
            .ok_or(M03TelemetryOutboxError::CorruptSourceRow)?;
        Ok(TelemetryEventV1::Death(Box::new(DeathEventV1 {
            death_id: TelemetryId::new(stored.death_id)?,
            class_id: StableTelemetryId::new(row.try_get::<String, _>("class_id")?)?,
            level: u16::try_from(row.try_get::<i16, _>("level")?)
                .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
            oath_id: row
                .try_get::<Option<String>, _>("oath_id")?
                .map(StableTelemetryId::new)
                .transpose()?,
            active_bargain_ids,
            lifetime_millis: positive_or_zero(row.try_get("lifetime_ms")?)?,
            session_duration_millis: None,
            killer_content_id: StableTelemetryId::new(killer_content_id)?,
            killer_pattern_id: row
                .try_get::<Option<String>, _>("killer_pattern_id")?
                .map(StableTelemetryId::new)
                .transpose()?,
            damage_type,
            raw_damage: u32::try_from(row.try_get::<i32, _>("raw_damage")?)
                .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
            final_damage: u32::try_from(row.try_get::<i32, _>("final_damage")?)
                .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
            pre_hit_health: u32::try_from(row.try_get::<i32, _>("pre_hit_health")?)
                .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)?,
            status_ids,
            dungeon_id: Some(StableTelemetryId::new(
                row.try_get::<String, _>("region_id")?,
            )?),
            room_id: Some(StableTelemetryId::new(
                row.try_get::<String, _>("room_id")?,
            )?),
            boss_phase_id: None,
            party_size: None,
            contribution_basis_points: None,
            item_power_band: None,
            network_health: None,
            recall_state,
            cause,
        })))
    }

    async fn acknowledge(
        &mut self,
        accepted: &[TelemetryId],
    ) -> Result<Vec<TelemetryId>, M03TelemetryOutboxError> {
        let mut sorted = accepted.to_vec();
        sorted.sort_unstable();
        if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(M03TelemetryOutboxError::InvalidAcknowledgement);
        }
        let mut transaction = self.pool.begin().await?;
        for event_id in &sorted {
            let family = self
                .in_flight
                .get(event_id)
                .copied()
                .ok_or(M03TelemetryOutboxError::UnknownAcknowledgement)?;
            acknowledge_one(&mut transaction, family, *event_id).await?;
        }
        transaction.commit().await?;
        for event_id in &sorted {
            self.in_flight.remove(event_id);
        }
        Ok(sorted)
    }
}

impl CommittedTelemetrySource for PostgresM03TelemetryOutboxAdapter {
    type Error = M03TelemetryOutboxError;

    async fn poll_unpublished(
        &mut self,
        limit: usize,
    ) -> Result<Vec<CommittedOutboxEventV1>, Self::Error> {
        self.poll(limit).await
    }

    async fn acknowledge_published(
        &mut self,
        accepted: &[TelemetryId],
    ) -> Result<Vec<TelemetryId>, Self::Error> {
        self.acknowledge(accepted).await
    }
}

async fn acknowledge_one(
    transaction: &mut Transaction<'_, Postgres>,
    family: M03OutboxFamily,
    event_id: TelemetryId,
) -> Result<(), M03TelemetryOutboxError> {
    let query = match family {
        M03OutboxFamily::Death => sqlx::query(
            "UPDATE death_outbox_events SET published_at=transaction_timestamp() \
             WHERE namespace_id=$1 AND event_id=$2 AND published_at IS NULL",
        ),
        M03OutboxFamily::Extraction => sqlx::query(
            "UPDATE extraction_terminal_outbox_events_v1 \
             SET published_at=transaction_timestamp() \
             WHERE namespace_id=$1 AND event_id=$2 AND published_at IS NULL",
        ),
        M03OutboxFamily::Recall => sqlx::query(
            "UPDATE recall_terminal_outbox_events_v1 SET published_at=transaction_timestamp() \
             WHERE namespace_id=$1 AND event_id=$2 AND published_at IS NULL",
        ),
        M03OutboxFamily::Successor => sqlx::query(
            "UPDATE successor_mutation_outbox_events_v1 \
             SET published_at=transaction_timestamp() \
             WHERE namespace_id=$1 AND event_id=$2 AND published_at IS NULL",
        ),
    };
    let affected = query
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(event_id.as_bytes().as_slice())
        .execute(&mut **transaction)
        .await?
        .rows_affected();
    if affected != 1 {
        return Err(M03TelemetryOutboxError::PublicationConflict);
    }
    Ok(())
}

fn stable_ids(values: Vec<String>) -> Result<Vec<StableTelemetryId>, M03TelemetryOutboxError> {
    values
        .into_iter()
        .map(StableTelemetryId::new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn exact_id(value: Vec<u8>) -> Result<[u8; 16], M03TelemetryOutboxError> {
    value
        .try_into()
        .map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)
}

fn positive(value: i64) -> Result<u64, M03TelemetryOutboxError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value != 0)
        .ok_or(M03TelemetryOutboxError::CorruptSourceRow)
}

fn positive_or_zero(value: i64) -> Result<u64, M03TelemetryOutboxError> {
    u64::try_from(value).map_err(|_| M03TelemetryOutboxError::CorruptSourceRow)
}

fn optional_positive(value: Option<i64>) -> Result<Option<u64>, M03TelemetryOutboxError> {
    value.map(positive).transpose()
}

#[derive(Debug, Error)]
pub enum M03TelemetryOutboxError {
    #[error("telemetry pseudonymization key must be nonzero")]
    InvalidPseudonymizationKey,
    #[error("telemetry poll limit is outside the supported bound")]
    InvalidPollLimit,
    #[error("committed telemetry outbox row is corrupt")]
    CorruptSourceRow,
    #[error("two outbox families contain the same event identity")]
    DuplicateCrossFamilyEventId,
    #[error("telemetry acknowledgement contains duplicate identities")]
    InvalidAcknowledgement,
    #[error("telemetry acknowledgement was not returned by this adapter")]
    UnknownAcknowledgement,
    #[error("durable telemetry publication marker conflicted")]
    PublicationConflict,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Stored(#[from] PersistenceError),
    #[error(transparent)]
    Identifier(#[from] TelemetryIdentifierError),
    #[error(transparent)]
    Event(#[from] TelemetryEventError),
    #[error(transparent)]
    Outbox(#[from] CommittedOutboxError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudonyms_are_deterministic_domain_separated_and_debug_redacted() {
        let first_key = TelemetryPseudonymizationKeyV1::new([0x31; 32]).unwrap();
        let second_key = TelemetryPseudonymizationKeyV1::new([0x32; 32]).unwrap();
        let account = [0x41; 16];
        assert_eq!(
            first_key.pseudonymize(account).unwrap(),
            first_key.pseudonymize(account).unwrap()
        );
        assert_ne!(
            first_key.pseudonymize(account).unwrap(),
            second_key.pseudonymize(account).unwrap()
        );
        let debug = format!("{first_key:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("31313131"));
        assert!(TelemetryPseudonymizationKeyV1::new([0; 32]).is_err());
    }

    #[test]
    fn polling_is_bounded_to_committed_terminal_outboxes_and_unpublished_rows() {
        for table in [
            "death_outbox_events",
            "extraction_terminal_outbox_events_v1",
            "recall_terminal_outbox_events_v1",
            "successor_mutation_outbox_events_v1",
        ] {
            assert!(POLL_SQL.contains(table), "missing {table}");
        }
        assert_eq!(POLL_SQL.matches("published_at IS NULL").count(), 4);
        assert!(POLL_SQL.contains("ORDER BY commit_order, event_id, family"));
        assert!(POLL_SQL.contains("LIMIT $2"));
        for live_table in [
            "character_world_locations",
            "character_inventory_heads",
            "active_instance",
            "session",
        ] {
            assert!(!POLL_SQL.contains(live_table), "read live {live_table}");
        }
    }
}
