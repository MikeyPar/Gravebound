//! One-snapshot `PostgreSQL` projection for the canonical Core death-terminal signature.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-001`, `DTH-020`,
//! `TECH-020`-`TECH-023`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-ECHO-009`, `CONT-HUB-001`, `CONT-HUB-002`; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-02D`, `GB-M03-06`, `GB-M03-13`.
//!
//! The reader uses explicit columns, canonical ordering, and one serializable snapshot. It never
//! reconstructs localized presentation or accepts client-authored destinations, causes, versions,
//! destruction rows, Echo eligibility, or projector outcomes.

use std::collections::BTreeMap;

use sqlx::{PgConnection, Row};

use crate::durable_death::canonical_durable_death_destruction_digest_v1;
use crate::durable_terminal_recovery::load_committed_death_terminal_v1_on;
use crate::{
    AuthoritativeDeathPlanV1, DURABLE_DEATH_CONTRACT, DURABLE_DEATH_SCHEMA_VERSION,
    DeathAggregateVersionsV1, DeathVersionAdvanceV1, DurableCombatTraceEntryV1,
    DurableDamageTypeV1, DurableDeathCauseV1, DurableDeathEventV1,
    DurableDeathPresentationAuthorityV1, DurableDeathProvenanceV1, DurableDestructionEntryV1,
    DurableDestructionLocationV1, DurableEchoEnvelopeV1, DurableEchoOutcomeV1, DurableEchoRecordV1,
    DurableEchoStateV1, DurableEchoTransitionReasonV1, DurableEchoTransitionV1,
    DurableEquipmentSlotV1, DurableMemorialRecordV1, DurableNetworkStateV1,
    DurableOrderedContentIdV1, DurableRecallStateV1, DurableSummaryDamageReferenceV1,
    DurableSummaryProjectionEntryV1, DurableSummaryProjectionKindV1, DurableSummaryProjectionsV1,
    DurableTraceStatusV1, PersistenceError, PostgresPersistence,
    StoredCoreDeathTerminalSignatureV1, StoredDeathTerminalAggregateV1, StoredDeathTerminalAuditV1,
    StoredDeathTerminalBargainCleanupV1, StoredDeathTerminalEchoTransitionV1,
    StoredDeathTerminalEchoV1, StoredDeathTerminalGraphCountsV1, StoredDeathTerminalGraphRootV1,
    StoredDeathTerminalItemLedgerV1, StoredDeathTerminalItemV1, StoredDeathTerminalMaterialV1,
    StoredDeathTerminalOutboxV1, StoredDeathTerminalTraceConflictV1,
    StoredDeathTerminalTracePromotionV1, StoredDeathTerminalTraceProvenanceV1,
    StoredDeathTerminalTraceReceiptV1, WIPEABLE_CORE_NAMESPACE,
};

const SIGNATURE_CONTRACT_VERSION: u16 = 1;

