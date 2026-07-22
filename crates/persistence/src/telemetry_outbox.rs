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
    CommittedOutboxError, CommittedOutboxEventV1, CommittedTelemetrySource, CrashEventV1,
    CrashKindV1, CrashSourceV1, DamageTypeV1, DeathCauseV1, DeathEventV1, ExtractionEventV1,
    LootActionV1, LootEventV1, OnboardingEventV1, PseudonymousAccountId, RecallEventV1,
    RecallStateV1, RecallTriggerV1, SessionEndReasonV1, SessionEventV1, StableTelemetryId,
    SuccessorEventV1, TelemetryContextV1, TelemetryEnvironmentV1, TelemetryEventError,
    TelemetryEventV1, TelemetryId, TelemetryIdentifierError, TelemetryPlatformV1,
    VersionedTelemetryEnvelopeV1,
};
use thiserror::Error;

use crate::{
    M03TelemetryPublicationV1, M03TelemetrySourceError, M03TelemetrySourceFamilyV1,
    PersistenceError, PostgresPersistence, ProductionRecallTriggerV1, StoredExtractionLocationV1,
    StoredM03CrashKindV1, StoredM03CrashSourceV1, StoredM03LootActionV1,
    StoredM03OnboardingEventV1, StoredM03SessionEndReasonV1, StoredM03SessionEventV1,
    StoredM03TelemetryContextV1, StoredM03TelemetryEnvironmentV1, StoredM03TelemetryEventV1,
    StoredM03TelemetryPlatformV1, StoredM03TelemetrySourceV1, StoredProductionExtractionResultV1,
    StoredProductionRecallResultV1, StoredRecallLocationV1, StoredSuccessorResultV1,
    WIPEABLE_CORE_NAMESPACE,
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

/// Adapter for the committed onboarding, logical-session, and redacted crash sources introduced
/// by schema 0070. Unlike the terminal adapter, every TEL-001 context field comes from the owning
/// durable session row; only the account pseudonymization key is process configuration.
#[derive(Debug)]
pub struct PostgresM03TelemetryDomainAdapter {
    persistence: PostgresPersistence,
    pseudonymization_key: TelemetryPseudonymizationKeyV1,
    in_flight: BTreeMap<TelemetryId, M03TelemetrySourceFamilyV1>,
}

impl PostgresM03TelemetryDomainAdapter {
    #[must_use]
    pub fn new(
        persistence: PostgresPersistence,
        pseudonymization_key: TelemetryPseudonymizationKeyV1,
    ) -> Self {
        Self {
            persistence,
            pseudonymization_key,
            in_flight: BTreeMap::new(),
        }
    }

    async fn poll_domain(
        &mut self,
        limit: usize,
    ) -> Result<Vec<CommittedOutboxEventV1>, M03TelemetryOutboxError> {
        if limit == 0 || limit > crate::MAX_M03_TELEMETRY_SOURCE_POLL_V1 {
            return Err(M03TelemetryOutboxError::InvalidPollLimit);
        }
        let sources = self
            .persistence
            .poll_m03_telemetry_sources_v1(limit)
            .await?;
        let mut next_in_flight = self.in_flight.clone();
        let mut projected = Vec::with_capacity(sources.len());
        for source in sources {
            let family = domain_family(&source.event);
            let outbox_id = TelemetryId::new(source.event_id)?;
            if next_in_flight
                .insert(outbox_id, family)
                .is_some_and(|existing| existing != family)
            {
                return Err(M03TelemetryOutboxError::DuplicateCrossFamilyEventId);
            }
            projected.push(project_domain_source(source, &self.pseudonymization_key)?);
        }
        self.in_flight = next_in_flight;
        Ok(projected)
    }

    async fn acknowledge_domain(
        &mut self,
        accepted: &[TelemetryId],
    ) -> Result<Vec<TelemetryId>, M03TelemetryOutboxError> {
        let mut canonical = accepted.to_vec();
        canonical.sort_unstable();
        if canonical.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(M03TelemetryOutboxError::InvalidAcknowledgement);
        }
        let publications = canonical
            .iter()
            .map(|event_id| {
                let family = self
                    .in_flight
                    .get(event_id)
                    .copied()
                    .ok_or(M03TelemetryOutboxError::UnknownAcknowledgement)?;
                Ok(M03TelemetryPublicationV1 {
                    family,
                    event_id: event_id.as_bytes(),
                })
            })
            .collect::<Result<Vec<_>, M03TelemetryOutboxError>>()?;
        let published = self
            .persistence
            .acknowledge_m03_telemetry_sources_v1(&publications)
            .await?;
        let published_ids = published
            .into_iter()
            .map(|publication| TelemetryId::new(publication.event_id))
            .collect::<Result<Vec<_>, _>>()?;
        if published_ids != canonical {
            return Err(M03TelemetryOutboxError::PublicationConflict);
        }
        for event_id in &canonical {
            self.in_flight.remove(event_id);
        }
        Ok(canonical)
    }
}

