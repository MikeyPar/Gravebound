//! Server-owned assembly for the durable `GB-M03-06` permadeath transaction.
//!
//! Authority is deliberately split at this boundary:
//! - `sim_core` compiles clocks, deeds, the lethal cause, and the ordered ten-second trace;
//! - `server_app` binds those facts to the authenticated selected character, active danger
//!   lineage, promoted content, canonical custody plan, presentation snapshot, and Echo projector
//!   material;
//! - `persistence` validates and commits the complete graph under its single-writer locks.
//!
//! No type in this module is decoded from a client frame. The lethal route remains unexposed
//! until the shared terminal arbiter, durable acknowledgement gate, and `GB-M03-06E` evidence pass.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-001`, `DTH-020`, and
//! `TECH-020`-`023`; `Gravebound_Content_Production_Spec_v1.md` `CONT-ECHO-009` and
//! `CONT-HUB-002`; `Gravebound_Development_Roadmap_v1.md` `GB-M03-06` and `GB-M03-13`; and the
//! owner-approved `SPEC-CONFLICT-009` split recorded by tasks `GB-M03-06B`/`06C`.

use std::collections::{BTreeMap, BTreeSet};

use content_schema::CoreDeathViewCopyKind;
use persistence::{
    AuthoritativeDeathPlanV1, DURABLE_DEATH_SCHEMA_VERSION, DURABLE_DEATH_SUMMARY_REVISION,
    DeathAggregateVersionsV1, DurableCombatTraceEntryV1, DurableDamageTypeV1, DurableDeathCauseV1,
    DurableDeathCommitRequestV1, DurableDeathContentAuthorityV1, DurableDeathEventV1,
    DurableDeathPresentationAuthorityV1, DurableDeathProvenanceV1, DurableDeathSummaryV1,
    DurableDeathTelemetryContextV1, DurableDeathTracePromotionV1, DurableDestructionEntryV1,
    DurableEchoEnvelopeV1, DurableEchoOutcomeV1, DurableEchoRecordV1, DurableEchoStateV1,
    DurableEchoTransitionReasonV1, DurableEchoTransitionV1, DurableMemorialRecordV1,
    DurableNetworkStateV1, DurableOrderedContentIdV1, DurableRecallStateV1,
    DurableSummaryDamageReferenceV1, DurableSummaryProjectionEntryV1,
    DurableSummaryProjectionKindV1, DurableSummaryProjectionsV1, DurableTraceStatusV1,
    MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES, PersistenceError, WIPEABLE_CORE_NAMESPACE,
    compare_canonical_durable_death_destruction_v1, derive_durable_death_bargain_cleanup_event_id,
    derive_durable_death_item_ledger_event_id, validate_durable_death_destruction_v1,
};
use sim_content::CoreDevelopmentDeathView;
use sim_core::{
    AuthoritativeDeathCauseKind, AuthoritativeDeathInputs, DEATH_AUTHORITY_SCHEMA_VERSION,
    DEED_NONE_ID, DamageTraceAggregate, DamageTraceCheckpointV1, DamageTraceEntry, DamageType,
    DeathTraceNetworkState, DeathTraceRecallState, ECHO_COMBAT_ELIGIBILITY_TICKS, EntityId,
    core_deed_en_us, ticks_to_milliseconds,
};
use thiserror::Error;

use crate::PreparedTerminalLiveDamageTrace;
use crate::identity::{AuthenticatedAccount, AuthenticatedNamespace};

const DURABLE_TRACE_DIGEST_CONTEXT: &str = "gravebound.durable-death.trace.v1";
const DURABLE_DESTRUCTION_DIGEST_CONTEXT: &str = "gravebound.durable-death.destruction.v1";

/// Authenticated, journaled identity accepted by the server terminal arbiter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathMutationAuthority {
    pub authenticated_account: AuthenticatedAccount,
    pub selected_character_id: [u8; 16],
    pub former_roster_ordinal: u8,
    pub mutation_id: [u8; 16],
    pub death_id: [u8; 16],
    pub issued_at_unix_ms: u64,
    /// Provisional server acceptance time. `PostgreSQL` rebinds this to its transaction clock.
    pub accepted_at_unix_ms: u64,
}

/// Active permadeath-enabled world lineage associated with the accepted lethal tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathWorldAuthority {
    pub instance_id: [u8; 16],
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub region_id: String,
    pub room_id: String,
    pub lineage_state: DeathLineageState,
}

/// Server admission and provenance state for the lineage that observed the lethal tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathLineageState {
    ActivePermadeath(DeathProvenance),
    ActiveNonPermadeath,
    Inactive,
    Superseded,
}

/// Reviewable server provenance used by the exact `ECH-001` exclusion predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathProvenance {
    OrdinaryGameplay,
    VerifiedServerIncident,
    AdministrativeAction,
}

/// Immutable hero and `DTH-020` presentation snapshot read from server authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathHeroSnapshot {
    pub hero_label_key: String,
    pub character_name: String,
    pub class_id: String,
    pub level: u8,
    pub oath_id: Option<String>,
    /// Stable IDs in acquisition order.
    pub bargain_ids: Vec<String>,
    pub memorial_presentation_key: String,
}

/// Stable identity mapping from simulation-local entities to the server instance journal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeathEntityIdentityAuthority {
    pub by_sim_entity: BTreeMap<EntityId, [u8; 16]>,
}

/// One server-authoritative item currently owned by the dying character in at-risk custody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathAtRiskItem {
    pub content_id: String,
    pub item_uid: [u8; 16],
    pub location: persistence::DurableDestructionLocationV1,
    pub item_version: u64,
}

/// One server-authoritative run-material pouch stack currently owned by the dying character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathAtRiskRunMaterial {
    pub material_id: String,
    pub quantity: u32,
    pub material_version: u64,
}

/// Complete raw custody authority consumed by the pure destruction planner.
///
/// Input order is deliberately irrelevant. The planner emits the exact canonical ledger order
/// and derives every post-version and item-ledger identity itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeathCustodySnapshot {
    pub items: Vec<DeathAtRiskItem>,
    pub run_materials: Vec<DeathAtRiskRunMaterial>,
}

/// Account-locked Echo availability decision consumed by the persistence projector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EchoAvailabilityProjection {
    /// Another Echo is already Available, so the newly created Echo stays Dormant.
    ExistingAvailable { echo_id: [u8; 16] },
    /// Promote the oldest Dormant Echo. This may be the newly created Echo.
    PromoteOldestDormant {
        echo_id: [u8; 16],
        echo_death_id: [u8; 16],
        next_transition_ordinal: u16,
    },
}

/// Immutable Echo snapshot material selected from server content and the dead life.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EligibleEchoProjection {
    pub echo_id: [u8; 16],
    pub appearance_snapshot_id: String,
    pub appearance_theme_id: String,
    pub weapon_signature_tag: Option<String>,
    pub relic_signature_tag: Option<String>,
    pub deed_tags: Vec<String>,
    pub power_band: u8,
    pub availability: EchoAvailabilityProjection,
}

/// Complete server-owned context. Nothing here is client-authored destination or death data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAuthoredDeathContext {
    pub mutation: DeathMutationAuthority,
    pub world: DeathWorldAuthority,
    pub content: DurableDeathContentAuthorityV1,
    pub versions: DeathAggregateVersionsV1,
    /// Raw server custody. The builder invokes the canonical planner; callers cannot author
    /// ordinals, post-versions, or item-ledger identities.
    pub custody: DeathCustodySnapshot,
    pub hero: DeathHeroSnapshot,
    /// Lethal evidence prepared only by the server-owned live trace service. Its private fields
    /// prevent a client or external crate from constructing an alternate terminal window.
    pub terminal_trace: PreparedTerminalLiveDamageTrace,
    /// Same-frame TEL-003 facts produced by the private route and QUIC transport owners.
    pub telemetry: DurableDeathTelemetryContextV1,
    /// Must be present exactly when the simulation time-and-deed predicate is eligible.
    pub echo: Option<EligibleEchoProjection>,
}

/// A sealed request paired with the promoted content authority that must validate it at commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDurableDeathCommit {
    request: DurableDeathCommitRequestV1,
    content: DurableDeathContentAuthorityV1,
    promotion: DurableDeathTracePromotionV1,
}

impl PreparedDurableDeathCommit {
    #[must_use]
    pub const fn request(&self) -> &DurableDeathCommitRequestV1 {
        &self.request
    }

    #[must_use]
    pub const fn content(&self) -> &DurableDeathContentAuthorityV1 {
        &self.content
    }

    #[must_use]
    pub const fn promotion(&self) -> &DurableDeathTracePromotionV1 {
        &self.promotion
    }

    /// Rechecks the cross-domain terminal binding before the prepared value can enter arbitration.
    /// Construction is private to this module, but callers still fail closed if memory or future
    /// internal refactors ever present mismatched request and promotion authority.
    pub fn validate_terminal_binding(&self) -> Result<(), PersistenceError> {
        self.promotion.validate_request_binding(&self.request)
    }

    #[cfg(test)]
    pub(crate) fn from_test_parts(
        request: DurableDeathCommitRequestV1,
        content: DurableDeathContentAuthorityV1,
        promotion: DurableDeathTracePromotionV1,
    ) -> Self {
        Self {
            request,
            content,
            promotion,
        }
    }
}

