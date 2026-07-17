use std::{path::Path, sync::OnceLock};

use protocol::{
    DEATH_VIEW_SCHEMA_VERSION, DeathCharacterName, DeathDamageTypeV1, DeathEchoOutcomeV1,
    DeathNetworkStateV1, DeathRecallStateV1, DeathSummaryProjectionEntryV1,
    DeathSummaryProjectionKindV1, DeathSummaryViewV1, DeathTraceEntryV1, DeathTraceStatusV1,
    DeathViewContentRevisionV1, DeathViewResultCodeV1, DeathViewResultV1, LatestCommittedDeathV1,
    ManifestHash, WireText,
};

use super::*;

const CHARACTER_ID: [u8; 16] = [2; 16];
pub(super) const ITEM_ID: &str = "item.weapon.crossbow.pine_crossbow";
const MATERIAL_ID: &str = "material.bell_brass";
const SOURCE_ID: &str = "miniboss.sepulcher_knight";
const PATTERN_ID: &str = "miniboss.sepulcher_knight.charge_lane";

pub(super) fn catalog() -> sim_content::CoreDevelopmentDeathView {
    sim_content::load_core_development_death_view(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"),
    )
    .unwrap()
}

pub(super) fn model() -> DeathViewClientModel {
    DeathViewClientModel::new(catalog()).unwrap()
}

pub(super) fn revision() -> DeathViewContentRevisionV1 {
    static REVISION: OnceLock<DeathViewContentRevisionV1> = OnceLock::new();
    REVISION
        .get_or_init(|| {
            let catalog = catalog();
            DeathViewContentRevisionV1 {
                records_blake3: ManifestHash::new(catalog.hashes().records_blake3.clone()).unwrap(),
                assets_blake3: ManifestHash::new(catalog.hashes().assets_blake3.clone()).unwrap(),
                localization_blake3: ManifestHash::new(
                    catalog.hashes().localization_blake3.clone(),
                )
                .unwrap(),
            }
        })
        .clone()
}

pub(super) fn content_revision() -> String {
    static CONTENT_REVISION: OnceLock<String> = OnceLock::new();
    CONTENT_REVISION
        .get_or_init(|| catalog().item_content_revision().to_owned())
        .clone()
}

pub(super) const fn uuid_v7(seed: u8) -> [u8; 16] {
    let mut value = [seed; 16];
    value[6] = 0x70 | (seed & 0x0f);
    value[8] = 0x80 | (seed & 0x3f);
    value
}

fn latest() -> LatestCommittedDeathV1 {
    LatestCommittedDeathV1 {
        death_id: uuid_v7(1),
        character_id: CHARACTER_ID,
        death_at_unix_ms: 1_000,
        death_tick: 301,
        cause: protocol::DeathCauseV1::DirectHit,
        killer_content_id: WireText::new(SOURCE_ID).unwrap(),
        killer_pattern_id: Some(WireText::new(PATTERN_ID).unwrap()),
        network_state: DeathNetworkStateV1::Connected,
        recall_state: DeathRecallStateV1::Inactive,
        trace_entry_count: 2,
        trace_digest: [2; 32],
        destruction_entry_count: 1,
        destruction_digest: [3; 32],
        summary_snapshot_digest: [4; 32],
        content_revision: WireText::new(content_revision()).unwrap(),
        presentation_revision: revision(),
    }
}

pub(super) fn trace_entry(ordinal: u16, lethal: bool) -> DeathTraceEntryV1 {
    DeathTraceEntryV1 {
        ordinal,
        event_tick: 300 + u64::from(ordinal),
        event_ordinal: u32::from(ordinal),
        source_content_id: WireText::new(SOURCE_ID).unwrap(),
        source_entity_id: Some([7; 16]),
        pattern_id: Some(WireText::new(PATTERN_ID).unwrap()),
        attack_id: WireText::new(PATTERN_ID).unwrap(),
        raw_damage: if lethal { 8 } else { 4 },
        final_damage: if lethal { 8 } else { 4 },
        damage_type: DeathDamageTypeV1::Physical,
        pre_health: if lethal { 8 } else { 12 },
        post_health: if lethal { 0 } else { 8 },
        source_x_milli_tiles: 1_000,
        source_y_milli_tiles: -2_000,
        network_state: DeathNetworkStateV1::Connected,
        recall_state: DeathRecallStateV1::Inactive,
        lethal,
        statuses: vec![DeathTraceStatusV1 {
            ordinal: 0,
            status_id: WireText::new("status.bleed").unwrap(),
            remaining_ticks: 10,
            stack_count: 1,
        }],
    }
}