impl CommittedTelemetrySource for PostgresM03TelemetryDomainAdapter {
    type Error = M03TelemetryOutboxError;

    async fn poll_unpublished(
        &mut self,
        limit: usize,
    ) -> Result<Vec<CommittedOutboxEventV1>, Self::Error> {
        self.poll_domain(limit).await
    }

    async fn acknowledge_published(
        &mut self,
        accepted: &[TelemetryId],
    ) -> Result<Vec<TelemetryId>, Self::Error> {
        self.acknowledge_domain(accepted).await
    }
}

fn project_domain_source(
    source: StoredM03TelemetrySourceV1,
    pseudonymization_key: &TelemetryPseudonymizationKeyV1,
) -> Result<CommittedOutboxEventV1, M03TelemetryOutboxError> {
    let outbox_id = TelemetryId::new(source.event_id)?;
    let _source_id = TelemetryId::new(source.source_id)?;
    validate_domain_source_binding(&source)?;
    let event = project_domain_event(source.event)?;
    let context = project_domain_context(source.context, pseudonymization_key)?;
    let envelope = VersionedTelemetryEnvelopeV1::new(
        outbox_id,
        source.occurred_at_utc_millis,
        context,
        event,
    )?;
    let committed_at_utc_millis = source.commit_sequence / 1_000;
    if committed_at_utc_millis == 0 {
        return Err(M03TelemetryOutboxError::CorruptSourceRow);
    }
    Ok(CommittedOutboxEventV1::from_committed_row(
        outbox_id,
        source.commit_sequence,
        committed_at_utc_millis,
        envelope,
    )?)
}