#[derive(Debug, Error)]
pub enum DurableDeathBuildError {
    #[error("server death identity is missing, malformed, or not UUIDv7")]
    InvalidIdentity,
    #[error("authoritative death evidence is internally inconsistent: {0}")]
    EvidenceMismatch(&'static str),
    #[error("the bound lineage is not active and permadeath-enabled")]
    WorldAuthorityMismatch,
    #[error("simulation entity {0} has no bound server journal identity")]
    MissingEntityIdentity(u64),
    #[error("Echo projection material does not match authoritative eligibility")]
    EchoEligibilityMismatch,
    #[error("death presentation content is missing or incompatible: {0}")]
    PresentationContentMismatch(&'static str),
    #[error("server death custody could not be planned: {0}")]
    DestructionPlanning(#[from] DurableDeathPlanningError),
    #[error("durable death DTO validation failed")]
    Persistence(#[source] PersistenceError),
}

impl From<PersistenceError> for DurableDeathBuildError {
    fn from(value: PersistenceError) -> Self {
        Self::Persistence(value)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DurableDeathPlanningError {
    #[error("death destruction authority is missing or malformed")]
    InvalidAuthority,
    #[error("death custody snapshot is malformed, duplicated, or exceeds durable bounds")]
    InvalidCustodySnapshot,
    #[error("an item or material version cannot advance")]
    VersionExhausted,
}

/// Produces the exact deterministic destruction ledger for one accepted terminal death.
///
/// The primary order is Equipment, Belt, `RunBackpack`, `PersonalGround`, then run materials.
/// Durable unit UID is the stable tie-break inside stacked Belt and `RunBackpack` slots; this
/// keeps the GDD's slot-first order total without depending on query or insertion order.
pub fn plan_durable_death_destruction(
    death_id: [u8; 16],
    mutation_id: [u8; 16],
    custody: &DeathCustodySnapshot,
) -> Result<Vec<DurableDestructionEntryV1>, DurableDeathPlanningError> {
    if !is_uuid_v7(death_id) || !is_uuid_v7(mutation_id) {
        return Err(DurableDeathPlanningError::InvalidAuthority);
    }
    let entry_count = custody
        .items
        .len()
        .checked_add(custody.run_materials.len())
        .ok_or(DurableDeathPlanningError::InvalidCustodySnapshot)?;
    if entry_count > MAX_DURABLE_DEATH_DESTRUCTION_ENTRIES {
        return Err(DurableDeathPlanningError::InvalidCustodySnapshot);
    }

    let mut destruction = Vec::with_capacity(entry_count);
    for item in &custody.items {
        destruction.push(DurableDestructionEntryV1::Item {
            ordinal: 0,
            content_id: item.content_id.clone(),
            item_uid: item.item_uid,
            location: item.location.clone(),
            pre_item_version: item.item_version,
            post_item_version: item
                .item_version
                .checked_add(1)
                .ok_or(DurableDeathPlanningError::VersionExhausted)?,
            ledger_event_id: derive_durable_death_item_ledger_event_id(
                death_id,
                mutation_id,
                item.item_uid,
            ),
        });
    }
    for material in &custody.run_materials {
        destruction.push(DurableDestructionEntryV1::RunMaterial {
            ordinal: 0,
            material_id: material.material_id.clone(),
            destroyed_quantity: material.quantity,
            pre_material_quantity: material.quantity,
            pre_material_version: material.material_version,
            post_material_version: material
                .material_version
                .checked_add(1)
                .ok_or(DurableDeathPlanningError::VersionExhausted)?,
        });
    }
    destruction.sort_by(compare_canonical_durable_death_destruction_v1);
    for (index, entry) in destruction.iter_mut().enumerate() {
        let ordinal =
            u16::try_from(index).map_err(|_| DurableDeathPlanningError::InvalidCustodySnapshot)?;
        match entry {
            DurableDestructionEntryV1::Item {
                ordinal: target, ..
            }
            | DurableDestructionEntryV1::RunMaterial {
                ordinal: target, ..
            } => *target = ordinal,
        }
    }
    validate_durable_death_destruction_v1(&destruction)
        .map_err(|_| DurableDeathPlanningError::InvalidCustodySnapshot)?;
    Ok(destruction)
}

/// Binds simulation-authored death evidence to one complete, sealed persistence request.
///
/// This builder is pure. In particular, it neither accepts a client command nor commits a lethal
/// outcome. The caller must obtain `context` from authenticated live-server and account-locked
/// repository authority.
#[allow(
    clippy::too_many_lines,
    reason = "the audited DTO assembly remains contiguous so every authoritative field is visible"
)]
pub fn build_durable_death_commit(
    inputs: &AuthoritativeDeathInputs,
    context: &ServerAuthoredDeathContext,
    presentation: &CoreDevelopmentDeathView,
) -> Result<PreparedDurableDeathCommit, DurableDeathBuildError> {
    validate_server_authority(context)?;
    validate_simulation_evidence(inputs)?;
    validate_terminal_evidence(inputs, &context.terminal_trace)?;
    let destruction = plan_durable_death_destruction(
        context.mutation.death_id,
        context.mutation.mutation_id,
        &context.custody,
    )?;
    validate_presentation_content(inputs, context, &destruction, presentation)?;

    let trace = map_trace(inputs, context.terminal_trace.entity_identities())?;
    let trace_digest = canonical_digest(DURABLE_TRACE_DIGEST_CONTEXT, &trace)?;
    let destruction_digest = canonical_digest(DURABLE_DESTRUCTION_DIGEST_CONTEXT, &destruction)?;
    let lethal = trace
        .last()
        .ok_or(DurableDeathBuildError::EvidenceMismatch(
            "the ordered trace is empty",
        ))?;
    let death_tick = lethal.event_tick;

    let echo = build_echo(inputs, context)?;
    let echo_outcome = echo
        .as_ref()
        .map_or(DurableEchoOutcomeV1::NotEligible, |projection| {
            if projection.created.state == DurableEchoStateV1::Available {
                DurableEchoOutcomeV1::Available
            } else {
                DurableEchoOutcomeV1::Dormant
            }
        });

    let event = DurableDeathEventV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        death_id: context.mutation.death_id,
        account_id: context.mutation.authenticated_account.account_id.as_bytes(),
        character_id: context.mutation.selected_character_id,
        former_roster_ordinal: context.mutation.former_roster_ordinal,
        mutation_id: context.mutation.mutation_id,
        bargain_cleanup_event_id: derive_durable_death_bargain_cleanup_event_id(
            context.mutation.death_id,
            context.mutation.mutation_id,
        ),
        canonical_request_hash: [1; 32],
        content_revision: context.content.content_revision.clone(),
        records_blake3: context.content.records_blake3.clone(),
        assets_blake3: context.content.assets_blake3.clone(),
        localization_blake3: context.content.localization_blake3.clone(),
        presentation: DurableDeathPresentationAuthorityV1 {
            records_blake3: presentation.hashes().records_blake3.clone(),
            assets_blake3: presentation.hashes().assets_blake3.clone(),
            localization_blake3: presentation.hashes().localization_blake3.clone(),
        },
        instance_id: context.world.instance_id,
        lineage_id: context.world.lineage_id,
        restore_point_id: context.world.restore_point_id,
        region_id: context.world.region_id.clone(),
        room_id: context.world.room_id.clone(),
        provenance: map_provenance(context.world.lineage_state),
        death_tick,
        committed_at_unix_ms: context.mutation.accepted_at_unix_ms,
        cause: map_cause(inputs.cause.kind),
        killer_content_id: lethal.source_content_id.clone(),
        killer_pattern_id: lethal.pattern_id.clone(),
        killer_attack_id: lethal.attack_id.clone(),
        raw_damage: lethal.raw_damage,
        final_damage: lethal.final_damage,
        damage_type: lethal.damage_type,
        pre_hit_health: lethal.pre_health,
        source_x_milli_tiles: lethal.source_x_milli_tiles,
        source_y_milli_tiles: lethal.source_y_milli_tiles,
        network_state: lethal.network_state,
        recall_state: lethal.recall_state,
        telemetry: context.telemetry.clone(),
        lifetime_ticks: inputs.clocks.lifetime_ticks,
        permadeath_combat_ticks: inputs.clocks.permadeath_combat_ticks,
        versions: context.versions.clone(),
        trace_entry_count: u16::try_from(trace.len()).map_err(|_| {
            DurableDeathBuildError::EvidenceMismatch("trace entry count exceeds durable bounds")
        })?,
        trace_digest,
        destruction_entry_count: u16::try_from(destruction.len()).map_err(|_| {
            DurableDeathBuildError::EvidenceMismatch(
                "destruction entry count exceeds durable bounds",
            )
        })?,
        destruction_digest,
    };

    let bargains = ordered_ids(&context.hero.bargain_ids)?;
    let mut summary = DurableDeathSummaryV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        death_id: context.mutation.death_id,
        summary_revision: DURABLE_DEATH_SUMMARY_REVISION,
        hero_label_key: context.hero.hero_label_key.clone(),
        character_name_snapshot: context.hero.character_name.clone(),
        class_id: context.hero.class_id.clone(),
        level: context.hero.level,
        oath_id: context.hero.oath_id.clone(),
        bargains,
        lifetime_ms: inputs.clocks.lifetime_ms,
        final_deed_id: inputs.final_deed.deed_id.clone(),
        lethal_trace_ordinal: lethal.ordinal,
        last_five_damage: last_five_references(trace.len())?,
        projections: summary_projections(&destruction),
        echo_outcome,
        content_revision: context.content.content_revision.clone(),
        snapshot_digest: [0; 32],
    };
    summary.snapshot_digest = summary.expected_snapshot_digest()?;

    let mut memorial = DurableMemorialRecordV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        death_id: context.mutation.death_id,
        account_id: context.mutation.authenticated_account.account_id.as_bytes(),
        death_at_unix_ms: context.mutation.accepted_at_unix_ms,
        summary_revision: DURABLE_DEATH_SUMMARY_REVISION,
        summary_snapshot_digest: summary.snapshot_digest,
        presentation_key: context.hero.memorial_presentation_key.clone(),
        presentation_digest: [0; 32],
    };
    memorial.presentation_digest = memorial.expected_presentation_digest()?;

    let plan = AuthoritativeDeathPlanV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        event,
        trace,
        destruction,
        summary,
        memorial,
        echo,
    };
    let request = DurableDeathCommitRequestV1::seal(plan, context.mutation.issued_at_unix_ms)?;
    if !context.content.matches_event(&request.plan.event) {
        return Err(PersistenceError::DurableDeathContentMismatch.into());
    }
    let promotion = DurableDeathTracePromotionV1::seal(
        &request,
        context.terminal_trace.request().clone(),
        context.terminal_trace.full_window(),
    )?;