impl PostgresPersistence {
    /// Loads the complete committed terminal graph for one account-bound character.
    ///
    /// `Ok(None)` is reserved for a character without a committed durable permadeath. Any partial,
    /// ambiguous, cross-owned, or noncanonical graph fails closed.
    pub async fn load_core_death_terminal_signature_v1(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<Option<StoredCoreDeathTerminalSignatureV1>, PersistenceError> {
        if account_id == [0; 16] || character_id == [0; 16] {
            return Err(corrupt());
        }
        let mut transaction = self.begin_read_transaction().await?;
        let signature = load_core_death_terminal_signature_v1_on(
            transaction.connection(),
            account_id,
            character_id,
        )
        .await?;
        transaction.rollback().await?;
        Ok(signature)
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "assembly order mirrors the immutable terminal graph and remains audit-visible"
)]
pub(crate) async fn load_core_death_terminal_signature_v1_on(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Option<StoredCoreDeathTerminalSignatureV1>, PersistenceError> {
    let Some(terminal) =
        load_committed_death_terminal_v1_on(connection, account_id, character_id).await?
    else {
        return Ok(None);
    };
    let death_id = terminal.result.death_id;
    let mut root = load_root_projection(connection, account_id, character_id, death_id).await?;
    let trace = load_trace(connection, death_id).await?;
    let destruction = load_destruction(connection, death_id).await?;
    root.event.trace_entry_count = bounded_len_u16(trace.len())?;
    root.event.destruction_entry_count = bounded_len_u16(destruction.len())?;
    root.event.destruction_digest =
        canonical_durable_death_destruction_digest_v1(&destruction).map_err(|_| corrupt())?;

    let summary = load_summary(connection, death_id, &trace).await?;
    let memorial = load_memorial(connection, account_id, death_id, summary.snapshot_digest).await?;
    let echo_graph = load_echo_graph(
        connection,
        account_id,
        death_id,
        EchoExpectation {
            echo_expected: root.echo_expected,
            preexisting_available_echo_id: root.preexisting_available_echo_id,
            promoted_echo_id: root.promoted_echo_id,
            terminal_echo_outcome: terminal.result.echo_outcome,
            terminal_created_echo_id: terminal.result.created_echo_id,
            terminal_promoted_echo_id: terminal.result.promoted_echo_id,
        },
    )
    .await?;
    let plan = AuthoritativeDeathPlanV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        event: root.event.clone(),
        trace,
        destruction,
        summary,
        memorial,
        echo: echo_graph.plan,
    };

    let trace_promotion = load_trace_promotion(connection, death_id).await?;
    let trace_conflicts = load_trace_conflicts(connection, death_id).await?;
    let items = load_items(connection, account_id, character_id, death_id).await?;
    let item_ledger = load_item_ledger(connection, account_id, character_id, death_id).await?;
    let materials = load_materials(connection, account_id, character_id, death_id).await?;
    let bargain_cleanup = load_bargain_cleanup(
        connection,
        account_id,
        character_id,
        root.event.bargain_cleanup_event_id,
    )
    .await?;
    let audits = load_audits(connection, account_id, character_id, death_id).await?;
    let outbox = load_outbox(connection, death_id).await?;
    let zero_counts = load_zero_counts(connection, account_id, character_id).await?;

    let graph_root = StoredDeathTerminalGraphRootV1 {
        mutation_id: root.event.mutation_id,
        lineage_id: root.event.lineage_id,
        restore_point_id: root.event.restore_point_id,
        death_tick: root.event.death_tick,
        lifetime_ticks: root.event.lifetime_ticks,
        permadeath_combat_ticks: root.event.permadeath_combat_ticks,
        provenance: root.event.provenance,
        trace_entry_count: root.event.trace_entry_count,
        destruction_entry_count: root.event.destruction_entry_count,
        former_roster_ordinal: u16::from(root.event.former_roster_ordinal),
        echo_expected: root.echo_expected,
        preexisting_available_echo_id: root.preexisting_available_echo_id,
        promoted_echo_id: root.promoted_echo_id,
        content_revision: root.event.content_revision.clone(),
        world_records_blake3: root.event.records_blake3.clone(),
        world_assets_blake3: root.event.assets_blake3.clone(),
        world_localization_blake3: root.event.localization_blake3.clone(),
        presentation_records_blake3: root.event.presentation.records_blake3.clone(),
        presentation_assets_blake3: root.event.presentation.assets_blake3.clone(),
        presentation_localization_blake3: root.event.presentation.localization_blake3.clone(),
        bargain_cleanup_event_id: root.event.bargain_cleanup_event_id,
        versions: root.event.versions.clone(),
        trace_digest: root.event.trace_digest,
        destruction_digest: root.event.destruction_digest,
        summary_digest: plan.summary.snapshot_digest,
        memorial_digest: plan.memorial.presentation_digest,
    };
    let counts = StoredDeathTerminalGraphCountsV1 {
        trace_entries: u32::from(root.event.trace_entry_count),
        trace_statuses: plan
            .trace
            .iter()
            .try_fold(0_u32, |total, entry| {
                total.checked_add(u32::try_from(entry.statuses.len()).ok()?)
            })
            .ok_or_else(corrupt)?,
        summary_bargains: bounded_len_u32(plan.summary.bargains.len())?,
        summary_damage_entries: bounded_len_u32(plan.summary.last_five_damage.len())?,
        summary_projection_entries: bounded_len_u32(
            plan.summary.projections.lost.len()
                + plan.summary.projections.preserved.len()
                + plan.summary.projections.created.len(),
        )?,
        memorial_records: 1,
        destruction_entries: u32::from(root.event.destruction_entry_count),
        mutation_results: 1,
        retained_trace_sets: 1,
        retained_trace_receipt_links: bounded_len_u32(trace_promotion.receipts.len())?,
        retained_trace_provenance_entries: bounded_len_u32(trace_promotion.provenance.len())?,
        retained_trace_conflicts: bounded_len_u32(trace_conflicts.len())?,
        item_records: bounded_len_u32(items.len())?,
        item_ledger_entries: bounded_len_u32(item_ledger.len())?,
        material_records: bounded_len_u32(materials.len())?,
        echo_records: bounded_len_u32(echo_graph.stored.len())?,
        echo_transitions: bounded_len_u32(echo_graph.transitions.len())?,
        audit_events: bounded_len_u32(audits.len())?,
        outbox_events: bounded_len_u32(outbox.len())?,
        active_bargains: zero_counts.active_bargains,
        danger_checkpoints: zero_counts.danger_checkpoints,
        live_trace_ticks: zero_counts.live_trace_ticks,
        live_trace_entries: zero_counts.live_trace_entries,
        live_trace_statuses: zero_counts.live_trace_statuses,
    };
    let signature = StoredCoreDeathTerminalSignatureV1 {
        contract_version: SIGNATURE_CONTRACT_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
        account_id,
        character_id,
        death_id,
        terminal,
        plan,
        aggregate: root.aggregate,
        graph_root,
        trace_promotion,
        trace_conflicts,
        items,
        item_ledger,
        materials,
        bargain_cleanup,
        echoes: echo_graph.stored,
        echo_transitions: echo_graph.transitions,
        audits,
        outbox,
        counts,
    };
    signature.canonical_bytes()?;
    Ok(Some(signature))
}

struct RootProjection {
    event: DurableDeathEventV1,
    aggregate: StoredDeathTerminalAggregateV1,
    echo_expected: bool,
    preexisting_available_echo_id: Option<[u8; 16]>,
    promoted_echo_id: Option<[u8; 16]>,
}

#[allow(
    clippy::too_many_lines,
    reason = "one explicit row binds all post-death aggregate heads to the immutable event"
)]
async fn load_root_projection(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    death_id: [u8; 16],
) -> Result<RootProjection, PersistenceError> {
    let rows = sqlx::query(
        "SELECT event.contract_kind,event.mutation_id,event.canonical_request_hash,\
                event.content_revision,event.instance_id,event.lineage_id,event.restore_point_id,\
                event.region_id,event.room_id,event.death_provenance,event.death_tick,event.cause_kind,\
                event.killer_content_id,event.killer_pattern_id,event.killer_attack_id,\
                event.raw_damage,event.final_damage,event.damage_type,event.pre_hit_health,\
                event.source_x_milli_tiles,event.source_y_milli_tiles,event.network_state,\
                event.recall_state,event.lifetime_ticks,event.permadeath_combat_ticks,\
                event.pre_account_version,event.post_account_version,\
                event.pre_character_version,event.post_character_version,\
                event.pre_progression_version,event.post_progression_version,\
                event.pre_inventory_version,event.post_inventory_version,\
                event.pre_oath_bargain_version,event.post_oath_bargain_version,\
                event.pre_life_metrics_version,event.post_life_metrics_version,\
                event.trace_digest,event.former_roster_ordinal,event.echo_expected,\
                event.preexisting_available_echo_id,event.promoted_echo_id,\
                event.world_records_blake3,event.world_assets_blake3,\
                event.world_localization_blake3,event.presentation_records_blake3,\
                event.presentation_assets_blake3,event.presentation_localization_blake3,\
                event.bargain_cleanup_event_id,\
                floor(extract(epoch FROM event.committed_at)*1000)::bigint AS committed_at_ms,\
                account.state_version AS account_version,\
                account.selected_character_id,character.life_state,character.roster_ordinal,\
                character.character_state_version AS character_version,\
                world.character_version AS world_character_version,world.location_kind,\
                world.location_content_id,world.instance_lineage_id,world.entry_restore_point_id,\
                progression.current_health,progression.progression_version,\
                inventory.inventory_version,oath.oath_bargain_version,\
                life.lifetime_ticks AS stored_lifetime_ticks,\
                life.permadeath_combat_ticks AS stored_combat_ticks,\
                life.life_metrics_version,wallet.balance AS ash_balance,\
                wallet.wallet_version AS ash_wallet_version,lineage.lineage_state,\
                restore.restore_state,restore.death_mutation_id \
         FROM death_events AS event \
         JOIN accounts AS account ON account.namespace_id=event.namespace_id \
            AND account.account_id=event.account_id \
         JOIN characters AS character ON character.namespace_id=event.namespace_id \
            AND character.account_id=event.account_id AND character.character_id=event.character_id \
         JOIN character_world_locations AS world ON world.namespace_id=event.namespace_id \
            AND world.account_id=event.account_id AND world.character_id=event.character_id \
         JOIN character_progression AS progression ON progression.namespace_id=event.namespace_id \
            AND progression.account_id=event.account_id \
            AND progression.character_id=event.character_id \
         JOIN character_inventories AS inventory ON inventory.namespace_id=event.namespace_id \
            AND inventory.account_id=event.account_id AND inventory.character_id=event.character_id \
         JOIN character_oath_bargain_state AS oath ON oath.namespace_id=event.namespace_id \
            AND oath.account_id=event.account_id AND oath.character_id=event.character_id \
         JOIN character_life_metrics AS life ON life.namespace_id=event.namespace_id \
            AND life.account_id=event.account_id AND life.character_id=event.character_id \
         JOIN ash_wallets AS wallet ON wallet.namespace_id=event.namespace_id \
            AND wallet.account_id=event.account_id \
         JOIN character_instance_lineages AS lineage ON lineage.namespace_id=event.namespace_id \
            AND lineage.account_id=event.account_id AND lineage.character_id=event.character_id \
            AND lineage.lineage_id=event.lineage_id \
         JOIN character_entry_restore_points AS restore ON restore.namespace_id=event.namespace_id \
            AND restore.account_id=event.account_id AND restore.character_id=event.character_id \
            AND restore.restore_point_id=event.restore_point_id \
         WHERE event.namespace_id=$1 AND event.account_id=$2 AND event.character_id=$3 \
           AND event.death_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    let [row] = rows.as_slice() else {
        return Err(corrupt());
    };
    if row.try_get::<String, _>("contract_kind")? != DURABLE_DEATH_CONTRACT {
        return Err(corrupt());
    }
    let versions = DeathAggregateVersionsV1 {
        account: version_advance(row, "pre_account_version", "post_account_version")?,
        character: version_advance(row, "pre_character_version", "post_character_version")?,
        progression: version_advance(row, "pre_progression_version", "post_progression_version")?,
        inventory: version_advance(row, "pre_inventory_version", "post_inventory_version")?,
        oath_bargain: version_advance(
            row,
            "pre_oath_bargain_version",
            "post_oath_bargain_version",
        )?,
        life_metrics: version_advance(
            row,
            "pre_life_metrics_version",
            "post_life_metrics_version",
        )?,
    };
    let event = DurableDeathEventV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
        death_id,
        account_id,
        character_id,
        former_roster_ordinal: u8_value(row.try_get("former_roster_ordinal")?)?,
        mutation_id: exact_id(row.try_get("mutation_id")?)?,
        bargain_cleanup_event_id: exact_id(row.try_get("bargain_cleanup_event_id")?)?,
        canonical_request_hash: exact_hash(row.try_get("canonical_request_hash")?)?,
        content_revision: row.try_get("content_revision")?,
        records_blake3: row.try_get("world_records_blake3")?,
        assets_blake3: row.try_get("world_assets_blake3")?,
        localization_blake3: row.try_get("world_localization_blake3")?,
        presentation: DurableDeathPresentationAuthorityV1 {
            records_blake3: row.try_get("presentation_records_blake3")?,
            assets_blake3: row.try_get("presentation_assets_blake3")?,
            localization_blake3: row.try_get("presentation_localization_blake3")?,
        },
        instance_id: exact_id(row.try_get("instance_id")?)?,
        lineage_id: exact_id(row.try_get("lineage_id")?)?,
        restore_point_id: exact_id(row.try_get("restore_point_id")?)?,
        region_id: row.try_get("region_id")?,
        room_id: row.try_get("room_id")?,
        provenance: death_provenance(row.try_get("death_provenance")?)?,
        death_tick: positive(row.try_get("death_tick")?)?,
        committed_at_unix_ms: positive(row.try_get("committed_at_ms")?)?,
        cause: death_cause(row.try_get("cause_kind")?)?,
        killer_content_id: row
            .try_get::<Option<String>, _>("killer_content_id")?
            .ok_or_else(corrupt)?,
        killer_pattern_id: row.try_get("killer_pattern_id")?,
        killer_attack_id: row
            .try_get::<Option<String>, _>("killer_attack_id")?
            .ok_or_else(corrupt)?,
        raw_damage: u32_value(row.try_get("raw_damage")?)?,
        final_damage: u32_value(row.try_get("final_damage")?)?,
        damage_type: damage_type(row.try_get("damage_type")?)?,
        pre_hit_health: u32_value(row.try_get("pre_hit_health")?)?,
        source_x_milli_tiles: row.try_get("source_x_milli_tiles")?,
        source_y_milli_tiles: row.try_get("source_y_milli_tiles")?,
        network_state: network_state(row.try_get("network_state")?)?,
        recall_state: recall_state(row.try_get("recall_state")?)?,
        lifetime_ticks: nonnegative(row.try_get("lifetime_ticks")?)?,
        permadeath_combat_ticks: nonnegative(row.try_get("permadeath_combat_ticks")?)?,
        versions,
        trace_entry_count: 0,
        trace_digest: exact_hash(row.try_get("trace_digest")?)?,
        destruction_entry_count: 0,
        destruction_digest: [0; 32],
    };
    let aggregate = StoredDeathTerminalAggregateV1 {
        account_version: unsigned(row.try_get("account_version")?)?,
        selected_character_id: optional_id(row.try_get("selected_character_id")?)?,
        character_life_state: u16_value(row.try_get("life_state")?)?,
        character_roster_ordinal: optional_u16(row.try_get("roster_ordinal")?)?,
        character_version: unsigned(row.try_get("character_version")?)?,
        world_character_version: unsigned(row.try_get("world_character_version")?)?,
        world_location_kind: u16_value(row.try_get("location_kind")?)?,
        world_location_content_id: row.try_get("location_content_id")?,
        world_lineage_id: optional_id(row.try_get("instance_lineage_id")?)?,
        world_restore_point_id: optional_id(row.try_get("entry_restore_point_id")?)?,
        current_health: u32_value(row.try_get("current_health")?)?,
        progression_version: unsigned(row.try_get("progression_version")?)?,
        inventory_version: unsigned(row.try_get("inventory_version")?)?,
        oath_bargain_version: unsigned(row.try_get("oath_bargain_version")?)?,
        lifetime_ticks: nonnegative(row.try_get("stored_lifetime_ticks")?)?,
        permadeath_combat_ticks: nonnegative(row.try_get("stored_combat_ticks")?)?,
        life_metrics_version: unsigned(row.try_get("life_metrics_version")?)?,
        ash_balance: u32_value(row.try_get("ash_balance")?)?,
        ash_wallet_version: unsigned(row.try_get("ash_wallet_version")?)?,
        lineage_state: u16_value(row.try_get("lineage_state")?)?,
        restore_state: u16_value(row.try_get("restore_state")?)?,
        restore_death_mutation_id: optional_id(row.try_get("death_mutation_id")?)?,
    };
    Ok(RootProjection {
        event,
        aggregate,
        echo_expected: row.try_get("echo_expected")?,
        preexisting_available_echo_id: optional_id(row.try_get("preexisting_available_echo_id")?)?,
        promoted_echo_id: optional_id(row.try_get("promoted_echo_id")?)?,
    })
}

