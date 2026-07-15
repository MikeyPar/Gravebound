//! Mandatory retained live-trace promotion for one durable Core permadeath.
//!
//! This contract follows the canonical GDD `DTH-001`, `DTH-020`, and `TECH-020..023`; the
//! Content Production Spec `CONT-ECHO-001`/`CONT-ECHO-009`; the Development Roadmap
//! `GB-M03-02D`, `GB-M03-06`, and `GB-M03-13`; and owner-approved `SPEC-CONFLICT-009`. A lethal
//! live tick is evidence for the existing atomic death/destruction/memorial/Echo transaction,
//! never an independent terminal writer.
//!
//! The sealed DTO proves that the complete bounded live window is exactly the durable combat
//! trace, preserves each retained tick identity, and binds simulation identities one-to-one with
//! durable journal identities. It intentionally reuses the live repository's canonical entry
//! digest seam instead of defining a second hashing rule.

use std::collections::{BTreeMap, BTreeSet};

use crate::live_damage_trace_repository::canonical_live_damage_trace_entry_digest_v1;
use crate::{
    DURABLE_DEATH_TRACE_WINDOW_TICKS, DurableCombatTraceEntryV1, DurableDamageTypeV1,
    DurableDeathCauseV1, DurableDeathCommitRequestV1, DurableNetworkStateV1, DurableRecallStateV1,
    LiveDamageTraceCauseV1, LiveDamageTraceDamageTypeV1, LiveDamageTraceEntryV1,
    LiveDamageTraceNetworkStateV1, LiveDamageTraceRecallStateV1, LiveDamageTraceTickRequestV1,
    PersistenceError, StoredLiveDamageTraceSnapshotEntryV1,
};

pub const DEATH_LIVE_TRACE_PROMOTION_DIGEST_CONTEXT_V1: &str =
    "gravebound.death-live-trace-promotion.v1";
pub const DEATH_TERMINAL_PAYLOAD_HASH_CONTEXT_V1: &str = "gravebound.death-terminal-payload.v1";

const CONTRACT_VERSION_V1: u16 = 1;
const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_RECEIPTS_V1: usize = 301;

/// Immutable provenance copied beside one durable combat-trace entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableDeathTraceEntryProvenanceV1 {
    pub trace_ordinal: u16,
    pub trace_tick_id: [u8; ID_BYTES],
    pub event_tick: u64,
    pub event_ordinal: u32,
    pub cause: DurableDeathCauseV1,
    pub source_entity_id: Option<[u8; ID_BYTES]>,
    pub source_sim_entity_id: Option<u64>,
    pub status_count: u16,
    pub live_entry_digest: [u8; HASH_BYTES],
}

pub(crate) struct DurableDeathTracePromotionDigestMaterialV1<'a> {
    pub contract_version: u16,
    pub death_id: [u8; ID_BYTES],
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub lineage_id: [u8; ID_BYTES],
    pub restore_point_id: [u8; ID_BYTES],
    pub checkpoint_tick: u64,
    pub terminal_character_version: u64,
    pub records_blake3: &'a str,
    pub assets_blake3: &'a str,
    pub localization_blake3: &'a str,
    pub first_event_tick: u64,
    pub death_tick: u64,
    pub receipt_count: u16,
    pub entry_count: u16,
    pub status_count: u32,
    pub lethal_trace_tick_id: [u8; ID_BYTES],
}

/// Sealed promotion authority consumed by the single durable-death transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableDeathTracePromotionV1 {
    contract_version: u16,
    death_id: [u8; ID_BYTES],
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    lineage_id: [u8; ID_BYTES],
    restore_point_id: [u8; ID_BYTES],
    checkpoint_tick: u64,
    terminal_character_version: u64,
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
    first_event_tick: u64,
    death_tick: u64,
    receipt_count: u16,
    entry_count: u16,
    status_count: u32,
    lethal_trace_tick_id: [u8; ID_BYTES],
    lethal_request: LiveDamageTraceTickRequestV1,
    entries: Vec<DurableDeathTraceEntryProvenanceV1>,
    promotion_digest: [u8; HASH_BYTES],
    terminal_payload_hash: [u8; HASH_BYTES],
}