    Ok(PreparedDurableDeathCommit {
        request,
        content: context.content.clone(),
        promotion,
    })
}

fn map_provenance(lineage: DeathLineageState) -> DurableDeathProvenanceV1 {
    match lineage {
        DeathLineageState::ActivePermadeath(DeathProvenance::OrdinaryGameplay) => {
            DurableDeathProvenanceV1::OrdinaryGameplay
        }
        DeathLineageState::ActivePermadeath(DeathProvenance::VerifiedServerIncident) => {
            DurableDeathProvenanceV1::VerifiedServerIncident
        }
        DeathLineageState::ActivePermadeath(DeathProvenance::AdministrativeAction) => {
            DurableDeathProvenanceV1::AdministrativeAction
        }
        DeathLineageState::ActiveNonPermadeath
        | DeathLineageState::Inactive
        | DeathLineageState::Superseded => {
            unreachable!("server authority validation rejects non-permadeath lineage states")
        }
    }
}

fn validate_presentation_content(
    inputs: &AuthoritativeDeathInputs,
    context: &ServerAuthoredDeathContext,
    destruction: &[DurableDestructionEntryV1],
    presentation: &CoreDevelopmentDeathView,
) -> Result<(), DurableDeathBuildError> {
    let hashes = presentation.hashes();
    let world = persistence::LiveDamageTraceContentAuthorityV1::core();
    let durable_presentation = DurableDeathPresentationAuthorityV1::core();
    if context.content.content_revision != presentation.item_content_revision()
        || context.content.records_blake3 != world.records_blake3
        || context.content.assets_blake3 != world.assets_blake3
        || context.content.localization_blake3 != world.localization_blake3
        || hashes.records_blake3 != durable_presentation.records_blake3
        || hashes.assets_blake3 != durable_presentation.assets_blake3
        || hashes.localization_blake3 != durable_presentation.localization_blake3
    {
        return Err(DurableDeathBuildError::PresentationContentMismatch(
            "revision binding",
        ));
    }
    if presentation
        .resolve_copy(
            CoreDeathViewCopyKind::HeroLabel,
            &context.hero.hero_label_key,
        )
        .is_none()
        || presentation.resolve_class(&context.hero.class_id).is_none()
        || context
            .hero
            .oath_id
            .as_deref()
            .is_some_and(|id| presentation.resolve_oath(id).is_none())
        || context
            .hero
            .bargain_ids
            .iter()
            .any(|id| presentation.resolve_bargain(id).is_none())
        || presentation
            .resolve_copy(
                CoreDeathViewCopyKind::MemorialPresentation,
                &context.hero.memorial_presentation_key,
            )
            .is_none()
    {
        return Err(DurableDeathBuildError::PresentationContentMismatch(
            "hero or Memorial snapshot",
        ));
    }
    if presentation
        .resolve_copy(CoreDeathViewCopyKind::Deed, &inputs.final_deed.deed_id)
        .is_none()
        || context.echo.as_ref().is_some_and(|echo| {
            echo.deed_tags.iter().any(|id| {
                presentation
                    .resolve_copy(CoreDeathViewCopyKind::Deed, id)
                    .is_none()
            })
        })
    {
        return Err(DurableDeathBuildError::PresentationContentMismatch(
            "deed snapshot",
        ));
    }
    for item in &context.content.enabled_items {
        if presentation.resolve_item(&item.template_id).is_none() {
            return Err(DurableDeathBuildError::PresentationContentMismatch(
                "enabled item",
            ));
        }
    }
    for entry in &inputs.trace {
        if presentation
            .resolve_source(&entry.source_content_id)
            .is_none()
            || presentation.resolve_attack(&entry.attack_id).is_none()
            || entry
                .pattern_id
                .as_deref()
                .is_some_and(|id| presentation.resolve_pattern(id).is_none())
            || entry
                .statuses
                .iter()
                .any(|status| presentation.resolve_status(&status.status_id).is_none())
        {
            return Err(DurableDeathBuildError::PresentationContentMismatch(
                "combat trace",
            ));
        }
    }
    for entry in destruction {
        let resolved = match entry {
            DurableDestructionEntryV1::Item { content_id, .. } => {
                presentation.resolve_item(content_id)
            }
            DurableDestructionEntryV1::RunMaterial { material_id, .. } => {
                presentation.resolve_copy(CoreDeathViewCopyKind::Material, material_id)
            }
        };
        if resolved.is_none() {
            return Err(DurableDeathBuildError::PresentationContentMismatch(
                "destruction projection",
            ));
        }
    }
    Ok(())
}

fn validate_terminal_evidence(
    inputs: &AuthoritativeDeathInputs,
    terminal: &PreparedTerminalLiveDamageTrace,
) -> Result<(), DurableDeathBuildError> {
    let rebuilt = terminal.aggregate().terminal_snapshot().map_err(|_| {
        DurableDeathBuildError::EvidenceMismatch("terminal aggregate is not lethal")
    })?;
    if terminal.terminal_snapshot() != &rebuilt
        || terminal.terminal_snapshot().trace != inputs.trace
        || terminal.terminal_snapshot().last_five != inputs.last_five
        || terminal.terminal_snapshot().cause != inputs.cause
        || terminal.terminal_snapshot().canonical_hash_blake3 != inputs.trace_digest
    {
        return Err(DurableDeathBuildError::EvidenceMismatch(
            "prepared live terminal does not match authoritative death inputs",
        ));
    }
    Ok(())
}

fn validate_server_authority(
    context: &ServerAuthoredDeathContext,
) -> Result<(), DurableDeathBuildError> {
    let authority_ids = [
        context.mutation.authenticated_account.account_id.as_bytes(),
        context.mutation.selected_character_id,
        context.world.instance_id,
        context.world.lineage_id,
        context.world.restore_point_id,
    ];
    if authority_ids.contains(&[0; 16])
        || context.mutation.authenticated_account.namespace != AuthenticatedNamespace::WipeableTest
        || !is_uuid_v7(context.mutation.mutation_id)
        || !is_uuid_v7(context.mutation.death_id)
        || context.mutation.issued_at_unix_ms == 0
        || context.mutation.accepted_at_unix_ms < context.mutation.issued_at_unix_ms
    {
        return Err(DurableDeathBuildError::InvalidIdentity);
    }
    if !matches!(
        context.world.lineage_state,
        DeathLineageState::ActivePermadeath(_)
    ) {
        return Err(DurableDeathBuildError::WorldAuthorityMismatch);
    }
    context.content.validate()?;
    for item in &context.custody.items {
        if context.content.item(&item.content_id).is_none() {
            return Err(PersistenceError::DurableDeathContentMismatch.into());
        }
    }
    Ok(())
}

fn validate_simulation_evidence(
    inputs: &AuthoritativeDeathInputs,
) -> Result<(), DurableDeathBuildError> {
    if !inputs.clocks.dead
        || inputs.clocks.danger_active
        || inputs.clocks.link_lost_ticks != 0
        || inputs.clocks.lifetime_ms
            != ticks_to_milliseconds(inputs.clocks.lifetime_ticks).map_err(|_| {
                DurableDeathBuildError::EvidenceMismatch("lifetime conversion overflowed")
            })?
        || inputs.clocks.echo_time_eligible
            != (inputs.clocks.permadeath_combat_ticks >= ECHO_COMBAT_ELIGIBILITY_TICKS)
    {
        return Err(DurableDeathBuildError::EvidenceMismatch(
            "terminal clock snapshot is not canonical",
        ));
    }

    let deed_tick = inputs.final_deed.achieved_tick.map(|tick| tick.0);
    if core_deed_en_us(&inputs.final_deed.deed_id).is_none()
        || (inputs.final_deed.deed_id == DEED_NONE_ID) != deed_tick.is_none()
    {
        return Err(DurableDeathBuildError::EvidenceMismatch(
            "final deed is not a canonical Core deed",
        ));
    }

    let aggregate = DamageTraceAggregate::from_checkpoint(DamageTraceCheckpointV1 {
        schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
        entries: inputs.trace.clone(),
    })
    .map_err(|_| {
        DurableDeathBuildError::EvidenceMismatch("trace checkpoint failed simulation validation")
    })?;
    let verified = aggregate.terminal_snapshot().map_err(|_| {
        DurableDeathBuildError::EvidenceMismatch("trace lacks one final lethal event")
    })?;
    if verified.trace != inputs.trace
        || verified.last_five != inputs.last_five
        || verified.cause != inputs.cause
        || verified.canonical_hash_blake3 != inputs.trace_digest
        || inputs
            .final_deed
            .achieved_tick
            .is_some_and(|tick| tick > verified.cause.lethal_entry.tick)
    {
        return Err(DurableDeathBuildError::EvidenceMismatch(
            "trace, cause, last-five, digest, or deed tick was altered",
        ));
    }
    Ok(())
}