fn fixed_projection(
    ordinal: u16,
    kind: DeathSummaryProjectionKindV1,
    content_id: &str,
) -> DeathSummaryProjectionEntryV1 {
    DeathSummaryProjectionEntryV1 {
        ordinal,
        kind,
        content_id: WireText::new(content_id).unwrap(),
        quantity: 1,
        item_uid: None,
    }
}

pub(super) fn fixed_preserved() -> Vec<DeathSummaryProjectionEntryV1> {
    [
        (
            DeathSummaryProjectionKindV1::PreservedAccountRecords,
            "projection.preserved.account_records",
        ),
        (
            DeathSummaryProjectionKindV1::PreservedCurrency,
            "projection.preserved.currency",
        ),
        (
            DeathSummaryProjectionKindV1::PreservedVault,
            "projection.preserved.vault",
        ),
        (
            DeathSummaryProjectionKindV1::PreservedCosmetics,
            "projection.preserved.cosmetics",
        ),
        (
            DeathSummaryProjectionKindV1::PreservedRecipes,
            "projection.preserved.recipes",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (kind, id))| fixed_projection(u16::try_from(index).unwrap(), kind, id))
    .collect()
}

pub(super) fn fixed_created() -> Vec<DeathSummaryProjectionEntryV1> {
    vec![
        fixed_projection(
            0,
            DeathSummaryProjectionKindV1::CreatedMemorial,
            "projection.created.memorial",
        ),
        fixed_projection(
            1,
            DeathSummaryProjectionKindV1::CreatedEcho,
            "projection.created.echo",
        ),
    ]
}

pub(super) fn item_loss(ordinal: u16) -> DeathSummaryProjectionEntryV1 {
    let mut item_uid = [0; 16];
    item_uid[..2].copy_from_slice(&ordinal.to_be_bytes());
    item_uid[2] = 1;
    DeathSummaryProjectionEntryV1 {
        ordinal,
        kind: DeathSummaryProjectionKindV1::LostItem,
        content_id: WireText::new(ITEM_ID).unwrap(),
        quantity: 1,
        item_uid: Some(item_uid),
    }
}

fn material_loss(ordinal: u16) -> DeathSummaryProjectionEntryV1 {
    DeathSummaryProjectionEntryV1 {
        ordinal,
        kind: DeathSummaryProjectionKindV1::LostRunMaterial,
        content_id: WireText::new(MATERIAL_ID).unwrap(),
        quantity: 3,
        item_uid: None,
    }
}

pub(super) fn summary_page(
    lost_total_count: u16,
    lost_start_ordinal: u16,
    lost: Vec<DeathSummaryProjectionEntryV1>,
) -> DeathSummaryViewV1 {
    let next_lost_ordinal = lost_start_ordinal
        .checked_add(u16::try_from(lost.len()).unwrap())
        .filter(|next| *next < lost_total_count);
    DeathSummaryViewV1 {
        death_id: uuid_v7(1),
        summary_revision: 1,
        hero_label_key: WireText::new("hero.core.grave_arbalist").unwrap(),
        character_name_snapshot: DeathCharacterName::new("Mara Ash").unwrap(),
        class_id: WireText::new("class.grave_arbalist").unwrap(),
        level: 10,
        oath_id: Some(WireText::new("oath.arbalist.long_vigil").unwrap()),
        bargains: vec![WireText::new("bargain.cinder_hunger").unwrap()],
        lifetime_ms: 600_000,
        final_deed_id: WireText::new("deed.core.sepulcher_knight_defeated").unwrap(),
        lethal_trace_ordinal: 1,
        last_five_damage: vec![trace_entry(0, false), trace_entry(1, true)],
        lost_total_count,
        lost_start_ordinal,
        lost,
        next_lost_ordinal,
        preserved: fixed_preserved(),
        created: fixed_created(),
        echo_outcome: DeathEchoOutcomeV1::Available,
        death_tick: 301,
        content_revision: WireText::new(content_revision()).unwrap(),
        snapshot_digest: [4; 32],
        presentation_revision: revision(),
    }
}

fn latest_result(sequence: u32, death: Option<LatestCommittedDeathV1>) -> DeathViewResultV1 {
    DeathViewResultV1::Latest {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        request_sequence: sequence,
        death,
    }
}

fn summary_result(sequence: u32, summary: DeathSummaryViewV1) -> DeathViewResultV1 {
    DeathViewResultV1::Summary {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        request_sequence: sequence,
        requested_lost_limit: TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
        summary,
    }
}

fn error_result(sequence: u32, code: DeathViewResultCodeV1) -> DeathViewResultV1 {
    DeathViewResultV1::Error {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        request_sequence: sequence,
        code,
    }
}

