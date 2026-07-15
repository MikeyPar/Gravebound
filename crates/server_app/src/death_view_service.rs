//! Authenticated, read-only death-summary and Memorial Wall authority for `GB-M03-06A/06D`.
//!
//! The service is intentionally a reader over committed domain records. It follows
//! `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-001`, `DTH-020`, `TECH-020`-`022`),
//! `Gravebound_Content_Production_Spec_v1.md` (`CONT-HUB-002`, `CONT-ECHO-009`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-06`). It cannot stage lethal outcomes or
//! reconstruct snapshots from mutable character state.

use std::future::Future;

use persistence::{
    DeathViewReadError, DurableCombatTraceEntryV1, DurableDamageTypeV1, DurableDeathCauseV1,
    DurableDeathPresentationAuthorityV1, DurableEchoOutcomeV1, DurableNetworkStateV1,
    DurableRecallStateV1, DurableSummaryProjectionEntryV1, DurableSummaryProjectionKindV1,
    PostgresPersistence, StoredDeathMemorialCursorV1, StoredDeathMemorialEntryV1,
    StoredDeathSummaryViewV1, StoredDeathTracePageV1, StoredLatestCommittedDeathV1,
};
use protocol::{
    DEATH_VIEW_SCHEMA_VERSION, DeathCauseV1, DeathCharacterName, DeathDamageTypeV1,
    DeathEchoOutcomeV1, DeathMemorialCursorV1, DeathMemorialEntryV1, DeathNetworkStateV1,
    DeathRecallStateV1, DeathSummaryProjectionEntryV1, DeathSummaryProjectionKindV1,
    DeathSummaryViewV1, DeathTraceEntryV1, DeathTracePageV1, DeathTraceStatusV1,
    DeathViewContentRevisionV1, DeathViewFrameV1, DeathViewRequestV1, DeathViewResultCodeV1,
    DeathViewResultV1, LatestCommittedDeathV1, ManifestHash, WireText,
};

