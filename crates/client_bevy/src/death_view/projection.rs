//! Renderer-independent projection of acknowledged durable-death records.
//!
//! The structures in this module retain authoritative integer values and stable IDs beside exact
//! compiled copy and canonical formatted siblings. Native widgets consume this projection instead
//! of inventing time, quantity, position, damage, or portrait policy.

use std::collections::BTreeSet;

use content_schema::CoreDeathViewCopyKind;
use protocol::{
    DeathCauseV1, DeathDamageTypeV1, DeathEchoOutcomeV1, DeathMemorialEntryV1, DeathNetworkStateV1,
    DeathRecallStateV1, DeathSummaryProjectionEntryV1, DeathSummaryProjectionKindV1,
    DeathSummaryViewV1, DeathTraceEntryV1, DeathViewContentRevisionV1, LatestCommittedDeathV1,
};
use sim_content::{CoreDeathViewSourcePortrait, CoreDevelopmentDeathView};
use thiserror::Error;

/// GDD `DTH-020` order. Widgets consume this constant instead of choosing their own layout order.
pub const DEATH_SUMMARY_SECTION_ORDER: [DeathSummarySection; 8] = [
    DeathSummarySection::Hero,
    DeathSummarySection::LethalCause,
    DeathSummarySection::DamageTimeline,
    DeathSummarySection::Network,
    DeathSummarySection::Lost,
    DeathSummarySection::Preserved,
    DeathSummarySection::Created,
    DeathSummarySection::Actions,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathSummarySection {
    Hero,
    LethalCause,
    DamageTimeline,
    Network,
    Lost,
    Preserved,
    Created,
    Actions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathLocalizedValue {
    pub content_id: String,
    pub label: String,
}

/// Closed portrait policy. Explicit absence is valid only for the catalog's environment and
/// connection-loss sources; an unknown policy is rejected during projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeathSourcePortraitPresentation {
    Asset { asset_id: String },
    ExplicitlyAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathSourcePresentation {
    pub value: DeathLocalizedValue,
    pub portrait: DeathSourcePortraitPresentation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathHeroPresentation {
    pub section_title: String,
    pub hero_label: DeathLocalizedValue,
    pub character_name: String,
    pub class: DeathLocalizedValue,
    pub level: u8,
    pub oath: Option<DeathLocalizedValue>,
    pub bargains: Vec<DeathLocalizedValue>,
    pub lifetime_ms: u64,
    pub formatted_lifetime: String,
    pub final_deed: DeathLocalizedValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathStatusPresentation {
    pub status: DeathLocalizedValue,
    pub remaining_ticks: u32,
    pub stack_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathDamageEventPresentation {
    pub ordinal: u16,
    pub event_tick: u64,
    pub event_ordinal: u32,
    pub source: DeathSourcePresentation,
    pub source_entity_id: Option<[u8; 16]>,
    pub pattern: Option<DeathLocalizedValue>,
    pub attack: DeathLocalizedValue,
    pub raw_damage: u32,
    pub formatted_raw_damage: String,
    pub final_damage: u32,
    pub formatted_final_damage: String,
    pub damage_type: DeathLocalizedValue,
    pub pre_health: u32,
    pub post_health: u32,
    pub source_x_milli_tiles: i32,
    pub source_y_milli_tiles: i32,
    pub formatted_source_position: String,
    pub network: DeathLocalizedValue,
    pub recall: DeathLocalizedValue,
    pub lethal: bool,
    pub statuses: Vec<DeathStatusPresentation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathLethalCausePresentation {
    pub section_title: String,
    /// Terminal summaries retain the stored cause. Memorial summaries omit it because the
    /// append-only Summary response carries every DTH-020 display field but not `DeathCauseV1`.
    pub cause: Option<DeathLocalizedValue>,
    pub killer: DeathSourcePresentation,
    pub pattern: Option<DeathLocalizedValue>,
    pub attack: DeathLocalizedValue,
    pub final_damage: u32,
    pub formatted_final_damage: String,
    pub damage_type: DeathLocalizedValue,
    pub source_x_milli_tiles: i32,
    pub source_y_milli_tiles: i32,
    pub formatted_source_position: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathSummaryContext {
    Terminal,
    Memorial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathTimelinePresentation {
    pub section_title: String,
    pub events: Vec<DeathDamageEventPresentation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathNetworkPresentation {
    pub section_title: String,
    pub network: DeathLocalizedValue,
    pub recall: DeathLocalizedValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeathLossPresentation {
    Item {
        ordinal: u16,
        item: DeathLocalizedValue,
        item_uid: [u8; 16],
        quantity: u32,
        formatted_quantity: String,
    },
    RunMaterial {
        ordinal: u16,
        material: DeathLocalizedValue,
        quantity: u32,
        formatted_quantity: String,
    },
}

impl DeathLossPresentation {
    #[must_use]
    pub const fn ordinal(&self) -> u16 {
        match self {
            Self::Item { ordinal, .. } | Self::RunMaterial { ordinal, .. } => *ordinal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathFixedProjectionPresentation {
    pub ordinal: u16,
    pub kind: DeathSummaryProjectionKindV1,
    pub value: DeathLocalizedValue,
    pub quantity: u32,
    pub formatted_quantity: String,
}

/// Read-only Memorial list row. The complete immutable wire authority remains embedded so
/// selection, ordering, and support diagnostics never depend on formatted strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorialEntryPresentation {
    pub authority: DeathMemorialEntryV1,
    pub formatted_death_at: String,
    pub presentation: DeathLocalizedValue,
    pub class: DeathLocalizedValue,
    pub echo_outcome: DeathLocalizedValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathSummaryAction {
    Retry,
    CreateSuccessor,
    InspectTrace,
    Memorial,
    CharacterSelect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathSummaryActionState {
    Enabled,
    Disabled,
}

impl DeathSummaryActionState {
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathSummaryActionPresentation {
    pub action: DeathSummaryAction,
    pub label: String,
    pub state: DeathSummaryActionState,
    pub unavailable_detail: Option<String>,
}

/// Actions are grouped by GDD `DTH-020` hierarchy so a widget cannot accidentally demote the
/// successor action or promote a secondary navigation action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathSummaryActionsPresentation {
    pub primary: DeathSummaryActionPresentation,
    pub secondary: [DeathSummaryActionPresentation; 3],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathSummaryPresentation {
    pub context: DeathSummaryContext,
    pub death_id: [u8; 16],
    /// The terminal path is anchored to the selected durable character. Historical Memorial
    /// Summary responses deliberately reveal no character aggregate identity.
    pub character_id: Option<[u8; 16]>,
    pub death_at_unix_ms: u64,
    pub formatted_death_at: String,
    pub death_tick: u64,
    pub content_revision: String,
    pub presentation_revision: DeathViewContentRevisionV1,
    pub snapshot_digest: [u8; 32],
    pub eyebrow: String,
    pub title: String,
    pub hero: DeathHeroPresentation,
    pub lethal_cause: DeathLethalCausePresentation,
    pub timeline: DeathTimelinePresentation,
    pub network: DeathNetworkPresentation,
    pub lost_section_title: String,
    pub lost_total_count: u16,
    pub lost: Vec<DeathLossPresentation>,
    pub next_lost_ordinal: Option<u16>,
    pub preserved_section_title: String,
    pub preserved: Vec<DeathFixedProjectionPresentation>,
    pub created_section_title: String,
    pub created: Vec<DeathFixedProjectionPresentation>,
    pub echo_outcome: DeathLocalizedValue,
    pub actions: DeathSummaryActionsPresentation,
}

impl DeathSummaryPresentation {
    #[must_use]
    pub const fn section_order() -> &'static [DeathSummarySection; 8] {
        &DEATH_SUMMARY_SECTION_ORDER
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DeathViewProjectionError {
    #[error("death presentation authority mismatch: {0}")]
    AuthorityMismatch(&'static str),
    #[error("death record does not match its acknowledged anchor: {0}")]
    AnchorMismatch(&'static str),
    #[error("missing typed {domain} copy for {content_id}")]
    MissingCopy {
        domain: &'static str,
        content_id: String,
    },
    #[error("loss continuation is invalid: {0}")]
    InvalidLossContinuation(&'static str),
    #[error("Memorial page is invalid: {0}")]
    InvalidMemorialPage(&'static str),
}

pub(crate) fn validate_latest(
    latest: &LatestCommittedDeathV1,
    captured_character_id: [u8; 16],
    required_revision: &DeathViewContentRevisionV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<(), DeathViewProjectionError> {
    if latest.character_id != captured_character_id {
        return Err(DeathViewProjectionError::AnchorMismatch("character"));
    }
    validate_authority(
        &latest.presentation_revision,
        latest.content_revision.as_str(),
        required_revision,
        catalog,
    )?;
    cause_value(latest.cause, catalog)?;
    project_source(latest.killer_content_id.as_str(), catalog)?;
    if let Some(pattern) = latest.killer_pattern_id.as_ref() {
        pattern_value(pattern.as_str(), catalog)?;
    }
    Ok(())
}

pub(crate) fn project_summary(
    latest: &LatestCommittedDeathV1,
    summary: &DeathSummaryViewV1,
    required_revision: &DeathViewContentRevisionV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathSummaryPresentation, DeathViewProjectionError> {
    validate_summary_anchor(latest, summary, required_revision, catalog)?;
    let anchor = SummaryProjectionAnchor {
        context: DeathSummaryContext::Terminal,
        character_id: Some(latest.character_id),
        death_at_unix_ms: latest.death_at_unix_ms,
        cause: Some(latest.cause),
        killer_pattern_id: latest
            .killer_pattern_id
            .as_ref()
            .map(protocol::WireText::as_str),
        network_state: latest.network_state,
        recall_state: latest.recall_state,
    };
    project_summary_with_anchor(summary, anchor, catalog)
}

pub(crate) fn project_memorial_summary(
    memorial: &DeathMemorialEntryV1,
    summary: &DeathSummaryViewV1,
    required_revision: &DeathViewContentRevisionV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathSummaryPresentation, DeathViewProjectionError> {
    validate_memorial_summary_anchor(memorial, summary, required_revision, catalog)?;
    let lethal = summary
        .last_five_damage
        .last()
        .ok_or(DeathViewProjectionError::AnchorMismatch("lethal trace"))?;
    let anchor = SummaryProjectionAnchor {
        context: DeathSummaryContext::Memorial,
        character_id: None,
        death_at_unix_ms: memorial.cursor.death_at_unix_ms,
        cause: None,
        killer_pattern_id: lethal.pattern_id.as_ref().map(protocol::WireText::as_str),
        network_state: lethal.network_state,
        recall_state: lethal.recall_state,
    };
    project_summary_with_anchor(summary, anchor, catalog)
}

pub(crate) fn project_memorial_page(
    entries: Vec<DeathMemorialEntryV1>,
    required_revision: &DeathViewContentRevisionV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<Vec<MemorialEntryPresentation>, DeathViewProjectionError> {
    entries
        .into_iter()
        .map(|authority| {
            if authority.presentation_revision != *required_revision {
                return Err(DeathViewProjectionError::AuthorityMismatch(
                    "memorial page presentation revision",
                ));
            }
            let formatted_death_at =
                catalog.format_timestamp_utc(authority.cursor.death_at_unix_ms);
            let presentation = death_owned_value(
                catalog,
                CoreDeathViewCopyKind::MemorialPresentation,
                "Memorial presentation",
                authority.presentation_key.as_str(),
            )?;
            let class = dependency_value(
                "class",
                authority.class_id.as_str(),
                catalog.resolve_class(authority.class_id.as_str()),
            )?;
            let echo_outcome = echo_value(authority.echo_outcome, catalog)?;
            Ok(MemorialEntryPresentation {
                authority,
                formatted_death_at,
                presentation,
                class,
                echo_outcome,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct SummaryProjectionAnchor<'a> {
    context: DeathSummaryContext,
    character_id: Option<[u8; 16]>,
    death_at_unix_ms: u64,
    cause: Option<DeathCauseV1>,
    killer_pattern_id: Option<&'a str>,
    network_state: DeathNetworkStateV1,
    recall_state: DeathRecallStateV1,
}

fn project_summary_with_anchor(
    summary: &DeathSummaryViewV1,
    anchor: SummaryProjectionAnchor<'_>,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathSummaryPresentation, DeathViewProjectionError> {
    let timeline = summary
        .last_five_damage
        .iter()
        .map(|entry| project_damage_event(entry, catalog))
        .collect::<Result<Vec<_>, _>>()?;
    let lethal = timeline
        .last()
        .cloned()
        .ok_or(DeathViewProjectionError::AnchorMismatch("lethal trace"))?;
    let lost = project_losses(&summary.lost, catalog)?;
    validate_loss_set(&lost, summary.lost_total_count, summary.next_lost_ordinal)?;

    let hero = project_hero(summary, catalog)?;
    let lethal_cause = project_lethal_cause(anchor, &lethal, catalog)?;
    let timeline = project_timeline(timeline, catalog)?;
    let network = project_network(anchor.network_state, anchor.recall_state, catalog)?;
    let actions = project_actions(anchor.context, catalog)?;

    Ok(DeathSummaryPresentation {
        context: anchor.context,
        death_id: summary.death_id,
        character_id: anchor.character_id,
        death_at_unix_ms: anchor.death_at_unix_ms,
        formatted_death_at: catalog.format_timestamp_utc(anchor.death_at_unix_ms),
        death_tick: summary.death_tick,
        content_revision: summary.content_revision.as_str().to_owned(),
        presentation_revision: summary.presentation_revision.clone(),
        snapshot_digest: summary.snapshot_digest,
        eyebrow: copy(
            catalog,
            CoreDeathViewCopyKind::Surface,
            "death.summary.eyebrow",
        )?,
        title: copy(
            catalog,
            CoreDeathViewCopyKind::Surface,
            "death.summary.title",
        )?,
        hero,
        lethal_cause,
        timeline,
        network,
        lost_section_title: copy(
            catalog,
            CoreDeathViewCopyKind::Section,
            "death.section.lost",
        )?,
        lost_total_count: summary.lost_total_count,
        lost,
        next_lost_ordinal: summary.next_lost_ordinal,
        preserved_section_title: copy(
            catalog,
            CoreDeathViewCopyKind::Section,
            "death.section.preserved",
        )?,
        preserved: project_fixed(&summary.preserved, catalog)?,
        created_section_title: copy(
            catalog,
            CoreDeathViewCopyKind::Section,
            "death.section.created",
        )?,
        created: project_fixed(&summary.created, catalog)?,
        echo_outcome: echo_value(summary.echo_outcome, catalog)?,
        actions,
    })
}

fn project_lethal_cause(
    anchor: SummaryProjectionAnchor<'_>,
    lethal: &DeathDamageEventPresentation,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathLethalCausePresentation, DeathViewProjectionError> {
    Ok(DeathLethalCausePresentation {
        section_title: copy(
            catalog,
            CoreDeathViewCopyKind::Section,
            "death.section.what_happened",
        )?,
        cause: anchor
            .cause
            .map(|cause| cause_value(cause, catalog))
            .transpose()?,
        killer: lethal.source.clone(),
        pattern: anchor
            .killer_pattern_id
            .map(|id| pattern_value(id, catalog))
            .transpose()?,
        attack: lethal.attack.clone(),
        final_damage: lethal.final_damage,
        formatted_final_damage: lethal.formatted_final_damage.clone(),
        damage_type: lethal.damage_type.clone(),
        source_x_milli_tiles: lethal.source_x_milli_tiles,
        source_y_milli_tiles: lethal.source_y_milli_tiles,
        formatted_source_position: lethal.formatted_source_position.clone(),
    })
}

fn project_timeline(
    events: Vec<DeathDamageEventPresentation>,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathTimelinePresentation, DeathViewProjectionError> {
    Ok(DeathTimelinePresentation {
        section_title: copy(
            catalog,
            CoreDeathViewCopyKind::Section,
            "death.section.timeline",
        )?,
        events,
    })
}

fn project_network(
    network_state: DeathNetworkStateV1,
    recall_state: DeathRecallStateV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathNetworkPresentation, DeathViewProjectionError> {
    Ok(DeathNetworkPresentation {
        section_title: copy(
            catalog,
            CoreDeathViewCopyKind::Section,
            "death.section.network",
        )?,
        network: network_value(network_state, catalog)?,
        recall: recall_value(recall_state, catalog)?,
    })
}

fn project_actions(
    context: DeathSummaryContext,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathSummaryActionsPresentation, DeathViewProjectionError> {
    let enabled = |action, content_id| -> Result<_, DeathViewProjectionError> {
        Ok(DeathSummaryActionPresentation {
            action,
            label: copy(catalog, CoreDeathViewCopyKind::Action, content_id)?,
            state: DeathSummaryActionState::Enabled,
            unavailable_detail: None,
        })
    };
    let mut actions = DeathSummaryActionsPresentation {
        primary: DeathSummaryActionPresentation {
            action: DeathSummaryAction::CreateSuccessor,
            label: copy(
                catalog,
                CoreDeathViewCopyKind::Action,
                "death.action.create_successor",
            )?,
            state: DeathSummaryActionState::Disabled,
            unavailable_detail: Some(copy(
                catalog,
                CoreDeathViewCopyKind::Action,
                "death.action.successor_unavailable",
            )?),
        },
        secondary: [
            enabled(
                DeathSummaryAction::InspectTrace,
                "death.action.inspect_trace",
            )?,
            enabled(DeathSummaryAction::Memorial, "death.action.memorial")?,
            enabled(
                DeathSummaryAction::CharacterSelect,
                "death.action.character_select",
            )?,
        ],
    };
    if context == DeathSummaryContext::Memorial {
        // Historical Memorial inspection is permanently read-only. Keep this explicit so the
        // later GB-M03-07 terminal enablement cannot leak successor creation into old deaths.
        actions.primary.state = DeathSummaryActionState::Disabled;
        actions.secondary[1].state = DeathSummaryActionState::Disabled;
        actions.secondary[2].state = DeathSummaryActionState::Disabled;
    }
    Ok(actions)
}

pub(crate) fn project_summary_continuation(
    latest: &LatestCommittedDeathV1,
    anchor: &DeathSummaryViewV1,
    page: &DeathSummaryViewV1,
    current: &DeathSummaryPresentation,
    required_revision: &DeathViewContentRevisionV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathSummaryLossContinuation, DeathViewProjectionError> {
    validate_summary_anchor(latest, page, required_revision, catalog)?;
    project_loss_continuation(anchor, page, current, catalog)
}

pub(crate) fn project_memorial_summary_continuation(
    memorial: &DeathMemorialEntryV1,
    anchor: &DeathSummaryViewV1,
    page: &DeathSummaryViewV1,
    current: &DeathSummaryPresentation,
    required_revision: &DeathViewContentRevisionV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathSummaryLossContinuation, DeathViewProjectionError> {
    validate_memorial_summary_anchor(memorial, page, required_revision, catalog)?;
    project_loss_continuation(anchor, page, current, catalog)
}

fn project_loss_continuation(
    anchor: &DeathSummaryViewV1,
    page: &DeathSummaryViewV1,
    current: &DeathSummaryPresentation,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathSummaryLossContinuation, DeathViewProjectionError> {
    if !summary_metadata_matches(anchor, page) {
        return Err(DeathViewProjectionError::InvalidLossContinuation(
            "snapshot metadata changed",
        ));
    }
    let expected_start = u16::try_from(current.lost.len()).map_err(|_| {
        DeathViewProjectionError::InvalidLossContinuation("accumulated loss count overflow")
    })?;
    if page.lost_start_ordinal != expected_start
        || current.next_lost_ordinal != Some(page.lost_start_ordinal)
    {
        return Err(DeathViewProjectionError::InvalidLossContinuation(
            "page did not begin at the exact continuation ordinal",
        ));
    }
    let additions = project_losses(&page.lost, catalog)?;
    validate_loss_continuation(
        &current.lost,
        &additions,
        current.lost_total_count,
        page.next_lost_ordinal,
    )?;
    Ok(DeathSummaryLossContinuation {
        additions,
        next_lost_ordinal: page.next_lost_ordinal,
    })
}

fn validate_memorial_summary_anchor(
    memorial: &DeathMemorialEntryV1,
    summary: &DeathSummaryViewV1,
    required_revision: &DeathViewContentRevisionV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<(), DeathViewProjectionError> {
    validate_authority(
        &summary.presentation_revision,
        summary.content_revision.as_str(),
        required_revision,
        catalog,
    )?;
    if memorial.presentation_revision != *required_revision {
        return Err(DeathViewProjectionError::AuthorityMismatch(
            "memorial presentation revision",
        ));
    }
    if summary.death_id != memorial.cursor.death_id
        || summary.summary_revision != memorial.summary_revision
        || summary.snapshot_digest != memorial.summary_snapshot_digest
        || summary.presentation_revision != memorial.presentation_revision
        || summary.character_name_snapshot != memorial.character_name_snapshot
        || summary.class_id != memorial.class_id
        || summary.level != memorial.level
        || summary.echo_outcome != memorial.echo_outcome
    {
        return Err(DeathViewProjectionError::AnchorMismatch(
            "memorial entry and summary",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeathSummaryLossContinuation {
    pub additions: Vec<DeathLossPresentation>,
    pub next_lost_ordinal: Option<u16>,
}

fn validate_summary_anchor(
    latest: &LatestCommittedDeathV1,
    summary: &DeathSummaryViewV1,
    required_revision: &DeathViewContentRevisionV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<(), DeathViewProjectionError> {
    validate_authority(
        &summary.presentation_revision,
        summary.content_revision.as_str(),
        required_revision,
        catalog,
    )?;
    let expected_trace_count = summary
        .lethal_trace_ordinal
        .checked_add(1)
        .ok_or(DeathViewProjectionError::AnchorMismatch("trace count"))?;
    let lethal = summary
        .last_five_damage
        .last()
        .ok_or(DeathViewProjectionError::AnchorMismatch("lethal trace"))?;
    if summary.death_id != latest.death_id
        || summary.snapshot_digest != latest.summary_snapshot_digest
        || summary.death_tick != latest.death_tick
        || summary.content_revision != latest.content_revision
        || summary.presentation_revision != latest.presentation_revision
        || expected_trace_count != latest.trace_entry_count
        || summary.lost_total_count != latest.destruction_entry_count
        || lethal.source_content_id != latest.killer_content_id
        || lethal.pattern_id != latest.killer_pattern_id
        || lethal.network_state != latest.network_state
        || lethal.recall_state != latest.recall_state
    {
        return Err(DeathViewProjectionError::AnchorMismatch(
            "latest and summary",
        ));
    }
    Ok(())
}

fn validate_authority(
    actual: &DeathViewContentRevisionV1,
    content_revision: &str,
    required: &DeathViewContentRevisionV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<(), DeathViewProjectionError> {
    if actual != required {
        return Err(DeathViewProjectionError::AuthorityMismatch(
            "presentation revision",
        ));
    }
    if content_revision != catalog.item_content_revision() {
        return Err(DeathViewProjectionError::AuthorityMismatch(
            "item content revision",
        ));
    }
    Ok(())
}

fn project_hero(
    summary: &DeathSummaryViewV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathHeroPresentation, DeathViewProjectionError> {
    Ok(DeathHeroPresentation {
        section_title: copy(
            catalog,
            CoreDeathViewCopyKind::Section,
            "death.section.hero",
        )?,
        hero_label: death_owned_value(
            catalog,
            CoreDeathViewCopyKind::HeroLabel,
            "hero label",
            summary.hero_label_key.as_str(),
        )?,
        character_name: summary.character_name_snapshot.as_str().to_owned(),
        class: dependency_value(
            "class",
            summary.class_id.as_str(),
            catalog.resolve_class(summary.class_id.as_str()),
        )?,
        level: summary.level,
        oath: summary
            .oath_id
            .as_ref()
            .map(|id| dependency_value("oath", id.as_str(), catalog.resolve_oath(id.as_str())))
            .transpose()?,
        bargains: summary
            .bargains
            .iter()
            .map(|id| {
                dependency_value("bargain", id.as_str(), catalog.resolve_bargain(id.as_str()))
            })
            .collect::<Result<Vec<_>, _>>()?,
        lifetime_ms: summary.lifetime_ms,
        formatted_lifetime: catalog.format_lifetime(summary.lifetime_ms),
        final_deed: death_owned_value(
            catalog,
            CoreDeathViewCopyKind::Deed,
            "deed",
            summary.final_deed_id.as_str(),
        )?,
    })
}

fn project_damage_event(
    entry: &DeathTraceEntryV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathDamageEventPresentation, DeathViewProjectionError> {
    Ok(DeathDamageEventPresentation {
        ordinal: entry.ordinal,
        event_tick: entry.event_tick,
        event_ordinal: entry.event_ordinal,
        source: project_source(entry.source_content_id.as_str(), catalog)?,
        source_entity_id: entry.source_entity_id,
        pattern: entry
            .pattern_id
            .as_ref()
            .map(|id| pattern_value(id.as_str(), catalog))
            .transpose()?,
        attack: attack_value(entry.attack_id.as_str(), catalog)?,
        raw_damage: entry.raw_damage,
        formatted_raw_damage: catalog.format_damage(entry.raw_damage),
        final_damage: entry.final_damage,
        formatted_final_damage: catalog.format_damage(entry.final_damage),
        damage_type: damage_type_value(entry.damage_type, catalog)?,
        pre_health: entry.pre_health,
        post_health: entry.post_health,
        source_x_milli_tiles: entry.source_x_milli_tiles,
        source_y_milli_tiles: entry.source_y_milli_tiles,
        formatted_source_position: catalog
            .format_position(entry.source_x_milli_tiles, entry.source_y_milli_tiles),
        network: network_value(entry.network_state, catalog)?,
        recall: recall_value(entry.recall_state, catalog)?,
        lethal: entry.lethal,
        statuses: entry
            .statuses
            .iter()
            .map(|status| {
                Ok(DeathStatusPresentation {
                    status: dependency_value(
                        "status",
                        status.status_id.as_str(),
                        catalog.resolve_status(status.status_id.as_str()),
                    )?,
                    remaining_ticks: status.remaining_ticks,
                    stack_count: status.stack_count,
                })
            })
            .collect::<Result<Vec<_>, DeathViewProjectionError>>()?,
    })
}

fn project_losses(
    entries: &[DeathSummaryProjectionEntryV1],
    catalog: &CoreDevelopmentDeathView,
) -> Result<Vec<DeathLossPresentation>, DeathViewProjectionError> {
    entries
        .iter()
        .map(|entry| match entry.kind {
            DeathSummaryProjectionKindV1::LostItem => Ok(DeathLossPresentation::Item {
                ordinal: entry.ordinal,
                item: dependency_value(
                    "item",
                    entry.content_id.as_str(),
                    catalog.resolve_item(entry.content_id.as_str()),
                )?,
                item_uid: entry.item_uid.ok_or(
                    DeathViewProjectionError::InvalidLossContinuation("item UID missing"),
                )?,
                quantity: entry.quantity,
                formatted_quantity: catalog.format_quantity(entry.quantity),
            }),
            DeathSummaryProjectionKindV1::LostRunMaterial => {
                Ok(DeathLossPresentation::RunMaterial {
                    ordinal: entry.ordinal,
                    material: death_owned_value(
                        catalog,
                        CoreDeathViewCopyKind::Material,
                        "material",
                        entry.content_id.as_str(),
                    )?,
                    quantity: entry.quantity,
                    formatted_quantity: catalog.format_quantity(entry.quantity),
                })
            }
            _ => Err(DeathViewProjectionError::InvalidLossContinuation(
                "non-loss projection in loss page",
            )),
        })
        .collect()
}

fn validate_loss_set(
    entries: &[DeathLossPresentation],
    total_count: u16,
    next: Option<u16>,
) -> Result<(), DeathViewProjectionError> {
    let mut item_uids = BTreeSet::new();
    let mut materials = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.ordinal() != u16::try_from(index).unwrap_or(u16::MAX) {
            return Err(DeathViewProjectionError::InvalidLossContinuation(
                "loss ordinals are not contiguous",
            ));
        }
        match entry {
            DeathLossPresentation::Item { item_uid, .. } => {
                if !item_uids.insert(*item_uid) {
                    return Err(DeathViewProjectionError::InvalidLossContinuation(
                        "duplicate item UID across pages",
                    ));
                }
            }
            DeathLossPresentation::RunMaterial { material, .. } => {
                if !materials.insert(material.content_id.as_str()) {
                    return Err(DeathViewProjectionError::InvalidLossContinuation(
                        "duplicate material across pages",
                    ));
                }
            }
        }
    }
    let accumulated = u16::try_from(entries.len()).map_err(|_| {
        DeathViewProjectionError::InvalidLossContinuation("accumulated loss count overflow")
    })?;
    let expected_next = (accumulated < total_count).then_some(accumulated);
    if next != expected_next {
        return Err(DeathViewProjectionError::InvalidLossContinuation(
            "next ordinal does not match accumulated losses",
        ));
    }
    Ok(())
}

fn validate_loss_continuation(
    current: &[DeathLossPresentation],
    additions: &[DeathLossPresentation],
    total_count: u16,
    next: Option<u16>,
) -> Result<(), DeathViewProjectionError> {
    let mut item_uids = BTreeSet::new();
    let mut materials = BTreeSet::new();
    for entry in current {
        match entry {
            DeathLossPresentation::Item { item_uid, .. } => {
                item_uids.insert(*item_uid);
            }
            DeathLossPresentation::RunMaterial { material, .. } => {
                materials.insert(material.content_id.as_str());
            }
        }
    }

    let start = u16::try_from(current.len()).map_err(|_| {
        DeathViewProjectionError::InvalidLossContinuation("accumulated loss count overflow")
    })?;
    for (index, entry) in additions.iter().enumerate() {
        let page_index = u16::try_from(index).map_err(|_| {
            DeathViewProjectionError::InvalidLossContinuation("page index overflow")
        })?;
        let expected = start.checked_add(page_index).ok_or(
            DeathViewProjectionError::InvalidLossContinuation("loss ordinal overflow"),
        )?;
        if entry.ordinal() != expected {
            return Err(DeathViewProjectionError::InvalidLossContinuation(
                "loss ordinals are not contiguous",
            ));
        }
        match entry {
            DeathLossPresentation::Item { item_uid, .. } => {
                if !item_uids.insert(*item_uid) {
                    return Err(DeathViewProjectionError::InvalidLossContinuation(
                        "duplicate item UID across pages",
                    ));
                }
            }
            DeathLossPresentation::RunMaterial { material, .. } => {
                if !materials.insert(material.content_id.as_str()) {
                    return Err(DeathViewProjectionError::InvalidLossContinuation(
                        "duplicate material across pages",
                    ));
                }
            }
        }
    }

    let addition_count = u16::try_from(additions.len())
        .map_err(|_| DeathViewProjectionError::InvalidLossContinuation("page length overflow"))?;
    let accumulated = start.checked_add(addition_count).ok_or(
        DeathViewProjectionError::InvalidLossContinuation("accumulated loss count overflow"),
    )?;
    let expected_next = (accumulated < total_count).then_some(accumulated);
    if next != expected_next {
        return Err(DeathViewProjectionError::InvalidLossContinuation(
            "next ordinal does not match accumulated losses",
        ));
    }
    Ok(())
}

fn project_fixed(
    entries: &[DeathSummaryProjectionEntryV1],
    catalog: &CoreDevelopmentDeathView,
) -> Result<Vec<DeathFixedProjectionPresentation>, DeathViewProjectionError> {
    entries
        .iter()
        .map(|entry| {
            Ok(DeathFixedProjectionPresentation {
                ordinal: entry.ordinal,
                kind: entry.kind,
                value: death_owned_value(
                    catalog,
                    CoreDeathViewCopyKind::Projection,
                    "projection",
                    entry.content_id.as_str(),
                )?,
                quantity: entry.quantity,
                formatted_quantity: catalog.format_quantity(entry.quantity),
            })
        })
        .collect()
}

fn summary_metadata_matches(left: &DeathSummaryViewV1, right: &DeathSummaryViewV1) -> bool {
    left.death_id == right.death_id
        && left.summary_revision == right.summary_revision
        && left.hero_label_key == right.hero_label_key
        && left.character_name_snapshot == right.character_name_snapshot
        && left.class_id == right.class_id
        && left.level == right.level
        && left.oath_id == right.oath_id
        && left.bargains == right.bargains
        && left.lifetime_ms == right.lifetime_ms
        && left.final_deed_id == right.final_deed_id
        && left.lethal_trace_ordinal == right.lethal_trace_ordinal
        && left.last_five_damage == right.last_five_damage
        && left.lost_total_count == right.lost_total_count
        && left.preserved == right.preserved
        && left.created == right.created
        && left.echo_outcome == right.echo_outcome
        && left.death_tick == right.death_tick
        && left.content_revision == right.content_revision
        && left.snapshot_digest == right.snapshot_digest
        && left.presentation_revision == right.presentation_revision
}

fn cause_value(
    cause: DeathCauseV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathLocalizedValue, DeathViewProjectionError> {
    let id = match cause {
        DeathCauseV1::DirectHit => "death.cause.direct_hit",
        DeathCauseV1::DamageOverTime => "death.cause.damage_over_time",
        DeathCauseV1::Environment => "death.cause.environment",
        DeathCauseV1::Disconnect => "death.cause.disconnect",
    };
    death_owned_value(catalog, CoreDeathViewCopyKind::Cause, "cause", id)
}

fn damage_type_value(
    value: DeathDamageTypeV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathLocalizedValue, DeathViewProjectionError> {
    let id = match value {
        DeathDamageTypeV1::Physical => "death.damage_type.physical",
        DeathDamageTypeV1::Veil => "death.damage_type.veil",
    };
    death_owned_value(
        catalog,
        CoreDeathViewCopyKind::DamageType,
        "damage type",
        id,
    )
}

fn network_value(
    value: DeathNetworkStateV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathLocalizedValue, DeathViewProjectionError> {
    let id = match value {
        DeathNetworkStateV1::Connected => "death.network.connected",
        DeathNetworkStateV1::Degraded => "death.network.degraded",
        DeathNetworkStateV1::LinkLost => "death.network.link_lost",
        DeathNetworkStateV1::Reattached => "death.network.reattached",
    };
    death_owned_value(
        catalog,
        CoreDeathViewCopyKind::NetworkState,
        "network state",
        id,
    )
}

fn recall_value(
    value: DeathRecallStateV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathLocalizedValue, DeathViewProjectionError> {
    let id = match value {
        DeathRecallStateV1::Inactive => "death.recall.inactive",
        DeathRecallStateV1::Channeling => "death.recall.channeling",
        DeathRecallStateV1::CompletionPending => "death.recall.completion_pending",
    };
    death_owned_value(
        catalog,
        CoreDeathViewCopyKind::RecallState,
        "Recall state",
        id,
    )
}

fn echo_value(
    value: DeathEchoOutcomeV1,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathLocalizedValue, DeathViewProjectionError> {
    let id = match value {
        DeathEchoOutcomeV1::NotEligible => "death.echo.not_eligible",
        DeathEchoOutcomeV1::Dormant => "death.echo.dormant",
        DeathEchoOutcomeV1::Available => "death.echo.available",
    };
    death_owned_value(
        catalog,
        CoreDeathViewCopyKind::EchoOutcome,
        "Echo outcome",
        id,
    )
}

fn project_source(
    content_id: &str,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathSourcePresentation, DeathViewProjectionError> {
    let value = dependency_value("source", content_id, catalog.resolve_source(content_id))?;
    let portrait = match catalog.resolve_source_portrait(content_id) {
        Some(CoreDeathViewSourcePortrait::Asset(asset_id)) => {
            DeathSourcePortraitPresentation::Asset {
                asset_id: asset_id.to_owned(),
            }
        }
        Some(CoreDeathViewSourcePortrait::ExplicitlyAbsent) => {
            DeathSourcePortraitPresentation::ExplicitlyAbsent
        }
        None => {
            return Err(DeathViewProjectionError::MissingCopy {
                domain: "source portrait policy",
                content_id: content_id.to_owned(),
            });
        }
    };
    Ok(DeathSourcePresentation { value, portrait })
}

fn attack_value(
    content_id: &str,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathLocalizedValue, DeathViewProjectionError> {
    dependency_value("attack", content_id, catalog.resolve_attack(content_id))
}

fn pattern_value(
    content_id: &str,
    catalog: &CoreDevelopmentDeathView,
) -> Result<DeathLocalizedValue, DeathViewProjectionError> {
    dependency_value("pattern", content_id, catalog.resolve_pattern(content_id))
}

fn death_owned_value(
    catalog: &CoreDevelopmentDeathView,
    kind: CoreDeathViewCopyKind,
    domain: &'static str,
    content_id: &str,
) -> Result<DeathLocalizedValue, DeathViewProjectionError> {
    dependency_value(domain, content_id, catalog.resolve_copy(kind, content_id))
}

fn dependency_value(
    domain: &'static str,
    content_id: &str,
    label: Option<&str>,
) -> Result<DeathLocalizedValue, DeathViewProjectionError> {
    Ok(DeathLocalizedValue {
        content_id: content_id.to_owned(),
        label: label
            .ok_or_else(|| DeathViewProjectionError::MissingCopy {
                domain,
                content_id: content_id.to_owned(),
            })?
            .to_owned(),
    })
}

fn copy(
    catalog: &CoreDevelopmentDeathView,
    kind: CoreDeathViewCopyKind,
    content_id: &str,
) -> Result<String, DeathViewProjectionError> {
    catalog
        .resolve_copy(kind, content_id)
        .map(str::to_owned)
        .ok_or_else(|| DeathViewProjectionError::MissingCopy {
            domain: "fixed UI",
            content_id: content_id.to_owned(),
        })
}