fn begin_to_summary(model: &mut DeathViewClientModel, death: LatestCommittedDeathV1) -> u32 {
    let latest_request = model.begin_committed_death_lookup(CHARACTER_ID).unwrap();
    let outcome = model
        .handle_result(&latest_result(latest_request.sequence, Some(death)))
        .unwrap();
    outcome.follow_up.unwrap().sequence
}

fn complete_one_loss_summary(model: &mut DeathViewClientModel) {
    let sequence = begin_to_summary(model, latest());
    model
        .handle_result(&summary_result(
            sequence,
            summary_page(1, 0, vec![item_loss(0)]),
        ))
        .unwrap();
}

#[test]
fn local_health_zero_exposes_no_durable_data_or_actions() {
    let mut model = model();
    model.observe_local_health_zero(CHARACTER_ID).unwrap();
    model.observe_local_health_zero(CHARACTER_ID).unwrap();
    assert_eq!(
        model.terminal().phase(),
        TerminalDeathPhase::PossibleDeathObserved
    );
    assert!(model.pending().is_none());
    assert!(model.terminal().latest().is_none());
    assert!(model.terminal().summary().is_none());
    assert!(model.terminal_successor_authority().is_none());
    assert_eq!(model.phase_copy(), Some("Recording the final moment"));
    assert!(model.awaiting_detail_copy().is_some());
    for action in [
        DeathSummaryAction::Retry,
        DeathSummaryAction::InspectTrace,
        DeathSummaryAction::Memorial,
        DeathSummaryAction::CharacterSelect,
        DeathSummaryAction::CreateSuccessor,
    ] {
        assert_eq!(
            model.terminal().action_state(action),
            DeathSummaryActionState::Disabled
        );
    }
}

#[test]
fn ui_copy_is_complete_and_uses_only_compiled_death_authority() {
    let model = model();
    let copy = model.ui_copy();

    assert_eq!(copy.fields.attack, "Attack");
    assert_eq!(copy.fields.cause, "Cause");
    assert_eq!(copy.fields.class, "Class");
    assert_eq!(copy.fields.damage, "Final damage");
    assert_eq!(copy.fields.damage_type, "Damage type");
    assert_eq!(copy.fields.final_deed, "Final deed");
    assert_eq!(copy.fields.killer, "Killer");
    assert_eq!(copy.fields.level, "Level");
    assert_eq!(copy.fields.lifetime, "Lifetime");
    assert_eq!(copy.fields.network, "Network");
    assert_eq!(copy.fields.recall, "Recall");
    assert_eq!(copy.fields.source_position, "Source position");
    assert_eq!(copy.memorial_title, "Memorial Wall");
    assert_eq!(copy.back_action, "Back");
    assert_eq!(copy.load_more_action, "Load More");
    assert_eq!(copy.retry_action, "Retry");
}

#[test]
fn late_local_health_zero_cannot_erase_acknowledged_or_fatal_state() {
    let mut ready = model();
    complete_one_loss_summary(&mut ready);
    let retained = ready.terminal().summary().unwrap().clone();
    assert!(matches!(
        ready.observe_local_health_zero(CHARACTER_ID),
        Err(DeathViewClientError::InvalidTerminalPhase)
    ));
    assert_eq!(ready.terminal().phase(), TerminalDeathPhase::SummaryReady);
    assert_eq!(ready.terminal().summary(), Some(&retained));

    let refresh = ready.refresh_terminal_summary().unwrap();
    ready
        .handle_result(&error_result(
            refresh.sequence,
            DeathViewResultCodeV1::CorruptStoredRecord,
        ))
        .unwrap();
    assert!(matches!(
        ready.observe_local_health_zero(CHARACTER_ID),
        Err(DeathViewClientError::InvalidTerminalPhase)
    ));
    assert_eq!(
        ready.terminal().phase(),
        TerminalDeathPhase::FatalRecordError
    );
    assert_eq!(ready.terminal().retained_summary(), Some(&retained));
}

