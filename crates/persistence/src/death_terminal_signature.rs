//! Canonical restart signature for one committed Core permadeath terminal.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-001`, `DTH-020`,
//! `TECH-020`-`TECH-023`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-ECHO-009`, `CONT-HUB-001`, `CONT-HUB-002`; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-02D`, `GB-M03-06`,
//! `GB-M03-13`, plus the M03 restart, atomicity, and nonduplication gates.
//!
//! This module is deliberately read-only. It gives hosted tests and operators one stable,
//! persistence-owned value to compare before response loss, after replay/reconnect, and after a
//! newly bound process. Mutable delivery bookkeeping such as outbox `published_at` is excluded.

use std::collections::{BTreeMap, BTreeSet};

use crate::death_live_trace_promotion::{
    DurableDeathTracePromotionDigestMaterialV1, canonical_stored_death_trace_promotion_digest_v1,
};
use crate::live_damage_trace_repository::{
    LiveDamageTracePromotionReceiptV1, canonical_live_damage_trace_receipt_window_digest_v1,
};
use crate::{
    AuthoritativeDeathPlanV1, BARGAIN_LIFE_CLEANUP_EVENT_SCHEMA_VERSION, BargainLifeCleanupEventV1,
    BargainLifeEndReason, DeathAggregateVersionsV1, DurableDeathCauseV1, DurableDeathProvenanceV1,
    DurableDeathTraceEntryProvenanceV1, DurableDestructionEntryV1, DurableDestructionLocationV1,
    DurableEchoOutcomeV1, DurableEchoRecordV1, DurableEchoStateV1, DurableEchoTransitionV1,
    DurableOrderedContentIdV1, LiveDamageTraceContentAuthorityV1, LiveDamageTraceDangerAuthorityV1,
    PersistenceError, StoredCommittedDeathTerminalV1, WIPEABLE_CORE_NAMESPACE,
    canonical_death_terminal_payload_hash_v1,
};

pub const CORE_DEATH_TERMINAL_SIGNATURE_CONTEXT_V1: &str =
    "gravebound.m03-06e.death-terminal-signature.v1";

const DEATH_TERMINAL_SIGNATURE_CONTRACT_VERSION: u16 = 1;
const LIFE_STATE_DEAD: u16 = 1;
const WORLD_LOCATION_DANGER: u16 = 2;
const RESTORE_STATE_DEATH_COMMITTED: u16 = 2;
const LINEAGE_STATE_TERMINATED: u16 = 3;
const SECURITY_SAFE: u16 = 0;
const SECURITY_AT_RISK_EQUIPPED: u16 = 1;
const SECURITY_AT_RISK_PENDING: u16 = 2;
const SECURITY_DESTROYED: u16 = 3;
const LOCATION_DESTROYED: u16 = 4;
const LOCATION_CHARACTER_SAFE: u16 = 5;
const LOCATION_VAULT: u16 = 6;
const LEDGER_EVENT_DESTROYED: u16 = 2;
const LEDGER_SOURCE_DEATH: u16 = 3;
const ECHO_STATE_DORMANT: u16 = 0;
const ECHO_STATE_AVAILABLE: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredCoreDeathTerminalSignatureV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub death_id: [u8; 16],
    pub terminal: StoredCommittedDeathTerminalV1,
    pub plan: AuthoritativeDeathPlanV1,
    pub aggregate: StoredDeathTerminalAggregateV1,
    pub graph_root: StoredDeathTerminalGraphRootV1,
    pub trace_promotion: StoredDeathTerminalTracePromotionV1,
    pub trace_conflicts: Vec<StoredDeathTerminalTraceConflictV1>,
    pub items: Vec<StoredDeathTerminalItemV1>,
    pub item_ledger: Vec<StoredDeathTerminalItemLedgerV1>,
    pub materials: Vec<StoredDeathTerminalMaterialV1>,
    pub bargain_cleanup: StoredDeathTerminalBargainCleanupV1,
    pub echoes: Vec<StoredDeathTerminalEchoV1>,
    pub echo_transitions: Vec<StoredDeathTerminalEchoTransitionV1>,
    pub audits: Vec<StoredDeathTerminalAuditV1>,
    pub outbox: Vec<StoredDeathTerminalOutboxV1>,
    pub counts: StoredDeathTerminalGraphCountsV1,
}