impl DurableDeathTracePromotionV1 {
    /// Seals complete live evidence against an already validated durable death request.
    pub fn seal(
        death: &DurableDeathCommitRequestV1,
        lethal_request: LiveDamageTraceTickRequestV1,
        full_window: &[StoredLiveDamageTraceSnapshotEntryV1],
    ) -> Result<Self, PersistenceError> {
        death.validate()?;
        lethal_request.validate()?;
        validate_authority(death, &lethal_request)?;
        validate_window_shape(death, &lethal_request, full_window)?;

        let mut durable_by_sim = BTreeMap::new();
        let mut sim_by_durable = BTreeMap::new();
        let mut entries = Vec::with_capacity(full_window.len());
        let mut status_count = 0_u32;
        for (index, (stored, durable)) in full_window.iter().zip(&death.plan.trace).enumerate() {
            validate_exact_entry(stored, durable)?;
            validate_identity_pair(&mut durable_by_sim, &mut sim_by_durable, &stored.entry)?;
            let entry_status_count =
                u16::try_from(stored.entry.statuses.len()).map_err(|_| corrupt())?;
            status_count = status_count
                .checked_add(u32::from(entry_status_count))
                .ok_or_else(corrupt)?;
            let live_entry_digest =
                canonical_live_damage_trace_entry_digest_v1(stored.event_tick, &stored.entry)?;
            if is_zero(&stored.trace_tick_id) || is_zero(&live_entry_digest) {
                return Err(corrupt());
            }
            entries.push(DurableDeathTraceEntryProvenanceV1 {
                trace_ordinal: u16::try_from(index).map_err(|_| corrupt())?,
                trace_tick_id: stored.trace_tick_id,
                event_tick: stored.event_tick,
                event_ordinal: stored.entry.event_ordinal,
                cause: durable_cause(stored.entry.cause),
                source_entity_id: stored.entry.source_entity_id,
                source_sim_entity_id: stored.entry.source_sim_entity_id,
                status_count: entry_status_count,
                live_entry_digest,
            });
        }

        let receipt_count = count_receipts(full_window)?;
        let command = &lethal_request.command;
        let event = &death.plan.event;
        let mut promotion = Self {
            contract_version: CONTRACT_VERSION_V1,
            death_id: event.death_id,
            account_id: event.account_id,
            character_id: event.character_id,
            lineage_id: event.lineage_id,
            restore_point_id: event.restore_point_id,
            checkpoint_tick: command.danger.checkpoint_tick,
            terminal_character_version: command.expected_character_version,
            records_blake3: event.records_blake3.clone(),
            assets_blake3: event.assets_blake3.clone(),
            localization_blake3: event.localization_blake3.clone(),
            first_event_tick: full_window.first().ok_or_else(corrupt)?.event_tick,
            death_tick: event.death_tick,
            receipt_count,
            entry_count: u16::try_from(entries.len()).map_err(|_| corrupt())?,
            status_count,
            lethal_trace_tick_id: command.trace_tick_id,
            lethal_request,
            entries,
            promotion_digest: [0; HASH_BYTES],
            terminal_payload_hash: [0; HASH_BYTES],
        };
        promotion.promotion_digest = promotion.expected_promotion_digest()?;
        promotion.terminal_payload_hash = canonical_death_terminal_payload_hash_v1(
            death.canonical_request_hash,
            promotion.promotion_digest,
        )?;
        promotion.validate_against(death, full_window)?;
        Ok(promotion)
    }

    /// Revalidates a sealed value against the exact death and complete normalized window.
    pub fn validate_against(
        &self,
        death: &DurableDeathCommitRequestV1,
        full_window: &[StoredLiveDamageTraceSnapshotEntryV1],
    ) -> Result<(), PersistenceError> {
        self.validate_request_binding(death)?;
        validate_window_shape(death, &self.lethal_request, full_window)?;
        if self.first_event_tick != full_window.first().ok_or_else(corrupt)?.event_tick
            || usize::from(self.receipt_count) != usize::from(count_receipts(full_window)?)
        {
            return Err(corrupt());
        }

        let mut status_count = 0_u32;
        for (index, ((provenance, stored), durable)) in self
            .entries
            .iter()
            .zip(full_window)
            .zip(&death.plan.trace)
            .enumerate()
        {
            validate_exact_entry(stored, durable)?;
            let expected_status_count =
                u16::try_from(stored.entry.statuses.len()).map_err(|_| corrupt())?;
            status_count = status_count
                .checked_add(u32::from(expected_status_count))
                .ok_or_else(corrupt)?;
            if provenance.trace_ordinal != u16::try_from(index).map_err(|_| corrupt())?
                || provenance.trace_tick_id != stored.trace_tick_id
                || provenance.event_tick != stored.event_tick
                || provenance.event_ordinal != stored.entry.event_ordinal
                || provenance.cause != durable_cause(stored.entry.cause)
                || provenance.source_entity_id != stored.entry.source_entity_id
                || provenance.source_sim_entity_id != stored.entry.source_sim_entity_id
                || provenance.status_count != expected_status_count
                || provenance.live_entry_digest
                    != canonical_live_damage_trace_entry_digest_v1(
                        stored.event_tick,
                        &stored.entry,
                    )?
            {
                return Err(corrupt());
            }
        }
        if self.status_count != status_count {
            return Err(corrupt());
        }
        Ok(())
    }