#[test]
fn lost_latest_and_summary_responses_remain_safely_gated() {
    let mut model = model();
    let first = model.begin_committed_death_lookup(CHARACTER_ID).unwrap();
    model.handle_response_loss().unwrap();
    assert_eq!(
        model.terminal().phase(),
        TerminalDeathPhase::RecoverableError
    );
    assert!(model.terminal().summary().is_none());
    assert!(
        model
            .terminal()
            .action_state(DeathSummaryAction::Retry)
            .is_enabled()
    );

    let retry = model.retry().unwrap();
    assert_eq!(retry.request, first.request);
    let summary_request = model
        .handle_result(&latest_result(retry.sequence, Some(latest())))
        .unwrap()
        .follow_up
        .unwrap();
    model.handle_response_loss().unwrap();
    assert!(model.terminal().summary().is_none());
    assert_eq!(
        model
            .terminal()
            .action_state(DeathSummaryAction::CharacterSelect),
        DeathSummaryActionState::Disabled
    );
    let summary_retry = model.retry().unwrap();
    assert_eq!(summary_retry.request, summary_request.request);
    model
        .handle_result(&summary_result(
            summary_retry.sequence,
            summary_page(1, 0, vec![item_loss(0)]),
        ))
        .unwrap();
    assert_eq!(model.terminal().phase(), TerminalDeathPhase::SummaryReady);
    assert!(matches!(
        model.handle_response_loss(),
        Err(DeathViewClientError::NoResponsePending)
    ));
}

#[test]
fn acknowledged_summary_projects_exact_order_copy_and_action_gates() {
    let mut model = model();
    complete_one_loss_summary(&mut model);
    let summary = model.terminal().summary().unwrap();
    assert_eq!(
        DeathSummaryPresentation::section_order(),
        &DEATH_SUMMARY_SECTION_ORDER
    );
    assert_eq!(
        DEATH_SUMMARY_SECTION_ORDER,
        [
            DeathSummarySection::Hero,
            DeathSummarySection::LethalCause,
            DeathSummarySection::DamageTimeline,
            DeathSummarySection::Network,
            DeathSummarySection::Lost,
            DeathSummarySection::Preserved,
            DeathSummarySection::Created,
            DeathSummarySection::Actions,
        ]
    );
    assert_eq!(summary.context, DeathSummaryContext::Terminal);
    assert_eq!(summary.character_id, Some(CHARACTER_ID));
    assert_eq!(summary.hero.character_name, "Mara Ash");
    assert_eq!(summary.hero.class.label, "Grave Arbalist");
    assert_eq!(summary.hero.lifetime_ms, 600_000);
    assert_eq!(summary.hero.formatted_lifetime, "0h 10m 00s");
    assert_eq!(summary.death_at_unix_ms, 1_000);
    assert_eq!(summary.formatted_death_at, "1970-01-01 00:00:01 UTC");
    assert_eq!(summary.lethal_cause.killer.value.label, "Sepulcher Knight");
    assert_eq!(
        summary.lethal_cause.killer.portrait,
        DeathSourcePortraitPresentation::Asset {
            asset_id: "portrait.miniboss.sepulcher_knight".to_owned(),
        }
    );
    assert_eq!(
        summary
            .lethal_cause
            .cause
            .as_ref()
            .map(|cause| cause.content_id.as_str()),
        Some("death.cause.direct_hit")
    );
    assert_eq!(summary.lethal_cause.source_x_milli_tiles, 1_000);
    assert_eq!(summary.lethal_cause.source_y_milli_tiles, -2_000);
    assert_eq!(summary.lethal_cause.final_damage, 8);
    assert_eq!(summary.lethal_cause.formatted_final_damage, "8 HP");
    assert_eq!(
        summary.lethal_cause.formatted_source_position,
        "(1.000, -2.000) tiles"
    );
    assert_eq!(summary.timeline.events[0].source_y_milli_tiles, -2_000);
    assert_eq!(summary.timeline.events[0].raw_damage, 4);
    assert_eq!(summary.timeline.events[0].formatted_raw_damage, "4 HP");
    assert_eq!(summary.timeline.events[1].formatted_final_damage, "8 HP");
    assert_eq!(
        summary.timeline.events[0].formatted_source_position,
        "(1.000, -2.000) tiles"
    );
    assert_eq!(summary.lost.len(), 1);
    assert!(matches!(
        &summary.lost[0],
        DeathLossPresentation::Item {
            quantity: 1,
            formatted_quantity,
            ..
        } if formatted_quantity == "×1"
    ));
    assert_eq!(summary.preserved[0].quantity, 1);
    assert_eq!(summary.preserved[0].formatted_quantity, "×1");
    assert_eq!(
        summary.actions.primary.action,
        DeathSummaryAction::CreateSuccessor
    );
    assert_eq!(
        summary.actions.primary.state,
        DeathSummaryActionState::Disabled
    );
    assert!(summary.actions.primary.unavailable_detail.is_some());
    assert_eq!(
        summary
            .actions
            .secondary
            .iter()
            .map(|action| action.action)
            .collect::<Vec<_>>(),
        vec![
            DeathSummaryAction::InspectTrace,
            DeathSummaryAction::Memorial,
            DeathSummaryAction::CharacterSelect,
        ]
    );
    assert!(
        summary
            .actions
            .secondary
            .iter()
            .all(|action| action.state.is_enabled() && action.unavailable_detail.is_none())
    );
    assert_eq!(model.terminal().phase(), TerminalDeathPhase::SummaryReady);
    assert_eq!(
        model
            .terminal_successor_authority()
            .map(super::TerminalSuccessorAuthority::death_id),
        Some(uuid_v7(1))
    );
}