fn version_advance(
    row: &sqlx::postgres::PgRow,
    pre: &str,
    post: &str,
) -> Result<DeathVersionAdvanceV1, PersistenceError> {
    Ok(DeathVersionAdvanceV1 {
        pre: unsigned(row.try_get(pre)?)?,
        post: unsigned(row.try_get(post)?)?,
    })
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredDeathTerminalSignature
}

fn exact_id(value: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    value.try_into().map_err(|_| corrupt())
}

fn optional_id(value: Option<Vec<u8>>) -> Result<Option<[u8; 16]>, PersistenceError> {
    value.map(exact_id).transpose()
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; 32], PersistenceError> {
    value.try_into().map_err(|_| corrupt())
}

fn unsigned(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| corrupt())
}

fn positive(value: i64) -> Result<u64, PersistenceError> {
    let value = unsigned(value)?;
    (value > 0).then_some(value).ok_or_else(corrupt)
}

fn nonnegative(value: i64) -> Result<u64, PersistenceError> {
    unsigned(value)
}

fn u32_value(value: i32) -> Result<u32, PersistenceError> {
    u32::try_from(value).map_err(|_| corrupt())
}

fn u16_value(value: i16) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| corrupt())
}

fn u8_value(value: i16) -> Result<u8, PersistenceError> {
    u8::try_from(value).map_err(|_| corrupt())
}

fn optional_u16(value: Option<i16>) -> Result<Option<u16>, PersistenceError> {
    value.map(u16_value).transpose()
}

fn bounded_len_u16(value: usize) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| corrupt())
}

fn bounded_len_u32(value: usize) -> Result<u32, PersistenceError> {
    u32::try_from(value).map_err(|_| corrupt())
}

fn death_cause(value: i16) -> Result<DurableDeathCauseV1, PersistenceError> {
    match value {
        0 => Ok(DurableDeathCauseV1::DirectHit),
        1 => Ok(DurableDeathCauseV1::DamageOverTime),
        2 => Ok(DurableDeathCauseV1::Environment),
        3 => Ok(DurableDeathCauseV1::Disconnect),
        _ => Err(corrupt()),
    }
}

fn death_provenance(value: i16) -> Result<DurableDeathProvenanceV1, PersistenceError> {
    match value {
        0 => Ok(DurableDeathProvenanceV1::OrdinaryGameplay),
        1 => Ok(DurableDeathProvenanceV1::VerifiedServerIncident),
        2 => Ok(DurableDeathProvenanceV1::AdministrativeAction),
        _ => Err(corrupt()),
    }
}

fn damage_type(value: i16) -> Result<DurableDamageTypeV1, PersistenceError> {
    match value {
        0 => Ok(DurableDamageTypeV1::Physical),
        1 => Ok(DurableDamageTypeV1::Veil),
        _ => Err(corrupt()),
    }
}

fn network_state(value: i16) -> Result<DurableNetworkStateV1, PersistenceError> {
    match value {
        0 => Ok(DurableNetworkStateV1::Connected),
        1 => Ok(DurableNetworkStateV1::Degraded),
        2 => Ok(DurableNetworkStateV1::LinkLost),
        3 => Ok(DurableNetworkStateV1::Reattached),
        _ => Err(corrupt()),
    }
}

fn recall_state(value: i16) -> Result<DurableRecallStateV1, PersistenceError> {
    match value {
        0 => Ok(DurableRecallStateV1::Inactive),
        1 => Ok(DurableRecallStateV1::Channeling),
        2 => Ok(DurableRecallStateV1::CompletionPending),
        _ => Err(corrupt()),
    }
}

struct EchoGraph {
    plan: Option<DurableEchoEnvelopeV1>,
    stored: Vec<StoredDeathTerminalEchoV1>,
    transitions: Vec<StoredDeathTerminalEchoTransitionV1>,
}

struct ZeroCounts {
    active_bargains: u32,
    danger_checkpoints: u32,
    live_trace_ticks: u32,
    live_trace_entries: u32,
    live_trace_statuses: u32,
}