impl StoredCoreDeathTerminalSignatureV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, PersistenceError> {
        validate_signature(self)?;
        postcard::to_stdvec(self).map_err(|_| PersistenceError::CorruptStoredDeathTerminalSignature)
    }

    pub fn digest(&self) -> Result<[u8; 32], PersistenceError> {
        Ok(blake3::derive_key(
            CORE_DEATH_TERMINAL_SIGNATURE_CONTEXT_V1,
            &self.canonical_bytes()?,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalAggregateV1 {
    pub account_version: u64,
    pub selected_character_id: Option<[u8; 16]>,
    pub character_life_state: u16,
    pub character_roster_ordinal: Option<u16>,
    pub character_version: u64,
    pub world_character_version: u64,
    pub world_location_kind: u16,
    pub world_location_content_id: Option<String>,
    pub world_lineage_id: Option<[u8; 16]>,
    pub world_restore_point_id: Option<[u8; 16]>,
    pub current_health: u32,
    pub progression_version: u64,
    pub inventory_version: u64,
    pub oath_bargain_version: u64,
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub life_metrics_version: u64,
    pub ash_balance: u32,
    pub ash_wallet_version: u64,
    pub lineage_state: u16,
    pub restore_state: u16,
    pub restore_death_mutation_id: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalGraphRootV1 {
    pub mutation_id: [u8; 16],
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub death_tick: u64,
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub provenance: DurableDeathProvenanceV1,
    pub trace_entry_count: u16,
    pub destruction_entry_count: u16,
    pub former_roster_ordinal: u16,
    pub echo_expected: bool,
    pub preexisting_available_echo_id: Option<[u8; 16]>,
    pub promoted_echo_id: Option<[u8; 16]>,
    pub content_revision: String,
    pub world_records_blake3: String,
    pub world_assets_blake3: String,
    pub world_localization_blake3: String,
    pub presentation_records_blake3: String,
    pub presentation_assets_blake3: String,
    pub presentation_localization_blake3: String,
    pub bargain_cleanup_event_id: [u8; 16],
    pub versions: DeathAggregateVersionsV1,
    pub trace_digest: [u8; 32],
    pub destruction_digest: [u8; 32],
    pub summary_digest: [u8; 32],
    pub memorial_digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalItemV1 {
    pub item_uid: [u8; 16],
    pub character_id: Option<[u8; 16]>,
    pub template_id: String,
    pub content_revision: String,
    pub item_kind: u16,
    pub item_level: Option<u16>,
    pub rarity: Option<u16>,
    pub creation_kind: u16,
    pub creation_request_id: [u8; 16],
    pub roll_index: u16,
    pub unit_ordinal: u16,
    pub item_version: u64,
    pub security_state: u16,
    pub location_kind: u16,
    pub slot_index: Option<u16>,
    pub destruction_reason: Option<String>,
    pub terminal_death_id: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalItemLedgerV1 {
    pub ledger_event_id: [u8; 16],
    pub item_uid: [u8; 16],
    pub mutation_id: [u8; 16],
    pub event_kind: u16,
    pub source_kind: u16,
    pub pre_item_version: u64,
    pub post_item_version: u64,
    pub pre_security_state: Option<u16>,
    pub post_security_state: u16,
    pub pre_location_kind: Option<u16>,
    pub post_location_kind: u16,
    pub reason: Option<String>,
    pub terminal_death_id: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalMaterialV1 {
    pub material_id: String,
    pub quantity: u32,
    pub material_version: u64,
    pub security_state: u16,
    pub terminal_reason: Option<String>,
    pub terminal_restore_point_id: Option<[u8; 16]>,
    pub terminal_death_id: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalBargainCleanupV1 {
    pub event_id: [u8; 16],
    pub event_type: String,
    pub aggregate_version: u64,
    pub event: BargainLifeCleanupEventV1,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalTracePromotionV1 {
    pub contract_version: u16,
    pub death_id: [u8; 16],
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub checkpoint_tick: u64,
    pub terminal_character_version: u64,
    pub first_event_tick: u64,
    pub death_tick: u64,
    pub receipt_count: u16,
    pub entry_count: u16,
    pub status_count: u32,
    pub lethal_trace_tick_id: [u8; 16],
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
    pub receipt_window_digest: [u8; 32],
    pub promotion_digest: [u8; 32],
    pub terminal_payload_hash: [u8; 32],
    pub receipts: Vec<StoredDeathTerminalTraceReceiptV1>,
    pub provenance: Vec<StoredDeathTerminalTraceProvenanceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalTraceReceiptV1 {
    pub receipt_ordinal: u16,
    pub trace_tick_id: [u8; 16],
    pub expected_character_version: u64,
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub checkpoint_tick: u64,
    pub event_tick: u64,
    pub entry_count: u16,
    pub status_count: u16,
    pub lethal_count: u16,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
    pub request_hash: [u8; 32],
    pub tick_digest: [u8; 32],
    pub result_digest: [u8; 32],
    pub issued_at_unix_ms: u64,
    pub committed_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalTraceProvenanceV1 {
    pub trace_ordinal: u16,
    pub receipt_ordinal: u16,
    pub trace_tick_id: [u8; 16],
    pub event_tick: u64,
    pub event_ordinal: u32,
    pub cause: DurableDeathCauseV1,
    pub source_entity_id: Option<[u8; 16]>,
    pub source_sim_entity_id: Option<u64>,
    pub status_count: u16,
    pub live_entry_digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalTraceConflictV1 {
    pub audit_id: [u8; 16],
    pub conflict_code: u16,
    pub stored_promotion_digest: [u8; 32],
    pub attempted_promotion_digest: [u8; 32],
    pub stored_terminal_payload_hash: [u8; 32],
    pub attempted_terminal_payload_hash: [u8; 32],
    pub attempted_issued_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalEchoV1 {
    pub echo_id: [u8; 16],
    pub death_id: [u8; 16],
    pub account_id: [u8; 16],
    pub character_name_snapshot: String,
    pub class_id: String,
    pub oath_id: Option<String>,
    pub level: u16,
    pub appearance_snapshot_id: String,
    pub appearance_theme_id: String,
    pub weapon_signature_tag: Option<String>,
    pub relic_signature_tag: Option<String>,
    pub bargains: Vec<DurableOrderedContentIdV1>,
    pub deed_tags: Vec<DurableOrderedContentIdV1>,
    pub killer_content_id: String,
    pub killer_pattern_id: Option<String>,
    pub death_region_id: String,
    pub power_band: u16,
    pub state: u16,
    pub content_revision: String,
    pub snapshot_digest: [u8; 32],
    pub created_at_unix_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalEchoTransitionV1 {
    pub echo_id: [u8; 16],
    pub transition_ordinal: u16,
    pub previous_state: Option<u16>,
    pub next_state: u16,
    pub reason: u16,
    pub source_death_id: Option<[u8; 16]>,
    pub trigger_death_id: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalAuditV1 {
    pub audit_event_id: [u8; 16],
    pub death_id: Option<[u8; 16]>,
    pub mutation_id: [u8; 16],
    pub audit_kind: u16,
    pub audit_digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalOutboxV1 {
    pub outbox_event_id: [u8; 16],
    pub death_id: [u8; 16],
    pub event_type: String,
    pub echo_id: Option<[u8; 16]>,
    pub echo_transition_ordinal: Option<u16>,
    pub trigger_death_id: Option<[u8; 16]>,
    pub event_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredDeathTerminalGraphCountsV1 {
    pub trace_entries: u32,
    pub trace_statuses: u32,
    pub summary_bargains: u32,
    pub summary_damage_entries: u32,
    pub summary_projection_entries: u32,
    pub memorial_records: u32,
    pub destruction_entries: u32,
    pub mutation_results: u32,
    pub retained_trace_sets: u32,
    pub retained_trace_receipt_links: u32,
    pub retained_trace_provenance_entries: u32,
    pub retained_trace_conflicts: u32,
    pub item_records: u32,
    pub item_ledger_entries: u32,
    pub material_records: u32,
    pub echo_records: u32,
    pub echo_transitions: u32,
    pub audit_events: u32,
    pub outbox_events: u32,
    pub active_bargains: u32,
    pub danger_checkpoints: u32,
    pub live_trace_ticks: u32,
    pub live_trace_entries: u32,
    pub live_trace_statuses: u32,
}

fn validate_signature(
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> Result<(), PersistenceError> {
    let result = &signature.terminal.result;
    if signature.contract_version != DEATH_TERMINAL_SIGNATURE_CONTRACT_VERSION
        || signature.namespace_id != WIPEABLE_CORE_NAMESPACE
        || signature.account_id == [0; 16]
        || signature.character_id == [0; 16]
        || !is_uuid_v7(signature.death_id)
        || result.account_id != signature.account_id
        || result.character_id != signature.character_id
        || result.death_id != signature.death_id
    {
        return Err(corrupt());
    }

    signature.terminal.validate().map_err(|_| corrupt())?;
    signature.plan.validate().map_err(|_| corrupt())?;
    validate_root(signature)?;
    validate_aggregate(signature)?;
    validate_trace_promotion(signature)?;
    validate_custody(signature)?;
    validate_cleanup(signature)?;
    validate_echo_graph(signature)?;
    validate_operational_graph(signature)?;
    Ok(())
}

fn validate_root(signature: &StoredCoreDeathTerminalSignatureV1) -> Result<(), PersistenceError> {
    let root = &signature.graph_root;
    let terminal = &signature.terminal;
    let result = &terminal.result;
    let event = &signature.plan.event;
    if root.mutation_id != result.mutation_id
        || root.lineage_id != terminal.lineage_id
        || root.restore_point_id != terminal.restore_point_id
        || root.death_tick != terminal.death_tick
        || root.death_tick == 0
        || root.trace_entry_count == 0
        || usize::from(root.destruction_entry_count) != terminal_destruction_count(signature)
        || root.former_roster_ordinal == 0
        || root.versions != result.versions
        || root.trace_digest != result.trace_digest
        || root.destruction_digest != result.destruction_digest
        || root.summary_digest != result.summary_digest
        || root.memorial_digest != result.memorial_digest
        || signature
            .plan
            .canonical_plan_hash()
            .map_err(|_| corrupt())?
            != result.canonical_plan_hash
        || event.namespace_id != signature.namespace_id
        || event.account_id != signature.account_id
        || event.character_id != signature.character_id
        || event.death_id != signature.death_id
        || event.mutation_id != root.mutation_id
        || event.lineage_id != root.lineage_id
        || event.restore_point_id != root.restore_point_id
        || event.death_tick != root.death_tick
        || event.lifetime_ticks != root.lifetime_ticks
        || event.permadeath_combat_ticks != root.permadeath_combat_ticks
        || event.provenance != root.provenance
        || u16::from(event.former_roster_ordinal) != root.former_roster_ordinal
        || event.content_revision != root.content_revision
        || event.records_blake3 != root.world_records_blake3
        || event.assets_blake3 != root.world_assets_blake3
        || event.localization_blake3 != root.world_localization_blake3
        || event.presentation.records_blake3 != root.presentation_records_blake3
        || event.presentation.assets_blake3 != root.presentation_assets_blake3
        || event.presentation.localization_blake3 != root.presentation_localization_blake3
        || event.bargain_cleanup_event_id != root.bargain_cleanup_event_id
        || event.versions != root.versions
        || event.trace_entry_count != root.trace_entry_count
        || event.destruction_entry_count != root.destruction_entry_count
        || event.trace_digest != root.trace_digest
        || event.destruction_digest != root.destruction_digest
        || signature.plan.summary.snapshot_digest != root.summary_digest
        || signature.plan.memorial.presentation_digest != root.memorial_digest
        || event.canonical_request_hash != result.canonical_request_hash
        || root.echo_expected != signature.plan.echo.is_some()
        || root.preexisting_available_echo_id
            != signature
                .plan
                .echo
                .as_ref()
                .and_then(|echo| echo.preexisting_available_echo_id)
        || root.promoted_echo_id
            != signature
                .plan
                .echo
                .as_ref()
                .and_then(|echo| echo.promotion.as_ref().map(|transition| transition.echo_id))
        || root.bargain_cleanup_event_id == [0; 16]
        || !valid_core_revision(&root.content_revision)
        || [
            &root.world_records_blake3,
            &root.world_assets_blake3,
            &root.world_localization_blake3,
            &root.presentation_records_blake3,
            &root.presentation_assets_blake3,
            &root.presentation_localization_blake3,
        ]
        .into_iter()
        .any(|hash| !valid_hex_hash(hash))
    {
        return Err(corrupt());
    }
    Ok(())
}

fn validate_aggregate(
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> Result<(), PersistenceError> {
    let aggregate = &signature.aggregate;
    let root = &signature.graph_root;
    let versions = &signature.terminal.result.versions;
    if aggregate.account_version != versions.account.post
        || aggregate.selected_character_id.is_some()
        || aggregate.character_life_state != LIFE_STATE_DEAD
        || aggregate.character_roster_ordinal.is_some()
        || aggregate.character_version != versions.character.post
        || aggregate.world_character_version != versions.character.post
        || aggregate.world_location_kind != WORLD_LOCATION_DANGER
        || aggregate.world_lineage_id != Some(root.lineage_id)
        || aggregate.world_restore_point_id != Some(root.restore_point_id)
        || aggregate.current_health != 0
        || aggregate.progression_version != versions.progression.post
        || aggregate.inventory_version != versions.inventory.post
        || aggregate.oath_bargain_version != versions.oath_bargain.post
        || aggregate.life_metrics_version != versions.life_metrics.post
        || aggregate.lifetime_ticks != root.lifetime_ticks
        || aggregate.permadeath_combat_ticks != root.permadeath_combat_ticks
        || aggregate.ash_wallet_version == 0
        || aggregate.lineage_state != LINEAGE_STATE_TERMINATED
        || aggregate.restore_state != RESTORE_STATE_DEATH_COMMITTED
        || aggregate.restore_death_mutation_id != Some(root.mutation_id)
    {
        return Err(corrupt());
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "the retained trace is one cross-table canonical evidence graph"
)]
fn validate_trace_promotion(
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> Result<(), PersistenceError> {
    let promotion = &signature.trace_promotion;
    let terminal = &signature.terminal;
    let root = &signature.graph_root;
    let trace = &signature.plan.trace;
    if promotion.contract_version != 1
        || promotion.death_id != signature.death_id
        || promotion.account_id != signature.account_id
        || promotion.character_id != signature.character_id
        || promotion.lineage_id != root.lineage_id
        || promotion.restore_point_id != root.restore_point_id
        || promotion.terminal_character_version != terminal.result.versions.character.pre
        || promotion.first_event_tick != trace.first().map_or(0, |entry| entry.event_tick)
        || promotion.death_tick != root.death_tick
        || usize::from(promotion.receipt_count) != promotion.receipts.len()
        || usize::from(promotion.entry_count) != trace.len()
        || usize::from(promotion.entry_count) != promotion.provenance.len()
        || promotion.status_count
            != trace
                .iter()
                .map(|entry| u32::try_from(entry.statuses.len()).unwrap_or(u32::MAX))
                .sum::<u32>()
        || promotion.lethal_trace_tick_id == [0; 16]
        || promotion.records_blake3 != root.world_records_blake3
        || promotion.assets_blake3 != root.world_assets_blake3
        || promotion.localization_blake3 != root.world_localization_blake3
        || promotion.promotion_digest != terminal.promotion_digest
        || promotion.terminal_payload_hash != terminal.terminal_payload_hash
        || promotion.receipt_window_digest == [0; 32]
        || !strictly_sorted_by(&promotion.receipts, |receipt| receipt.receipt_ordinal)
        || !strictly_sorted_by(&promotion.provenance, |entry| entry.trace_ordinal)
    {
        return Err(corrupt());
    }

    let receipts = promotion
        .receipts
        .iter()
        .enumerate()
        .map(|(ordinal, receipt)| {
            if usize::from(receipt.receipt_ordinal) != ordinal
                || receipt.trace_tick_id == [0; 16]
                || receipt.expected_character_version != promotion.terminal_character_version
                || receipt.lineage_id != promotion.lineage_id
                || receipt.restore_point_id != promotion.restore_point_id
                || receipt.checkpoint_tick != promotion.checkpoint_tick
                || receipt.event_tick == 0
                || receipt.records_blake3 != promotion.records_blake3
                || receipt.assets_blake3 != promotion.assets_blake3
                || receipt.localization_blake3 != promotion.localization_blake3
                || [
                    receipt.request_hash,
                    receipt.tick_digest,
                    receipt.result_digest,
                ]
                .contains(&[0; 32])
                || receipt.issued_at_unix_ms == 0
                || receipt.committed_at_unix_ms < receipt.issued_at_unix_ms
                || (ordinal + 1 == promotion.receipts.len() && receipt.lethal_count != 1)
                || (ordinal + 1 < promotion.receipts.len() && receipt.lethal_count != 0)
                || (ordinal > 0 && promotion.receipts[ordinal - 1].event_tick >= receipt.event_tick)
            {
                return Err(corrupt());
            }
            Ok(LiveDamageTracePromotionReceiptV1 {
                account_id: signature.account_id,
                character_id: signature.character_id,
                trace_tick_id: receipt.trace_tick_id,
                expected_character_version: receipt.expected_character_version,
                danger: LiveDamageTraceDangerAuthorityV1 {
                    lineage_id: receipt.lineage_id,
                    restore_point_id: receipt.restore_point_id,
                    checkpoint_tick: receipt.checkpoint_tick,
                },
                event_tick: receipt.event_tick,
                entry_count: usize::from(receipt.entry_count),
                status_count: usize::from(receipt.status_count),
                lethal_count: usize::from(receipt.lethal_count),
                content: LiveDamageTraceContentAuthorityV1 {
                    records_blake3: receipt.records_blake3.clone(),
                    assets_blake3: receipt.assets_blake3.clone(),
                    localization_blake3: receipt.localization_blake3.clone(),
                },
                request_hash: receipt.request_hash,
                tick_digest: receipt.tick_digest,
                result_digest: receipt.result_digest,
                issued_at_unix_ms: receipt.issued_at_unix_ms,
                committed_at_unix_ms: receipt.committed_at_unix_ms,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    if canonical_live_damage_trace_receipt_window_digest_v1(&receipts).map_err(|_| corrupt())?
        != promotion.receipt_window_digest
    {
        return Err(corrupt());
    }

    let mut durable_by_sim = BTreeMap::<u64, [u8; 16]>::new();
    let mut sim_by_durable = BTreeMap::<[u8; 16], u64>::new();
    let provenance = promotion
        .provenance
        .iter()
        .enumerate()
        .map(|(ordinal, entry)| {
            let durable = trace.get(ordinal).ok_or_else(corrupt)?;
            let receipt = promotion
                .receipts
                .get(usize::from(entry.receipt_ordinal))
                .ok_or_else(corrupt)?;
            if usize::from(entry.trace_ordinal) != ordinal
                || entry.trace_tick_id != receipt.trace_tick_id
                || entry.event_tick != receipt.event_tick
                || entry.event_tick != durable.event_tick
                || entry.event_ordinal != durable.event_ordinal
                || entry.source_entity_id != durable.source_entity_id
                || entry.source_entity_id.is_some() != entry.source_sim_entity_id.is_some()
                || usize::from(entry.status_count) != durable.statuses.len()
                || entry.live_entry_digest == [0; 32]
            {
                return Err(corrupt());
            }
            if let (Some(durable_id), Some(sim_id)) =
                (entry.source_entity_id, entry.source_sim_entity_id)
                && (durable_by_sim
                    .insert(sim_id, durable_id)
                    .is_some_and(|existing| existing != durable_id)
                    || sim_by_durable
                        .insert(durable_id, sim_id)
                        .is_some_and(|existing| existing != sim_id))
            {
                return Err(corrupt());
            }
            Ok(DurableDeathTraceEntryProvenanceV1 {
                trace_ordinal: entry.trace_ordinal,
                trace_tick_id: entry.trace_tick_id,
                event_tick: entry.event_tick,
                event_ordinal: entry.event_ordinal,
                cause: entry.cause,
                source_entity_id: entry.source_entity_id,
                source_sim_entity_id: entry.source_sim_entity_id,
                status_count: entry.status_count,
                live_entry_digest: entry.live_entry_digest,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    for (receipt_index, receipt) in promotion.receipts.iter().enumerate() {
        let linked: Vec<_> = promotion
            .provenance
            .iter()
            .enumerate()
            .filter(|(_, entry)| usize::from(entry.receipt_ordinal) == receipt_index)
            .collect();
        let linked_statuses: usize = linked
            .iter()
            .map(|(_, entry)| usize::from(entry.status_count))
            .sum();
        let linked_lethal = linked
            .iter()
            .filter(|(index, _)| trace[*index].lethal)
            .count();
        if linked.len() != usize::from(receipt.entry_count)
            || linked_statuses != usize::from(receipt.status_count)
            || linked_lethal != usize::from(receipt.lethal_count)
        {
            return Err(corrupt());
        }
    }

    let lethal_receipt = receipts.last().ok_or_else(corrupt)?;
    let expected_promotion_digest = canonical_stored_death_trace_promotion_digest_v1(
        &DurableDeathTracePromotionDigestMaterialV1 {
            contract_version: promotion.contract_version,
            death_id: promotion.death_id,
            account_id: promotion.account_id,
            character_id: promotion.character_id,
            lineage_id: promotion.lineage_id,
            restore_point_id: promotion.restore_point_id,
            checkpoint_tick: promotion.checkpoint_tick,
            terminal_character_version: promotion.terminal_character_version,
            records_blake3: &promotion.records_blake3,
            assets_blake3: &promotion.assets_blake3,
            localization_blake3: &promotion.localization_blake3,
            first_event_tick: promotion.first_event_tick,
            death_tick: promotion.death_tick,
            receipt_count: promotion.receipt_count,
            entry_count: promotion.entry_count,
            status_count: promotion.status_count,
            lethal_trace_tick_id: promotion.lethal_trace_tick_id,
        },
        lethal_receipt.request_hash,
        &provenance,
    )
    .map_err(|_| corrupt())?;
    if expected_promotion_digest != promotion.promotion_digest
        || canonical_death_terminal_payload_hash_v1(
            terminal.result.canonical_request_hash,
            expected_promotion_digest,
        )
        .map_err(|_| corrupt())?
            != promotion.terminal_payload_hash
        || !strictly_sorted_by(&signature.trace_conflicts, |conflict| conflict.audit_id)
        || signature.trace_conflicts.iter().any(|conflict| {
            conflict.audit_id == [0; 16]
                || conflict.conflict_code != 0
                || conflict.stored_promotion_digest != promotion.promotion_digest
                || conflict.attempted_promotion_digest == [0; 32]
                || conflict.attempted_promotion_digest == promotion.promotion_digest
                || conflict.stored_terminal_payload_hash != promotion.terminal_payload_hash
                || conflict.attempted_terminal_payload_hash == [0; 32]
                || canonical_death_terminal_payload_hash_v1(
                    terminal.result.canonical_request_hash,
                    conflict.attempted_promotion_digest,
                )
                .ok()
                    != Some(conflict.attempted_terminal_payload_hash)
                || conflict.attempted_issued_at_unix_ms == 0
        })
        || usize::try_from(signature.counts.retained_trace_conflicts).ok()
            != Some(signature.trace_conflicts.len())
    {
        return Err(corrupt());
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "custody validation is one destruction-to-item/material bijection"
)]
fn validate_custody(
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> Result<(), PersistenceError> {
    if !strictly_sorted_by(&signature.items, |item| item.item_uid)
        || !strictly_sorted_by(&signature.item_ledger, |entry| {
            (
                entry.item_uid,
                entry.post_item_version,
                entry.ledger_event_id,
            )
        })
        || !strictly_sorted_by(&signature.materials, |material| {
            material.material_id.clone()
        })
        || signature.items.iter().any(|item| {
            item.item_uid == [0; 16]
                || item.creation_request_id == [0; 16]
                || item.item_version == 0
                || !valid_core_revision(&item.content_revision)
                || item.content_revision != signature.graph_root.content_revision
                || matches!(
                    item.security_state,
                    SECURITY_AT_RISK_EQUIPPED | SECURITY_AT_RISK_PENDING
                )
                || !valid_terminal_item(item, signature)
        })
        || signature
            .materials
            .iter()
            .any(|material| !valid_terminal_material(material, signature.death_id))
    {
        return Err(corrupt());
    }

    let mut safe_slots: BTreeMap<_, Vec<&StoredDeathTerminalItemV1>> = BTreeMap::new();
    for item in &signature.items {
        if let Some(slot_index) = item.slot_index {
            safe_slots
                .entry((item.location_kind, item.character_id, slot_index))
                .or_default()
                .push(item);
        }
    }
    if safe_slots.values().any(|slot| {
        let first = slot[0];
        match first.item_kind {
            0 => slot.len() != 1,
            1 => {
                slot.len() > 6
                    || slot
                        .iter()
                        .any(|item| item.item_kind != 1 || item.template_id != first.template_id)
            }
            _ => true,
        }
    }) {
        return Err(corrupt());
    }

    let destroyed_items: BTreeMap<_, _> = signature
        .items
        .iter()
        .filter(|item| item.terminal_death_id == Some(signature.death_id))
        .map(|item| (item.item_uid, item))
        .collect();
    let terminal_ledger: Vec<_> = signature
        .item_ledger
        .iter()
        .filter(|entry| entry.terminal_death_id == Some(signature.death_id))
        .collect();
    if terminal_ledger.len() != destroyed_items.len() {
        return Err(corrupt());
    }
    let mut seen = BTreeSet::new();
    for entry in terminal_ledger {
        let Some(item) = destroyed_items.get(&entry.item_uid) else {
            return Err(corrupt());
        };
        if !seen.insert(entry.item_uid)
            || entry.ledger_event_id == [0; 16]
            || entry.mutation_id != signature.graph_root.mutation_id
            || entry.event_kind != LEDGER_EVENT_DESTROYED
            || entry.source_kind != LEDGER_SOURCE_DEATH
            || entry.pre_item_version == 0
            || entry.post_item_version != entry.pre_item_version.saturating_add(1)
            || entry.post_item_version != item.item_version
            || !matches!(
                entry.pre_security_state,
                Some(SECURITY_AT_RISK_EQUIPPED | SECURITY_AT_RISK_PENDING)
            )
            || entry.post_security_state != SECURITY_DESTROYED
            || entry.post_location_kind != LOCATION_DESTROYED
            || entry.reason.as_deref() != Some("permadeath")
        {
            return Err(corrupt());
        }
    }

    for destruction in &signature.plan.destruction {
        match destruction {
            DurableDestructionEntryV1::Item {
                content_id,
                item_uid,
                location,
                pre_item_version,
                post_item_version,
                ledger_event_id,
                ..
            } => {
                let Some(item) = destroyed_items.get(item_uid) else {
                    return Err(corrupt());
                };
                let Some(ledger) = signature
                    .item_ledger
                    .iter()
                    .find(|entry| entry.ledger_event_id == *ledger_event_id)
                else {
                    return Err(corrupt());
                };
                let expected_location = match location {
                    DurableDestructionLocationV1::Equipment { .. } => 0,
                    DurableDestructionLocationV1::Belt { .. } => 1,
                    DurableDestructionLocationV1::RunBackpack { .. } => 2,
                    DurableDestructionLocationV1::PersonalGround { .. } => 3,
                };
                if item.template_id != *content_id
                    || item.item_version != *post_item_version
                    || ledger.item_uid != *item_uid
                    || ledger.pre_item_version != *pre_item_version
                    || ledger.post_item_version != *post_item_version
                    || ledger.pre_location_kind != Some(expected_location)
                {
                    return Err(corrupt());
                }
            }
            DurableDestructionEntryV1::RunMaterial {
                material_id,
                pre_material_version,
                post_material_version,
                ..
            } => {
                let Some(material) = signature
                    .materials
                    .iter()
                    .find(|material| material.material_id == *material_id)
                else {
                    return Err(corrupt());
                };
                if material.material_version != *post_material_version
                    || pre_material_version.saturating_add(1) != *post_material_version
                {
                    return Err(corrupt());
                }
            }
        }
    }

    if terminal_destruction_count(signature)
        != usize::from(signature.graph_root.destruction_entry_count)
    {
        return Err(corrupt());
    }
    Ok(())
}

fn validate_cleanup(
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> Result<(), PersistenceError> {
    let cleanup = &signature.bargain_cleanup;
    let versions = &signature.terminal.result.versions.oath_bargain;
    if cleanup.event_id != signature.graph_root.bargain_cleanup_event_id
        || cleanup.event_type != "bargains_cleared_death"
        || cleanup.aggregate_version != versions.post
        || cleanup.event.schema_version != BARGAIN_LIFE_CLEANUP_EVENT_SCHEMA_VERSION
        || cleanup.event.reason != BargainLifeEndReason::Death
        || u64::try_from(cleanup.event.pre_oath_bargain_version).ok() != Some(versions.pre)
        || u64::try_from(cleanup.event.post_oath_bargain_version).ok() != Some(versions.post)
        || !strictly_sorted_by(&cleanup.event.active_bargains, |bargain| {
            bargain.acquisition_ordinal
        })
        || cleanup.event.active_bargains.iter().any(|bargain| {
            bargain.bargain_id.is_empty()
                || bargain.acquisition_ordinal <= 0
                || bargain.acquired_by_offer_id == [0; 16]
        })
        || cleanup.event.active_bargains.len() != signature.plan.summary.bargains.len()
        || cleanup
            .event
            .active_bargains
            .iter()
            .zip(&signature.plan.summary.bargains)
            .enumerate()
            .any(|(index, (bargain, summary))| {
                usize::try_from(bargain.acquisition_ordinal).ok() != Some(index + 1)
                    || usize::from(summary.ordinal) != index
                    || bargain.bargain_id != summary.content_id
            })
    {
        return Err(corrupt());
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "Echo queue and transition-tail validation form one projector invariant"
)]
fn validate_echo_graph(
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> Result<(), PersistenceError> {
    if !strictly_sorted_by(&signature.echoes, |echo| {
        (echo.created_at_unix_micros, echo.echo_id)
    }) || !strictly_sorted_by(&signature.echo_transitions, |transition| {
        (transition.echo_id, transition.transition_ordinal)
    }) || signature
        .echoes
        .iter()
        .any(|echo| !valid_echo(echo, signature))
        || signature
            .echo_transitions
            .iter()
            .any(|transition| !valid_echo_transition(transition, signature))
        || signature
            .echoes
            .iter()
            .filter(|echo| echo.state == ECHO_STATE_AVAILABLE)
            .count()
            > 1
    {
        return Err(corrupt());
    }

    for echo in &signature.echoes {
        let transitions: Vec<_> = signature
            .echo_transitions
            .iter()
            .filter(|transition| transition.echo_id == echo.echo_id)
            .collect();
        if transitions.is_empty()
            || transitions.iter().enumerate().any(|(index, transition)| {
                usize::from(transition.transition_ordinal) != index
                    || (index == 0
                        && (transition.previous_state.is_some()
                            || transition.next_state != ECHO_STATE_DORMANT
                            || transition.reason != 0
                            || transition.source_death_id != Some(echo.death_id)
                            || transition.trigger_death_id != echo.death_id))
                    || (index > 0
                        && (transition.previous_state != Some(transitions[index - 1].next_state)
                            || transition.previous_state != Some(ECHO_STATE_DORMANT)
                            || transition.next_state != ECHO_STATE_AVAILABLE
                            || transition.reason != 1
                            || transition.source_death_id.is_some()))
            })
            || transitions.last().map(|transition| transition.next_state) != Some(echo.state)
        {
            return Err(corrupt());
        }
    }

    let result = &signature.terminal.result;
    let Some(envelope) = &signature.plan.echo else {
        return if result.echo_outcome == DurableEchoOutcomeV1::NotEligible
            && result.created_echo_id.is_none()
            && result.promoted_echo_id.is_none()
            && !signature.graph_root.echo_expected
            && !signature
                .echoes
                .iter()
                .any(|echo| echo.death_id == signature.death_id)
        {
            Ok(())
        } else {
            Err(corrupt())
        };
    };

    let Some(created) = signature
        .echoes
        .iter()
        .find(|echo| echo.echo_id == envelope.created.echo_id)
    else {
        return Err(corrupt());
    };
    let creation = &envelope.creation_transition;
    if result.created_echo_id != Some(created.echo_id)
        || !stored_echo_matches_plan(created, &envelope.created)
        || !signature
            .echo_transitions
            .iter()
            .any(|transition| stored_transition_matches_plan(transition, creation))
    {
        return Err(corrupt());
    }

    match (
        envelope.preexisting_available_echo_id,
        envelope.promotion.as_ref(),
    ) {
        (Some(available_id), None) => {
            if result.echo_outcome != DurableEchoOutcomeV1::Dormant
                || result.promoted_echo_id.is_some()
                || created.state != ECHO_STATE_DORMANT
                || !signature
                    .echoes
                    .iter()
                    .any(|echo| echo.echo_id == available_id && echo.state == ECHO_STATE_AVAILABLE)
            {
                return Err(corrupt());
            }
        }
        (None, Some(promotion)) => {
            let Some(promoted) = signature
                .echoes
                .iter()
                .find(|echo| echo.echo_id == promotion.echo_id)
            else {
                return Err(corrupt());
            };
            let expected_outcome = if promotion.echo_id == created.echo_id {
                DurableEchoOutcomeV1::Available
            } else {
                DurableEchoOutcomeV1::Dormant
            };
            if result.echo_outcome != expected_outcome
                || result.promoted_echo_id != Some(promoted.echo_id)
                || signature.graph_root.promoted_echo_id != Some(promoted.echo_id)
                || promoted.state != ECHO_STATE_AVAILABLE
                || created.state
                    != if promotion.echo_id == created.echo_id {
                        ECHO_STATE_AVAILABLE
                    } else {
                        ECHO_STATE_DORMANT
                    }
                || !signature
                    .echo_transitions
                    .iter()
                    .any(|transition| stored_transition_matches_plan(transition, promotion))
                || oldest_dormant_before_trigger(signature, created) != Some(promoted.echo_id)
            {
                return Err(corrupt());
            }
        }
        (Some(_), Some(_)) | (None, None) => return Err(corrupt()),
    }
    Ok(())
}

fn validate_operational_graph(
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> Result<(), PersistenceError> {
    let counts = &signature.counts;
    if counts.trace_entries != u32::from(signature.graph_root.trace_entry_count)
        || usize::try_from(counts.trace_statuses).ok()
            != Some(
                signature
                    .plan
                    .trace
                    .iter()
                    .map(|entry| entry.statuses.len())
                    .sum(),
            )
        || usize::try_from(counts.summary_bargains).ok()
            != Some(signature.plan.summary.bargains.len())
        || usize::try_from(counts.summary_damage_entries).ok()
            != Some(signature.plan.summary.last_five_damage.len())
        || usize::try_from(counts.summary_projection_entries).ok()
            != Some(
                signature.plan.summary.projections.lost.len()
                    + signature.plan.summary.projections.preserved.len()
                    + signature.plan.summary.projections.created.len(),
            )
        || counts.destruction_entries != u32::from(signature.graph_root.destruction_entry_count)
        || counts.memorial_records != 1
        || counts.mutation_results != 1
        || counts.retained_trace_sets != 1
        || usize::try_from(counts.retained_trace_receipt_links).ok()
            != Some(signature.trace_promotion.receipts.len())
        || usize::try_from(counts.retained_trace_provenance_entries).ok()
            != Some(signature.trace_promotion.provenance.len())
        || usize::try_from(counts.item_records).ok() != Some(signature.items.len())
        || usize::try_from(counts.item_ledger_entries).ok() != Some(signature.item_ledger.len())
        || usize::try_from(counts.material_records).ok() != Some(signature.materials.len())
        || usize::try_from(counts.echo_records).ok() != Some(signature.echoes.len())
        || usize::try_from(counts.echo_transitions).ok() != Some(signature.echo_transitions.len())
        || usize::try_from(counts.audit_events).ok() != Some(signature.audits.len())
        || usize::try_from(counts.outbox_events).ok() != Some(signature.outbox.len())
        || counts.active_bargains != 0
        || counts.danger_checkpoints != 0
        || counts.live_trace_ticks != 0
        || counts.live_trace_entries != 0
        || counts.live_trace_statuses != 0
        || !strictly_sorted_by(&signature.audits, |audit| audit.audit_event_id)
        || !strictly_sorted_by(&signature.outbox, |event| {
            (
                event.death_id,
                event.event_type.clone(),
                event.outbox_event_id,
            )
        })
        || signature.audits.iter().any(|audit| {
            audit.audit_event_id == [0; 16]
                || audit.audit_digest == [0; 32]
                || audit.death_id != Some(signature.death_id)
                || match audit.audit_kind {
                    0 => {
                        audit.mutation_id != signature.graph_root.mutation_id
                            || audit.audit_digest != signature.terminal.result_hash
                    }
                    1 => audit.mutation_id == [0; 16],
                    _ => true,
                }
        })
        || signature
            .audits
            .iter()
            .filter(|audit| audit.audit_kind == 0)
            .count()
            != 1
        || signature
            .outbox
            .iter()
            .any(|event| !valid_outbox_event(event, signature))
        || signature.outbox.len()
            != 1 + usize::from(signature.plan.echo.is_some())
                + usize::from(
                    signature
                        .plan
                        .echo
                        .as_ref()
                        .and_then(|echo| echo.promotion.as_ref())
                        .is_some(),
                )
    {
        return Err(corrupt());
    }
    Ok(())
}

fn valid_outbox_event(
    event: &StoredDeathTerminalOutboxV1,
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> bool {
    if event.outbox_event_id == [0; 16]
        || event.event_payload.is_empty()
        || event.event_payload.len() > 65_536
    {
        return false;
    }
    match event.event_type.as_str() {
        "death_committed" => {
            event.death_id == signature.death_id
                && event.echo_id.is_none()
                && event.echo_transition_ordinal.is_none()
                && event.trigger_death_id.is_none()
                && signature
                    .terminal
                    .result
                    .payload()
                    .is_ok_and(|payload| payload == event.event_payload)
        }
        "echo_created" => signature.plan.echo.as_ref().is_some_and(|echo| {
            event.death_id == signature.death_id
                && event.echo_id == Some(echo.created.echo_id)
                && event.echo_transition_ordinal == Some(0)
                && event.trigger_death_id == Some(signature.death_id)
                && postcard::to_stdvec(&echo.created)
                    .is_ok_and(|payload| payload == event.event_payload)
        }),
        "echo_promoted" => signature.plan.echo.as_ref().is_some_and(|echo| {
            echo.promotion.as_ref().is_some_and(|promotion| {
                event.death_id == promotion.echo_death_id
                    && event.echo_id == Some(promotion.echo_id)
                    && event.echo_transition_ordinal == Some(promotion.ordinal)
                    && event.trigger_death_id == Some(signature.death_id)
                    && postcard::to_stdvec(promotion)
                        .is_ok_and(|payload| payload == event.event_payload)
            })
        }),
        _ => false,
    }
}

fn valid_terminal_item(
    item: &StoredDeathTerminalItemV1,
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> bool {
    let kind_valid = matches!(
        (item.item_kind, item.item_level, item.rarity),
        (0, Some(1..=10), Some(0..=4)) | (1, None, None)
    );
    item.item_uid != [0; 16]
        && item.creation_request_id != [0; 16]
        && matches!(item.creation_kind, 0 | 1)
        && (3..=96).contains(&item.template_id.len())
        && item.item_version > 0
        && kind_valid
        && match item.location_kind {
            LOCATION_DESTROYED => {
                item.character_id == Some(signature.character_id)
                    && item.slot_index.is_none()
                    && item.security_state == SECURITY_DESTROYED
                    && item.destruction_reason.as_deref() == Some("permadeath")
                    && item.terminal_death_id == Some(signature.death_id)
            }
            LOCATION_CHARACTER_SAFE => {
                item.character_id == Some(signature.character_id)
                    && matches!(item.slot_index, Some(0..=7))
                    && item.security_state == SECURITY_SAFE
                    && item.destruction_reason.is_none()
                    && item.terminal_death_id.is_none()
            }
            LOCATION_VAULT => {
                item.character_id.is_none()
                    && matches!(item.slot_index, Some(0..=159))
                    && item.security_state == SECURITY_SAFE
                    && item.destruction_reason.is_none()
                    && item.terminal_death_id.is_none()
            }
            _ => false,
        }
}

fn valid_terminal_material(material: &StoredDeathTerminalMaterialV1, death_id: [u8; 16]) -> bool {
    !material.material_id.is_empty()
        && material.quantity == 0
        && material.material_version > 0
        && material.security_state == SECURITY_DESTROYED
        && material.terminal_reason.as_deref() == Some("permadeath")
        && material.terminal_restore_point_id.is_none()
        && material.terminal_death_id == Some(death_id)
}

fn valid_echo(
    echo: &StoredDeathTerminalEchoV1,
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> bool {
    is_uuid_v7(echo.echo_id)
        && is_uuid_v7(echo.death_id)
        && echo.account_id == signature.account_id
        && !echo.character_name_snapshot.is_empty()
        && !echo.class_id.is_empty()
        && echo.level == 10
        && !echo.appearance_snapshot_id.is_empty()
        && !echo.appearance_theme_id.is_empty()
        && !echo.killer_content_id.is_empty()
        && !echo.death_region_id.is_empty()
        && matches!(echo.power_band, 1..=5)
        && matches!(echo.state, ECHO_STATE_DORMANT | ECHO_STATE_AVAILABLE)
        && valid_core_revision(&echo.content_revision)
        && echo.snapshot_digest != [0; 32]
        && echo.created_at_unix_micros > 0
        && contiguous_unique_content(&echo.bargains, 3)
        && contiguous_unique_content(&echo.deed_tags, 32)
}

fn valid_echo_transition(
    transition: &StoredDeathTerminalEchoTransitionV1,
    signature: &StoredCoreDeathTerminalSignatureV1,
) -> bool {
    is_uuid_v7(transition.echo_id)
        && is_uuid_v7(transition.trigger_death_id)
        && matches!(transition.previous_state, None | Some(0..=4))
        && transition.next_state <= ECHO_STATE_AVAILABLE
        && transition.reason <= 1
        && signature
            .echoes
            .iter()
            .any(|echo| echo.echo_id == transition.echo_id)
}

fn stored_echo_matches_plan(
    stored: &StoredDeathTerminalEchoV1,
    plan: &DurableEchoRecordV1,
) -> bool {
    stored.echo_id == plan.echo_id
        && stored.death_id == plan.death_id
        && stored.account_id == plan.account_id
        && stored.character_name_snapshot == plan.character_name_snapshot
        && stored.class_id == plan.class_id
        && stored.oath_id == plan.oath_id
        && stored.level == u16::from(plan.level)
        && stored.appearance_snapshot_id == plan.appearance_snapshot_id
        && stored.appearance_theme_id == plan.appearance_theme_id
        && stored.weapon_signature_tag == plan.weapon_signature_tag
        && stored.relic_signature_tag == plan.relic_signature_tag
        && stored.bargains == plan.bargains
        && stored.deed_tags == plan.deed_tags
        && stored.killer_content_id == plan.killer_content_id
        && stored.killer_pattern_id == plan.killer_pattern_id
        && stored.death_region_id == plan.death_region_id
        && stored.power_band == u16::from(plan.power_band)
        && stored.content_revision == plan.content_revision
        && stored.snapshot_digest == plan.snapshot_digest
        && stored.created_at_unix_micros / 1_000 == plan.created_at_unix_ms
}

fn stored_transition_matches_plan(
    stored: &StoredDeathTerminalEchoTransitionV1,
    plan: &DurableEchoTransitionV1,
) -> bool {
    stored.echo_id == plan.echo_id
        && stored.transition_ordinal == plan.ordinal
        && stored.previous_state == plan.previous_state.map(echo_state)
        && stored.next_state == echo_state(plan.next_state)
        && stored.reason
            == match plan.reason {
                crate::DurableEchoTransitionReasonV1::EligibleDeath => 0,
                crate::DurableEchoTransitionReasonV1::OldestDormantPromotion => 1,
            }
        && stored.source_death_id == plan.source_death_id
        && stored.trigger_death_id == plan.trigger_death_id
}

fn oldest_dormant_before_trigger(
    signature: &StoredCoreDeathTerminalSignatureV1,
    created: &StoredDeathTerminalEchoV1,
) -> Option<[u8; 16]> {
    signature
        .echoes
        .iter()
        .filter(|echo| {
            (echo.created_at_unix_micros, echo.echo_id)
                <= (created.created_at_unix_micros, created.echo_id)
                && state_before_trigger(signature, echo.echo_id, signature.death_id)
                    == Some(ECHO_STATE_DORMANT)
        })
        .min_by_key(|echo| (echo.created_at_unix_micros, echo.echo_id))
        .map(|echo| echo.echo_id)
}

fn state_before_trigger(
    signature: &StoredCoreDeathTerminalSignatureV1,
    echo_id: [u8; 16],
    trigger_death_id: [u8; 16],
) -> Option<u16> {
    let mut state = None;
    for transition in signature
        .echo_transitions
        .iter()
        .filter(|transition| transition.echo_id == echo_id)
    {
        if transition.trigger_death_id == trigger_death_id
            && transition.next_state == ECHO_STATE_AVAILABLE
        {
            return transition.previous_state.or(state);
        }
        state = Some(transition.next_state);
    }
    state
}

fn contiguous_unique_content(values: &[DurableOrderedContentIdV1], max: usize) -> bool {
    values.len() <= max
        && values.iter().enumerate().all(|(index, value)| {
            usize::from(value.ordinal) == index && !value.content_id.is_empty()
        })
        && values
            .iter()
            .map(|value| value.content_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == values.len()
}

const fn echo_state(value: DurableEchoStateV1) -> u16 {
    match value {
        DurableEchoStateV1::Dormant => ECHO_STATE_DORMANT,
        DurableEchoStateV1::Available => ECHO_STATE_AVAILABLE,
    }
}

fn terminal_destruction_count(signature: &StoredCoreDeathTerminalSignatureV1) -> usize {
    signature
        .items
        .iter()
        .filter(|item| item.terminal_death_id == Some(signature.death_id))
        .count()
        + signature
            .materials
            .iter()
            .filter(|material| material.terminal_death_id == Some(signature.death_id))
            .count()
}

fn strictly_sorted_by<T, K: Ord>(values: &[T], key: impl Fn(&T) -> K) -> bool {
    values.windows(2).all(|pair| key(&pair[0]) < key(&pair[1]))
}

fn valid_core_revision(value: &str) -> bool {
    value
        .strip_prefix("core-dev.blake3.")
        .is_some_and(valid_hex_hash)
}

fn valid_hex_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_uuid_v7(value: [u8; 16]) -> bool {
    value != [0; 16] && value[6] >> 4 == 7 && value[8] & 0xc0 == 0x80
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredDeathTerminalSignature
}

#[cfg(test)]
mod tests {
    use crate::{
        BargainLifeCleanupEventBargainV1, DURABLE_TERMINAL_RECOVERY_SCHEMA_VERSION,
        DurableDeathCommitRequestV1, StoredCommittedDeathResultV1,
        durable_death::tests::valid_request,
    };

    use super::*;

    fn live_receipt(
        signature: &StoredDeathTerminalTraceReceiptV1,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> LiveDamageTracePromotionReceiptV1 {
        LiveDamageTracePromotionReceiptV1 {
            account_id,
            character_id,
            trace_tick_id: signature.trace_tick_id,
            expected_character_version: signature.expected_character_version,
            danger: LiveDamageTraceDangerAuthorityV1 {
                lineage_id: signature.lineage_id,
                restore_point_id: signature.restore_point_id,
                checkpoint_tick: signature.checkpoint_tick,
            },
            event_tick: signature.event_tick,
            entry_count: usize::from(signature.entry_count),
            status_count: usize::from(signature.status_count),
            lethal_count: usize::from(signature.lethal_count),
            content: LiveDamageTraceContentAuthorityV1 {
                records_blake3: signature.records_blake3.clone(),
                assets_blake3: signature.assets_blake3.clone(),
                localization_blake3: signature.localization_blake3.clone(),
            },
            request_hash: signature.request_hash,
            tick_digest: signature.tick_digest,
            result_digest: signature.result_digest,
            issued_at_unix_ms: signature.issued_at_unix_ms,
            committed_at_unix_ms: signature.committed_at_unix_ms,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn trace_promotion(
        request: &DurableDeathCommitRequestV1,
    ) -> StoredDeathTerminalTracePromotionV1 {
        let event = &request.plan.event;
        let records = event.records_blake3.clone();
        let assets = event.assets_blake3.clone();
        let localization = event.localization_blake3.clone();
        let receipts = vec![
            StoredDeathTerminalTraceReceiptV1 {
                receipt_ordinal: 0,
                trace_tick_id: [30; 16],
                expected_character_version: event.versions.character.pre,
                lineage_id: event.lineage_id,
                restore_point_id: event.restore_point_id,
                checkpoint_tick: 0,
                event_tick: 999,
                entry_count: 1,
                status_count: 1,
                lethal_count: 0,
                records_blake3: records.clone(),
                assets_blake3: assets.clone(),
                localization_blake3: localization.clone(),
                request_hash: [31; 32],
                tick_digest: [32; 32],
                result_digest: [33; 32],
                issued_at_unix_ms: 1_700,
                committed_at_unix_ms: 1_750,
            },
            StoredDeathTerminalTraceReceiptV1 {
                receipt_ordinal: 1,
                trace_tick_id: [34; 16],
                expected_character_version: event.versions.character.pre,
                lineage_id: event.lineage_id,
                restore_point_id: event.restore_point_id,
                checkpoint_tick: 0,
                event_tick: 1_000,
                entry_count: 1,
                status_count: 0,
                lethal_count: 1,
                records_blake3: records.clone(),
                assets_blake3: assets.clone(),
                localization_blake3: localization.clone(),
                request_hash: [35; 32],
                tick_digest: [36; 32],
                result_digest: [37; 32],
                issued_at_unix_ms: 1_800,
                committed_at_unix_ms: 1_850,
            },
        ];
        let provenance = vec![
            StoredDeathTerminalTraceProvenanceV1 {
                trace_ordinal: 0,
                receipt_ordinal: 0,
                trace_tick_id: receipts[0].trace_tick_id,
                event_tick: 999,
                event_ordinal: 0,
                cause: DurableDeathCauseV1::DirectHit,
                source_entity_id: Some([8; 16]),
                source_sim_entity_id: Some(800),
                status_count: 1,
                live_entry_digest: [38; 32],
            },
            StoredDeathTerminalTraceProvenanceV1 {
                trace_ordinal: 1,
                receipt_ordinal: 1,
                trace_tick_id: receipts[1].trace_tick_id,
                event_tick: 1_000,
                event_ordinal: 0,
                cause: DurableDeathCauseV1::DirectHit,
                source_entity_id: Some([8; 16]),
                source_sim_entity_id: Some(800),
                status_count: 0,
                live_entry_digest: [39; 32],
            },
        ];
        let live_receipts: Vec<_> = receipts
            .iter()
            .map(|receipt| live_receipt(receipt, event.account_id, event.character_id))
            .collect();
        let receipt_window_digest =
            canonical_live_damage_trace_receipt_window_digest_v1(&live_receipts).unwrap();
        let durable_provenance: Vec<_> = provenance
            .iter()
            .map(|entry| DurableDeathTraceEntryProvenanceV1 {
                trace_ordinal: entry.trace_ordinal,
                trace_tick_id: entry.trace_tick_id,
                event_tick: entry.event_tick,
                event_ordinal: entry.event_ordinal,
                cause: entry.cause,
                source_entity_id: entry.source_entity_id,
                source_sim_entity_id: entry.source_sim_entity_id,
                status_count: entry.status_count,
                live_entry_digest: entry.live_entry_digest,
            })
            .collect();
        let promotion_digest = canonical_stored_death_trace_promotion_digest_v1(
            &DurableDeathTracePromotionDigestMaterialV1 {
                contract_version: 1,
                death_id: event.death_id,
                account_id: event.account_id,
                character_id: event.character_id,
                lineage_id: event.lineage_id,
                restore_point_id: event.restore_point_id,
                checkpoint_tick: 0,
                terminal_character_version: event.versions.character.pre,
                records_blake3: &records,
                assets_blake3: &assets,
                localization_blake3: &localization,
                first_event_tick: 999,
                death_tick: event.death_tick,
                receipt_count: 2,
                entry_count: 2,
                status_count: 1,
                lethal_trace_tick_id: receipts[1].trace_tick_id,
            },
            receipts[1].request_hash,
            &durable_provenance,
        )
        .unwrap();
        let terminal_payload_hash = canonical_death_terminal_payload_hash_v1(
            request.canonical_request_hash,
            promotion_digest,
        )
        .unwrap();
        StoredDeathTerminalTracePromotionV1 {
            contract_version: 1,
            death_id: event.death_id,
            account_id: event.account_id,
            character_id: event.character_id,
            lineage_id: event.lineage_id,
            restore_point_id: event.restore_point_id,
            checkpoint_tick: 0,
            terminal_character_version: event.versions.character.pre,
            first_event_tick: 999,
            death_tick: event.death_tick,
            receipt_count: 2,
            entry_count: 2,
            status_count: 1,
            lethal_trace_tick_id: receipts[1].trace_tick_id,
            records_blake3: records,
            assets_blake3: assets,
            localization_blake3: localization,
            receipt_window_digest,
            promotion_digest,
            terminal_payload_hash,
            receipts,
            provenance,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn signature_from_request(
        request: &DurableDeathCommitRequestV1,
    ) -> StoredCoreDeathTerminalSignatureV1 {
        let plan = request.plan.clone();
        let event = &plan.event;
        let versions = event.versions.clone();
        let result = StoredCommittedDeathResultV1::from_request(request).unwrap();
        let result_hash = result.digest().unwrap();
        let promotion = trace_promotion(request);
        let summary_projection_count = plan.summary.projections.lost.len()
            + plan.summary.projections.preserved.len()
            + plan.summary.projections.created.len();
        let bargain_cleanup_event_id = event.bargain_cleanup_event_id;
        StoredCoreDeathTerminalSignatureV1 {
            contract_version: 1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: event.account_id,
            character_id: event.character_id,
            death_id: event.death_id,
            terminal: StoredCommittedDeathTerminalV1 {
                schema_version: DURABLE_TERMINAL_RECOVERY_SCHEMA_VERSION,
                result: result.clone(),
                result_hash,
                lineage_id: event.lineage_id,
                restore_point_id: event.restore_point_id,
                death_tick: event.death_tick,
                promotion_digest: promotion.promotion_digest,
                terminal_payload_hash: promotion.terminal_payload_hash,
            },
            plan: plan.clone(),
            aggregate: StoredDeathTerminalAggregateV1 {
                account_version: versions.account.post,
                selected_character_id: None,
                character_life_state: LIFE_STATE_DEAD,
                character_roster_ordinal: None,
                character_version: versions.character.post,
                world_character_version: versions.character.post,
                world_location_kind: WORLD_LOCATION_DANGER,
                world_location_content_id: Some("dungeon.core.private_01".into()),
                world_lineage_id: Some(event.lineage_id),
                world_restore_point_id: Some(event.restore_point_id),
                current_health: 0,
                progression_version: versions.progression.post,
                inventory_version: versions.inventory.post,
                oath_bargain_version: versions.oath_bargain.post,
                lifetime_ticks: event.lifetime_ticks,
                permadeath_combat_ticks: event.permadeath_combat_ticks,
                life_metrics_version: versions.life_metrics.post,
                ash_balance: 10,
                ash_wallet_version: 2,
                lineage_state: LINEAGE_STATE_TERMINATED,
                restore_state: RESTORE_STATE_DEATH_COMMITTED,
                restore_death_mutation_id: Some(event.mutation_id),
            },
            graph_root: StoredDeathTerminalGraphRootV1 {
                mutation_id: event.mutation_id,
                lineage_id: event.lineage_id,
                restore_point_id: event.restore_point_id,
                death_tick: event.death_tick,
                lifetime_ticks: event.lifetime_ticks,
                permadeath_combat_ticks: event.permadeath_combat_ticks,
                provenance: event.provenance,
                trace_entry_count: event.trace_entry_count,
                destruction_entry_count: event.destruction_entry_count,
                former_roster_ordinal: u16::from(event.former_roster_ordinal),
                echo_expected: plan.echo.is_some(),
                preexisting_available_echo_id: plan
                    .echo
                    .as_ref()
                    .and_then(|echo| echo.preexisting_available_echo_id),
                promoted_echo_id: plan
                    .echo
                    .as_ref()
                    .and_then(|echo| echo.promotion.as_ref().map(|value| value.echo_id)),
                content_revision: event.content_revision.clone(),
                world_records_blake3: event.records_blake3.clone(),
                world_assets_blake3: event.assets_blake3.clone(),
                world_localization_blake3: event.localization_blake3.clone(),
                presentation_records_blake3: event.presentation.records_blake3.clone(),
                presentation_assets_blake3: event.presentation.assets_blake3.clone(),
                presentation_localization_blake3: event.presentation.localization_blake3.clone(),
                bargain_cleanup_event_id,
                versions: versions.clone(),
                trace_digest: event.trace_digest,
                destruction_digest: event.destruction_digest,
                summary_digest: plan.summary.snapshot_digest,
                memorial_digest: plan.memorial.presentation_digest,
            },
            trace_promotion: promotion,
            trace_conflicts: vec![],
            items: vec![StoredDeathTerminalItemV1 {
                item_uid: [9; 16],
                character_id: Some(event.character_id),
                template_id: "item.warden_blade".into(),
                content_revision: event.content_revision.clone(),
                item_kind: 0,
                item_level: Some(10),
                rarity: Some(0),
                creation_kind: 0,
                creation_request_id: [40; 16],
                roll_index: 0,
                unit_ordinal: 0,
                item_version: 3,
                security_state: SECURITY_DESTROYED,
                location_kind: LOCATION_DESTROYED,
                slot_index: None,
                destruction_reason: Some("permadeath".into()),
                terminal_death_id: Some(event.death_id),
            }],
            item_ledger: vec![StoredDeathTerminalItemLedgerV1 {
                ledger_event_id: [10; 16],
                item_uid: [9; 16],
                mutation_id: event.mutation_id,
                event_kind: LEDGER_EVENT_DESTROYED,
                source_kind: LEDGER_SOURCE_DEATH,
                pre_item_version: 2,
                post_item_version: 3,
                pre_security_state: Some(SECURITY_AT_RISK_EQUIPPED),
                post_security_state: SECURITY_DESTROYED,
                pre_location_kind: Some(0),
                post_location_kind: LOCATION_DESTROYED,
                reason: Some("permadeath".into()),
                terminal_death_id: Some(event.death_id),
            }],
            materials: vec![],
            bargain_cleanup: StoredDeathTerminalBargainCleanupV1 {
                event_id: bargain_cleanup_event_id,
                event_type: "bargains_cleared_death".into(),
                aggregate_version: versions.oath_bargain.post,
                event: BargainLifeCleanupEventV1 {
                    schema_version: BARGAIN_LIFE_CLEANUP_EVENT_SCHEMA_VERSION,
                    reason: BargainLifeEndReason::Death,
                    pre_oath_bargain_version: i64::try_from(versions.oath_bargain.pre).unwrap(),
                    post_oath_bargain_version: i64::try_from(versions.oath_bargain.post).unwrap(),
                    active_bargains: vec![BargainLifeCleanupEventBargainV1 {
                        bargain_id: plan.summary.bargains[0].content_id.clone(),
                        acquisition_ordinal: 1,
                        acquired_by_offer_id: [41; 16],
                    }],
                },
            },
            echoes: vec![],
            echo_transitions: vec![],
            audits: vec![StoredDeathTerminalAuditV1 {
                audit_event_id: [42; 16],
                death_id: Some(event.death_id),
                mutation_id: event.mutation_id,
                audit_kind: 0,
                audit_digest: result_hash,
            }],
            outbox: vec![StoredDeathTerminalOutboxV1 {
                outbox_event_id: [43; 16],
                death_id: event.death_id,
                event_type: "death_committed".into(),
                echo_id: None,
                echo_transition_ordinal: None,
                trigger_death_id: None,
                event_payload: result.payload().unwrap(),
            }],
            counts: StoredDeathTerminalGraphCountsV1 {
                trace_entries: u32::from(event.trace_entry_count),
                trace_statuses: 1,
                summary_bargains: u32::try_from(plan.summary.bargains.len()).unwrap(),
                summary_damage_entries: u32::try_from(plan.summary.last_five_damage.len()).unwrap(),
                summary_projection_entries: u32::try_from(summary_projection_count).unwrap(),
                memorial_records: 1,
                destruction_entries: u32::from(event.destruction_entry_count),
                mutation_results: 1,
                retained_trace_sets: 1,
                retained_trace_receipt_links: 2,
                retained_trace_provenance_entries: 2,
                retained_trace_conflicts: 0,
                item_records: 1,
                item_ledger_entries: 1,
                material_records: 0,
                echo_records: 0,
                echo_transitions: 0,
                audit_events: 1,
                outbox_events: 1,
                active_bargains: 0,
                danger_checkpoints: 0,
                live_trace_ticks: 0,
                live_trace_entries: 0,
                live_trace_statuses: 0,
            },
        }
    }

    fn signature() -> StoredCoreDeathTerminalSignatureV1 {
        signature_from_request(&valid_request())
    }

    #[test]
    fn digest_is_stable_and_typed_plan_children_are_bound() {
        let original = signature();
        let mut changed = original.clone();
        changed.aggregate.ash_balance += 1;
        assert_eq!(original.digest().unwrap(), original.digest().unwrap());
        assert_ne!(original.digest().unwrap(), changed.digest().unwrap());

        let mut corrupt_plan = original;
        corrupt_plan.plan.summary.hero_label_key = "hero.changed".into();
        assert!(matches!(
            corrupt_plan.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));
    }

    #[test]
    fn live_or_selected_character_fails_closed() {
        let mut value = signature();
        value.aggregate.selected_character_id = Some(value.character_id);
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));
        value.aggregate.selected_character_id = None;
        value.aggregate.character_life_state = 0;
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));
    }

    #[test]
    fn missing_terminal_ledger_or_surviving_risk_fails_closed() {
        let mut value = signature();
        value.item_ledger.clear();
        value.counts.item_ledger_entries = 0;
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));

        let mut value = signature();
        value.items[0].security_state = SECURITY_AT_RISK_PENDING;
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));
    }

    #[test]
    fn cleanup_and_zero_row_assertions_fail_closed() {
        let mut value = signature();
        value.bargain_cleanup.event.reason = BargainLifeEndReason::Retirement;
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));

        let mut value = signature();
        value.counts.danger_checkpoints = 1;
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));
    }

    #[test]
    fn attempted_identity_audit_is_bound_without_replacing_accepted_authority() {
        let mut value = signature();
        value.audits.push(StoredDeathTerminalAuditV1 {
            audit_event_id: [44; 16],
            death_id: Some(value.death_id),
            mutation_id: [45; 16],
            audit_kind: 1,
            audit_digest: [46; 32],
        });
        value.counts.audit_events += 1;
        assert!(value.canonical_bytes().is_ok());

        value.outbox[0].event_payload[0] ^= 1;
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));
    }

    #[test]
    fn trace_conflict_binds_the_attempted_promotion_to_the_original_request() {
        let mut value = signature();
        let attempted_promotion_digest = [48; 32];
        let attempted_terminal_payload_hash = canonical_death_terminal_payload_hash_v1(
            value.terminal.result.canonical_request_hash,
            attempted_promotion_digest,
        )
        .unwrap();
        value
            .trace_conflicts
            .push(StoredDeathTerminalTraceConflictV1 {
                audit_id: [47; 16],
                conflict_code: 0,
                stored_promotion_digest: value.trace_promotion.promotion_digest,
                attempted_promotion_digest,
                stored_terminal_payload_hash: value.trace_promotion.terminal_payload_hash,
                attempted_terminal_payload_hash,
                attempted_issued_at_unix_ms: 2_001,
            });
        value.counts.retained_trace_conflicts = 1;
        assert!(value.canonical_bytes().is_ok());

        value.trace_conflicts[0].attempted_terminal_payload_hash[0] ^= 1;
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));
    }

    #[test]
    fn promotion_receipts_and_provenance_fail_closed_on_corruption() {
        let mut value = signature();
        value.trace_promotion.provenance[0].live_entry_digest[0] ^= 1;
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));

        let mut value = signature();
        value.trace_promotion.receipts.swap(0, 1);
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredDeathTerminalSignature)
        ));
    }

    #[test]
    fn zero_lifetime_death_remains_valid() {
        let mut plan = valid_request().plan;
        plan.event.lifetime_ticks = 0;
        plan.event.permadeath_combat_ticks = 0;
        plan.summary.lifetime_ms = 0;
        plan.summary.snapshot_digest = plan.summary.expected_snapshot_digest().unwrap();
        plan.memorial.summary_snapshot_digest = plan.summary.snapshot_digest;
        plan.memorial.presentation_digest = plan.memorial.expected_presentation_digest().unwrap();
        let request = DurableDeathCommitRequestV1::seal(plan, 1_900).unwrap();
        assert!(signature_from_request(&request).canonical_bytes().is_ok());
    }

    #[test]
    fn combat_clock_may_exceed_the_independent_lifetime_clock() {
        let mut plan = valid_request().plan;
        plan.event.lifetime_ticks = 1;
        plan.event.permadeath_combat_ticks = 2;
        plan.summary.lifetime_ms = 33;
        plan.summary.snapshot_digest = plan.summary.expected_snapshot_digest().unwrap();
        plan.memorial.summary_snapshot_digest = plan.summary.snapshot_digest;
        plan.memorial.presentation_digest = plan.memorial.expected_presentation_digest().unwrap();
        let request = DurableDeathCommitRequestV1::seal(plan, 1_900).unwrap();
        assert!(signature_from_request(&request).canonical_bytes().is_ok());
    }
}