fn complete_portraitless_summary(
    source_id: &str,
    attack_id: &str,
    cause: protocol::DeathCauseV1,
    network_state: DeathNetworkStateV1,
) -> DeathSummaryPresentation {
    let mut model = model();
    let mut death = latest();
    death.cause = cause;
    death.killer_content_id = WireText::new(source_id).unwrap();
    death.killer_pattern_id = None;
    death.network_state = network_state;
    let sequence = begin_to_summary(&mut model, death);
    let mut summary = summary_page(1, 0, vec![item_loss(0)]);
    for entry in &mut summary.last_five_damage {
        entry.source_content_id = WireText::new(source_id).unwrap();
        entry.source_entity_id = None;
        entry.pattern_id = None;
        entry.attack_id = WireText::new(attack_id).unwrap();
        entry.network_state = network_state;
        entry.statuses.clear();
    }
    model
        .handle_result(&summary_result(sequence, summary))
        .unwrap();
    model.terminal().summary().unwrap().clone()
}

#[test]
fn explicit_portraitless_sources_never_become_unknown_or_fallback_assets() {
    for (source_id, attack_id, cause, network_state) in [
        (
            "environment.core.hazard",
            "attack.environment.core_hazard",
            protocol::DeathCauseV1::Environment,
            DeathNetworkStateV1::Connected,
        ),
        (
            "network.disconnect",
            "attack.network.disconnect",
            protocol::DeathCauseV1::Disconnect,
            DeathNetworkStateV1::LinkLost,
        ),
    ] {
        let summary = complete_portraitless_summary(source_id, attack_id, cause, network_state);
        assert_eq!(summary.lethal_cause.killer.value.content_id, source_id);
        assert_eq!(
            summary.lethal_cause.killer.portrait,
            DeathSourcePortraitPresentation::ExplicitlyAbsent
        );
        assert!(summary.timeline.events.iter().all(|entry| {
            entry.source.portrait == DeathSourcePortraitPresentation::ExplicitlyAbsent
        }));
    }
}

#[test]
fn stale_wrong_kind_and_duplicate_results_cannot_mutate_pending_state() {
    let mut model = model();
    let request = model.begin_committed_death_lookup(CHARACTER_ID).unwrap();
    let pending = model.pending().cloned();
    let wrong_kind = DeathViewResultV1::MemorialPage {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        request_sequence: request.sequence,
        requested_limit: 1,
        entries: Vec::new(),
        next_cursor: None,
    };
    assert_eq!(
        model.handle_result(&wrong_kind).unwrap().disposition,
        DeathViewApplyDisposition::IgnoredUnexpectedKind
    );
    assert_eq!(model.pending(), pending.as_ref());

    assert_eq!(
        model
            .handle_result(&latest_result(request.sequence + 10, None))
            .unwrap()
            .disposition,
        DeathViewApplyDisposition::IgnoredStale
    );
    let accepted = latest_result(request.sequence, Some(latest()));
    model.handle_result(&accepted).unwrap();
    let summary_pending = model.pending().cloned();
    assert_eq!(
        model.handle_result(&accepted).unwrap().disposition,
        DeathViewApplyDisposition::IgnoredDuplicate
    );
    assert_eq!(model.pending(), summary_pending.as_ref());
}

#[test]
fn invalid_pending_result_is_fatal_while_invalid_stale_result_is_ignored() {
    let mut model = model();
    let latest_request = model.begin_committed_death_lookup(CHARACTER_ID).unwrap();
    let before_terminal = model.terminal().clone();
    let before_pending = model.pending().cloned();
    let stale_invalid = DeathViewResultV1::Error {
        schema_version: 0,
        request_sequence: latest_request.sequence + 1,
        code: DeathViewResultCodeV1::ServiceUnavailable,
    };
    assert!(matches!(
        model.handle_result(&stale_invalid),
        Err(DeathViewClientError::InvalidResponse)
    ));
    assert_eq!(model.terminal(), &before_terminal);
    assert_eq!(model.pending(), before_pending.as_ref());

    let summary_sequence = model
        .handle_result(&latest_result(latest_request.sequence, Some(latest())))
        .unwrap()
        .follow_up
        .unwrap()
        .sequence;
    let mut invalid_summary = summary_page(1, 0, vec![item_loss(0)]);
    invalid_summary.last_five_damage.last_mut().unwrap().lethal = false;
    assert!(matches!(
        model.handle_result(&summary_result(summary_sequence, invalid_summary)),
        Err(DeathViewClientError::InvalidResponse)
    ));
    assert_eq!(
        model.terminal().phase(),
        TerminalDeathPhase::FatalRecordError
    );
    assert_eq!(
        model.terminal().failure().map(|failure| failure.code),
        Some(DeathViewResultCodeV1::CorruptStoredRecord)
    );
    assert!(model.pending().is_none());
}