async fn load_trace(
    connection: &mut PgConnection,
    death_id: [u8; 16],
) -> Result<Vec<DurableCombatTraceEntryV1>, PersistenceError> {
    let status_rows = sqlx::query(
        "SELECT trace_ordinal,status_ordinal,status_id,remaining_ticks,stack_count \
         FROM death_combat_trace_statuses WHERE namespace_id=$1 AND death_id=$2 \
         ORDER BY trace_ordinal,status_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let mut statuses = BTreeMap::<u16, Vec<DurableTraceStatusV1>>::new();
    for row in status_rows {
        let trace_ordinal = u16_value(row.try_get("trace_ordinal")?)?;
        let entry = statuses.entry(trace_ordinal).or_default();
        let ordinal = u8_value(row.try_get("status_ordinal")?)?;
        if usize::from(ordinal) != entry.len() {
            return Err(corrupt());
        }
        entry.push(DurableTraceStatusV1 {
            ordinal,
            status_id: row.try_get("status_id")?,
            remaining_ticks: u32_value(row.try_get("remaining_ticks")?)?,
            stack_count: u16_value(row.try_get("stack_count")?)?,
        });
    }

    let rows = sqlx::query(
        "SELECT trace_ordinal,event_tick,event_ordinal,source_content_id,source_entity_id,\
                pattern_id,attack_id,raw_damage,final_damage,damage_type,pre_health,post_health,\
                source_x_milli_tiles,source_y_milli_tiles,network_state,recall_state,lethal \
         FROM death_combat_trace_entries WHERE namespace_id=$1 AND death_id=$2 \
         ORDER BY trace_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    let trace = rows
        .iter()
        .enumerate()
        .map(|(expected, row)| {
            let ordinal = u16_value(row.try_get("trace_ordinal")?)?;
            if usize::from(ordinal) != expected {
                return Err(corrupt());
            }
            Ok(DurableCombatTraceEntryV1 {
                ordinal,
                event_tick: positive(row.try_get("event_tick")?)?,
                event_ordinal: u32_value(row.try_get("event_ordinal")?)?,
                source_content_id: row
                    .try_get::<Option<String>, _>("source_content_id")?
                    .ok_or_else(corrupt)?,
                source_entity_id: optional_id(row.try_get("source_entity_id")?)?,
                pattern_id: row.try_get("pattern_id")?,
                attack_id: row
                    .try_get::<Option<String>, _>("attack_id")?
                    .ok_or_else(corrupt)?,
                raw_damage: u32_value(row.try_get("raw_damage")?)?,
                final_damage: u32_value(row.try_get("final_damage")?)?,
                damage_type: damage_type(row.try_get("damage_type")?)?,
                pre_health: u32_value(row.try_get("pre_health")?)?,
                post_health: u32_value(row.try_get("post_health")?)?,
                source_x_milli_tiles: row.try_get("source_x_milli_tiles")?,
                source_y_milli_tiles: row.try_get("source_y_milli_tiles")?,
                network_state: network_state(row.try_get("network_state")?)?,
                recall_state: recall_state(row.try_get("recall_state")?)?,
                lethal: row.try_get("lethal")?,
                statuses: statuses.remove(&ordinal).unwrap_or_default(),
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    if !statuses.is_empty() {
        return Err(corrupt());
    }
    Ok(trace)
}

async fn load_destruction(
    connection: &mut PgConnection,
    death_id: [u8; 16],
) -> Result<Vec<DurableDestructionEntryV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT destroyed.destruction_ordinal,destroyed.entry_kind,destroyed.item_uid,\
                destroyed.material_id,destroyed.quantity,destroyed.pre_location_kind,\
                destroyed.pre_slot_index,destroyed.pre_instance_id,destroyed.pre_pickup_id,\
                destroyed.pre_item_version,destroyed.post_item_version,\
                destroyed.ledger_event_id,destroyed.account_id,destroyed.character_id,\
                destroyed.pre_material_version,destroyed.post_material_version,\
                destroyed.pre_material_quantity,item.template_id \
         FROM death_destruction_entries AS destroyed \
         LEFT JOIN item_instances AS item ON item.namespace_id=destroyed.namespace_id \
            AND item.item_uid=destroyed.item_uid \
         WHERE destroyed.namespace_id=$1 AND destroyed.death_id=$2 \
         ORDER BY destroyed.destruction_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.iter()
        .enumerate()
        .map(|(expected, row)| {
            let ordinal = u16_value(row.try_get("destruction_ordinal")?)?;
            if usize::from(ordinal) != expected {
                return Err(corrupt());
            }
            match row.try_get::<i16, _>("entry_kind")? {
                0 => Ok(DurableDestructionEntryV1::Item {
                    ordinal,
                    content_id: row
                        .try_get::<Option<String>, _>("template_id")?
                        .ok_or_else(corrupt)?,
                    item_uid: exact_id(
                        row.try_get::<Option<Vec<u8>>, _>("item_uid")?
                            .ok_or_else(corrupt)?,
                    )?,
                    location: destruction_location(
                        row.try_get::<Option<i16>, _>("pre_location_kind")?
                            .ok_or_else(corrupt)?,
                        row.try_get("pre_slot_index")?,
                        row.try_get("pre_instance_id")?,
                        row.try_get("pre_pickup_id")?,
                    )?,
                    pre_item_version: unsigned(
                        row.try_get::<Option<i64>, _>("pre_item_version")?
                            .ok_or_else(corrupt)?,
                    )?,
                    post_item_version: unsigned(
                        row.try_get::<Option<i64>, _>("post_item_version")?
                            .ok_or_else(corrupt)?,
                    )?,
                    ledger_event_id: exact_id(
                        row.try_get::<Option<Vec<u8>>, _>("ledger_event_id")?
                            .ok_or_else(corrupt)?,
                    )?,
                }),
                1 => {
                    let destroyed_quantity = u32_value(row.try_get("quantity")?)?;
                    Ok(DurableDestructionEntryV1::RunMaterial {
                        ordinal,
                        material_id: row
                            .try_get::<Option<String>, _>("material_id")?
                            .ok_or_else(corrupt)?,
                        destroyed_quantity,
                        pre_material_quantity: u32_value(
                            row.try_get::<Option<i32>, _>("pre_material_quantity")?
                                .ok_or_else(corrupt)?,
                        )?,
                        pre_material_version: unsigned(
                            row.try_get::<Option<i64>, _>("pre_material_version")?
                                .ok_or_else(corrupt)?,
                        )?,
                        post_material_version: unsigned(
                            row.try_get::<Option<i64>, _>("post_material_version")?
                                .ok_or_else(corrupt)?,
                        )?,
                    })
                }
                _ => Err(corrupt()),
            }
        })
        .collect()
}

fn destruction_location(
    location_kind: i16,
    slot_index: Option<i16>,
    instance_id: Option<Vec<u8>>,
    pickup_id: Option<Vec<u8>>,
) -> Result<DurableDestructionLocationV1, PersistenceError> {
    match location_kind {
        0 => Ok(DurableDestructionLocationV1::Equipment {
            slot: match slot_index.ok_or_else(corrupt)? {
                0 => DurableEquipmentSlotV1::Weapon,
                1 => DurableEquipmentSlotV1::Relic,
                2 => DurableEquipmentSlotV1::Armor,
                3 => DurableEquipmentSlotV1::Charm,
                _ => return Err(corrupt()),
            },
        }),
        1 => Ok(DurableDestructionLocationV1::Belt {
            index: u8_value(slot_index.ok_or_else(corrupt)?)?,
        }),
        2 => Ok(DurableDestructionLocationV1::RunBackpack {
            index: u8_value(slot_index.ok_or_else(corrupt)?)?,
        }),
        3 => Ok(DurableDestructionLocationV1::PersonalGround {
            instance_id: exact_id(instance_id.ok_or_else(corrupt)?)?,
            pickup_id: exact_id(pickup_id.ok_or_else(corrupt)?)?,
        }),
        _ => Err(corrupt()),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the normalized summary root and its three ordered child families form one snapshot"
)]
async fn load_summary(
    connection: &mut PgConnection,
    death_id: [u8; 16],
    trace: &[DurableCombatTraceEntryV1],
) -> Result<crate::DurableDeathSummaryV1, PersistenceError> {
    let roots = sqlx::query(
        "SELECT summary_revision,hero_label_key,character_name_snapshot,class_id,level,oath_id,\
                lifetime_ms,final_deed_id,echo_outcome,content_revision,snapshot_digest \
         FROM death_summary_snapshots WHERE namespace_id=$1 AND death_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let [root] = roots.as_slice() else {
        return Err(corrupt());
    };
    let bargains = load_ordered_content(
        connection,
        "SELECT bargain_ordinal AS ordinal,bargain_id AS content_id \
         FROM death_summary_bargains WHERE namespace_id=$1 AND death_id=$2 \
         ORDER BY bargain_ordinal",
        death_id,
    )
    .await?;
    let damage_rows = sqlx::query(
        "SELECT summary_ordinal,trace_ordinal FROM death_summary_damage_entries \
         WHERE namespace_id=$1 AND death_id=$2 ORDER BY summary_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let last_five_damage = damage_rows
        .iter()
        .enumerate()
        .map(|(expected, row)| {
            let ordinal = u8_value(row.try_get("summary_ordinal")?)?;
            if usize::from(ordinal) != expected {
                return Err(corrupt());
            }
            Ok(DurableSummaryDamageReferenceV1 {
                ordinal,
                trace_ordinal: u16_value(row.try_get("trace_ordinal")?)?,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let projection_rows = sqlx::query(
        "SELECT section_kind,entry_ordinal,projection_kind,content_id,quantity,item_uid \
         FROM death_summary_projection_entries WHERE namespace_id=$1 AND death_id=$2 \
         ORDER BY section_kind,entry_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    let mut projections = DurableSummaryProjectionsV1 {
        lost: Vec::new(),
        preserved: Vec::new(),
        created: Vec::new(),
    };
    for row in projection_rows {
        let section = row.try_get::<i16, _>("section_kind")?;
        let target = match section {
            0 => &mut projections.lost,
            1 => &mut projections.preserved,
            2 => &mut projections.created,
            _ => return Err(corrupt()),
        };
        let ordinal = u16_value(row.try_get("entry_ordinal")?)?;
        if usize::from(ordinal) != target.len() {
            return Err(corrupt());
        }
        target.push(DurableSummaryProjectionEntryV1 {
            ordinal,
            kind: projection_kind(row.try_get("projection_kind")?)?,
            content_id: row.try_get("content_id")?,
            quantity: u32_value(row.try_get("quantity")?)?,
            item_uid: optional_id(row.try_get("item_uid")?)?,
        });
    }
    let lethal = trace
        .iter()
        .filter(|entry| entry.lethal)
        .map(|entry| entry.ordinal)
        .collect::<Vec<_>>();
    let [lethal_trace_ordinal] = lethal.as_slice() else {
        return Err(corrupt());
    };
    Ok(crate::DurableDeathSummaryV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
        death_id,
        summary_revision: u16_value(root.try_get("summary_revision")?)?,
        hero_label_key: root.try_get("hero_label_key")?,
        character_name_snapshot: root.try_get("character_name_snapshot")?,
        class_id: root.try_get("class_id")?,
        level: u8_value(root.try_get("level")?)?,
        oath_id: root.try_get("oath_id")?,
        bargains,
        lifetime_ms: nonnegative(root.try_get("lifetime_ms")?)?,
        final_deed_id: root.try_get("final_deed_id")?,
        lethal_trace_ordinal: *lethal_trace_ordinal,
        last_five_damage,
        projections,
        echo_outcome: echo_outcome(root.try_get("echo_outcome")?)?,
        content_revision: root.try_get("content_revision")?,
        snapshot_digest: exact_hash(root.try_get("snapshot_digest")?)?,
    })
}

async fn load_ordered_content(
    connection: &mut PgConnection,
    query: &'static str,
    identity: [u8; 16],
) -> Result<Vec<DurableOrderedContentIdV1>, PersistenceError> {
    let rows = sqlx::query(query)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(identity.as_slice())
        .fetch_all(connection)
        .await?;
    rows.iter()
        .enumerate()
        .map(|(expected, row)| {
            let ordinal = u16_value(row.try_get("ordinal")?)?;
            if usize::from(ordinal) != expected {
                return Err(corrupt());
            }
            Ok(DurableOrderedContentIdV1 {
                ordinal,
                content_id: row.try_get("content_id")?,
            })
        })
        .collect()
}

async fn load_memorial(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    death_id: [u8; 16],
    summary_snapshot_digest: [u8; 32],
) -> Result<DurableMemorialRecordV1, PersistenceError> {
    let rows = sqlx::query(
        "SELECT account_id,floor(extract(epoch FROM death_at)*1000)::bigint AS death_at_ms,\
                summary_revision,presentation_key,presentation_digest \
         FROM memorial_records WHERE namespace_id=$1 AND account_id=$2 AND death_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    let [row] = rows.as_slice() else {
        return Err(corrupt());
    };
    Ok(DurableMemorialRecordV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
        death_id,
        account_id: exact_id(row.try_get("account_id")?)?,
        death_at_unix_ms: positive(row.try_get("death_at_ms")?)?,
        summary_revision: u16_value(row.try_get("summary_revision")?)?,
        summary_snapshot_digest,
        presentation_key: row.try_get("presentation_key")?,
        presentation_digest: exact_hash(row.try_get("presentation_digest")?)?,
    })
}

fn projection_kind(value: i16) -> Result<DurableSummaryProjectionKindV1, PersistenceError> {
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
        _ => Err(corrupt()),
    }
}

fn echo_outcome(value: i16) -> Result<DurableEchoOutcomeV1, PersistenceError> {
    match value {
        0 => Ok(DurableEchoOutcomeV1::NotEligible),
        1 => Ok(DurableEchoOutcomeV1::Dormant),
        2 => Ok(DurableEchoOutcomeV1::Available),
        _ => Err(corrupt()),
    }
}

struct LoadedEchoTransition {
    stored: StoredDeathTerminalEchoTransitionV1,
    committed_at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct EchoExpectation {
    echo_expected: bool,
    preexisting_available_echo_id: Option<[u8; 16]>,
    promoted_echo_id: Option<[u8; 16]>,
    terminal_echo_outcome: DurableEchoOutcomeV1,
    terminal_created_echo_id: Option<[u8; 16]>,
    terminal_promoted_echo_id: Option<[u8; 16]>,
}

#[allow(
    clippy::too_many_lines,
    reason = "account Echo queue and current projector envelope are one audited graph"
)]
async fn load_echo_graph(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    death_id: [u8; 16],
    expectation: EchoExpectation,
) -> Result<EchoGraph, PersistenceError> {
    let rows = sqlx::query(
        "SELECT echo_id,death_id,account_id,character_name_snapshot,class_id,oath_id,level,\
                appearance_snapshot_id,appearance_theme_id,weapon_signature_tag,\
                relic_signature_tag,killer_content_id,killer_pattern_id,death_region_id,\
                power_band,state,content_revision,snapshot_digest,\
                floor(extract(epoch FROM created_at)*1000000)::bigint AS created_at_micros \
         FROM echo_records WHERE namespace_id=$1 AND account_id=$2 \
         ORDER BY created_at,echo_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let mut stored = rows
        .iter()
        .map(|row| {
            Ok(StoredDeathTerminalEchoV1 {
                echo_id: exact_id(row.try_get("echo_id")?)?,
                death_id: exact_id(row.try_get("death_id")?)?,
                account_id: exact_id(row.try_get("account_id")?)?,
                character_name_snapshot: row.try_get("character_name_snapshot")?,
                class_id: row.try_get("class_id")?,
                oath_id: row.try_get("oath_id")?,
                level: u16_value(row.try_get("level")?)?,
                appearance_snapshot_id: row.try_get("appearance_snapshot_id")?,
                appearance_theme_id: row.try_get("appearance_theme_id")?,
                weapon_signature_tag: row.try_get("weapon_signature_tag")?,
                relic_signature_tag: row.try_get("relic_signature_tag")?,
                bargains: Vec::new(),
                deed_tags: Vec::new(),
                killer_content_id: row
                    .try_get::<Option<String>, _>("killer_content_id")?
                    .ok_or_else(corrupt)?,
                killer_pattern_id: row.try_get("killer_pattern_id")?,
                death_region_id: row.try_get("death_region_id")?,
                power_band: u16_value(row.try_get("power_band")?)?,
                state: u16_value(row.try_get("state")?)?,
                content_revision: row.try_get("content_revision")?,
                snapshot_digest: exact_hash(row.try_get("snapshot_digest")?)?,
                created_at_unix_micros: positive(row.try_get("created_at_micros")?)?,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let mut positions = stored
        .iter()
        .enumerate()
        .map(|(index, echo)| (echo.echo_id, index))
        .collect::<BTreeMap<_, _>>();
    if positions.len() != stored.len() {
        return Err(corrupt());
    }

    let bargain_rows = sqlx::query(
        "SELECT echo.echo_id,bargain.bargain_ordinal,bargain.bargain_id \
         FROM echo_records AS echo JOIN echo_bargain_snapshots AS bargain \
           ON bargain.namespace_id=echo.namespace_id AND bargain.echo_id=echo.echo_id \
         WHERE echo.namespace_id=$1 AND echo.account_id=$2 \
         ORDER BY echo.echo_id,bargain.bargain_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    for row in bargain_rows {
        let echo_id = exact_id(row.try_get("echo_id")?)?;
        let echo = positions
            .get(&echo_id)
            .copied()
            .and_then(|index| stored.get_mut(index))
            .ok_or_else(corrupt)?;
        let ordinal = u16_value(row.try_get("bargain_ordinal")?)?;
        if usize::from(ordinal) != echo.bargains.len() {
            return Err(corrupt());
        }
        echo.bargains.push(DurableOrderedContentIdV1 {
            ordinal,
            content_id: row.try_get("bargain_id")?,
        });
    }
    let deed_rows = sqlx::query(
        "SELECT echo.echo_id,deed.deed_ordinal,deed.deed_tag \
         FROM echo_records AS echo JOIN echo_deed_tags AS deed \
           ON deed.namespace_id=echo.namespace_id AND deed.echo_id=echo.echo_id \
         WHERE echo.namespace_id=$1 AND echo.account_id=$2 \
         ORDER BY echo.echo_id,deed.deed_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    for row in deed_rows {
        let echo_id = exact_id(row.try_get("echo_id")?)?;
        let echo = positions
            .get(&echo_id)
            .copied()
            .and_then(|index| stored.get_mut(index))
            .ok_or_else(corrupt)?;
        let ordinal = u16_value(row.try_get("deed_ordinal")?)?;
        if usize::from(ordinal) != echo.deed_tags.len() {
            return Err(corrupt());
        }
        echo.deed_tags.push(DurableOrderedContentIdV1 {
            ordinal,
            content_id: row.try_get("deed_tag")?,
        });
    }

    let transition_rows = sqlx::query(
        "SELECT transition.echo_id,transition.transition_ordinal,transition.previous_state,\
                transition.next_state,transition.reason_kind,transition.source_death_id,\
                transition.trigger_death_id,\
                floor(extract(epoch FROM transition.committed_at)*1000)::bigint AS committed_at_ms \
         FROM echo_state_transitions AS transition JOIN echo_records AS echo \
           ON echo.namespace_id=transition.namespace_id AND echo.echo_id=transition.echo_id \
         WHERE echo.namespace_id=$1 AND echo.account_id=$2 \
         ORDER BY transition.echo_id,transition.transition_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_all(connection)
    .await?;
    let loaded_transitions = transition_rows
        .iter()
        .map(|row| {
            Ok(LoadedEchoTransition {
                stored: StoredDeathTerminalEchoTransitionV1 {
                    echo_id: exact_id(row.try_get("echo_id")?)?,
                    transition_ordinal: u16_value(row.try_get("transition_ordinal")?)?,
                    previous_state: optional_u16(row.try_get("previous_state")?)?,
                    next_state: u16_value(row.try_get("next_state")?)?,
                    reason: u16_value(row.try_get("reason_kind")?)?,
                    source_death_id: optional_id(row.try_get("source_death_id")?)?,
                    trigger_death_id: exact_id(row.try_get("trigger_death_id")?)?,
                },
                committed_at_unix_ms: positive(row.try_get("committed_at_ms")?)?,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let transitions = loaded_transitions
        .iter()
        .map(|transition| transition.stored.clone())
        .collect::<Vec<_>>();

    let created = stored
        .iter()
        .filter(|echo| echo.death_id == death_id)
        .collect::<Vec<_>>();
    if !expectation.echo_expected {
        if expectation.terminal_echo_outcome != DurableEchoOutcomeV1::NotEligible
            || expectation.terminal_created_echo_id.is_some()
            || expectation.terminal_promoted_echo_id.is_some()
            || !created.is_empty()
            || expectation.preexisting_available_echo_id.is_some()
            || expectation.promoted_echo_id.is_some()
            || loaded_transitions.iter().any(|transition| {
                transition.stored.trigger_death_id == death_id && transition.stored.reason == 1
            })
        {
            return Err(corrupt());
        }
        return Ok(EchoGraph {
            plan: None,
            stored,
            transitions,
        });
    }
    let [created] = created.as_slice() else {
        return Err(corrupt());
    };
    if expectation.terminal_created_echo_id != Some(created.echo_id)
        || expectation.terminal_promoted_echo_id != expectation.promoted_echo_id
    {
        return Err(corrupt());
    }
    let created_terminal_state = if expectation.terminal_promoted_echo_id == Some(created.echo_id) {
        DurableEchoStateV1::Available
    } else {
        DurableEchoStateV1::Dormant
    };
    if !matches!(
        (expectation.terminal_echo_outcome, created_terminal_state),
        (
            DurableEchoOutcomeV1::Available,
            DurableEchoStateV1::Available
        ) | (DurableEchoOutcomeV1::Dormant, DurableEchoStateV1::Dormant)
    ) {
        return Err(corrupt());
    }
    let creation = loaded_transitions
        .iter()
        .filter(|transition| {
            transition.stored.echo_id == created.echo_id
                && transition.stored.transition_ordinal == 0
        })
        .collect::<Vec<_>>();
    let [creation] = creation.as_slice() else {
        return Err(corrupt());
    };
    let promotions = loaded_transitions
        .iter()
        .filter(|transition| {
            transition.stored.trigger_death_id == death_id && transition.stored.reason == 1
        })
        .collect::<Vec<_>>();
    if promotions.len() > 1 {
        return Err(corrupt());
    }
    let promotion = promotions
        .first()
        .map(|transition| {
            let owner = positions
                .get(&transition.stored.echo_id)
                .and_then(|index| stored.get(*index))
                .ok_or_else(corrupt)?;
            durable_echo_transition(transition, owner.death_id)
        })
        .transpose()?;
    if promotion.as_ref().map(|value| value.echo_id) != expectation.promoted_echo_id {
        return Err(corrupt());
    }
    let plan = DurableEchoEnvelopeV1 {
        created: durable_echo_record(created, created_terminal_state)?,
        creation_transition: durable_echo_transition(creation, created.death_id)?,
        preexisting_available_echo_id: expectation.preexisting_available_echo_id,
        promotion,
    };
    positions.clear();
    Ok(EchoGraph {
        plan: Some(plan),
        stored,
        transitions,
    })
}

fn durable_echo_record(
    stored: &StoredDeathTerminalEchoV1,
    terminal_state: DurableEchoStateV1,
) -> Result<DurableEchoRecordV1, PersistenceError> {
    Ok(DurableEchoRecordV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
        echo_id: stored.echo_id,
        death_id: stored.death_id,
        account_id: stored.account_id,
        character_name_snapshot: stored.character_name_snapshot.clone(),
        class_id: stored.class_id.clone(),
        oath_id: stored.oath_id.clone(),
        level: u8::try_from(stored.level).map_err(|_| corrupt())?,
        appearance_snapshot_id: stored.appearance_snapshot_id.clone(),
        appearance_theme_id: stored.appearance_theme_id.clone(),
        weapon_signature_tag: stored.weapon_signature_tag.clone(),
        relic_signature_tag: stored.relic_signature_tag.clone(),
        bargains: stored.bargains.clone(),
        deed_tags: stored.deed_tags.clone(),
        killer_content_id: stored.killer_content_id.clone(),
        killer_pattern_id: stored.killer_pattern_id.clone(),
        death_region_id: stored.death_region_id.clone(),
        power_band: u8::try_from(stored.power_band).map_err(|_| corrupt())?,
        created_at_unix_ms: stored.created_at_unix_micros / 1_000,
        state: terminal_state,
        content_revision: stored.content_revision.clone(),
        snapshot_digest: stored.snapshot_digest,
    })
}

fn durable_echo_transition(
    transition: &LoadedEchoTransition,
    echo_death_id: [u8; 16],
) -> Result<DurableEchoTransitionV1, PersistenceError> {
    Ok(DurableEchoTransitionV1 {
        echo_id: transition.stored.echo_id,
        echo_death_id,
        ordinal: transition.stored.transition_ordinal,
        previous_state: transition
            .stored
            .previous_state
            .map(|value| {
                i16::try_from(value)
                    .map_err(|_| corrupt())
                    .and_then(echo_state)
            })
            .transpose()?,
        next_state: echo_state(
            i16::try_from(transition.stored.next_state).map_err(|_| corrupt())?,
        )?,
        reason: echo_reason(i16::try_from(transition.stored.reason).map_err(|_| corrupt())?)?,
        source_death_id: transition.stored.source_death_id,
        trigger_death_id: transition.stored.trigger_death_id,
        committed_at_unix_ms: transition.committed_at_unix_ms,
    })
}

fn echo_state(value: i16) -> Result<DurableEchoStateV1, PersistenceError> {
    match value {
        0 => Ok(DurableEchoStateV1::Dormant),
        1 => Ok(DurableEchoStateV1::Available),
        _ => Err(corrupt()),
    }
}

fn echo_reason(value: i16) -> Result<DurableEchoTransitionReasonV1, PersistenceError> {
    match value {
        0 => Ok(DurableEchoTransitionReasonV1::EligibleDeath),
        1 => Ok(DurableEchoTransitionReasonV1::OldestDormantPromotion),
        _ => Err(corrupt()),
    }
}

async fn load_items(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    death_id: [u8; 16],
) -> Result<Vec<StoredDeathTerminalItemV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT item_uid,character_id,template_id,content_revision,item_kind,item_level,rarity,\
                creation_kind,creation_request_id,roll_index,unit_ordinal,item_version,\
                security_state,location_kind,slot_index,destruction_reason,terminal_death_id \
         FROM item_instances WHERE namespace_id=$1 AND account_id=$2 AND (\
              terminal_death_id=$3 \
              OR (character_id=$4 AND location_kind=5) \
              OR (character_id IS NULL AND location_kind=6)) \
         ORDER BY item_uid",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(death_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(StoredDeathTerminalItemV1 {
                item_uid: exact_id(row.try_get("item_uid")?)?,
                character_id: optional_id(row.try_get("character_id")?)?,
                template_id: row.try_get("template_id")?,
                content_revision: row.try_get("content_revision")?,
                item_kind: u16_value(row.try_get("item_kind")?)?,
                item_level: optional_u16(row.try_get("item_level")?)?,
                rarity: optional_u16(row.try_get("rarity")?)?,
                creation_kind: u16_value(row.try_get("creation_kind")?)?,
                creation_request_id: exact_id(row.try_get("creation_request_id")?)?,
                roll_index: u16_from_i32(row.try_get("roll_index")?)?,
                unit_ordinal: u16_from_i32(row.try_get("unit_ordinal")?)?,
                item_version: unsigned(row.try_get("item_version")?)?,
                security_state: u16_value(row.try_get("security_state")?)?,
                location_kind: u16_value(row.try_get("location_kind")?)?,
                slot_index: optional_u16(row.try_get("slot_index")?)?,
                destruction_reason: row.try_get("destruction_reason")?,
                terminal_death_id: optional_id(row.try_get("terminal_death_id")?)?,
            })
        })
        .collect()
}

async fn load_item_ledger(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    death_id: [u8; 16],
) -> Result<Vec<StoredDeathTerminalItemLedgerV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT ledger_event_id,item_uid,mutation_id,event_kind,source_kind,pre_item_version,\
                post_item_version,pre_security_state,post_security_state,pre_location_kind,\
                post_location_kind,reason,terminal_death_id \
         FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
           AND terminal_death_id=$4 \
         ORDER BY item_uid,post_item_version,ledger_event_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(StoredDeathTerminalItemLedgerV1 {
                ledger_event_id: exact_id(row.try_get("ledger_event_id")?)?,
                item_uid: exact_id(row.try_get("item_uid")?)?,
                mutation_id: exact_id(row.try_get("mutation_id")?)?,
                event_kind: u16_value(row.try_get("event_kind")?)?,
                source_kind: u16_value(row.try_get("source_kind")?)?,
                pre_item_version: unsigned(row.try_get("pre_item_version")?)?,
                post_item_version: unsigned(row.try_get("post_item_version")?)?,
                pre_security_state: optional_u16(row.try_get("pre_security_state")?)?,
                post_security_state: u16_value(row.try_get("post_security_state")?)?,
                pre_location_kind: optional_u16(row.try_get("pre_location_kind")?)?,
                post_location_kind: u16_value(row.try_get("post_location_kind")?)?,
                reason: row.try_get("reason")?,
                terminal_death_id: optional_id(row.try_get("terminal_death_id")?)?,
            })
        })
        .collect()
}

async fn load_materials(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    death_id: [u8; 16],
) -> Result<Vec<StoredDeathTerminalMaterialV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT material_id,quantity,material_version,security_state,terminal_reason,\
                terminal_restore_point_id,terminal_death_id \
         FROM character_run_material_stacks WHERE namespace_id=$1 AND account_id=$2 \
           AND character_id=$3 AND terminal_death_id=$4 \
         ORDER BY material_id COLLATE \"C\"",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(StoredDeathTerminalMaterialV1 {
                material_id: row.try_get("material_id")?,
                quantity: u32_value(row.try_get("quantity")?)?,
                material_version: unsigned(row.try_get("material_version")?)?,
                security_state: u16_value(row.try_get("security_state")?)?,
                terminal_reason: row.try_get("terminal_reason")?,
                terminal_restore_point_id: optional_id(row.try_get("terminal_restore_point_id")?)?,
                terminal_death_id: optional_id(row.try_get("terminal_death_id")?)?,
            })
        })
        .collect()
}

async fn load_bargain_cleanup(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    event_id: [u8; 16],
) -> Result<StoredDeathTerminalBargainCleanupV1, PersistenceError> {
    let rows = sqlx::query(
        "SELECT event_id,event_type,aggregate_version,event_payload \
         FROM character_life_outbox WHERE namespace_id=$1 AND account_id=$2 \
           AND character_id=$3 AND event_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(event_id.as_slice())
    .fetch_all(connection)
    .await?;
    let [row] = rows.as_slice() else {
        return Err(corrupt());
    };
    let payload: Vec<u8> = row.try_get("event_payload")?;
    Ok(StoredDeathTerminalBargainCleanupV1 {
        event_id: exact_id(row.try_get("event_id")?)?,
        event_type: row.try_get("event_type")?,
        aggregate_version: unsigned(row.try_get("aggregate_version")?)?,
        event: crate::BargainLifeCleanupEventV1::decode(&payload).map_err(|_| corrupt())?,
    })
}

async fn load_audits(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    death_id: [u8; 16],
) -> Result<Vec<StoredDeathTerminalAuditV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT audit_event_id,death_id,mutation_id,event_kind,event_digest \
         FROM death_audit_events WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
           AND death_id=$4 ORDER BY audit_event_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(StoredDeathTerminalAuditV1 {
                audit_event_id: exact_id(row.try_get("audit_event_id")?)?,
                death_id: optional_id(row.try_get("death_id")?)?,
                mutation_id: exact_id(row.try_get("mutation_id")?)?,
                audit_kind: u16_value(row.try_get("event_kind")?)?,
                audit_digest: exact_hash(row.try_get("event_digest")?)?,
            })
        })
        .collect()
}

async fn load_outbox(
    connection: &mut PgConnection,
    death_id: [u8; 16],
) -> Result<Vec<StoredDeathTerminalOutboxV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT event_id,death_id,event_type,echo_id,echo_transition_ordinal,\
                trigger_death_id,event_payload \
         FROM death_outbox_events WHERE namespace_id=$1 AND (\
              (event_type='death_committed' AND death_id=$2) \
              OR (event_type='echo_created' AND death_id=$2 AND trigger_death_id=$2) \
              OR (event_type='echo_promoted' AND trigger_death_id=$2)) \
         ORDER BY death_id,event_type COLLATE \"C\",event_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(StoredDeathTerminalOutboxV1 {
                outbox_event_id: exact_id(row.try_get("event_id")?)?,
                death_id: exact_id(row.try_get("death_id")?)?,
                event_type: row.try_get("event_type")?,
                echo_id: optional_id(row.try_get("echo_id")?)?,
                echo_transition_ordinal: optional_u16(row.try_get("echo_transition_ordinal")?)?,
                trigger_death_id: optional_id(row.try_get("trigger_death_id")?)?,
                event_payload: row.try_get("event_payload")?,
            })
        })
        .collect()
}