    /// Validates every window-independent field before a repository can take an early replay
    /// branch. This prevents a promotion sealed for another same-ID request from being logged as
    /// an altered trace or misclassified as stored corruption.
    pub fn validate_request_binding(
        &self,
        death: &DurableDeathCommitRequestV1,
    ) -> Result<(), PersistenceError> {
        death.validate()?;
        self.lethal_request.validate()?;
        validate_authority(death, &self.lethal_request)?;
        if self.contract_version != CONTRACT_VERSION_V1
            || self.death_id != death.plan.event.death_id
            || self.account_id != death.plan.event.account_id
            || self.character_id != death.plan.event.character_id
            || self.lineage_id != death.plan.event.lineage_id
            || self.restore_point_id != death.plan.event.restore_point_id
            || self.checkpoint_tick != self.lethal_request.command.danger.checkpoint_tick
            || self.terminal_character_version
                != self.lethal_request.command.expected_character_version
            || self.records_blake3 != death.plan.event.records_blake3
            || self.assets_blake3 != death.plan.event.assets_blake3
            || self.localization_blake3 != death.plan.event.localization_blake3
            || self.death_tick != death.plan.event.death_tick
            || self.receipt_count == 0
            || usize::from(self.receipt_count) > MAX_RECEIPTS_V1
            || usize::from(self.entry_count) != death.plan.trace.len()
            || self.entries.len() != death.plan.trace.len()
            || self.lethal_trace_tick_id != self.lethal_request.command.trace_tick_id
            || is_zero(&self.promotion_digest)
            || self.promotion_digest != self.expected_promotion_digest()?
            || is_zero(&self.terminal_payload_hash)
            || self.terminal_payload_hash
                != canonical_death_terminal_payload_hash_v1(
                    death.canonical_request_hash,
                    self.promotion_digest,
                )?
        {
            return Err(corrupt());
        }
        let mut status_count = 0_u32;
        for (index, (provenance, durable)) in self.entries.iter().zip(&death.plan.trace).enumerate()
        {
            let expected_status_count =
                u16::try_from(durable.statuses.len()).map_err(|_| corrupt())?;
            status_count = status_count
                .checked_add(u32::from(expected_status_count))
                .ok_or_else(corrupt)?;
            if provenance.trace_ordinal != u16::try_from(index).map_err(|_| corrupt())?
                || provenance.event_tick != durable.event_tick
                || provenance.event_ordinal != durable.event_ordinal
                || provenance.source_entity_id != durable.source_entity_id
                || provenance.status_count != expected_status_count
            {
                return Err(corrupt());
            }
        }
        if self.status_count != status_count {
            return Err(corrupt());
        }
        Ok(())
    }

    fn expected_promotion_digest(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        canonical_stored_death_trace_promotion_digest_v1(
            &DurableDeathTracePromotionDigestMaterialV1 {
                contract_version: self.contract_version,
                death_id: self.death_id,
                account_id: self.account_id,
                character_id: self.character_id,
                lineage_id: self.lineage_id,
                restore_point_id: self.restore_point_id,
                checkpoint_tick: self.checkpoint_tick,
                terminal_character_version: self.terminal_character_version,
                records_blake3: &self.records_blake3,
                assets_blake3: &self.assets_blake3,
                localization_blake3: &self.localization_blake3,
                first_event_tick: self.first_event_tick,
                death_tick: self.death_tick,
                receipt_count: self.receipt_count,
                entry_count: self.entry_count,
                status_count: self.status_count,
                lethal_trace_tick_id: self.lethal_trace_tick_id,
            },
            self.lethal_request.request_hash,
            &self.entries,
        )
    }

    #[must_use]
    pub const fn contract_version(&self) -> u16 {
        self.contract_version
    }

    #[must_use]
    pub const fn death_id(&self) -> [u8; ID_BYTES] {
        self.death_id
    }

    #[must_use]
    pub const fn account_id(&self) -> [u8; ID_BYTES] {
        self.account_id
    }

    #[must_use]
    pub const fn character_id(&self) -> [u8; ID_BYTES] {
        self.character_id
    }

    #[must_use]
    pub const fn lineage_id(&self) -> [u8; ID_BYTES] {
        self.lineage_id
    }

    #[must_use]
    pub const fn restore_point_id(&self) -> [u8; ID_BYTES] {
        self.restore_point_id
    }

    #[must_use]
    pub const fn checkpoint_tick(&self) -> u64 {
        self.checkpoint_tick
    }