fn project_domain_context(
    source: StoredM03TelemetryContextV1,
    pseudonymization_key: &TelemetryPseudonymizationKeyV1,
) -> Result<TelemetryContextV1, M03TelemetryOutboxError> {
    Ok(TelemetryContextV1 {
        pseudonymous_account_id: pseudonymization_key.pseudonymize(source.account_id)?,
        character_id: source.character_id.map(TelemetryId::new).transpose()?,
        session_id: TelemetryId::new(source.session_id)?,
        build_id: StableTelemetryId::new(source.build_id)?,
        content_bundle_version: StableTelemetryId::new(source.content_bundle_version)?,
        platform: match source.platform {
            StoredM03TelemetryPlatformV1::Windows => TelemetryPlatformV1::Windows,
            StoredM03TelemetryPlatformV1::Linux => TelemetryPlatformV1::Linux,
            StoredM03TelemetryPlatformV1::MacOs => TelemetryPlatformV1::MacOs,
            StoredM03TelemetryPlatformV1::Unknown => TelemetryPlatformV1::Unknown,
        },
        region_id: StableTelemetryId::new(source.region_id)?,
        environment: match source.environment {
            StoredM03TelemetryEnvironmentV1::Local => TelemetryEnvironmentV1::Local,
            StoredM03TelemetryEnvironmentV1::Test => TelemetryEnvironmentV1::Test,
            StoredM03TelemetryEnvironmentV1::Staging => TelemetryEnvironmentV1::Staging,
            StoredM03TelemetryEnvironmentV1::Production => TelemetryEnvironmentV1::Production,
        },
        cohort_tags: source
            .cohort_tags
            .into_iter()
            .map(StableTelemetryId::new)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn project_domain_event(
    source: StoredM03TelemetryEventV1,
) -> Result<TelemetryEventV1, M03TelemetryOutboxError> {
    Ok(match source {
        StoredM03TelemetryEventV1::Onboarding(event) => {
            TelemetryEventV1::Onboarding(match event {
                StoredM03OnboardingEventV1::AccountCreated => OnboardingEventV1::AccountCreated,
                StoredM03OnboardingEventV1::CharacterCreated { class_id } => {
                    OnboardingEventV1::CharacterCreated {
                        class_id: StableTelemetryId::new(class_id)?,
                    }
                }
                StoredM03OnboardingEventV1::CharacterEnteredCombat {
                    class_id,
                    source_content_id,
                } => {
                    // The v1 envelope has no content-source payload field. Validate the durable
                    // stable ID before projecting the exact required TEL-002 event.
                    let _source_content_id = StableTelemetryId::new(source_content_id)?;
                    OnboardingEventV1::CharacterEnteredCombat {
                        class_id: StableTelemetryId::new(class_id)?,
                    }
                }
            })
        }
        StoredM03TelemetryEventV1::Session(event) => TelemetryEventV1::Session(match event {
            StoredM03SessionEventV1::Started => SessionEventV1::Started,
            StoredM03SessionEventV1::Ended {
                duration_millis,
                reason,
            } => SessionEventV1::Ended {
                duration_millis,
                reason: match reason {
                    StoredM03SessionEndReasonV1::CleanExit => SessionEndReasonV1::CleanExit,
                    StoredM03SessionEndReasonV1::LinkLost => SessionEndReasonV1::LinkLost,
                    StoredM03SessionEndReasonV1::TransportClosed => {
                        SessionEndReasonV1::TransportClosed
                    }
                    StoredM03SessionEndReasonV1::ClientCrash => SessionEndReasonV1::ClientCrash,
                    StoredM03SessionEndReasonV1::ServerShutdown => {
                        SessionEndReasonV1::ServerShutdown
                    }
                },
            },
            StoredM03SessionEventV1::Disconnected => SessionEventV1::Disconnected,
            StoredM03SessionEventV1::Reconnected { link_lost_millis } => {
                SessionEventV1::Reconnected { link_lost_millis }
            }
        }),
        StoredM03TelemetryEventV1::Crash(event) => TelemetryEventV1::Crash(CrashEventV1 {
            crash_id: TelemetryId::new(event.crash_id)?,
            source: match event.source {
                StoredM03CrashSourceV1::Client => CrashSourceV1::Client,
                StoredM03CrashSourceV1::Server => CrashSourceV1::Server,
            },
            kind: match event.kind {
                StoredM03CrashKindV1::Panic => CrashKindV1::Panic,
                StoredM03CrashKindV1::AccessViolation => CrashKindV1::AccessViolation,
                StoredM03CrashKindV1::OutOfMemory => CrashKindV1::OutOfMemory,
                StoredM03CrashKindV1::Watchdog => CrashKindV1::Watchdog,
                StoredM03CrashKindV1::Unknown => CrashKindV1::Unknown,
            },
            signature: event.signature,
            uptime_millis: event.uptime_millis,
        }),
        StoredM03TelemetryEventV1::Loot(event) => TelemetryEventV1::Loot(LootEventV1 {
            action: match event.action {
                StoredM03LootActionV1::Created => LootActionV1::Created,
                StoredM03LootActionV1::PickedUp => LootActionV1::PickedUp,
                StoredM03LootActionV1::Equipped => LootActionV1::Equipped,
                StoredM03LootActionV1::Extracted => LootActionV1::Extracted,
                StoredM03LootActionV1::Destroyed => LootActionV1::Destroyed,
            },
            item_id: TelemetryId::new(event.item_uid)?,
            template_id: StableTelemetryId::new(event.template_id)?,
            source_content_id: StableTelemetryId::new(event.source_content_id)?,
            item_version: event.item_version,
        }),
    })
}

fn validate_domain_source_binding(
    source: &StoredM03TelemetrySourceV1,
) -> Result<(), M03TelemetryOutboxError> {
    let valid = match &source.event {
        StoredM03TelemetryEventV1::Onboarding(StoredM03OnboardingEventV1::AccountCreated) => {
            source.context.character_id.is_none() && source.source_id == source.context.account_id
        }
        StoredM03TelemetryEventV1::Onboarding(
            StoredM03OnboardingEventV1::CharacterCreated { .. }
            | StoredM03OnboardingEventV1::CharacterEnteredCombat { .. },
        ) => source.context.character_id == Some(source.source_id),
        StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Started) => {
            source.context.character_id.is_none() && source.source_id == source.context.session_id
        }
        StoredM03TelemetryEventV1::Session(_) => source.context.character_id.is_none(),
        StoredM03TelemetryEventV1::Crash(event) => source.source_id == event.crash_id,
        StoredM03TelemetryEventV1::Loot(_) => source.context.character_id.is_some(),
    };
    if !valid {
        return Err(M03TelemetryOutboxError::CorruptSourceRow);
    }
    Ok(())
}

const fn domain_family(event: &StoredM03TelemetryEventV1) -> M03TelemetrySourceFamilyV1 {
    match event {
        StoredM03TelemetryEventV1::Onboarding(_) => M03TelemetrySourceFamilyV1::Onboarding,
        StoredM03TelemetryEventV1::Session(_) => M03TelemetrySourceFamilyV1::Session,
        StoredM03TelemetryEventV1::Crash(_) => M03TelemetrySourceFamilyV1::Crash,
        StoredM03TelemetryEventV1::Loot(_) => M03TelemetrySourceFamilyV1::Loot,
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
    #[error(transparent)]
    DomainSource(#[from] M03TelemetrySourceError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use telemetry::{
        TelemetryConnectivity, TelemetryIngestOutcome, TelemetryPipeline, TelemetryPipelineMode,
    };

    const DOMAIN_ACCOUNT: [u8; 16] = [0x41; 16];
    const DOMAIN_CHARACTER: [u8; 16] = [0x42; 16];
    const DOMAIN_SESSION: [u8; 16] = [0x43; 16];

    fn domain_source(
        source_id: [u8; 16],
        character_id: Option<[u8; 16]>,
        event: StoredM03TelemetryEventV1,
    ) -> StoredM03TelemetrySourceV1 {
        StoredM03TelemetrySourceV1 {
            event_id: [0x51; 16],
            source_id,
            commit_sequence: 1_750_000_000_100_000,
            occurred_at_utc_millis: 1_750_000_000_000,
            context: StoredM03TelemetryContextV1 {
                account_id: DOMAIN_ACCOUNT,
                character_id,
                session_id: DOMAIN_SESSION,
                build_id: "m03-core-dev-telemetry-1".into(),
                content_bundle_version: "core-dev".into(),
                platform: StoredM03TelemetryPlatformV1::Windows,
                region_id: "local".into(),
                environment: StoredM03TelemetryEnvironmentV1::Test,
                cohort_tags: vec!["cohort.private".into(), "staff".into()],
            },
            event,
        }
    }

    fn schema_0070_0071_mapping_cases() -> Vec<(StoredM03TelemetrySourceV1, &'static str)> {
        vec![
            (
                domain_source(
                    DOMAIN_ACCOUNT,
                    None,
                    StoredM03TelemetryEventV1::Onboarding(
                        StoredM03OnboardingEventV1::AccountCreated,
                    ),
                ),
                "account_created",
            ),
            (
                domain_source(
                    DOMAIN_CHARACTER,
                    Some(DOMAIN_CHARACTER),
                    StoredM03TelemetryEventV1::Onboarding(
                        StoredM03OnboardingEventV1::CharacterCreated {
                            class_id: "class.grave_arbalist".into(),
                        },
                    ),
                ),
                "character_created",
            ),
            (
                domain_source(
                    DOMAIN_CHARACTER,
                    Some(DOMAIN_CHARACTER),
                    StoredM03TelemetryEventV1::Onboarding(
                        StoredM03OnboardingEventV1::CharacterEnteredCombat {
                            class_id: "class.grave_arbalist".into(),
                            source_content_id: "world.core_microrealm_01".into(),
                        },
                    ),
                ),
                "character_entered_combat",
            ),
            (
                domain_source(
                    DOMAIN_SESSION,
                    None,
                    StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Started),
                ),
                "session_started",
            ),
            (
                domain_source(
                    [0x44; 16],
                    None,
                    StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Ended {
                        duration_millis: 100,
                        reason: StoredM03SessionEndReasonV1::ServerShutdown,
                    }),
                ),
                "session_ended",
            ),
            (
                domain_source(
                    [0x45; 16],
                    None,
                    StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Disconnected),
                ),
                "disconnect",
            ),
            (
                domain_source(
                    [0x46; 16],
                    None,
                    StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Reconnected {
                        link_lost_millis: 50,
                    }),
                ),
                "reconnect",
            ),
            (
                domain_source(
                    [0x47; 16],
                    Some(DOMAIN_CHARACTER),
                    StoredM03TelemetryEventV1::Loot(crate::StoredM03LootEventV1 {
                        action: StoredM03LootActionV1::Created,
                        item_uid: [0x48; 16],
                        template_id: "item.weapon.crossbow.pine.t1".into(),
                        source_content_id: "reward.normal_outer".into(),
                        item_version: 1,
                    }),
                ),
                "item_created",
            ),
        ]
    }

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

    #[test]
    fn schema_0070_projection_uses_durable_context_and_redacts_the_account() {
        let crash_id = [0x61; 16];
        let source = domain_source(
            crash_id,
            Some(DOMAIN_CHARACTER),
            StoredM03TelemetryEventV1::Crash(crate::StoredM03CrashEventV1 {
                crash_id,
                source: StoredM03CrashSourceV1::Client,
                kind: StoredM03CrashKindV1::Panic,
                reporter: crate::StoredM03CrashReporterV1::AuthenticatedClient,
                signature: [0x71; 32],
                uptime_millis: 900,
            }),
        );
        let projected = project_domain_source(
            source,
            &TelemetryPseudonymizationKeyV1::new([0x81; 32]).unwrap(),
        )
        .unwrap();
        assert_eq!(projected.envelope().event_name(), "client_crash");
        assert_eq!(projected.commit_sequence(), 1_750_000_000_100_000);
        assert_eq!(projected.committed_at_utc_millis(), 1_750_000_000_100);

        let mut pipeline = TelemetryPipeline::new(
            TelemetryPipelineMode::Enabled,
            TelemetryConnectivity::Online,
            1,
        )
        .unwrap();
        assert_eq!(
            pipeline.ingest_committed(projected),
            TelemetryIngestOutcome::Queued
        );
        let document = pipeline.prepare_redacted_batch(1).unwrap().remove(0);
        for exact_context in [
            "m03-core-dev-telemetry-1",
            "core-dev",
            "\"platform\":\"windows\"",
            "\"region_id\":\"local\"",
            "\"environment\":\"test\"",
            "\"cohort_tags\":[\"cohort.private\",\"staff\"]",
        ] {
            assert!(
                document.json.contains(exact_context),
                "missing {exact_context}"
            );
        }
        assert!(!document.json.contains(&"41".repeat(16)));
        for forbidden in ["reporter", "stack", "message", "auth_ticket", "ip_address"] {
            assert!(!document.json.contains(forbidden));
        }
    }

    #[test]
    fn schema_0070_0071_event_mapping_and_source_bindings_are_closed() {
        let key = TelemetryPseudonymizationKeyV1::new([0x91; 32]).unwrap();
        for (index, (mut source, expected_name)) in
            schema_0070_0071_mapping_cases().into_iter().enumerate()
        {
            source.event_id[0] = u8::try_from(index + 1).unwrap();
            let projected = project_domain_source(source, &key).unwrap();
            assert_eq!(projected.envelope().event_name(), expected_name);
        }

        let mut mismatched = domain_source(
            [0xa1; 16],
            None,
            StoredM03TelemetryEventV1::Onboarding(StoredM03OnboardingEventV1::AccountCreated),
        );
        assert!(matches!(
            project_domain_source(mismatched.clone(), &key),
            Err(M03TelemetryOutboxError::CorruptSourceRow)
        ));
        mismatched.context.character_id = Some(DOMAIN_CHARACTER);
        assert!(matches!(
            project_domain_source(mismatched, &key),
            Err(M03TelemetryOutboxError::CorruptSourceRow)
        ));
    }
}