async fn load_zero_counts(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<ZeroCounts, PersistenceError> {
    let row = sqlx::query(
        "SELECT \
            (SELECT count(*) FROM character_active_bargains WHERE namespace_id=$1 \
              AND account_id=$2 AND character_id=$3) AS active_bargains,\
            (SELECT count(*) FROM character_danger_checkpoints WHERE namespace_id=$1 \
              AND account_id=$2 AND character_id=$3) AS danger_checkpoints,\
            (SELECT count(*) FROM character_live_damage_trace_ticks_v1 WHERE namespace_id=$1 \
              AND account_id=$2 AND character_id=$3) AS live_trace_ticks,\
            (SELECT count(*) FROM character_live_damage_trace_entries_v1 WHERE namespace_id=$1 \
              AND account_id=$2 AND character_id=$3) AS live_trace_entries,\
            (SELECT count(*) FROM character_live_damage_trace_statuses_v1 WHERE namespace_id=$1 \
              AND account_id=$2 AND character_id=$3) AS live_trace_statuses",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_one(connection)
    .await?;
    Ok(ZeroCounts {
        active_bargains: count_u32(row.try_get("active_bargains")?)?,
        danger_checkpoints: count_u32(row.try_get("danger_checkpoints")?)?,
        live_trace_ticks: count_u32(row.try_get("live_trace_ticks")?)?,
        live_trace_entries: count_u32(row.try_get("live_trace_entries")?)?,
        live_trace_statuses: count_u32(row.try_get("live_trace_statuses")?)?,
    })
}

fn u16_from_i32(value: i32) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| corrupt())
}