fn map_trace(
    inputs: &AuthoritativeDeathInputs,
    identities: &DeathEntityIdentityAuthority,
) -> Result<Vec<DurableCombatTraceEntryV1>, DurableDeathBuildError> {
    inputs
        .trace
        .iter()
        .enumerate()
        .map(|(index, entry)| map_trace_entry(index, entry, identities))
        .collect()
}

fn map_trace_entry(
    index: usize,
    entry: &DamageTraceEntry,
    identities: &DeathEntityIdentityAuthority,
) -> Result<DurableCombatTraceEntryV1, DurableDeathBuildError> {
    let source_entity_id = entry
        .source_entity_id
        .map(|entity_id| {
            identities
                .by_sim_entity
                .get(&entity_id)
                .copied()
                .filter(|identity| *identity != [0; 16])
                .ok_or(DurableDeathBuildError::MissingEntityIdentity(
                    entity_id.get(),
                ))
        })
        .transpose()?;
    let statuses = entry
        .statuses
        .iter()
        .enumerate()
        .map(|(status_index, status)| {
            Ok(DurableTraceStatusV1 {
                ordinal: u8::try_from(status_index).map_err(|_| {
                    DurableDeathBuildError::EvidenceMismatch(
                        "trace status count exceeds durable bounds",
                    )
                })?,
                status_id: status.status_id.clone(),
                remaining_ticks: status.remaining_ticks,
                stack_count: status.stack_count,
            })
        })
        .collect::<Result<Vec<_>, DurableDeathBuildError>>()?;
    Ok(DurableCombatTraceEntryV1 {
        ordinal: u16::try_from(index).map_err(|_| {
            DurableDeathBuildError::EvidenceMismatch("trace entry count exceeds durable bounds")
        })?,
        event_tick: entry.tick.0,
        event_ordinal: entry.event_ordinal,
        source_content_id: entry.source_content_id.clone(),
        source_entity_id,
        pattern_id: entry.pattern_id.clone(),
        attack_id: entry.attack_id.clone(),
        raw_damage: entry.raw_damage,
        final_damage: entry.final_damage,
        damage_type: map_damage_type(entry.damage_type),
        pre_health: entry.pre_health,
        post_health: entry.post_health,
        source_x_milli_tiles: entry.source_x_milli_tiles,
        source_y_milli_tiles: entry.source_y_milli_tiles,
        network_state: map_network_state(entry.network_state),
        recall_state: map_recall_state(entry.recall_state),
        lethal: entry.lethal,
        statuses,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the immutable Echo snapshot and its two legal availability outcomes form one contract"
)]
fn build_echo(
    inputs: &AuthoritativeDeathInputs,
    context: &ServerAuthoredDeathContext,
) -> Result<Option<DurableEchoEnvelopeV1>, DurableDeathBuildError> {
    let eligible = context.hero.level >= 10
        && inputs.clocks.echo_time_eligible
        && inputs.echo_deed_eligible
        && context.world.lineage_state
            == DeathLineageState::ActivePermadeath(DeathProvenance::OrdinaryGameplay);
    let Some(material) = context.echo.as_ref() else {
        return if eligible {
            Err(DurableDeathBuildError::EchoEligibilityMismatch)
        } else {
            Ok(None)
        };
    };
    if !eligible || !is_uuid_v7(material.echo_id) {
        return Err(DurableDeathBuildError::EchoEligibilityMismatch);
    }

    let deed_tags = canonical_deed_tags(&material.deed_tags, &inputs.final_deed.deed_id)?;
    let (preexisting_available_echo_id, promotion, created_state) = match material.availability {
        EchoAvailabilityProjection::ExistingAvailable { echo_id } => {
            if !is_uuid_v7(echo_id) || echo_id == material.echo_id {
                return Err(DurableDeathBuildError::EchoEligibilityMismatch);
            }
            (Some(echo_id), None, DurableEchoStateV1::Dormant)
        }
        EchoAvailabilityProjection::PromoteOldestDormant {
            echo_id,
            echo_death_id,
            next_transition_ordinal,
        } => {
            if !is_uuid_v7(echo_id)
                || !is_uuid_v7(echo_death_id)
                || next_transition_ordinal == 0
                || (echo_id == material.echo_id
                    && (echo_death_id != context.mutation.death_id || next_transition_ordinal != 1))
                || (echo_id != material.echo_id && echo_death_id == context.mutation.death_id)
            {
                return Err(DurableDeathBuildError::EchoEligibilityMismatch);
            }
            let transition = DurableEchoTransitionV1 {
                echo_id,
                echo_death_id,
                ordinal: next_transition_ordinal,
                previous_state: Some(DurableEchoStateV1::Dormant),
                next_state: DurableEchoStateV1::Available,
                reason: DurableEchoTransitionReasonV1::OldestDormantPromotion,
                source_death_id: None,
                trigger_death_id: context.mutation.death_id,
                committed_at_unix_ms: context.mutation.accepted_at_unix_ms,
            };
            let state = if echo_id == material.echo_id {
                DurableEchoStateV1::Available
            } else {
                DurableEchoStateV1::Dormant
            };
            (None, Some(transition), state)
        }
    };

    let bargains = ordered_ids(&context.hero.bargain_ids)?;
    let lethal = &inputs.cause.lethal_entry;
    let mut created = DurableEchoRecordV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        echo_id: material.echo_id,
        death_id: context.mutation.death_id,
        account_id: context.mutation.authenticated_account.account_id.as_bytes(),
        character_name_snapshot: context.hero.character_name.clone(),
        class_id: context.hero.class_id.clone(),
        oath_id: context.hero.oath_id.clone(),
        level: context.hero.level,
        appearance_snapshot_id: material.appearance_snapshot_id.clone(),
        appearance_theme_id: material.appearance_theme_id.clone(),
        weapon_signature_tag: material.weapon_signature_tag.clone(),
        relic_signature_tag: material.relic_signature_tag.clone(),
        bargains,
        deed_tags,
        killer_content_id: lethal.source_content_id.clone(),
        killer_pattern_id: lethal.pattern_id.clone(),
        death_region_id: context.world.region_id.clone(),
        power_band: material.power_band,
        created_at_unix_ms: context.mutation.accepted_at_unix_ms,
        state: created_state,
        content_revision: context.content.content_revision.clone(),
        snapshot_digest: [0; 32],
    };
    created.snapshot_digest = created.expected_snapshot_digest()?;
    let creation_transition = DurableEchoTransitionV1 {
        echo_id: material.echo_id,
        echo_death_id: context.mutation.death_id,
        ordinal: 0,
        previous_state: None,
        next_state: DurableEchoStateV1::Dormant,
        reason: DurableEchoTransitionReasonV1::EligibleDeath,
        source_death_id: Some(context.mutation.death_id),
        trigger_death_id: context.mutation.death_id,
        committed_at_unix_ms: context.mutation.accepted_at_unix_ms,
    };
    Ok(Some(DurableEchoEnvelopeV1 {
        created,
        creation_transition,
        preexisting_available_echo_id,
        promotion,
    }))
}

fn canonical_deed_tags(
    tags: &[String],
    final_deed_id: &str,
) -> Result<Vec<DurableOrderedContentIdV1>, DurableDeathBuildError> {
    let unique: BTreeSet<&str> = tags.iter().map(String::as_str).collect();
    if unique.len() != tags.len()
        || final_deed_id == DEED_NONE_ID
        || !unique.contains(final_deed_id)
    {
        return Err(DurableDeathBuildError::EchoEligibilityMismatch);
    }
    ordered_ids(&unique.into_iter().map(str::to_owned).collect::<Vec<_>>())
}

fn ordered_ids(
    values: &[String],
) -> Result<Vec<DurableOrderedContentIdV1>, DurableDeathBuildError> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            Ok(DurableOrderedContentIdV1 {
                ordinal: u16::try_from(index).map_err(|_| {
                    DurableDeathBuildError::EvidenceMismatch(
                        "ordered content count exceeds durable bounds",
                    )
                })?,
                content_id: value.clone(),
            })
        })
        .collect()
}