#[test]
fn foreign_latest_character_enters_support_safe_fatal_state() {
    let mut model = model();
    let request = model.begin_committed_death_lookup(CHARACTER_ID).unwrap();
    let mut foreign = latest();
    foreign.character_id = [99; 16];
    assert!(matches!(
        model.handle_result(&latest_result(request.sequence, Some(foreign))),
        Err(DeathViewClientError::Projection(
            DeathViewProjectionError::AnchorMismatch("character")
        ))
    ));
    assert_eq!(
        model.terminal().phase(),
        TerminalDeathPhase::FatalRecordError
    );
    assert_eq!(
        model.terminal().failure().map(|failure| failure.code),
        Some(DeathViewResultCodeV1::CorruptStoredRecord)
    );
    assert!(model.pending().is_none());
    assert!(model.terminal().summary().is_none());
}

fn assert_summary_rejected(mut alter: impl FnMut(&mut DeathSummaryViewV1)) {
    let mut model = model();
    let sequence = begin_to_summary(&mut model, latest());
    let mut summary = summary_page(1, 0, vec![item_loss(0)]);
    alter(&mut summary);
    assert!(matches!(
        model.handle_result(&summary_result(sequence, summary)),
        Err(DeathViewClientError::Projection(
            DeathViewProjectionError::AnchorMismatch(_)
                | DeathViewProjectionError::AuthorityMismatch(_)
        ))
    ));
    assert!(matches!(
        model.terminal().phase(),
        TerminalDeathPhase::FatalContentError | TerminalDeathPhase::FatalRecordError
    ));
    assert!(model.pending().is_none());
    assert!(model.terminal().summary().is_none());
}

#[test]
fn latest_summary_anchor_cross_checks_fail_independently() {
    assert_summary_rejected(|summary| summary.snapshot_digest[0] ^= 1);
    assert_summary_rejected(|summary| summary.death_tick += 1);
    assert_summary_rejected(|summary| {
        summary.content_revision =
            WireText::new(format!("core-dev.blake3.{}", "f".repeat(64))).unwrap();
    });
    assert_summary_rejected(|summary| {
        summary.presentation_revision.records_blake3 = ManifestHash::new("f".repeat(64)).unwrap();
    });
    assert_summary_rejected(|summary| {
        summary
            .last_five_damage
            .last_mut()
            .unwrap()
            .source_content_id = WireText::new("boss.sir_caldus").unwrap();
    });
    assert_summary_rejected(|summary| {
        summary.last_five_damage.last_mut().unwrap().network_state = DeathNetworkStateV1::Degraded;
    });

    let mut model = model();
    let mut death = latest();
    death.destruction_entry_count = 2;
    let sequence = begin_to_summary(&mut model, death);
    assert!(matches!(
        model.handle_result(&summary_result(
            sequence,
            summary_page(1, 0, vec![item_loss(0)]),
        )),
        Err(DeathViewClientError::Projection(
            DeathViewProjectionError::AnchorMismatch(_)
        ))
    ));
}

#[test]
fn unknown_stored_dependency_is_record_corruption_not_content_mismatch() {
    let mut model = model();
    let sequence = begin_to_summary(&mut model, latest());
    let mut summary = summary_page(1, 0, vec![item_loss(0)]);
    summary.class_id = WireText::new("class.unrecognized").unwrap();
    assert!(matches!(
        model.handle_result(&summary_result(sequence, summary)),
        Err(DeathViewClientError::Projection(
            DeathViewProjectionError::MissingCopy { .. }
        ))
    ));
    assert_eq!(
        model.terminal().phase(),
        TerminalDeathPhase::FatalRecordError
    );
    assert_eq!(
        model.terminal().failure().map(|failure| failure.code),
        Some(DeathViewResultCodeV1::CorruptStoredRecord)
    );
    assert!(model.pending().is_none());
}

fn first_page_33() -> DeathSummaryViewV1 {
    let mut losses = (0..32).map(item_loss).collect::<Vec<_>>();
    losses[0] = material_loss(0);
    summary_page(33, 0, losses)
}