use crate::{AuthenticatedAccount, AuthenticatedNamespace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathViewRepositoryError {
    FeatureDisabled,
    DeathNotFound,
    DeathNotOwned,
    PageOutOfRange,
    ContentMismatch,
    CorruptStoredRecord,
    ServiceUnavailable,
}

pub trait DeathViewRepository: Send + Sync {
    fn latest(
        &self,
        account_id: [u8; 16],
    ) -> impl Future<Output = Result<Option<LatestCommittedDeathV1>, DeathViewRepositoryError>> + Send;

    fn summary(
        &self,
        account_id: [u8; 16],
        death_id: [u8; 16],
        lost_start_ordinal: u16,
        lost_limit: u16,
    ) -> impl Future<Output = Result<DeathSummaryViewV1, DeathViewRepositoryError>> + Send;

    fn memorial_page(
        &self,
        account_id: [u8; 16],
        after: Option<DeathMemorialCursorV1>,
        limit: u8,
    ) -> impl Future<
        Output = Result<
            (Vec<DeathMemorialEntryV1>, Option<DeathMemorialCursorV1>),
            DeathViewRepositoryError,
        >,
    > + Send;

    fn trace_page(
        &self,
        account_id: [u8; 16],
        death_id: [u8; 16],
        start_ordinal: u16,
        limit: u8,
    ) -> impl Future<Output = Result<DeathTracePageV1, DeathViewRepositoryError>> + Send;
}

/// Explicit fail-closed adapter for process-local and otherwise nonpersistent routes.
#[derive(Debug, Clone, Copy, Default)]
pub struct DisabledDeathViewRepository;

impl DeathViewRepository for DisabledDeathViewRepository {
    async fn latest(
        &self,
        _account_id: [u8; 16],
    ) -> Result<Option<LatestCommittedDeathV1>, DeathViewRepositoryError> {
        Err(DeathViewRepositoryError::FeatureDisabled)
    }

    async fn summary(
        &self,
        _account_id: [u8; 16],
        _death_id: [u8; 16],
        _lost_start_ordinal: u16,
        _lost_limit: u16,
    ) -> Result<DeathSummaryViewV1, DeathViewRepositoryError> {
        Err(DeathViewRepositoryError::FeatureDisabled)
    }

    async fn memorial_page(
        &self,
        _account_id: [u8; 16],
        _after: Option<DeathMemorialCursorV1>,
        _limit: u8,
    ) -> Result<(Vec<DeathMemorialEntryV1>, Option<DeathMemorialCursorV1>), DeathViewRepositoryError>
    {
        Err(DeathViewRepositoryError::FeatureDisabled)
    }

    async fn trace_page(
        &self,
        _account_id: [u8; 16],
        _death_id: [u8; 16],
        _start_ordinal: u16,
        _limit: u8,
    ) -> Result<DeathTracePageV1, DeathViewRepositoryError> {
        Err(DeathViewRepositoryError::FeatureDisabled)
    }
}

#[derive(Debug, Clone)]
pub struct PostgresDeathViewRepository {
    persistence: PostgresPersistence,
}

impl PostgresDeathViewRepository {
    #[must_use]
    pub const fn new(persistence: PostgresPersistence) -> Self {
        Self { persistence }
    }
}

impl DeathViewRepository for PostgresDeathViewRepository {
    async fn latest(
        &self,
        account_id: [u8; 16],
    ) -> Result<Option<LatestCommittedDeathV1>, DeathViewRepositoryError> {
        let stored = self
            .persistence
            .load_latest_committed_death_view(account_id)
            .await
            .map_err(map_read_error)?;
        stored.map(latest_projection).transpose()
    }

    async fn summary(
        &self,
        account_id: [u8; 16],
        death_id: [u8; 16],
        lost_start_ordinal: u16,
        lost_limit: u16,
    ) -> Result<DeathSummaryViewV1, DeathViewRepositoryError> {
        let stored = self
            .persistence
            .load_owned_death_summary_view(account_id, death_id, lost_start_ordinal, lost_limit)
            .await
            .map_err(map_read_error)?;
        summary_projection(stored)
    }

    async fn memorial_page(
        &self,
        account_id: [u8; 16],
        after: Option<DeathMemorialCursorV1>,
        limit: u8,
    ) -> Result<(Vec<DeathMemorialEntryV1>, Option<DeathMemorialCursorV1>), DeathViewRepositoryError>
    {
        let stored = self
            .persistence
            .load_death_memorial_page(account_id, after.map(stored_cursor), limit)
            .await
            .map_err(map_read_error)?;
        Ok((
            stored
                .entries
                .into_iter()
                .map(memorial_projection)
                .collect::<Result<_, _>>()?,
            stored.next_cursor.map(protocol_cursor),
        ))
    }

    async fn trace_page(
        &self,
        account_id: [u8; 16],
        death_id: [u8; 16],
        start_ordinal: u16,
        limit: u8,
    ) -> Result<DeathTracePageV1, DeathViewRepositoryError> {
        trace_page_projection(
            self.persistence
                .load_owned_death_trace_page(account_id, death_id, start_ordinal, limit)
                .await
                .map_err(map_read_error)?,
        )
    }
}

#[derive(Debug, Clone)]
pub struct DeathViewService<Repository> {
    repository: Repository,
    content_revision: DeathViewContentRevisionV1,
}

impl<Repository> DeathViewService<Repository>
where
    Repository: DeathViewRepository,
{
    #[must_use]
    pub const fn new(repository: Repository, content_revision: DeathViewContentRevisionV1) -> Self {
        Self {
            repository,
            content_revision,
        }
    }

    pub async fn handle(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &DeathViewFrameV1,
    ) -> DeathViewResultV1 {
        if frame.validate().is_err()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        {
            return error(frame.sequence, DeathViewResultCodeV1::FeatureDisabled);
        }
        if frame.content_revision != self.content_revision {
            return error(frame.sequence, DeathViewResultCodeV1::ContentMismatch);
        }

        let account_id = authenticated.account_id.as_bytes();
        let result = match &frame.request {
            DeathViewRequestV1::LatestCommitted => {
                self.repository
                    .latest(account_id)
                    .await
                    .map(|death| DeathViewResultV1::Latest {
                        schema_version: DEATH_VIEW_SCHEMA_VERSION,
                        request_sequence: frame.sequence,
                        death,
                    })
            }
            DeathViewRequestV1::Summary {
                death_id,
                lost_start_ordinal,
                lost_limit,
            } => self
                .repository
                .summary(account_id, *death_id, *lost_start_ordinal, *lost_limit)
                .await
                .map(|summary| DeathViewResultV1::Summary {
                    schema_version: DEATH_VIEW_SCHEMA_VERSION,
                    request_sequence: frame.sequence,
                    requested_lost_limit: *lost_limit,
                    summary,
                }),
            DeathViewRequestV1::MemorialPage { after, limit } => self
                .repository
                .memorial_page(account_id, *after, *limit)
                .await
                .map(|(entries, next_cursor)| DeathViewResultV1::MemorialPage {
                    schema_version: DEATH_VIEW_SCHEMA_VERSION,
                    request_sequence: frame.sequence,
                    requested_limit: *limit,
                    entries,
                    next_cursor,
                }),
            DeathViewRequestV1::TracePage {
                death_id,
                start_ordinal,
                limit,
            } => self
                .repository
                .trace_page(account_id, *death_id, *start_ordinal, *limit)
                .await
                .map(|page| DeathViewResultV1::TracePage {
                    schema_version: DEATH_VIEW_SCHEMA_VERSION,
                    request_sequence: frame.sequence,
                    requested_limit: *limit,
                    page,
                }),
        };
        match result {
            Ok(result) if result.validate().is_err() => {
                error(frame.sequence, DeathViewResultCodeV1::CorruptStoredRecord)
            }
            Ok(result) if !result_uses_supported_presentation(&result, &self.content_revision) => {
                error(frame.sequence, DeathViewResultCodeV1::ContentMismatch)
            }
            Ok(result) => result,
            Err(cause) => error(frame.sequence, result_code(cause)),
        }
    }
}

fn result_uses_supported_presentation(
    result: &DeathViewResultV1,
    required: &DeathViewContentRevisionV1,
) -> bool {
    match result {
        DeathViewResultV1::Latest { death, .. } => death
            .as_ref()
            .is_none_or(|death| death.presentation_revision == *required),
        DeathViewResultV1::Summary { summary, .. } => summary.presentation_revision == *required,
        DeathViewResultV1::MemorialPage { entries, .. } => entries
            .iter()
            .all(|entry| entry.presentation_revision == *required),
        DeathViewResultV1::TracePage { page, .. } => page.presentation_revision == *required,
        DeathViewResultV1::Error { .. } => true,
    }
}

fn latest_projection(
    stored: StoredLatestCommittedDeathV1,
) -> Result<LatestCommittedDeathV1, DeathViewRepositoryError> {
    Ok(LatestCommittedDeathV1 {
        death_id: stored.death_id,
        character_id: stored.character_id,
        death_at_unix_ms: stored.death_at_unix_ms,
        death_tick: stored.death_tick,
        cause: cause(stored.cause),
        killer_content_id: text(stored.killer_content_id)?,
        killer_pattern_id: stored.killer_pattern_id.map(text).transpose()?,
        network_state: network_state(stored.network_state),
        recall_state: recall_state(stored.recall_state),
        trace_entry_count: stored.trace_entry_count,
        trace_digest: stored.trace_digest,
        destruction_entry_count: stored.destruction_entry_count,
        destruction_digest: stored.destruction_digest,
        summary_snapshot_digest: stored.summary_snapshot_digest,
        content_revision: text(stored.content_revision)?,
        presentation_revision: presentation_revision(stored.presentation)?,
    })
}

fn summary_projection(
    stored: StoredDeathSummaryViewV1,
) -> Result<DeathSummaryViewV1, DeathViewRepositoryError> {
    Ok(DeathSummaryViewV1 {
        death_id: stored.death_id,
        summary_revision: stored.summary_revision,
        hero_label_key: text(stored.hero_label_key)?,
        character_name_snapshot: DeathCharacterName::new(stored.character_name_snapshot)
            .map_err(|_| corrupt())?,
        class_id: text(stored.class_id)?,
        level: stored.level,
        oath_id: stored.oath_id.map(text).transpose()?,
        bargains: stored
            .bargains
            .into_iter()
            .map(|entry| text(entry.content_id))
            .collect::<Result<_, _>>()?,
        lifetime_ms: stored.lifetime_ms,
        final_deed_id: text(stored.final_deed_id)?,
        lethal_trace_ordinal: stored.lethal_trace_ordinal,
        last_five_damage: stored
            .last_five_damage
            .into_iter()
            .map(trace_projection)
            .collect::<Result<_, _>>()?,
        lost_total_count: stored.lost_total_count,
        lost_start_ordinal: stored.lost_start_ordinal,
        lost: stored
            .lost
            .into_iter()
            .map(summary_entry_projection)
            .collect::<Result<_, _>>()?,
        next_lost_ordinal: stored.next_lost_ordinal,
        preserved: stored
            .preserved
            .into_iter()
            .map(summary_entry_projection)
            .collect::<Result<_, _>>()?,
        created: stored
            .created
            .into_iter()
            .map(summary_entry_projection)
            .collect::<Result<_, _>>()?,
        echo_outcome: echo_outcome(stored.echo_outcome),
        death_tick: stored.death_tick,
        content_revision: text(stored.content_revision)?,
        presentation_revision: presentation_revision(stored.presentation)?,
        snapshot_digest: stored.snapshot_digest,
    })
}

fn memorial_projection(
    stored: StoredDeathMemorialEntryV1,
) -> Result<DeathMemorialEntryV1, DeathViewRepositoryError> {
    Ok(DeathMemorialEntryV1 {
        cursor: protocol_cursor(stored.cursor),
        summary_revision: stored.summary_revision,
        summary_snapshot_digest: stored.summary_snapshot_digest,
        presentation_key: text(stored.presentation_key)?,
        presentation_digest: stored.presentation_digest,
        character_name_snapshot: DeathCharacterName::new(stored.character_name_snapshot)
            .map_err(|_| corrupt())?,
        class_id: text(stored.class_id)?,
        level: stored.level,
        echo_outcome: echo_outcome(stored.echo_outcome),
        presentation_revision: presentation_revision(stored.presentation)?,
    })
}

fn presentation_revision(
    stored: DurableDeathPresentationAuthorityV1,
) -> Result<DeathViewContentRevisionV1, DeathViewRepositoryError> {
    Ok(DeathViewContentRevisionV1 {
        records_blake3: ManifestHash::new(stored.records_blake3).map_err(|_| corrupt())?,
        assets_blake3: ManifestHash::new(stored.assets_blake3).map_err(|_| corrupt())?,
        localization_blake3: ManifestHash::new(stored.localization_blake3)
            .map_err(|_| corrupt())?,
    })
}

fn trace_page_projection(
    stored: StoredDeathTracePageV1,
) -> Result<DeathTracePageV1, DeathViewRepositoryError> {
    Ok(DeathTracePageV1 {
        death_id: stored.death_id,
        presentation_revision: presentation_revision(stored.presentation)?,
        death_tick: stored.death_tick,
        total_entry_count: stored.total_entry_count,
        trace_digest: stored.trace_digest,
        start_ordinal: stored.start_ordinal,
        entries: stored
            .entries
            .into_iter()
            .map(trace_projection)
            .collect::<Result<_, _>>()?,
        next_ordinal: stored.next_ordinal,
    })
}

fn trace_projection(
    stored: DurableCombatTraceEntryV1,
) -> Result<DeathTraceEntryV1, DeathViewRepositoryError> {
    Ok(DeathTraceEntryV1 {
        ordinal: stored.ordinal,
        event_tick: stored.event_tick,
        event_ordinal: stored.event_ordinal,
        source_content_id: text(stored.source_content_id)?,
        source_entity_id: stored.source_entity_id,
        pattern_id: stored.pattern_id.map(text).transpose()?,
        attack_id: text(stored.attack_id)?,
        raw_damage: stored.raw_damage,
        final_damage: stored.final_damage,
        damage_type: damage_type(stored.damage_type),
        pre_health: stored.pre_health,
        post_health: stored.post_health,
        source_x_milli_tiles: stored.source_x_milli_tiles,
        source_y_milli_tiles: stored.source_y_milli_tiles,
        network_state: network_state(stored.network_state),
        recall_state: recall_state(stored.recall_state),
        lethal: stored.lethal,
        statuses: stored
            .statuses
            .into_iter()
            .map(|status| {
                Ok(DeathTraceStatusV1 {
                    ordinal: status.ordinal,
                    status_id: text(status.status_id)?,
                    remaining_ticks: status.remaining_ticks,
                    stack_count: status.stack_count,
                })
            })
            .collect::<Result<_, DeathViewRepositoryError>>()?,
    })
}

fn summary_entry_projection(
    stored: DurableSummaryProjectionEntryV1,
) -> Result<DeathSummaryProjectionEntryV1, DeathViewRepositoryError> {
    Ok(DeathSummaryProjectionEntryV1 {
        ordinal: stored.ordinal,
        kind: match stored.kind {
            DurableSummaryProjectionKindV1::LostItem => DeathSummaryProjectionKindV1::LostItem,
            DurableSummaryProjectionKindV1::LostRunMaterial => {
                DeathSummaryProjectionKindV1::LostRunMaterial
            }
            DurableSummaryProjectionKindV1::PreservedAccountRecords => {
                DeathSummaryProjectionKindV1::PreservedAccountRecords
            }
            DurableSummaryProjectionKindV1::PreservedCurrency => {
                DeathSummaryProjectionKindV1::PreservedCurrency
            }
            DurableSummaryProjectionKindV1::PreservedVault => {
                DeathSummaryProjectionKindV1::PreservedVault
            }
            DurableSummaryProjectionKindV1::PreservedCosmetics => {
                DeathSummaryProjectionKindV1::PreservedCosmetics
            }
            DurableSummaryProjectionKindV1::PreservedRecipes => {
                DeathSummaryProjectionKindV1::PreservedRecipes
            }
            DurableSummaryProjectionKindV1::CreatedMemorial => {
                DeathSummaryProjectionKindV1::CreatedMemorial
            }
            DurableSummaryProjectionKindV1::CreatedEcho => {
                DeathSummaryProjectionKindV1::CreatedEcho
            }
        },
        content_id: text(stored.content_id)?,
        quantity: stored.quantity,
        item_uid: stored.item_uid,
    })
}

const fn cause(value: DurableDeathCauseV1) -> DeathCauseV1 {
    match value {
        DurableDeathCauseV1::DirectHit => DeathCauseV1::DirectHit,
        DurableDeathCauseV1::DamageOverTime => DeathCauseV1::DamageOverTime,
        DurableDeathCauseV1::Environment => DeathCauseV1::Environment,
        DurableDeathCauseV1::Disconnect => DeathCauseV1::Disconnect,
    }
}

const fn damage_type(value: DurableDamageTypeV1) -> DeathDamageTypeV1 {
    match value {
        DurableDamageTypeV1::Physical => DeathDamageTypeV1::Physical,
        DurableDamageTypeV1::Veil => DeathDamageTypeV1::Veil,
    }
}

const fn network_state(value: DurableNetworkStateV1) -> DeathNetworkStateV1 {
    match value {
        DurableNetworkStateV1::Connected => DeathNetworkStateV1::Connected,
        DurableNetworkStateV1::Degraded => DeathNetworkStateV1::Degraded,
        DurableNetworkStateV1::LinkLost => DeathNetworkStateV1::LinkLost,
        DurableNetworkStateV1::Reattached => DeathNetworkStateV1::Reattached,
    }
}

const fn recall_state(value: DurableRecallStateV1) -> DeathRecallStateV1 {
    match value {
        DurableRecallStateV1::Inactive => DeathRecallStateV1::Inactive,
        DurableRecallStateV1::Channeling => DeathRecallStateV1::Channeling,
        DurableRecallStateV1::CompletionPending => DeathRecallStateV1::CompletionPending,
    }
}

const fn echo_outcome(value: DurableEchoOutcomeV1) -> DeathEchoOutcomeV1 {
    match value {
        DurableEchoOutcomeV1::NotEligible => DeathEchoOutcomeV1::NotEligible,
        DurableEchoOutcomeV1::Dormant => DeathEchoOutcomeV1::Dormant,
        DurableEchoOutcomeV1::Available => DeathEchoOutcomeV1::Available,
    }
}

const fn stored_cursor(value: DeathMemorialCursorV1) -> StoredDeathMemorialCursorV1 {
    StoredDeathMemorialCursorV1 {
        death_at_unix_ms: value.death_at_unix_ms,
        death_id: value.death_id,
    }
}

const fn protocol_cursor(value: StoredDeathMemorialCursorV1) -> DeathMemorialCursorV1 {
    DeathMemorialCursorV1 {
        death_at_unix_ms: value.death_at_unix_ms,
        death_id: value.death_id,
    }
}

fn text(value: String) -> Result<WireText<96>, DeathViewRepositoryError> {
    WireText::new(value).map_err(|_| corrupt())
}

const fn map_read_error(error: DeathViewReadError) -> DeathViewRepositoryError {
    match error {
        DeathViewReadError::DeathNotFound => DeathViewRepositoryError::DeathNotFound,
        DeathViewReadError::DeathNotOwned => DeathViewRepositoryError::DeathNotOwned,
        DeathViewReadError::PageOutOfRange => DeathViewRepositoryError::PageOutOfRange,
        DeathViewReadError::CorruptStoredRecord => DeathViewRepositoryError::CorruptStoredRecord,
        DeathViewReadError::ServiceUnavailable => DeathViewRepositoryError::ServiceUnavailable,
    }
}

const fn corrupt() -> DeathViewRepositoryError {
    DeathViewRepositoryError::CorruptStoredRecord
}

const fn result_code(error: DeathViewRepositoryError) -> DeathViewResultCodeV1 {
    match error {
        DeathViewRepositoryError::FeatureDisabled => DeathViewResultCodeV1::FeatureDisabled,
        DeathViewRepositoryError::DeathNotFound => DeathViewResultCodeV1::DeathNotFound,
        DeathViewRepositoryError::DeathNotOwned => DeathViewResultCodeV1::DeathNotOwned,
        DeathViewRepositoryError::PageOutOfRange => DeathViewResultCodeV1::PageOutOfRange,
        DeathViewRepositoryError::ContentMismatch => DeathViewResultCodeV1::ContentMismatch,
        DeathViewRepositoryError::CorruptStoredRecord => DeathViewResultCodeV1::CorruptStoredRecord,
        DeathViewRepositoryError::ServiceUnavailable => DeathViewResultCodeV1::ServiceUnavailable,
    }
}

const fn error(sequence: u32, code: DeathViewResultCodeV1) -> DeathViewResultV1 {
    DeathViewResultV1::Error {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        request_sequence: sequence,
        code,
    }
}

#[cfg(test)]
mod tests {
    use protocol::{ManifestHash, WireText};

    use super::*;
    use crate::AccountId;

    fn revision(byte: char) -> DeathViewContentRevisionV1 {
        DeathViewContentRevisionV1 {
            records_blake3: ManifestHash::new(byte.to_string().repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new(byte.to_string().repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new(byte.to_string().repeat(64)).unwrap(),
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([7; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn latest_frame() -> DeathViewFrameV1 {
        DeathViewFrameV1 {
            schema_version: DEATH_VIEW_SCHEMA_VERSION,
            sequence: 1,
            content_revision: revision('a'),
            request: DeathViewRequestV1::LatestCommitted,
        }
    }

    fn historical_death_id() -> [u8; 16] {
        let mut death_id = [1; 16];
        death_id[6] = 0x71;
        death_id[8] = 0x81;
        death_id
    }

    #[derive(Debug, Clone, Copy)]
    struct HistoricalRepository;

    impl DeathViewRepository for HistoricalRepository {
        async fn latest(
            &self,
            _account_id: [u8; 16],
        ) -> Result<Option<LatestCommittedDeathV1>, DeathViewRepositoryError> {
            Ok(Some(LatestCommittedDeathV1 {
                death_id: historical_death_id(),
                character_id: [2; 16],
                death_at_unix_ms: 1,
                death_tick: 1,
                cause: DeathCauseV1::DirectHit,
                killer_content_id: WireText::new("enemy.bell_acolyte").unwrap(),
                killer_pattern_id: Some(
                    WireText::new("pattern.enemy.bell_acolyte.alternating_fan").unwrap(),
                ),
                network_state: DeathNetworkStateV1::Connected,
                recall_state: DeathRecallStateV1::Inactive,
                trace_entry_count: 1,
                trace_digest: [3; 32],
                destruction_entry_count: 0,
                destruction_digest: [4; 32],
                summary_snapshot_digest: [5; 32],
                content_revision: WireText::new(format!("core-dev.blake3.{}", "c".repeat(64)))
                    .unwrap(),
                presentation_revision: revision('b'),
            }))
        }

        async fn summary(
            &self,
            _account_id: [u8; 16],
            _death_id: [u8; 16],
            _lost_start_ordinal: u16,
            _lost_limit: u16,
        ) -> Result<DeathSummaryViewV1, DeathViewRepositoryError> {
            Err(DeathViewRepositoryError::FeatureDisabled)
        }

        async fn memorial_page(
            &self,
            _account_id: [u8; 16],
            _after: Option<DeathMemorialCursorV1>,
            _limit: u8,
        ) -> Result<
            (Vec<DeathMemorialEntryV1>, Option<DeathMemorialCursorV1>),
            DeathViewRepositoryError,
        > {
            Err(DeathViewRepositoryError::FeatureDisabled)
        }

        async fn trace_page(
            &self,
            _account_id: [u8; 16],
            _death_id: [u8; 16],
            _start_ordinal: u16,
            _limit: u8,
        ) -> Result<DeathTracePageV1, DeathViewRepositoryError> {
            Ok(DeathTracePageV1 {
                death_id: historical_death_id(),
                presentation_revision: revision('b'),
                death_tick: 1,
                total_entry_count: 1,
                trace_digest: [3; 32],
                start_ordinal: 0,
                entries: vec![DeathTraceEntryV1 {
                    ordinal: 0,
                    event_tick: 1,
                    event_ordinal: 0,
                    source_content_id: WireText::new("enemy.bell_acolyte").unwrap(),
                    source_entity_id: Some([6; 16]),
                    pattern_id: Some(
                        WireText::new("pattern.enemy.bell_acolyte.alternating_fan").unwrap(),
                    ),
                    attack_id: WireText::new("pattern.enemy.bell_acolyte.alternating_fan").unwrap(),
                    raw_damage: 10,
                    final_damage: 10,
                    damage_type: DeathDamageTypeV1::Veil,
                    pre_health: 10,
                    post_health: 0,
                    source_x_milli_tiles: 1,
                    source_y_milli_tiles: 1,
                    network_state: DeathNetworkStateV1::Connected,
                    recall_state: DeathRecallStateV1::Inactive,
                    lethal: true,
                    statuses: Vec::new(),
                }],
                next_ordinal: None,
            })
        }
    }

    #[tokio::test]
    async fn disabled_repository_is_typed_and_never_fabricates_a_snapshot() {
        let service = DeathViewService::new(DisabledDeathViewRepository, revision('a'));
        assert_eq!(
            service.handle(authenticated(), &latest_frame()).await,
            error(1, DeathViewResultCodeV1::FeatureDisabled)
        );
    }

    #[tokio::test]
    async fn content_and_namespace_mismatches_fail_before_repository_access() {
        let service = DeathViewService::new(DisabledDeathViewRepository, revision('b'));
        assert_eq!(
            service.handle(authenticated(), &latest_frame()).await,
            error(1, DeathViewResultCodeV1::ContentMismatch)
        );

        let mut wrong_namespace = authenticated();
        wrong_namespace.namespace = AuthenticatedNamespace::Production;
        assert_eq!(
            DeathViewService::new(DisabledDeathViewRepository, revision('a'))
                .handle(wrong_namespace, &latest_frame())
                .await,
            error(1, DeathViewResultCodeV1::FeatureDisabled)
        );
    }

    #[test]
    fn protocol_feature_flag_remains_stable() {
        assert_eq!(protocol::CORE_DEATH_VIEW_FEATURE_FLAG, "core_death_views");
        assert_eq!(
            WireText::<64>::new(protocol::CORE_DEATH_VIEW_FEATURE_FLAG)
                .unwrap()
                .as_str(),
            "core_death_views"
        );
    }

    /// GDD `TECH-020`-`022`, Content Spec `CONT-LOC-001`/`CONT-HUB-002`, and Roadmap
    /// `GB-M03-02`/`06` require a valid older Memorial revision to remain durable data. A process
    /// that has not retained that package reports a typed compatibility miss, never corruption.
    #[tokio::test]
    async fn historical_presentation_revision_is_content_mismatch_not_corruption() {
        let result = DeathViewService::new(HistoricalRepository, revision('a'))
            .handle(authenticated(), &latest_frame())
            .await;
        assert_eq!(result, error(1, DeathViewResultCodeV1::ContentMismatch));

        let trace = DeathViewFrameV1 {
            schema_version: DEATH_VIEW_SCHEMA_VERSION,
            sequence: 2,
            content_revision: revision('a'),
            request: DeathViewRequestV1::TracePage {
                death_id: historical_death_id(),
                start_ordinal: 0,
                limit: 1,
            },
        };
        assert_eq!(
            DeathViewService::new(HistoricalRepository, revision('a'))
                .handle(authenticated(), &trace)
                .await,
            error(2, DeathViewResultCodeV1::ContentMismatch)
        );
    }
}