    #[must_use]
    pub const fn terminal_character_version(&self) -> u64 {
        self.terminal_character_version
    }

    #[must_use]
    pub fn records_blake3(&self) -> &str {
        &self.records_blake3
    }

    #[must_use]
    pub fn assets_blake3(&self) -> &str {
        &self.assets_blake3
    }

    #[must_use]
    pub fn localization_blake3(&self) -> &str {
        &self.localization_blake3
    }

    #[must_use]
    pub const fn first_event_tick(&self) -> u64 {
        self.first_event_tick
    }

    #[must_use]
    pub const fn death_tick(&self) -> u64 {
        self.death_tick
    }

    #[must_use]
    pub const fn receipt_count(&self) -> u16 {
        self.receipt_count
    }

    #[must_use]
    pub const fn entry_count(&self) -> u16 {
        self.entry_count
    }

    #[must_use]
    pub const fn status_count(&self) -> u32 {
        self.status_count
    }

    #[must_use]
    pub const fn lethal_trace_tick_id(&self) -> [u8; ID_BYTES] {
        self.lethal_trace_tick_id
    }

    #[must_use]
    pub const fn lethal_request(&self) -> &LiveDamageTraceTickRequestV1 {
        &self.lethal_request
    }

    #[must_use]
    pub fn entries(&self) -> &[DurableDeathTraceEntryProvenanceV1] {
        &self.entries
    }

    #[must_use]
    pub const fn promotion_digest(&self) -> [u8; HASH_BYTES] {
        self.promotion_digest
    }

    #[must_use]
    pub const fn terminal_payload_hash(&self) -> [u8; HASH_BYTES] {
        self.terminal_payload_hash
    }
}

pub(crate) fn canonical_stored_death_trace_promotion_digest_v1(
    material: &DurableDeathTracePromotionDigestMaterialV1<'_>,
    lethal_request_hash: [u8; HASH_BYTES],
    entries: &[DurableDeathTraceEntryProvenanceV1],
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new_derive_key(DEATH_LIVE_TRACE_PROMOTION_DIGEST_CONTEXT_V1);
    hash_field(&mut hasher, &material.contract_version.to_le_bytes())?;
    hash_field(&mut hasher, &material.death_id)?;
    hash_field(&mut hasher, &material.account_id)?;
    hash_field(&mut hasher, &material.character_id)?;
    hash_field(&mut hasher, &material.lineage_id)?;
    hash_field(&mut hasher, &material.restore_point_id)?;
    hash_field(&mut hasher, &material.checkpoint_tick.to_le_bytes())?;
    hash_field(
        &mut hasher,
        &material.terminal_character_version.to_le_bytes(),
    )?;
    hash_field(&mut hasher, material.records_blake3.as_bytes())?;
    hash_field(&mut hasher, material.assets_blake3.as_bytes())?;
    hash_field(&mut hasher, material.localization_blake3.as_bytes())?;
    hash_field(&mut hasher, &material.first_event_tick.to_le_bytes())?;
    hash_field(&mut hasher, &material.death_tick.to_le_bytes())?;
    hash_field(&mut hasher, &material.receipt_count.to_le_bytes())?;
    hash_field(&mut hasher, &material.entry_count.to_le_bytes())?;
    hash_field(&mut hasher, &material.status_count.to_le_bytes())?;
    hash_field(&mut hasher, &material.lethal_trace_tick_id)?;
    // The lethal request hash includes its predecessor head's result digest. Each retained result
    // digest includes that receipt's request hash, transitively binding the predecessor chain.
    hash_field(&mut hasher, &lethal_request_hash)?;
    for entry in entries {
        hash_field(&mut hasher, &entry.trace_ordinal.to_le_bytes())?;
        hash_field(&mut hasher, &entry.trace_tick_id)?;
        hash_field(&mut hasher, &entry.event_tick.to_le_bytes())?;
        hash_field(&mut hasher, &entry.event_ordinal.to_le_bytes())?;
        hash_field(&mut hasher, &[cause_code(entry.cause)])?;
        hash_optional(
            &mut hasher,
            entry.source_entity_id.as_ref().map(<[u8; 16]>::as_slice),
        )?;
        hash_optional(
            &mut hasher,
            entry
                .source_sim_entity_id
                .map(u64::to_le_bytes)
                .as_ref()
                .map(<[u8; 8]>::as_slice),
        )?;
        hash_field(&mut hasher, &entry.status_count.to_le_bytes())?;
        hash_field(&mut hasher, &entry.live_entry_digest)?;
    }
    let digest = *hasher.finalize().as_bytes();
    if is_zero(&digest) {
        return Err(corrupt());
    }
    Ok(digest)
}