#[test]
fn thirty_three_losses_retry_and_append_atomically() {
    let mut model = model();
    let mut death = latest();
    death.destruction_entry_count = 33;
    let sequence = begin_to_summary(&mut model, death);
    model
        .handle_result(&summary_result(sequence, first_page_33()))
        .unwrap();
    let first_page = model.terminal().summary().unwrap();
    assert_eq!(first_page.lost.len(), 32);
    assert!(matches!(
        &first_page.lost[0],
        DeathLossPresentation::RunMaterial {
            quantity: 3,
            formatted_quantity,
            ..
        } if formatted_quantity == "×3"
    ));

    let continuation = model.load_more_losses().unwrap();
    model
        .handle_result(&error_result(
            continuation.sequence,
            DeathViewResultCodeV1::ServiceUnavailable,
        ))
        .unwrap();
    assert_eq!(model.terminal().summary().unwrap().lost.len(), 32);
    assert_eq!(model.terminal().phase(), TerminalDeathPhase::SummaryReady);
    let retry = model.retry().unwrap();
    assert_eq!(retry.request, continuation.request);

    model
        .handle_result(&summary_result(
            retry.sequence,
            summary_page(33, 32, vec![item_loss(32)]),
        ))
        .unwrap();
    let ready = model.terminal().summary().unwrap();
    assert_eq!(ready.lost.len(), 33);
    assert!(ready.next_lost_ordinal.is_none());
    assert!(matches!(
        model.load_more_losses(),
        Err(DeathViewClientError::NoAdditionalLossPage)
    ));
}

#[test]
fn zero_full_page_and_protocol_maximum_loss_counts_are_exact() {
    for total in [0_u16, 32, 4_096] {
        let mut model = model();
        let mut death = latest();
        death.destruction_entry_count = total;
        let mut sequence = begin_to_summary(&mut model, death);
        let mut start = 0_u16;

        loop {
            let count = TERMINAL_SUMMARY_LOSS_PAGE_LIMIT.min(total.saturating_sub(start));
            let losses = (start..start + count).map(item_loss).collect();
            model
                .handle_result(&summary_result(
                    sequence,
                    summary_page(total, start, losses),
                ))
                .unwrap();
            start = start.saturating_add(count);
            if start == total {
                break;
            }
            sequence = model.load_more_losses().unwrap().sequence;
        }

        let ready = model.terminal().summary().unwrap();
        assert_eq!(ready.lost.len(), usize::from(total));
        assert_eq!(ready.lost_total_count, total);
        assert!(ready.next_lost_ordinal.is_none());
        assert!(matches!(
            model.load_more_losses(),
            Err(DeathViewClientError::NoAdditionalLossPage)
        ));
    }
}

#[test]
fn duplicate_item_or_material_across_pages_preserves_safe_summary() {
    for duplicate in [item_loss(0), material_loss(0)] {
        let mut model = model();
        let mut death = latest();
        death.destruction_entry_count = 33;
        let sequence = begin_to_summary(&mut model, death);
        let mut first = first_page_33();
        if matches!(duplicate.kind, DeathSummaryProjectionKindV1::LostItem) {
            first.lost[0] = item_loss(0);
        }
        model
            .handle_result(&summary_result(sequence, first))
            .unwrap();
        let before = model.terminal().summary().unwrap().clone();
        let continuation = model.load_more_losses().unwrap();
        let mut duplicate = duplicate;
        duplicate.ordinal = 32;
        assert!(matches!(
            model.handle_result(&summary_result(
                continuation.sequence,
                summary_page(33, 32, vec![duplicate]),
            )),
            Err(DeathViewClientError::Projection(
                DeathViewProjectionError::InvalidLossContinuation(_)
            ))
        ));
        assert_eq!(
            model.terminal().phase(),
            TerminalDeathPhase::FatalRecordError
        );
        assert_eq!(model.terminal().retained_summary(), Some(&before));
        assert!(model.terminal().summary().is_none());
        assert!(model.pending().is_none());
        assert_eq!(
            model
                .terminal()
                .action_state(DeathSummaryAction::CharacterSelect),
            DeathSummaryActionState::Disabled
        );
    }
}