fn summary_projections(destruction: &[DurableDestructionEntryV1]) -> DurableSummaryProjectionsV1 {
    let lost = destruction
        .iter()
        .enumerate()
        .map(|(index, entry)| match entry {
            DurableDestructionEntryV1::Item {
                content_id,
                item_uid,
                ..
            } => DurableSummaryProjectionEntryV1 {
                ordinal: u16::try_from(index).unwrap_or(u16::MAX),
                kind: DurableSummaryProjectionKindV1::LostItem,
                content_id: content_id.clone(),
                quantity: 1,
                item_uid: Some(*item_uid),
            },
            DurableDestructionEntryV1::RunMaterial {
                material_id,
                destroyed_quantity,
                ..
            } => DurableSummaryProjectionEntryV1 {
                ordinal: u16::try_from(index).unwrap_or(u16::MAX),
                kind: DurableSummaryProjectionKindV1::LostRunMaterial,
                content_id: material_id.clone(),
                quantity: *destroyed_quantity,
                item_uid: None,
            },
        })
        .collect();
    DurableSummaryProjectionsV1 {
        lost,
        preserved: fixed_projections(&[
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
        ]),
        created: fixed_projections(&[
            (
                DurableSummaryProjectionKindV1::CreatedMemorial,
                "projection.created.memorial",
            ),
            (
                DurableSummaryProjectionKindV1::CreatedEcho,
                "projection.created.echo",
            ),
        ]),
    }
}

fn fixed_projections(
    definitions: &[(DurableSummaryProjectionKindV1, &str)],
) -> Vec<DurableSummaryProjectionEntryV1> {
    definitions
        .iter()
        .enumerate()
        .map(
            |(index, (kind, content_id))| DurableSummaryProjectionEntryV1 {
                ordinal: u16::try_from(index).unwrap_or(u16::MAX),
                kind: *kind,
                content_id: (*content_id).into(),
                quantity: 1,
                item_uid: None,
            },
        )
        .collect()
}

fn last_five_references(
    trace_len: usize,
) -> Result<Vec<DurableSummaryDamageReferenceV1>, DurableDeathBuildError> {
    let start = trace_len.saturating_sub(5);
    (start..trace_len)
        .enumerate()
        .map(|(index, trace_index)| {
            Ok(DurableSummaryDamageReferenceV1 {
                ordinal: u8::try_from(index).map_err(|_| {
                    DurableDeathBuildError::EvidenceMismatch("last-five ordinal overflowed")
                })?,
                trace_ordinal: u16::try_from(trace_index).map_err(|_| {
                    DurableDeathBuildError::EvidenceMismatch("trace ordinal overflowed")
                })?,
            })
        })
        .collect()
}

fn canonical_digest<T: serde::Serialize>(
    context: &str,
    value: &T,
) -> Result<[u8; 32], DurableDeathBuildError> {
    let bytes =
        postcard::to_stdvec(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)?;
    Ok(blake3::derive_key(context, &bytes))
}

const fn map_cause(value: AuthoritativeDeathCauseKind) -> DurableDeathCauseV1 {
    match value {
        AuthoritativeDeathCauseKind::DirectHit => DurableDeathCauseV1::DirectHit,
        AuthoritativeDeathCauseKind::DamageOverTime => DurableDeathCauseV1::DamageOverTime,
        AuthoritativeDeathCauseKind::Environment => DurableDeathCauseV1::Environment,
        AuthoritativeDeathCauseKind::Disconnect => DurableDeathCauseV1::Disconnect,
    }
}

const fn map_damage_type(value: DamageType) -> DurableDamageTypeV1 {
    match value {
        DamageType::Physical => DurableDamageTypeV1::Physical,
        DamageType::Veil => DurableDamageTypeV1::Veil,
    }
}

const fn map_network_state(value: DeathTraceNetworkState) -> DurableNetworkStateV1 {
    match value {
        DeathTraceNetworkState::Connected => DurableNetworkStateV1::Connected,
        DeathTraceNetworkState::Degraded => DurableNetworkStateV1::Degraded,
        DeathTraceNetworkState::LinkLost => DurableNetworkStateV1::LinkLost,
        DeathTraceNetworkState::Reattached => DurableNetworkStateV1::Reattached,
    }
}

const fn map_recall_state(value: DeathTraceRecallState) -> DurableRecallStateV1 {
    match value {
        DeathTraceRecallState::Inactive => DurableRecallStateV1::Inactive,
        DeathTraceRecallState::Channeling => DurableRecallStateV1::Channeling,
        DeathTraceRecallState::CompletionPending => DurableRecallStateV1::CompletionPending,
    }
}