/// Binds the existing canonical death request identity to one exact live-trace promotion.
pub fn canonical_death_terminal_payload_hash_v1(
    canonical_death_request_hash: [u8; HASH_BYTES],
    promotion_digest: [u8; HASH_BYTES],
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    if is_zero(&canonical_death_request_hash) || is_zero(&promotion_digest) {
        return Err(corrupt());
    }
    let mut hasher = blake3::Hasher::new_derive_key(DEATH_TERMINAL_PAYLOAD_HASH_CONTEXT_V1);
    hash_field(&mut hasher, &canonical_death_request_hash)?;
    hash_field(&mut hasher, &promotion_digest)?;
    let hash = *hasher.finalize().as_bytes();
    if is_zero(&hash) {
        return Err(corrupt());
    }
    Ok(hash)
}

fn validate_authority(
    death: &DurableDeathCommitRequestV1,
    lethal: &LiveDamageTraceTickRequestV1,
) -> Result<(), PersistenceError> {
    let event = &death.plan.event;
    let command = &lethal.command;
    if command.account_id != event.account_id
        || command.character_id != event.character_id
        || command.expected_character_version != event.versions.character.pre
        || command.danger.lineage_id != event.lineage_id
        || command.danger.restore_point_id != event.restore_point_id
        || command.content.records_blake3 != event.records_blake3
        || command.content.assets_blake3 != event.assets_blake3
        || command.content.localization_blake3 != event.localization_blake3
        || command.event_tick != event.death_tick
        || command.issued_at_unix_ms > death.issued_at_unix_ms
        || command.entries.last().is_none_or(|entry| !entry.lethal)
        || durable_cause(command.entries.last().ok_or_else(corrupt)?.cause) != event.cause
    {
        return Err(corrupt());
    }
    Ok(())
}

fn validate_window_shape(
    death: &DurableDeathCommitRequestV1,
    lethal: &LiveDamageTraceTickRequestV1,
    full_window: &[StoredLiveDamageTraceSnapshotEntryV1],
) -> Result<(), PersistenceError> {
    let event = &death.plan.event;
    if full_window.is_empty()
        || full_window.len() != death.plan.trace.len()
        || full_window.len() > crate::MAX_DURABLE_DEATH_TRACE_ENTRIES
        || full_window.first().is_none_or(|first| {
            first.event_tick == 0
                || first.event_tick > event.death_tick
                || event.death_tick - first.event_tick > DURABLE_DEATH_TRACE_WINDOW_TICKS
        })
    {
        return Err(corrupt());
    }

    let mut previous_order = None;
    let mut active_tick = None;
    let mut seen_tick_ids = BTreeSet::new();
    for stored in full_window {
        let order = (stored.event_tick, stored.entry.event_ordinal);
        if stored.event_tick > event.death_tick
            || event.death_tick - stored.event_tick > DURABLE_DEATH_TRACE_WINDOW_TICKS
            || previous_order.is_some_and(|previous| previous >= order)
            || is_zero(&stored.trace_tick_id)
        {
            return Err(corrupt());
        }
        previous_order = Some(order);
        if active_tick != Some((stored.trace_tick_id, stored.event_tick)) {
            if !seen_tick_ids.insert(stored.trace_tick_id)
                || active_tick.is_some_and(|(_, event_tick)| event_tick >= stored.event_tick)
            {
                return Err(corrupt());
            }
            active_tick = Some((stored.trace_tick_id, stored.event_tick));
        }
    }
    if seen_tick_ids.len() > MAX_RECEIPTS_V1 {
        return Err(corrupt());
    }

    let lethal_tick_id = lethal.command.trace_tick_id;
    let suffix_start = full_window
        .iter()
        .position(|entry| entry.trace_tick_id == lethal_tick_id)
        .ok_or_else(corrupt)?;
    let suffix = &full_window[suffix_start..];
    if suffix.len() != lethal.command.entries.len()
        || suffix.iter().any(|entry| {
            entry.trace_tick_id != lethal_tick_id || entry.event_tick != lethal.command.event_tick
        })
        || suffix
            .iter()
            .zip(&lethal.command.entries)
            .any(|(stored, requested)| &stored.entry != requested)
        || suffix.last().is_none_or(|entry| !entry.entry.lethal)
        || full_window[..suffix_start]
            .iter()
            .any(|entry| entry.entry.lethal)
    {
        return Err(corrupt());
    }

    match (suffix_start, &lethal.command.expected_previous) {
        (0, None) => {}
        (0, Some(_)) | (_, None) => return Err(corrupt()),
        (start, Some(previous)) => {
            let prior = &full_window[start - 1];
            if previous.trace_tick_id != prior.trace_tick_id
                || previous.event_tick != prior.event_tick
            {
                return Err(corrupt());
            }
        }
    }
    Ok(())
}