#[test]
fn fatal_refresh_errors_hide_prior_summary_and_disable_actions() {
    for (code, phase) in [
        (
            DeathViewResultCodeV1::DeathNotOwned,
            TerminalDeathPhase::FatalRecordError,
        ),
        (
            DeathViewResultCodeV1::ContentMismatch,
            TerminalDeathPhase::FatalContentError,
        ),
        (
            DeathViewResultCodeV1::CorruptStoredRecord,
            TerminalDeathPhase::FatalRecordError,
        ),
    ] {
        let mut model = model();
        complete_one_loss_summary(&mut model);
        let retained = model.terminal().summary().unwrap().clone();
        let refresh = model.refresh_terminal_summary().unwrap();
        model
            .handle_result(&error_result(refresh.sequence, code))
            .unwrap();
        assert_eq!(model.terminal().phase(), phase, "{code:?}");
        assert_eq!(model.terminal().retained_summary(), Some(&retained));
        assert!(model.terminal().summary().is_none());
        for action in [
            DeathSummaryAction::InspectTrace,
            DeathSummaryAction::Memorial,
            DeathSummaryAction::CharacterSelect,
            DeathSummaryAction::CreateSuccessor,
        ] {
            assert_eq!(
                model.terminal().action_state(action),
                DeathSummaryActionState::Disabled,
                "{code:?}"
            );
        }
        assert!(matches!(
            model.refresh_terminal_summary(),
            Err(DeathViewClientError::InvalidTerminalPhase)
        ));
        assert!(matches!(
            model.load_more_losses(),
            Err(DeathViewClientError::InvalidTerminalPhase)
        ));
    }
}

#[test]
fn refresh_is_atomic_and_missing_latest_preserves_ready_summary() {
    let mut model = model();
    complete_one_loss_summary(&mut model);
    let before = model.terminal().summary().unwrap().clone();
    let refresh = model.refresh_terminal_summary().unwrap();
    model
        .handle_result(&latest_result(refresh.sequence, None))
        .unwrap();
    assert_eq!(model.terminal().summary(), Some(&before));
    assert_eq!(model.terminal().phase(), TerminalDeathPhase::SummaryReady);
    assert_eq!(
        model.terminal().failure().map(|failure| failure.code),
        Some(DeathViewResultCodeV1::DeathNotFound)
    );
    assert!(
        model
            .terminal()
            .action_state(DeathSummaryAction::CharacterSelect)
            .is_enabled()
    );
    assert!(
        model
            .terminal()
            .action_state(DeathSummaryAction::Retry)
            .is_enabled()
    );
}

#[test]
fn every_error_code_has_typed_copy_phase_and_retry_policy() {
    let cases = [
        (
            DeathViewResultCodeV1::Unauthenticated,
            TerminalDeathPhase::RecoverableError,
            DeathViewRetryDirective::Reconnect,
            true,
        ),
        (
            DeathViewResultCodeV1::FeatureDisabled,
            TerminalDeathPhase::SurfaceDisabled,
            DeathViewRetryDirective::Unavailable,
            false,
        ),
        (
            DeathViewResultCodeV1::DeathNotFound,
            TerminalDeathPhase::RecoverableError,
            DeathViewRetryDirective::RefreshLatest,
            true,
        ),
        (
            DeathViewResultCodeV1::DeathNotOwned,
            TerminalDeathPhase::FatalRecordError,
            DeathViewRetryDirective::Unavailable,
            false,
        ),
        (
            DeathViewResultCodeV1::PageOutOfRange,
            TerminalDeathPhase::RecoverableError,
            DeathViewRetryDirective::RefreshLatest,
            true,
        ),
        (
            DeathViewResultCodeV1::ContentMismatch,
            TerminalDeathPhase::FatalContentError,
            DeathViewRetryDirective::RestartAfterUpdate,
            false,
        ),
        (
            DeathViewResultCodeV1::CorruptStoredRecord,
            TerminalDeathPhase::FatalRecordError,
            DeathViewRetryDirective::Unavailable,
            false,
        ),
        (
            DeathViewResultCodeV1::ServiceUnavailable,
            TerminalDeathPhase::RecoverableError,
            DeathViewRetryDirective::RetryIdenticalQuery,
            true,
        ),
    ];
    for (code, phase, directive, retryable) in cases {
        let mut model = model();
        let request = model.begin_committed_death_lookup(CHARACTER_ID).unwrap();
        model
            .handle_result(&error_result(request.sequence, code))
            .unwrap();
        let failure = model.terminal().failure().unwrap();
        assert_eq!(model.terminal().phase(), phase, "{code:?}");
        assert_eq!(failure.retry, directive, "{code:?}");
        assert!(!failure.title.is_empty(), "{code:?}");
        assert!(!failure.detail.is_empty(), "{code:?}");
        assert_eq!(
            model
                .terminal()
                .action_state(DeathSummaryAction::Retry)
                .is_enabled(),
            retryable,
            "{code:?}"
        );
        assert!(matches!(
            model.begin_committed_death_lookup(CHARACTER_ID),
            Err(DeathViewClientError::InvalidTerminalPhase)
        ));
    }
}