fn count_u32(value: i64) -> Result<u32, PersistenceError> {
    u32::try_from(value).map_err(|_| corrupt())
}

#[allow(
    clippy::too_many_lines,
    reason = "the retained root, receipt window, and provenance links are one canonical proof"
)]
async fn load_trace_promotion(
    connection: &mut PgConnection,
    death_id: [u8; 16],
) -> Result<StoredDeathTerminalTracePromotionV1, PersistenceError> {
    let roots = sqlx::query(
        "SELECT contract_version,death_id,account_id,character_id,lineage_id,restore_point_id,\
                first_event_tick,death_tick,receipt_count,entry_count,status_count,\
                lethal_trace_tick_id,records_blake3,assets_blake3,localization_blake3,\
                receipt_window_digest,promotion_digest,terminal_payload_hash \
         FROM death_live_trace_sets_v1 WHERE namespace_id=$1 AND death_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let [root] = roots.as_slice() else {
        return Err(corrupt());
    };
    let receipt_rows = sqlx::query(
        "SELECT receipt_ordinal,trace_tick_id,expected_character_version,lineage_id,\
                restore_point_id,checkpoint_tick,event_tick,entry_count,status_count,lethal_count,\
                records_blake3,assets_blake3,localization_blake3,request_hash,tick_digest,\
                result_digest,floor(extract(epoch FROM issued_at)*1000)::bigint AS issued_at_ms,\
                floor(extract(epoch FROM receipt_committed_at)*1000)::bigint AS committed_at_ms \
         FROM death_live_trace_receipt_links_v1 WHERE namespace_id=$1 AND death_id=$2 \
         ORDER BY receipt_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let receipts = receipt_rows
        .iter()
        .enumerate()
        .map(|(expected, row)| {
            let receipt_ordinal = u16_value(row.try_get("receipt_ordinal")?)?;
            if usize::from(receipt_ordinal) != expected {
                return Err(corrupt());
            }
            Ok(StoredDeathTerminalTraceReceiptV1 {
                receipt_ordinal,
                trace_tick_id: exact_id(row.try_get("trace_tick_id")?)?,
                expected_character_version: unsigned(row.try_get("expected_character_version")?)?,
                lineage_id: exact_id(row.try_get("lineage_id")?)?,
                restore_point_id: exact_id(row.try_get("restore_point_id")?)?,
                checkpoint_tick: nonnegative(row.try_get("checkpoint_tick")?)?,
                event_tick: positive(row.try_get("event_tick")?)?,
                entry_count: u16_value(row.try_get("entry_count")?)?,
                status_count: u16_value(row.try_get("status_count")?)?,
                lethal_count: u16_value(row.try_get("lethal_count")?)?,
                records_blake3: row.try_get("records_blake3")?,
                assets_blake3: row.try_get("assets_blake3")?,
                localization_blake3: row.try_get("localization_blake3")?,
                request_hash: exact_hash(row.try_get("request_hash")?)?,
                tick_digest: exact_hash(row.try_get("tick_digest")?)?,
                result_digest: exact_hash(row.try_get("result_digest")?)?,
                issued_at_unix_ms: positive(row.try_get("issued_at_ms")?)?,
                committed_at_unix_ms: positive(row.try_get("committed_at_ms")?)?,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let lethal_receipt = receipts.last().ok_or_else(corrupt)?;
    let provenance_rows = sqlx::query(
        "SELECT trace_ordinal,receipt_ordinal,trace_tick_id,event_tick,event_ordinal,cause_kind,\
                source_entity_id,source_sim_entity_id,status_count,live_entry_digest \
         FROM death_live_trace_entry_provenance_v1 WHERE namespace_id=$1 AND death_id=$2 \
         ORDER BY trace_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    let provenance = provenance_rows
        .iter()
        .enumerate()
        .map(|(expected, row)| {
            let trace_ordinal = u16_value(row.try_get("trace_ordinal")?)?;
            if usize::from(trace_ordinal) != expected {
                return Err(corrupt());
            }
            Ok(StoredDeathTerminalTraceProvenanceV1 {
                trace_ordinal,
                receipt_ordinal: u16_value(row.try_get("receipt_ordinal")?)?,
                trace_tick_id: exact_id(row.try_get("trace_tick_id")?)?,
                event_tick: positive(row.try_get("event_tick")?)?,
                event_ordinal: u32_value(row.try_get("event_ordinal")?)?,
                cause: death_cause(row.try_get("cause_kind")?)?,
                source_entity_id: optional_id(row.try_get("source_entity_id")?)?,
                source_sim_entity_id: row
                    .try_get::<Option<Vec<u8>>, _>("source_sim_entity_id")?
                    .map(decode_source_sim_entity_id)
                    .transpose()?,
                status_count: u16_value(row.try_get("status_count")?)?,
                live_entry_digest: exact_hash(row.try_get("live_entry_digest")?)?,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    Ok(StoredDeathTerminalTracePromotionV1 {
        contract_version: u16_value(root.try_get("contract_version")?)?,
        death_id: exact_id(root.try_get("death_id")?)?,
        account_id: exact_id(root.try_get("account_id")?)?,
        character_id: exact_id(root.try_get("character_id")?)?,
        lineage_id: exact_id(root.try_get("lineage_id")?)?,
        restore_point_id: exact_id(root.try_get("restore_point_id")?)?,
        checkpoint_tick: lethal_receipt.checkpoint_tick,
        terminal_character_version: lethal_receipt.expected_character_version,
        first_event_tick: positive(root.try_get("first_event_tick")?)?,
        death_tick: positive(root.try_get("death_tick")?)?,
        receipt_count: u16_value(root.try_get("receipt_count")?)?,
        entry_count: u16_from_i32(root.try_get("entry_count")?)?,
        status_count: u32_value(root.try_get("status_count")?)?,
        lethal_trace_tick_id: exact_id(root.try_get("lethal_trace_tick_id")?)?,
        records_blake3: root.try_get("records_blake3")?,
        assets_blake3: root.try_get("assets_blake3")?,
        localization_blake3: root.try_get("localization_blake3")?,
        receipt_window_digest: exact_hash(root.try_get("receipt_window_digest")?)?,
        promotion_digest: exact_hash(root.try_get("promotion_digest")?)?,
        terminal_payload_hash: exact_hash(root.try_get("terminal_payload_hash")?)?,
        receipts,
        provenance,
    })
}

async fn load_trace_conflicts(
    connection: &mut PgConnection,
    death_id: [u8; 16],
) -> Result<Vec<StoredDeathTerminalTraceConflictV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT audit_id,conflict_code,stored_promotion_digest,attempted_promotion_digest,\
                stored_terminal_payload_hash,attempted_terminal_payload_hash,\
                floor(extract(epoch FROM attempted_issued_at)*1000)::bigint AS attempted_issued_ms \
         FROM death_live_trace_promotion_conflict_audits_v1 \
         WHERE namespace_id=$1 AND death_id=$2 ORDER BY audit_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(StoredDeathTerminalTraceConflictV1 {
                audit_id: exact_id(row.try_get("audit_id")?)?,
                conflict_code: u16_value(row.try_get("conflict_code")?)?,
                stored_promotion_digest: exact_hash(row.try_get("stored_promotion_digest")?)?,
                attempted_promotion_digest: exact_hash(row.try_get("attempted_promotion_digest")?)?,
                stored_terminal_payload_hash: exact_hash(
                    row.try_get("stored_terminal_payload_hash")?,
                )?,
                attempted_terminal_payload_hash: exact_hash(
                    row.try_get("attempted_terminal_payload_hash")?,
                )?,
                attempted_issued_at_unix_ms: positive(row.try_get("attempted_issued_ms")?)?,
            })
        })
        .collect()
}

fn decode_source_sim_entity_id(value: Vec<u8>) -> Result<u64, PersistenceError> {
    let bytes: [u8; 8] = value.try_into().map_err(|_| corrupt())?;
    let identity = u64::from_le_bytes(bytes);
    (identity != 0).then_some(identity).ok_or_else(corrupt)
}