fn validate_exact_entry(
    stored: &StoredLiveDamageTraceSnapshotEntryV1,
    durable: &DurableCombatTraceEntryV1,
) -> Result<(), PersistenceError> {
    let live = &stored.entry;
    if durable.event_tick != stored.event_tick
        || durable.event_ordinal != live.event_ordinal
        || durable.source_content_id != live.source_content_id
        || durable.source_entity_id != live.source_entity_id
        || durable.pattern_id != live.pattern_id
        || durable.attack_id != live.attack_id
        || durable.raw_damage != live.raw_damage
        || durable.final_damage != live.final_damage
        || durable.damage_type != durable_damage_type(live.damage_type)
        || durable.pre_health != live.pre_health
        || durable.post_health != live.post_health
        || durable.source_x_milli_tiles != live.source_x_milli_tiles
        || durable.source_y_milli_tiles != live.source_y_milli_tiles
        || durable.network_state != durable_network_state(live.network_state)
        || durable.recall_state != durable_recall_state(live.recall_state)
        || durable.lethal != live.lethal
        || durable.statuses.len() != live.statuses.len()
        || durable
            .statuses
            .iter()
            .zip(&live.statuses)
            .any(|(left, right)| {
                left.ordinal != right.status_ordinal
                    || left.status_id != right.status_id
                    || left.remaining_ticks != right.remaining_ticks
                    || left.stack_count != right.stack_count
            })
    {
        return Err(corrupt());
    }
    Ok(())
}

fn validate_identity_pair(
    durable_by_sim: &mut BTreeMap<u64, [u8; ID_BYTES]>,
    sim_by_durable: &mut BTreeMap<[u8; ID_BYTES], u64>,
    entry: &LiveDamageTraceEntryV1,
) -> Result<(), PersistenceError> {
    match (entry.source_sim_entity_id, entry.source_entity_id) {
        (None, None) => Ok(()),
        (Some(sim), Some(durable)) => {
            if durable_by_sim
                .insert(sim, durable)
                .is_some_and(|existing| existing != durable)
                || sim_by_durable
                    .insert(durable, sim)
                    .is_some_and(|existing| existing != sim)
            {
                Err(corrupt())
            } else {
                Ok(())
            }
        }
        _ => Err(corrupt()),
    }
}

fn count_receipts(
    full_window: &[StoredLiveDamageTraceSnapshotEntryV1],
) -> Result<u16, PersistenceError> {
    let count = full_window
        .iter()
        .map(|entry| entry.trace_tick_id)
        .collect::<BTreeSet<_>>()
        .len();
    u16::try_from(count)
        .ok()
        .filter(|value| (1..=u16::try_from(MAX_RECEIPTS_V1).unwrap_or(u16::MAX)).contains(value))
        .ok_or_else(corrupt)
}

const fn durable_cause(cause: LiveDamageTraceCauseV1) -> DurableDeathCauseV1 {
    match cause {
        LiveDamageTraceCauseV1::DirectHit => DurableDeathCauseV1::DirectHit,
        LiveDamageTraceCauseV1::DamageOverTime => DurableDeathCauseV1::DamageOverTime,
        LiveDamageTraceCauseV1::Environment => DurableDeathCauseV1::Environment,
        LiveDamageTraceCauseV1::Disconnect => DurableDeathCauseV1::Disconnect,
    }
}

const fn durable_damage_type(value: LiveDamageTraceDamageTypeV1) -> DurableDamageTypeV1 {
    match value {
        LiveDamageTraceDamageTypeV1::Physical => DurableDamageTypeV1::Physical,
        LiveDamageTraceDamageTypeV1::Veil => DurableDamageTypeV1::Veil,
    }
}

const fn durable_network_state(value: LiveDamageTraceNetworkStateV1) -> DurableNetworkStateV1 {
    match value {
        LiveDamageTraceNetworkStateV1::Connected => DurableNetworkStateV1::Connected,
        LiveDamageTraceNetworkStateV1::Degraded => DurableNetworkStateV1::Degraded,
        LiveDamageTraceNetworkStateV1::LinkLost => DurableNetworkStateV1::LinkLost,
        LiveDamageTraceNetworkStateV1::Reattached => DurableNetworkStateV1::Reattached,
    }
}

const fn durable_recall_state(value: LiveDamageTraceRecallStateV1) -> DurableRecallStateV1 {
    match value {
        LiveDamageTraceRecallStateV1::Inactive => DurableRecallStateV1::Inactive,
        LiveDamageTraceRecallStateV1::Channeling => DurableRecallStateV1::Channeling,
        LiveDamageTraceRecallStateV1::CompletionPending => DurableRecallStateV1::CompletionPending,
    }
}