fn is_uuid_v7(value: [u8; 16]) -> bool {
    value != [0; 16] && value[6] >> 4 == 7 && value[8] & 0b1100_0000 == 0b1000_0000
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use persistence::{
        DeathVersionAdvanceV1, DurableDeathItemContentAuthorityV1, DurableDestructionLocationV1,
        DurableEquipmentSlotV1, LiveDamageTraceCauseV1, LiveDamageTraceContentAuthorityV1,
        LiveDamageTraceDamageTypeV1, LiveDamageTraceDangerAuthorityV1, LiveDamageTraceEntryV1,
        LiveDamageTraceHeadV1, LiveDamageTraceNetworkStateV1, LiveDamageTraceRecallStateV1,
        LiveDamageTraceStatusV1, LiveDamageTraceTickCommandV1, LiveDamageTraceTickRequestV1,
        StoredLiveDamageTraceSnapshotEntryV1,
    };
    use sim_core::{
        AuthoritativeDeathCause, DeathClockSnapshot, DeathTraceStatus, FinalDeed, Tick,
    };

    fn uuid_v7(seed: u8) -> [u8; 16] {
        let mut value = [seed; 16];
        value[6] = 0x70 | (seed & 0x0f);
        value[8] = 0x80 | (seed & 0x3f);
        value
    }

    fn presentation() -> CoreDevelopmentDeathView {
        sim_content::load_core_development_death_view(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"),
        )
        .expect("valid Core death-presentation content")
    }

    fn build_test_commit(
        inputs: &AuthoritativeDeathInputs,
        context: &ServerAuthoredDeathContext,
    ) -> Result<PreparedDurableDeathCommit, DurableDeathBuildError> {
        build_durable_death_commit(inputs, context, &presentation())
    }

    fn validate_test_presentation(
        inputs: &AuthoritativeDeathInputs,
        context: &ServerAuthoredDeathContext,
        presentation: &CoreDevelopmentDeathView,
    ) -> Result<(), DurableDeathBuildError> {
        let destruction = plan_durable_death_destruction(
            context.mutation.death_id,
            context.mutation.mutation_id,
            &context.custody,
        )?;
        validate_presentation_content(inputs, context, &destruction, presentation)
    }

    fn trace_entry(
        tick: u64,
        event_ordinal: u32,
        pre_health: u32,
        final_damage: u32,
        lethal: bool,
    ) -> DamageTraceEntry {
        DamageTraceEntry {
            tick: Tick(tick),
            event_ordinal,
            cause_kind: AuthoritativeDeathCauseKind::DirectHit,
            source_content_id: "boss.sir_caldus".into(),
            source_entity_id: Some(EntityId::new(41).unwrap()),
            pattern_id: Some("boss.caldus.bell_ring".into()),
            attack_id: "boss.caldus.bell_ring".into(),
            raw_damage: final_damage,
            final_damage,
            damage_type: DamageType::Veil,
            pre_health,
            post_health: pre_health.saturating_sub(final_damage),
            source_x_milli_tiles: 1_250,
            source_y_milli_tiles: -750,
            statuses: vec![DeathTraceStatus {
                status_id: "status.frostbind".into(),
                remaining_ticks: 30,
                stack_count: 1,
            }],
            network_state: DeathTraceNetworkState::Degraded,
            recall_state: DeathTraceRecallState::Channeling,
            lethal,
        }
    }

    fn inputs() -> AuthoritativeDeathInputs {
        let trace = vec![
            trace_entry(995, 0, 30, 5, false),
            trace_entry(996, 0, 25, 5, false),
            trace_entry(997, 0, 20, 5, false),
            trace_entry(998, 0, 15, 5, false),
            trace_entry(999, 0, 10, 5, false),
            trace_entry(1_000, 0, 5, 5, true),
        ];
        let verified = DamageTraceAggregate::from_checkpoint(DamageTraceCheckpointV1 {
            schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
            entries: trace,
        })
        .unwrap()
        .terminal_snapshot()
        .unwrap();
        AuthoritativeDeathInputs {
            clocks: DeathClockSnapshot {
                lifetime_ticks: 17_000,
                lifetime_ms: ticks_to_milliseconds(17_000).unwrap(),
                permadeath_combat_ticks: 18_000,
                echo_time_eligible: true,
                danger_active: false,
                link_lost_ticks: 0,
                dead: true,
            },
            final_deed: FinalDeed {
                deed_id: "deed.core.sir_caldus_defeated".into(),
                achieved_tick: Some(Tick(900)),
            },
            echo_deed_eligible: true,
            cause: verified.cause,
            trace: verified.trace,
            last_five: verified.last_five,
            trace_digest: verified.canonical_hash_blake3,
        }
    }

    fn versions() -> DeathAggregateVersionsV1 {
        let advance = DeathVersionAdvanceV1 { pre: 4, post: 5 };
        DeathAggregateVersionsV1 {
            account: advance,
            character: advance,
            progression: advance,
            inventory: advance,
            oath_bargain: advance,
            life_metrics: advance,
        }
    }

    fn live_trace_tick_id(tick: u64) -> [u8; 16] {
        [u8::try_from(tick % 251 + 1).unwrap(); 16]
    }

    fn live_trace_entry(entry: &DamageTraceEntry) -> LiveDamageTraceEntryV1 {
        LiveDamageTraceEntryV1 {
            event_ordinal: entry.event_ordinal,
            cause: LiveDamageTraceCauseV1::DirectHit,
            source_content_id: entry.source_content_id.clone(),
            source_entity_id: entry.source_entity_id.map(|_| [10; 16]),
            source_sim_entity_id: entry.source_entity_id.map(EntityId::get),
            pattern_id: entry.pattern_id.clone(),
            attack_id: entry.attack_id.clone(),
            raw_damage: entry.raw_damage,
            final_damage: entry.final_damage,
            damage_type: LiveDamageTraceDamageTypeV1::Veil,
            pre_health: entry.pre_health,
            post_health: entry.post_health,
            source_x_milli_tiles: entry.source_x_milli_tiles,
            source_y_milli_tiles: entry.source_y_milli_tiles,
            network_state: LiveDamageTraceNetworkStateV1::Degraded,
            recall_state: LiveDamageTraceRecallStateV1::Channeling,
            lethal: entry.lethal,
            statuses: entry
                .statuses
                .iter()
                .enumerate()
                .map(|(ordinal, status)| LiveDamageTraceStatusV1 {
                    status_ordinal: u8::try_from(ordinal).unwrap(),
                    status_id: status.status_id.clone(),
                    remaining_ticks: status.remaining_ticks,
                    stack_count: status.stack_count,
                })
                .collect(),
        }
    }

    fn terminal_trace_fixture(
        inputs: &AuthoritativeDeathInputs,
        content: LiveDamageTraceContentAuthorityV1,
    ) -> PreparedTerminalLiveDamageTrace {
        let full_window = inputs
            .trace
            .iter()
            .map(|entry| StoredLiveDamageTraceSnapshotEntryV1 {
                trace_tick_id: live_trace_tick_id(entry.tick.0),
                event_tick: entry.tick.0,
                entry: live_trace_entry(entry),
            })
            .collect::<Vec<_>>();
        let lethal = full_window.last().unwrap();
        let previous = &full_window[full_window.len() - 2];
        let request = LiveDamageTraceTickRequestV1::seal(LiveDamageTraceTickCommandV1 {
            account_id: [1; 16],
            character_id: [2; 16],
            trace_tick_id: lethal.trace_tick_id,
            expected_character_version: versions().character.pre,
            expected_previous: Some(LiveDamageTraceHeadV1 {
                trace_tick_id: previous.trace_tick_id,
                event_tick: previous.event_tick,
                result_digest: [90; 32],
            }),
            event_tick: lethal.event_tick,
            danger: LiveDamageTraceDangerAuthorityV1 {
                lineage_id: [6; 16],
                restore_point_id: [7; 16],
                checkpoint_tick: 900,
            },
            content,
            entries: vec![lethal.entry.clone()],
            issued_at_unix_ms: 1_800,
        })
        .unwrap();
        let aggregate = DamageTraceAggregate::from_checkpoint(DamageTraceCheckpointV1 {
            schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
            entries: inputs.trace.clone(),
        })
        .unwrap();
        PreparedTerminalLiveDamageTrace::from_test_authority(
            request,
            aggregate,
            full_window,
            DeathEntityIdentityAuthority {
                by_sim_entity: BTreeMap::from([(EntityId::new(41).unwrap(), [10; 16])]),
            },
        )
        .unwrap()
    }

    fn context() -> ServerAuthoredDeathContext {
        let content_revision = persistence::CORE_ITEM_CONTENT_REVISION.to_owned();
        let live_content = LiveDamageTraceContentAuthorityV1::core();
        let terminal_trace = terminal_trace_fixture(&inputs(), live_content.clone());
        ServerAuthoredDeathContext {
            mutation: DeathMutationAuthority {
                authenticated_account: AuthenticatedAccount {
                    account_id: crate::identity::AccountId::new([1; 16]).unwrap(),
                    namespace: AuthenticatedNamespace::WipeableTest,
                },
                selected_character_id: [2; 16],
                former_roster_ordinal: 1,
                mutation_id: uuid_v7(3),
                death_id: uuid_v7(4),
                issued_at_unix_ms: 1_900,
                accepted_at_unix_ms: 2_000,
            },
            world: DeathWorldAuthority {
                instance_id: [5; 16],
                lineage_id: [6; 16],
                restore_point_id: [7; 16],
                region_id: "region.core.sunken_march".into(),
                room_id: "room.core.caldus_arena".into(),
                lineage_state: DeathLineageState::ActivePermadeath(
                    DeathProvenance::OrdinaryGameplay,
                ),
            },
            content: DurableDeathContentAuthorityV1 {
                content_revision,
                records_blake3: live_content.records_blake3,
                assets_blake3: live_content.assets_blake3,
                localization_blake3: live_content.localization_blake3,
                enabled_items: vec![DurableDeathItemContentAuthorityV1 {
                    template_id: "item.weapon.crossbow.pine_crossbow".into(),
                    echo_signature_tag: Some("signature.weapon.bow".into()),
                }],
            },
            versions: versions(),
            custody: DeathCustodySnapshot {
                items: vec![DeathAtRiskItem {
                    content_id: "item.weapon.crossbow.pine_crossbow".into(),
                    item_uid: [8; 16],
                    location: DurableDestructionLocationV1::Equipment {
                        slot: DurableEquipmentSlotV1::Weapon,
                    },
                    item_version: 7,
                }],
                run_materials: vec![],
            },
            hero: DeathHeroSnapshot {
                hero_label_key: "hero.core.grave_arbalist".into(),
                character_name: "Mara".into(),
                class_id: "class.grave_arbalist".into(),
                level: 10,
                oath_id: Some("oath.arbalist.long_vigil".into()),
                bargain_ids: vec!["bargain.cinder_hunger".into()],
                memorial_presentation_key: "memorial.presentation.core_default".into(),
            },
            terminal_trace,
            telemetry: persistence::DurableDeathTelemetryContextV1::Observed {
                schema_version: persistence::DURABLE_DEATH_TELEMETRY_CONTEXT_SCHEMA_VERSION,
                party_size: 1,
                boss_phase_id: Some("boss.caldus.phase_1".into()),
                contribution: Some(persistence::DurableDeathContributionV1 {
                    contribution_centi_units: 250_000,
                    reference_health: 7_200,
                }),
                network_health: persistence::DurableDeathNetworkHealthV1 {
                    transport_generation: 1,
                    sampled_at_unix_ms: 1_800,
                    ping_millis: 80,
                    jitter_millis: 12,
                    loss_basis_points: 100,
                    correction_count: None,
                },
            },
            echo: Some(EligibleEchoProjection {
                echo_id: uuid_v7(11),
                appearance_snapshot_id: persistence::CORE_ECHO_BASE_SILHOUETTE_ID.into(),
                appearance_theme_id: persistence::CORE_ECHO_PRESENTATION_PLACEHOLDER_ID.into(),
                weapon_signature_tag: Some("signature.weapon.bow".into()),
                relic_signature_tag: None,
                deed_tags: vec!["deed.core.sir_caldus_defeated".into()],
                power_band: 2,
                availability: EchoAvailabilityProjection::PromoteOldestDormant {
                    echo_id: uuid_v7(11),
                    echo_death_id: uuid_v7(4),
                    next_transition_ordinal: 1,
                },
            }),
        }
    }

    pub(crate) fn prepared_commit() -> PreparedDurableDeathCommit {
        build_test_commit(&inputs(), &context()).expect("canonical death fixture")
    }

    fn at_risk_item(
        uid: u8,
        location: DurableDestructionLocationV1,
        version: u64,
    ) -> DeathAtRiskItem {
        DeathAtRiskItem {
            content_id: if matches!(location, DurableDestructionLocationV1::Belt { .. }) {
                "consumable.red_tonic".into()
            } else {
                "item.weapon.crossbow.pine_crossbow".into()
            },
            item_uid: [uid; 16],
            location,
            item_version: version,
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the exhaustive custody-family matrix remains contiguous for order review"
    )]
    fn destruction_planner_covers_every_custody_family_in_exact_order() {
        let death_id = uuid_v7(21);
        let mutation_id = uuid_v7(22);
        let mut items = vec![
            at_risk_item(
                90,
                DurableDestructionLocationV1::PersonalGround {
                    instance_id: [4; 16],
                    pickup_id: [9; 16],
                },
                3,
            ),
            at_risk_item(
                91,
                DurableDestructionLocationV1::RunBackpack { index: 7 },
                2,
            ),
            at_risk_item(
                10,
                DurableDestructionLocationV1::Equipment {
                    slot: DurableEquipmentSlotV1::Weapon,
                },
                4,
            ),
            at_risk_item(
                13,
                DurableDestructionLocationV1::Equipment {
                    slot: DurableEquipmentSlotV1::Charm,
                },
                4,
            ),
            at_risk_item(
                12,
                DurableDestructionLocationV1::Equipment {
                    slot: DurableEquipmentSlotV1::Armor,
                },
                4,
            ),
            at_risk_item(
                11,
                DurableDestructionLocationV1::Equipment {
                    slot: DurableEquipmentSlotV1::Relic,
                },
                4,
            ),
        ];
        for slot in 0_u8..=1 {
            for unit in (0_u8..6).rev() {
                items.push(at_risk_item(
                    20 + slot * 10 + unit,
                    DurableDestructionLocationV1::Belt { index: slot },
                    5,
                ));
            }
        }
        for slot in (0_u8..8).rev() {
            for unit in (0_u8..6).rev() {
                items.push(at_risk_item(
                    40 + slot * 6 + unit,
                    DurableDestructionLocationV1::RunBackpack { index: slot },
                    6,
                ));
            }
        }
        items.push(at_risk_item(
            89,
            DurableDestructionLocationV1::PersonalGround {
                instance_id: [4; 16],
                pickup_id: [9; 16],
            },
            3,
        ));
        items.push(at_risk_item(
            88,
            DurableDestructionLocationV1::PersonalGround {
                instance_id: [3; 16],
                pickup_id: [9; 16],
            },
            3,
        ));
        let custody = DeathCustodySnapshot {
            items,
            run_materials: vec![
                DeathAtRiskRunMaterial {
                    material_id: "material.saltglass_shard".into(),
                    quantity: 9,
                    material_version: 2,
                },
                DeathAtRiskRunMaterial {
                    material_id: "material.bell_brass".into(),
                    quantity: 7,
                    material_version: 4,
                },
                DeathAtRiskRunMaterial {
                    material_id: "material.funeral_root".into(),
                    quantity: 5,
                    material_version: 3,
                },
            ],
        };

        let planned = plan_durable_death_destruction(death_id, mutation_id, &custody).unwrap();
        assert_eq!(
            planned.len(),
            custody.items.len() + custody.run_materials.len()
        );
        assert!(
            planned
                .iter()
                .enumerate()
                .all(|(index, entry)| { entry.ordinal() == u16::try_from(index).unwrap() })
        );
        assert!(planned.windows(2).all(|pair| {
            compare_canonical_durable_death_destruction_v1(&pair[0], &pair[1])
                == std::cmp::Ordering::Less
        }));
        assert_eq!(
            planned
                .iter()
                .filter(|entry| matches!(entry, DurableDestructionEntryV1::Item { .. }))
                .count(),
            custody.items.len()
        );
        assert_eq!(
            planned
                .iter()
                .filter_map(|entry| match entry {
                    DurableDestructionEntryV1::RunMaterial {
                        destroyed_quantity, ..
                    } => Some(*destroyed_quantity),
                    DurableDestructionEntryV1::Item { .. } => None,
                })
                .sum::<u32>(),
            custody
                .run_materials
                .iter()
                .map(|material| material.quantity)
                .sum::<u32>()
        );
        for entry in &planned {
            if let DurableDestructionEntryV1::Item {
                item_uid,
                pre_item_version,
                post_item_version,
                ledger_event_id,
                ..
            } = entry
            {
                assert_eq!(post_item_version, &(pre_item_version + 1));
                assert_eq!(
                    *ledger_event_id,
                    derive_durable_death_item_ledger_event_id(death_id, mutation_id, *item_uid)
                );
            }
        }
    }

    #[test]
    fn destruction_planner_is_permutation_stable_and_fails_closed() {
        let death_id = uuid_v7(31);
        let mutation_id = uuid_v7(32);
        let canonical = DeathCustodySnapshot {
            items: vec![
                at_risk_item(1, DurableDestructionLocationV1::Belt { index: 0 }, 1),
                at_risk_item(2, DurableDestructionLocationV1::RunBackpack { index: 0 }, 2),
            ],
            run_materials: vec![DeathAtRiskRunMaterial {
                material_id: "material.bell_brass".into(),
                quantity: 2,
                material_version: 3,
            }],
        };
        let mut reversed = canonical.clone();
        reversed.items.reverse();
        reversed.run_materials.reverse();
        assert_eq!(
            plan_durable_death_destruction(death_id, mutation_id, &canonical).unwrap(),
            plan_durable_death_destruction(death_id, mutation_id, &reversed).unwrap()
        );

        let mut duplicate_item = canonical.clone();
        duplicate_item.items.push(duplicate_item.items[0].clone());
        assert_eq!(
            plan_durable_death_destruction(death_id, mutation_id, &duplicate_item),
            Err(DurableDeathPlanningError::InvalidCustodySnapshot)
        );

        let mut duplicate_material = canonical.clone();
        duplicate_material
            .run_materials
            .push(duplicate_material.run_materials[0].clone());
        assert_eq!(
            plan_durable_death_destruction(death_id, mutation_id, &duplicate_material),
            Err(DurableDeathPlanningError::InvalidCustodySnapshot)
        );

        let mut exhausted = canonical.clone();
        exhausted.items[0].item_version = u64::MAX;
        assert_eq!(
            plan_durable_death_destruction(death_id, mutation_id, &exhausted),
            Err(DurableDeathPlanningError::VersionExhausted)
        );
        assert_eq!(
            plan_durable_death_destruction([0; 16], mutation_id, &canonical),
            Err(DurableDeathPlanningError::InvalidAuthority)
        );
    }

    #[test]
    fn maps_authoritative_evidence_and_server_identity_exactly() {
        let inputs = inputs();
        let context = context();
        let prepared = build_test_commit(&inputs, &context).unwrap();
        prepared.request.validate().unwrap();
        let plan = &prepared.request.plan;

        assert_eq!(
            plan.event.account_id,
            context.mutation.authenticated_account.account_id.as_bytes()
        );
        assert_eq!(
            plan.event.character_id,
            context.mutation.selected_character_id
        );
        assert_eq!(plan.event.death_id, context.mutation.death_id);
        assert_eq!(plan.event.mutation_id, context.mutation.mutation_id);
        assert_eq!(plan.event.instance_id, context.world.instance_id);
        assert_eq!(plan.event.lineage_id, context.world.lineage_id);
        assert_eq!(plan.event.restore_point_id, context.world.restore_point_id);
        assert_eq!(plan.event.death_tick, 1_000);
        assert_eq!(plan.event.cause, DurableDeathCauseV1::DirectHit);
        assert_eq!(plan.event.killer_content_id, "boss.sir_caldus");
        assert_eq!(plan.event.killer_attack_id, "boss.caldus.bell_ring");
        assert_eq!(plan.trace[0].source_entity_id, Some([10; 16]));
        assert_eq!(plan.trace[0].statuses[0].ordinal, 0);
        assert_eq!(plan.summary.final_deed_id, inputs.final_deed.deed_id);
        assert_eq!(plan.summary.last_five_damage.len(), 5);
        assert_eq!(plan.summary.last_five_damage[0].trace_ordinal, 1);
        assert_eq!(plan.summary.last_five_damage[4].trace_ordinal, 5);
        assert_eq!(plan.summary.projections.lost[0].item_uid, Some([8; 16]));
        assert_eq!(plan.summary.echo_outcome, DurableEchoOutcomeV1::Available);
        assert_eq!(
            plan.echo.as_ref().unwrap().created.state,
            DurableEchoStateV1::Available
        );
    }

    #[test]
    fn lifetime_and_combat_clocks_remain_independent() {
        let prepared = build_test_commit(&inputs(), &context()).unwrap();
        assert_eq!(prepared.request.plan.event.lifetime_ticks, 17_000);
        assert_eq!(prepared.request.plan.event.permadeath_combat_ticks, 18_000);
        assert!(
            prepared.request.plan.event.permadeath_combat_ticks
                > prepared.request.plan.event.lifetime_ticks
        );
    }

    #[test]
    fn changed_trace_cause_last_five_and_digest_fail_closed() {
        let mut changed_trace = inputs();
        changed_trace.trace[0].raw_damage += 1;
        assert!(matches!(
            build_test_commit(&changed_trace, &context()),
            Err(DurableDeathBuildError::EvidenceMismatch(_))
        ));

        let mut changed_cause = inputs();
        changed_cause.cause = AuthoritativeDeathCause {
            kind: AuthoritativeDeathCauseKind::Environment,
            lethal_entry: changed_cause.cause.lethal_entry.clone(),
        };
        assert!(matches!(
            build_test_commit(&changed_cause, &context()),
            Err(DurableDeathBuildError::EvidenceMismatch(_))
        ));

        let mut changed_last_five = inputs();
        changed_last_five.last_five.remove(0);
        assert!(matches!(
            build_test_commit(&changed_last_five, &context()),
            Err(DurableDeathBuildError::EvidenceMismatch(_))
        ));

        let mut changed_digest = inputs();
        changed_digest.trace_digest[0] ^= 1;
        assert!(matches!(
            build_test_commit(&changed_digest, &context()),
            Err(DurableDeathBuildError::EvidenceMismatch(_))
        ));
    }

    #[test]
    fn identity_content_and_entity_mismatches_fail_closed() {
        let mut wrong_mutation = context();
        wrong_mutation.mutation.mutation_id[6] = 0x40;
        assert!(matches!(
            build_test_commit(&inputs(), &wrong_mutation),
            Err(DurableDeathBuildError::InvalidIdentity)
        ));

        let mut wrong_content = context();
        wrong_content.content.enabled_items.clear();
        assert!(matches!(
            build_test_commit(&inputs(), &wrong_content),
            Err(DurableDeathBuildError::Persistence(
                PersistenceError::DurableDeathContentMismatch
            ))
        ));

        let mut missing_entity = context();
        missing_entity.terminal_trace = PreparedTerminalLiveDamageTrace::from_test_authority(
            missing_entity.terminal_trace.request().clone(),
            missing_entity.terminal_trace.aggregate().clone(),
            missing_entity.terminal_trace.full_window().to_vec(),
            DeathEntityIdentityAuthority::default(),
        )
        .unwrap();
        assert!(matches!(
            build_test_commit(&inputs(), &missing_entity),
            Err(DurableDeathBuildError::MissingEntityIdentity(41))
        ));
    }

    #[test]
    fn echo_material_is_required_exactly_at_the_combined_eligibility_boundary() {
        let mut missing = context();
        missing.echo = None;
        assert!(matches!(
            build_test_commit(&inputs(), &missing),
            Err(DurableDeathBuildError::EchoEligibilityMismatch)
        ));

        let mut ineligible_inputs = inputs();
        ineligible_inputs.clocks.permadeath_combat_ticks = 17_999;
        ineligible_inputs.clocks.echo_time_eligible = false;
        assert!(matches!(
            build_test_commit(&ineligible_inputs, &context()),
            Err(DurableDeathBuildError::EchoEligibilityMismatch)
        ));

        let mut ineligible_context = context();
        ineligible_context.echo = None;
        let prepared = build_test_commit(&ineligible_inputs, &ineligible_context).unwrap();
        assert!(prepared.request.plan.echo.is_none());
        assert_eq!(
            prepared.request.plan.summary.echo_outcome,
            DurableEchoOutcomeV1::NotEligible
        );

        let mut level_nine = context();
        level_nine.hero.level = 9;
        level_nine.echo = None;
        let prepared = build_test_commit(&inputs(), &level_nine).unwrap();
        assert!(prepared.request.plan.echo.is_none());

        let level_ten = context();
        assert_eq!(level_ten.hero.level, 10);
        assert!(
            build_test_commit(&inputs(), &level_ten)
                .unwrap()
                .request
                .plan
                .echo
                .is_some()
        );
    }

    #[test]
    fn incident_and_administrative_provenance_never_create_echoes() {
        for provenance in [
            DeathProvenance::VerifiedServerIncident,
            DeathProvenance::AdministrativeAction,
        ] {
            let mut with_echo = context();
            with_echo.world.lineage_state = DeathLineageState::ActivePermadeath(provenance);
            assert!(matches!(
                build_test_commit(&inputs(), &with_echo),
                Err(DurableDeathBuildError::EchoEligibilityMismatch)
            ));

            let mut without_echo = with_echo;
            without_echo.echo = None;
            let prepared = build_test_commit(&inputs(), &without_echo).unwrap();
            assert!(prepared.request.plan.echo.is_none());
            assert_eq!(
                prepared.request.plan.event.provenance,
                match provenance {
                    DeathProvenance::VerifiedServerIncident => {
                        DurableDeathProvenanceV1::VerifiedServerIncident
                    }
                    DeathProvenance::AdministrativeAction => {
                        DurableDeathProvenanceV1::AdministrativeAction
                    }
                    DeathProvenance::OrdinaryGameplay => unreachable!(),
                }
            );
            assert_eq!(
                prepared.request.plan.summary.echo_outcome,
                DurableEchoOutcomeV1::NotEligible
            );
        }
    }

    #[test]
    fn practice_inactive_and_superseded_lineages_fail_closed() {
        for lineage_state in [
            DeathLineageState::ActiveNonPermadeath,
            DeathLineageState::Inactive,
            DeathLineageState::Superseded,
        ] {
            let mut context = context();
            context.world.lineage_state = lineage_state;
            assert!(matches!(
                build_test_commit(&inputs(), &context),
                Err(DurableDeathBuildError::WorldAuthorityMismatch)
            ));
        }
    }

    #[test]
    fn echo_deed_tags_and_oldest_first_projection_are_canonical() {
        let mut context = context();
        let echo = context.echo.as_mut().unwrap();
        echo.deed_tags = vec![
            "deed.core.sir_caldus_defeated".into(),
            "deed.core.sepulcher_knight_defeated".into(),
        ];
        let prepared = build_test_commit(&inputs(), &context).unwrap();
        let tags = &prepared
            .request
            .plan
            .echo
            .as_ref()
            .unwrap()
            .created
            .deed_tags;
        assert_eq!(tags[0].content_id, "deed.core.sepulcher_knight_defeated");
        assert_eq!(tags[1].content_id, "deed.core.sir_caldus_defeated");

        let mut duplicate = context.clone();
        duplicate.echo.as_mut().unwrap().deed_tags = vec![
            "deed.core.sir_caldus_defeated".into(),
            "deed.core.sir_caldus_defeated".into(),
        ];
        assert!(matches!(
            build_test_commit(&inputs(), &duplicate),
            Err(DurableDeathBuildError::EchoEligibilityMismatch)
        ));

        let mut preexisting = context;
        preexisting.echo.as_mut().unwrap().availability =
            EchoAvailabilityProjection::ExistingAvailable {
                echo_id: uuid_v7(12),
            };
        let prepared = build_test_commit(&inputs(), &preexisting).unwrap();
        let echo = prepared.request.plan.echo.as_ref().unwrap();
        assert_eq!(echo.preexisting_available_echo_id, Some(uuid_v7(12)));
        assert!(echo.promotion.is_none());
        assert_eq!(echo.created.state, DurableEchoStateV1::Dormant);
    }

    #[test]
    fn content_revision_is_bound_across_event_summary_memorial_and_echo() {
        let prepared = build_test_commit(&inputs(), &context()).unwrap();
        let plan = &prepared.request.plan;
        assert_eq!(
            plan.event.content_revision,
            prepared.content.content_revision
        );
        assert_eq!(
            plan.summary.content_revision,
            prepared.content.content_revision
        );
        assert_eq!(
            plan.echo.as_ref().unwrap().created.content_revision,
            prepared.content.content_revision
        );
        assert_eq!(
            plan.memorial.summary_snapshot_digest,
            plan.summary.snapshot_digest
        );
    }

    #[test]
    fn dedicated_presentation_revision_is_required_before_commit_construction() {
        let mut ctx = context();
        ctx.content.records_blake3 = "0".repeat(64);
        assert!(matches!(
            build_test_commit(&inputs(), &ctx),
            Err(DurableDeathBuildError::PresentationContentMismatch(
                "revision binding"
            ))
        ));

        let mut ctx = context();
        ctx.content.content_revision = format!("core-dev.blake3.{}", "0".repeat(64));
        assert!(matches!(
            build_test_commit(&inputs(), &ctx),
            Err(DurableDeathBuildError::PresentationContentMismatch(
                "revision binding"
            ))
        ));
    }

    #[test]
    fn every_stored_presentation_domain_fails_closed() {
        let presentation = presentation();

        let mut ctx = context();
        ctx.hero.class_id = "status.bleed".into();
        assert!(matches!(
            validate_test_presentation(&inputs(), &ctx, &presentation),
            Err(DurableDeathBuildError::PresentationContentMismatch(
                "hero or Memorial snapshot"
            ))
        ));

        let mut ctx = context();
        ctx.echo.as_mut().unwrap().deed_tags = vec!["deed.core.unknown".into()];
        assert!(matches!(
            validate_test_presentation(&inputs(), &ctx, &presentation),
            Err(DurableDeathBuildError::PresentationContentMismatch(
                "deed snapshot"
            ))
        ));

        let mut ctx = context();
        ctx.content.enabled_items[0].template_id = "item.core.unknown".into();
        assert!(matches!(
            validate_test_presentation(&inputs(), &ctx, &presentation),
            Err(DurableDeathBuildError::PresentationContentMismatch(
                "enabled item"
            ))
        ));

        let mut death_inputs = inputs();
        death_inputs.trace[0].source_content_id = "source.core.unknown".into();
        assert!(matches!(
            validate_test_presentation(&death_inputs, &context(), &presentation),
            Err(DurableDeathBuildError::PresentationContentMismatch(
                "combat trace"
            ))
        ));

        let mut death_inputs = inputs();
        death_inputs.trace[0].attack_id = "attack.caldus.bell_ring".into();
        assert!(matches!(
            validate_test_presentation(&death_inputs, &context(), &presentation),
            Err(DurableDeathBuildError::PresentationContentMismatch(
                "combat trace"
            ))
        ));

        let mut death_inputs = inputs();
        death_inputs.trace[0].statuses[0].status_id = "class.grave_arbalist".into();
        assert!(matches!(
            validate_test_presentation(&death_inputs, &context(), &presentation),
            Err(DurableDeathBuildError::PresentationContentMismatch(
                "combat trace"
            ))
        ));

        let mut ctx = context();
        ctx.custody.items.clear();
        ctx.custody.run_materials = vec![DeathAtRiskRunMaterial {
            material_id: "material.core.unknown".into(),
            quantity: 1,
            material_version: 1,
        }];
        assert!(matches!(
            validate_test_presentation(&inputs(), &ctx, &presentation),
            Err(DurableDeathBuildError::PresentationContentMismatch(
                "destruction projection"
            ))
        ));
    }
}