const fn cause_code(cause: DurableDeathCauseV1) -> u8 {
    match cause {
        DurableDeathCauseV1::DirectHit => 0,
        DurableDeathCauseV1::DamageOverTime => 1,
        DurableDeathCauseV1::Environment => 2,
        DurableDeathCauseV1::Disconnect => 3,
    }
}

fn hash_field(hasher: &mut blake3::Hasher, bytes: &[u8]) -> Result<(), PersistenceError> {
    hasher.update(
        &u64::try_from(bytes.len())
            .map_err(|_| corrupt())?
            .to_le_bytes(),
    );
    hasher.update(bytes);
    Ok(())
}

fn hash_optional(
    hasher: &mut blake3::Hasher,
    bytes: Option<&[u8]>,
) -> Result<(), PersistenceError> {
    match bytes {
        None => hash_field(hasher, &[0]),
        Some(bytes) => {
            hash_field(hasher, &[1])?;
            hash_field(hasher, bytes)
        }
    }
}

fn is_zero<const N: usize>(value: &[u8; N]) -> bool {
    value.iter().all(|byte| *byte == 0)
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredDurableDeath
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CORE_WORLD_ASSETS_BLAKE3, CORE_WORLD_LOCALIZATION_BLAKE3, CORE_WORLD_RECORDS_BLAKE3,
        LiveDamageTraceContentAuthorityV1, LiveDamageTraceDangerAuthorityV1, LiveDamageTraceHeadV1,
        LiveDamageTraceStatusV1, LiveDamageTraceTickCommandV1, durable_death::tests::valid_request,
    };

    fn live_entry(durable: &DurableCombatTraceEntryV1, sim_id: u64) -> LiveDamageTraceEntryV1 {
        LiveDamageTraceEntryV1 {
            event_ordinal: durable.event_ordinal,
            cause: LiveDamageTraceCauseV1::DirectHit,
            source_content_id: durable.source_content_id.clone(),
            source_entity_id: durable.source_entity_id,
            source_sim_entity_id: durable.source_entity_id.map(|_| sim_id),
            pattern_id: durable.pattern_id.clone(),
            attack_id: durable.attack_id.clone(),
            raw_damage: durable.raw_damage,
            final_damage: durable.final_damage,
            damage_type: LiveDamageTraceDamageTypeV1::Physical,
            pre_health: durable.pre_health,
            post_health: durable.post_health,
            source_x_milli_tiles: durable.source_x_milli_tiles,
            source_y_milli_tiles: durable.source_y_milli_tiles,
            network_state: LiveDamageTraceNetworkStateV1::Connected,
            recall_state: LiveDamageTraceRecallStateV1::Inactive,
            lethal: durable.lethal,
            statuses: durable
                .statuses
                .iter()
                .map(|status| LiveDamageTraceStatusV1 {
                    status_ordinal: status.ordinal,
                    status_id: status.status_id.clone(),
                    remaining_ticks: status.remaining_ticks,
                    stack_count: status.stack_count,
                })
                .collect(),
        }
    }

    fn fixture() -> (
        DurableDeathCommitRequestV1,
        LiveDamageTraceTickRequestV1,
        Vec<StoredLiveDamageTraceSnapshotEntryV1>,
    ) {
        let mut death = valid_request();
        death.plan.event.records_blake3 = CORE_WORLD_RECORDS_BLAKE3.into();
        death.plan.event.assets_blake3 = CORE_WORLD_ASSETS_BLAKE3.into();
        death.plan.event.localization_blake3 = CORE_WORLD_LOCALIZATION_BLAKE3.into();
        death = DurableDeathCommitRequestV1::seal(death.plan, death.issued_at_unix_ms).unwrap();
        let first_tick_id = [21; 16];
        let lethal_tick_id = [22; 16];
        let window = vec![
            StoredLiveDamageTraceSnapshotEntryV1 {
                trace_tick_id: first_tick_id,
                event_tick: death.plan.trace[0].event_tick,
                entry: live_entry(&death.plan.trace[0], 81),
            },
            StoredLiveDamageTraceSnapshotEntryV1 {
                trace_tick_id: lethal_tick_id,
                event_tick: death.plan.trace[1].event_tick,
                entry: live_entry(&death.plan.trace[1], 81),
            },
        ];
        let command = LiveDamageTraceTickCommandV1 {
            account_id: death.plan.event.account_id,
            character_id: death.plan.event.character_id,
            trace_tick_id: lethal_tick_id,
            expected_character_version: death.plan.event.versions.character.pre,
            expected_previous: Some(LiveDamageTraceHeadV1 {
                trace_tick_id: first_tick_id,
                event_tick: death.plan.trace[0].event_tick,
                result_digest: [23; 32],
            }),
            event_tick: death.plan.event.death_tick,
            danger: LiveDamageTraceDangerAuthorityV1 {
                lineage_id: death.plan.event.lineage_id,
                restore_point_id: death.plan.event.restore_point_id,
                checkpoint_tick: 900,
            },
            content: LiveDamageTraceContentAuthorityV1::core(),
            entries: vec![window[1].entry.clone()],
            issued_at_unix_ms: death.issued_at_unix_ms - 1,
        };
        let lethal = LiveDamageTraceTickRequestV1::seal(command).unwrap();
        (death, lethal, window)
    }

    #[test]
    fn complete_window_seals_one_stable_terminal_identity() {
        let (death, lethal, window) = fixture();
        let promotion = DurableDeathTracePromotionV1::seal(&death, lethal, &window).unwrap();
        promotion.validate_against(&death, &window).unwrap();
        assert_eq!(promotion.receipt_count(), 2);
        assert_eq!(promotion.entry_count(), 2);
        assert_eq!(promotion.status_count(), 1);
        assert_ne!(promotion.promotion_digest(), [0; 32]);
        assert_eq!(
            promotion.terminal_payload_hash(),
            canonical_death_terminal_payload_hash_v1(
                death.canonical_request_hash,
                promotion.promotion_digest()
            )
            .unwrap()
        );
    }

    #[test]
    fn replay_binding_rejects_another_same_identity_death_request() {
        let (death, lethal, window) = fixture();
        let promotion = DurableDeathTracePromotionV1::seal(&death, lethal, &window).unwrap();
        let changed =
            DurableDeathCommitRequestV1::seal(death.plan.clone(), death.issued_at_unix_ms + 1)
                .unwrap();
        assert_ne!(changed.canonical_request_hash, death.canonical_request_hash);
        assert!(promotion.validate_request_binding(&changed).is_err());
    }

    #[test]
    fn altered_or_missing_window_entry_fails_closed() {
        let (death, lethal, mut window) = fixture();
        window[0].entry.attack_id = "attack.warden.changed".into();
        assert!(DurableDeathTracePromotionV1::seal(&death, lethal.clone(), &window).is_err());
        let (_, _, mut window) = fixture();
        window.remove(0);
        assert!(DurableDeathTracePromotionV1::seal(&death, lethal, &window).is_err());
    }

    #[test]
    fn reordered_window_or_tick_identity_fails_closed() {
        let (death, lethal, mut window) = fixture();
        window.swap(0, 1);
        assert!(DurableDeathTracePromotionV1::seal(&death, lethal.clone(), &window).is_err());
        let (_, _, mut window) = fixture();
        window[0].trace_tick_id = window[1].trace_tick_id;
        assert!(DurableDeathTracePromotionV1::seal(&death, lethal, &window).is_err());
    }

    #[test]
    fn non_bijective_simulation_identity_fails_closed() {
        let (death, lethal, mut window) = fixture();
        window[0].entry.source_sim_entity_id = Some(82);
        assert!(DurableDeathTracePromotionV1::seal(&death, lethal, &window).is_err());
    }

    #[test]
    fn altered_lethal_suffix_or_cause_fails_closed() {
        let (death, lethal, window) = fixture();
        let mut command = lethal.command.clone();
        command.entries[0].attack_id = "attack.warden.changed".into();
        let changed = LiveDamageTraceTickRequestV1::seal(command).unwrap();
        assert!(DurableDeathTracePromotionV1::seal(&death, changed, &window).is_err());

        let mut command = lethal.command;
        command.entries[0].cause = LiveDamageTraceCauseV1::Environment;
        let changed = LiveDamageTraceTickRequestV1::seal(command).unwrap();
        assert!(DurableDeathTracePromotionV1::seal(&death, changed, &window).is_err());
    }

    #[test]
    fn promotion_identity_transitively_binds_the_predecessor_receipt_chain() {
        let (death, lethal, window) = fixture();
        let original = DurableDeathTracePromotionV1::seal(&death, lethal.clone(), &window).unwrap();
        let mut command = lethal.command;
        command.expected_previous.as_mut().unwrap().result_digest[0] ^= 1;
        let changed = LiveDamageTraceTickRequestV1::seal(command).unwrap();
        let changed = DurableDeathTracePromotionV1::seal(&death, changed, &window).unwrap();
        assert_ne!(original.promotion_digest(), changed.promotion_digest());
        assert_ne!(
            original.terminal_payload_hash(),
            changed.terminal_payload_hash()
        );
    }
}
