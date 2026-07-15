use protocol::{
    DEATH_VIEW_SCHEMA_VERSION, DeathCharacterName, DeathEchoOutcomeV1, DeathMemorialCursorV1,
    DeathMemorialEntryV1, DeathViewRequestV1, DeathViewResultCodeV1, DeathViewResultV1, WireText,
};

use super::tests::{
    fixed_created, fixed_preserved, item_loss, model as new_model, revision, summary_page, uuid_v7,
};
use super::*;

fn memorial_id(index: u16) -> [u8; 16] {
    let mut value = uuid_v7(40);
    value[14..].copy_from_slice(&index.to_be_bytes());
    value
}

fn memorial_entry(index: u16, death_at_unix_ms: u64) -> DeathMemorialEntryV1 {
    DeathMemorialEntryV1 {
        cursor: DeathMemorialCursorV1 {
            death_at_unix_ms,
            death_id: memorial_id(index),
        },
        summary_revision: 1,
        summary_snapshot_digest: [4; 32],
        presentation_key: WireText::new("memorial.presentation.core_default").unwrap(),
        presentation_digest: [5; 32],
        character_name_snapshot: DeathCharacterName::new(format!("Hero {index}")).unwrap(),
        class_id: WireText::new("class.grave_arbalist").unwrap(),
        level: 10,
        echo_outcome: DeathEchoOutcomeV1::Available,
        presentation_revision: revision(),
    }
}

fn detail_entry() -> DeathMemorialEntryV1 {
    let mut entry = memorial_entry(1, 1_000);
    entry.cursor.death_id = uuid_v7(1);
    entry.character_name_snapshot = DeathCharacterName::new("Mara Ash").unwrap();
    entry
}

fn page_entries(start: u16, count: u16, newest_ms: u64) -> Vec<DeathMemorialEntryV1> {
    (0..count)
        .map(|offset| memorial_entry(start + offset, newest_ms - u64::from(offset)))
        .collect()
}

fn page_result(
    sequence: u32,
    entries: Vec<DeathMemorialEntryV1>,
    has_more: bool,
) -> DeathViewResultV1 {
    let next_cursor = has_more.then(|| entries.last().unwrap().cursor);
    DeathViewResultV1::MemorialPage {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        request_sequence: sequence,
        requested_limit: MEMORIAL_PAGE_LIMIT,
        entries,
        next_cursor,
    }
}

fn summary_result(sequence: u32, summary: protocol::DeathSummaryViewV1) -> DeathViewResultV1 {
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

fn open_with_entries(
    model: &mut DeathViewClientModel,
    entries: Vec<DeathMemorialEntryV1>,
    has_more: bool,
) {
    let request = model.open_memorial_wall().unwrap();
    model
        .handle_result(&page_result(request.sequence, entries, has_more))
        .unwrap();
}

#[test]
fn station_open_is_bounded_and_empty_is_explicit() {
    let mut model = new_model();
    let request = model.open_memorial_wall().unwrap();
    assert_eq!(
        request.request,
        DeathViewRequestV1::MemorialPage {
            after: None,
            limit: 32,
        }
    );
    assert_eq!(
        model.memorial().list_phase(),
        MemorialListPhase::LoadingInitial
    );
    assert_eq!(model.memorial().entries().count(), 0);

    model
        .handle_result(&page_result(request.sequence, Vec::new(), false))
        .unwrap();
    assert_eq!(model.memorial().list_phase(), MemorialListPhase::Empty);
    assert_eq!(model.memorial().cached_entry_count(), 0);
    assert!(!model.memorial().can_load_older());
}

#[test]
fn continuation_enforces_cross_page_order_and_death_identity_uniqueness() {
    let mut model = new_model();
    let mut duplicate_initial = vec![memorial_entry(1, 11_000), memorial_entry(2, 10_999)];
    duplicate_initial[1].cursor.death_id = duplicate_initial[0].cursor.death_id;
    let request = model.open_memorial_wall().unwrap();
    assert!(matches!(
        model.handle_result(&page_result(request.sequence, duplicate_initial, false)),
        Err(DeathViewClientError::Projection(
            DeathViewProjectionError::InvalidMemorialPage(
                "death identity was duplicated within a page"
            )
        ))
    ));

    let first_page = page_entries(1, 32, 10_000);
    let first_cursor = first_page.last().unwrap().cursor;
    let mut model = new_model();
    open_with_entries(&mut model, first_page.clone(), true);
    let request = model.load_older_memorials().unwrap();
    assert_eq!(
        request.request,
        DeathViewRequestV1::MemorialPage {
            after: Some(first_cursor),
            limit: 32,
        }
    );

    let mut duplicate_page = page_entries(33, 2, 9_000);
    duplicate_page[1].cursor.death_id = first_page[0].cursor.death_id;
    assert!(matches!(
        model.handle_result(&page_result(request.sequence, duplicate_page, false)),
        Err(DeathViewClientError::Projection(
            DeathViewProjectionError::InvalidMemorialPage(
                "death identity was duplicated across pages"
            )
        ))
    ));
    assert_eq!(
        model.memorial().list_phase(),
        MemorialListPhase::FatalRecordError
    );

    let mut model = new_model();
    open_with_entries(&mut model, first_page, true);
    let request = model.load_older_memorials().unwrap();
    let not_older = vec![memorial_entry(80, first_cursor.death_at_unix_ms + 1)];
    assert!(matches!(
        model.handle_result(&page_result(request.sequence, not_older, false)),
        Err(DeathViewClientError::Projection(
            DeathViewProjectionError::InvalidMemorialPage("continuation was not strictly older")
        ))
    ));
}

#[test]
fn equal_timestamp_ties_use_death_id_and_refresh_replaces_atomically() {
    let mut first_page = page_entries(1, 32, 20_000);
    first_page[31].cursor.death_at_unix_ms = 19_000;
    first_page[31].cursor.death_id = memorial_id(100);
    let after = first_page[31].cursor;
    let mut model = new_model();
    open_with_entries(&mut model, first_page, true);

    let continuation = model.load_older_memorials().unwrap();
    let tied_older = vec![memorial_entry(101, after.death_at_unix_ms)];
    model
        .handle_result(&page_result(continuation.sequence, tied_older, false))
        .unwrap();
    assert_eq!(model.memorial().entries().count(), 33);

    let retained = model
        .memorial()
        .entries()
        .map(|entry| entry.cursor)
        .collect::<Vec<_>>();
    let refresh = model.refresh_memorial_wall().unwrap();
    assert_eq!(model.memorial().list_phase(), MemorialListPhase::Refreshing);
    assert_eq!(
        model
            .memorial()
            .entries()
            .map(|entry| entry.cursor)
            .collect::<Vec<_>>(),
        retained
    );
    let newer = vec![memorial_entry(200, 30_000)];
    model
        .handle_result(&page_result(refresh.sequence, newer.clone(), false))
        .unwrap();
    assert_eq!(
        model.memorial().entries().cloned().collect::<Vec<_>>(),
        newer
    );
}

#[test]
fn recoverable_refresh_and_response_loss_preserve_safe_state_and_retry_exactly() {
    let mut model = new_model();
    let entries = vec![detail_entry()];
    open_with_entries(&mut model, entries.clone(), false);

    let refresh = model.refresh_memorial_wall().unwrap();
    model
        .handle_result(&error_result(
            refresh.sequence,
            DeathViewResultCodeV1::ServiceUnavailable,
        ))
        .unwrap();
    assert_eq!(
        model.memorial().list_phase(),
        MemorialListPhase::ReadyWithRecoverableError
    );
    assert_eq!(
        model.memorial().entries().cloned().collect::<Vec<_>>(),
        entries
    );
    let retry = model.retry_memorial().unwrap();
    assert!(retry.sequence > refresh.sequence);
    assert_eq!(retry.request, refresh.request);

    model.handle_response_loss().unwrap();
    assert_eq!(
        model.memorial().list_phase(),
        MemorialListPhase::ReadyWithRecoverableError
    );
    let retry_after_loss = model.retry_memorial().unwrap();
    assert!(retry_after_loss.sequence > retry.sequence);
    assert_eq!(retry_after_loss.request, refresh.request);
}

#[test]
fn selection_requires_a_held_immutable_anchor_and_projects_stored_detail() {
    let mut model = new_model();
    let entry = detail_entry();
    open_with_entries(&mut model, vec![entry.clone()], false);
    assert!(matches!(
        model.select_memorial(memorial_entry(99, 50).cursor),
        Err(DeathViewClientError::MemorialEntryNotHeld)
    ));

    let request = model.select_memorial(entry.cursor).unwrap();
    let summary = summary_page(1, 0, vec![item_loss(0)]);
    model
        .handle_result(&summary_result(request.sequence, summary))
        .unwrap();
    let memorial = model.memorial();
    assert_eq!(memorial.detail_phase(), MemorialDetailPhase::Ready);
    assert_eq!(memorial.selected_entry(), Some(&entry));
    let detail = memorial.detail().unwrap();
    assert_eq!(detail.context, DeathSummaryContext::Memorial);
    assert_eq!(detail.character_id, None);
    assert_eq!(detail.death_at_unix_ms, entry.cursor.death_at_unix_ms);
    assert_eq!(detail.hero.character_name, "Mara Ash");
    assert_eq!(detail.lethal_cause.cause, None);
    assert_eq!(
        detail.lethal_cause.killer.content_id,
        "miniboss.sepulcher_knight"
    );
    assert_eq!(
        detail.actions.primary.state,
        DeathSummaryActionState::Disabled
    );
    assert!(detail.actions.primary.unavailable_detail.is_some());
    assert_eq!(
        detail
            .actions
            .secondary
            .iter()
            .map(|action| action.state)
            .collect::<Vec<_>>(),
        vec![
            DeathSummaryActionState::Enabled,
            DeathSummaryActionState::Disabled,
            DeathSummaryActionState::Disabled,
        ]
    );
    assert_eq!(entry.presentation_digest, [5; 32]);
}

#[test]
fn selecting_after_a_list_error_dismisses_only_that_stale_error() {
    let mut model = new_model();
    let entry = detail_entry();
    open_with_entries(&mut model, vec![entry.clone()], false);
    let refresh = model.refresh_memorial_wall().unwrap();
    model
        .handle_result(&error_result(
            refresh.sequence,
            DeathViewResultCodeV1::ServiceUnavailable,
        ))
        .unwrap();
    assert!(model.memorial().failure().is_some());

    let detail = model.select_memorial(entry.cursor).unwrap();
    assert_eq!(model.memorial().list_phase(), MemorialListPhase::Ready);
    assert!(model.memorial().failure().is_none());
    model
        .handle_result(&summary_result(
            detail.sequence,
            summary_page(1, 0, vec![item_loss(0)]),
        ))
        .unwrap();
    assert!(matches!(
        model.retry_memorial(),
        Err(DeathViewClientError::NoRetryAvailable)
    ));
}

#[test]
fn every_memorial_detail_anchor_mismatch_fails_closed() {
    let mut cases = Vec::new();
    let mut wrong = summary_page(1, 0, vec![item_loss(0)]);
    wrong.snapshot_digest = [9; 32];
    cases.push(wrong);
    let mut wrong = summary_page(1, 0, vec![item_loss(0)]);
    wrong.character_name_snapshot = DeathCharacterName::new("Foreign Hero").unwrap();
    cases.push(wrong);
    let mut wrong = summary_page(1, 0, vec![item_loss(0)]);
    wrong.class_id = WireText::new("class.vanguard").unwrap();
    cases.push(wrong);
    let mut wrong = summary_page(1, 0, vec![item_loss(0)]);
    wrong.level = 9;
    cases.push(wrong);
    let mut wrong = summary_page(1, 0, vec![item_loss(0)]);
    wrong.echo_outcome = DeathEchoOutcomeV1::Dormant;
    cases.push(wrong);

    for wrong in cases {
        let mut model = new_model();
        let entry = detail_entry();
        open_with_entries(&mut model, vec![entry.clone()], false);
        let request = model.select_memorial(entry.cursor).unwrap();
        let result = model.handle_result(&summary_result(request.sequence, wrong));
        assert!(matches!(
            result,
            Err(DeathViewClientError::Projection(
                DeathViewProjectionError::AnchorMismatch("memorial entry and summary")
            ))
        ));
        assert_eq!(
            model.memorial().detail_phase(),
            MemorialDetailPhase::FatalRecordError
        );
        assert!(model.memorial().detail().is_none());
        assert_eq!(model.memorial().entries().count(), 1);
    }
}

#[test]
fn memorial_detail_loss_pages_are_anchored_and_append_atomically() {
    let mut model = new_model();
    let entry = detail_entry();
    open_with_entries(&mut model, vec![entry.clone()], false);
    let request = model.select_memorial(entry.cursor).unwrap();
    let mut first = summary_page(33, 0, (0..32).map(item_loss).collect());
    first.preserved = fixed_preserved();
    first.created = fixed_created();
    model
        .handle_result(&summary_result(request.sequence, first))
        .unwrap();
    assert_eq!(model.memorial().detail().unwrap().lost.len(), 32);

    let continuation = model.load_more_memorial_losses().unwrap();
    let last = summary_page(33, 32, vec![item_loss(32)]);
    model
        .handle_result(&summary_result(continuation.sequence, last))
        .unwrap();
    let detail = model.memorial().detail().unwrap();
    assert_eq!(detail.lost.len(), 33);
    assert_eq!(detail.next_lost_ordinal, None);
}

#[test]
fn detail_page_refresh_preserves_the_last_safe_snapshot_until_replacement() {
    let mut model = new_model();
    let entry = detail_entry();
    open_with_entries(&mut model, vec![entry.clone()], false);
    let request = model.select_memorial(entry.cursor).unwrap();
    let first = summary_page(33, 0, (0..32).map(item_loss).collect());
    model
        .handle_result(&summary_result(request.sequence, first))
        .unwrap();

    let continuation = model.load_more_memorial_losses().unwrap();
    model
        .handle_result(&error_result(
            continuation.sequence,
            DeathViewResultCodeV1::PageOutOfRange,
        ))
        .unwrap();
    assert_eq!(
        model.memorial().detail_phase(),
        MemorialDetailPhase::ReadyWithRecoverableError
    );
    assert_eq!(model.memorial().detail().unwrap().lost.len(), 32);

    let refresh = model.retry_memorial().unwrap();
    assert_eq!(
        model.memorial().detail_phase(),
        MemorialDetailPhase::Refreshing
    );
    assert_eq!(model.memorial().detail().unwrap().lost.len(), 32);
    assert_eq!(
        refresh.request,
        DeathViewRequestV1::Summary {
            death_id: entry.cursor.death_id,
            lost_start_ordinal: 0,
            lost_limit: TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
        }
    );
    model.handle_response_loss().unwrap();
    assert_eq!(
        model.memorial().detail_phase(),
        MemorialDetailPhase::ReadyWithRecoverableError
    );
    assert_eq!(model.memorial().detail().unwrap().lost.len(), 32);
}

#[test]
fn death_not_found_returns_detail_to_the_safe_list_and_refreshes() {
    let mut model = new_model();
    let entry = detail_entry();
    open_with_entries(&mut model, vec![entry.clone()], false);
    let request = model.select_memorial(entry.cursor).unwrap();
    model
        .handle_result(&error_result(
            request.sequence,
            DeathViewResultCodeV1::DeathNotFound,
        ))
        .unwrap();
    assert_eq!(model.memorial().detail_phase(), MemorialDetailPhase::Closed);
    assert_eq!(
        model.memorial().list_phase(),
        MemorialListPhase::ReadyWithRecoverableError
    );
    assert_eq!(model.memorial().entries().count(), 1);
    let refresh = model.retry_memorial().unwrap();
    assert_eq!(
        refresh.request,
        DeathViewRequestV1::MemorialPage {
            after: None,
            limit: MEMORIAL_PAGE_LIMIT,
        }
    );
}

#[test]
fn every_memorial_result_code_has_contextual_visibility_and_retry_policy() {
    let cases = [
        (
            DeathViewResultCodeV1::Unauthenticated,
            MemorialListPhase::ReadyWithRecoverableError,
            DeathViewRetryDirective::Reconnect,
            true,
        ),
        (
            DeathViewResultCodeV1::FeatureDisabled,
            MemorialListPhase::SurfaceDisabled,
            DeathViewRetryDirective::Unavailable,
            false,
        ),
        (
            DeathViewResultCodeV1::DeathNotFound,
            MemorialListPhase::ReadyWithRecoverableError,
            DeathViewRetryDirective::RefreshMemorial,
            true,
        ),
        (
            DeathViewResultCodeV1::DeathNotOwned,
            MemorialListPhase::ReadyWithRecoverableError,
            DeathViewRetryDirective::Unavailable,
            false,
        ),
        (
            DeathViewResultCodeV1::PageOutOfRange,
            MemorialListPhase::ReadyWithRecoverableError,
            DeathViewRetryDirective::RefreshMemorial,
            true,
        ),
        (
            DeathViewResultCodeV1::ContentMismatch,
            MemorialListPhase::FatalContentError,
            DeathViewRetryDirective::RestartAfterUpdate,
            false,
        ),
        (
            DeathViewResultCodeV1::CorruptStoredRecord,
            MemorialListPhase::FatalRecordError,
            DeathViewRetryDirective::Unavailable,
            false,
        ),
        (
            DeathViewResultCodeV1::ServiceUnavailable,
            MemorialListPhase::ReadyWithRecoverableError,
            DeathViewRetryDirective::RetryIdenticalQuery,
            true,
        ),
    ];

    for (code, expected_phase, expected_retry, retryable) in cases {
        let mut model = new_model();
        open_with_entries(&mut model, vec![detail_entry()], false);
        let refresh = model.refresh_memorial_wall().unwrap();
        model
            .handle_result(&error_result(refresh.sequence, code))
            .unwrap();
        assert_eq!(model.memorial().list_phase(), expected_phase, "{code:?}");
        assert_eq!(
            model.memorial().failure().unwrap().retry,
            expected_retry,
            "{code:?}"
        );
        if matches!(expected_phase, MemorialListPhase::ReadyWithRecoverableError) {
            assert_eq!(model.memorial().entries().count(), 1, "{code:?}");
        } else {
            assert_eq!(model.memorial().entries().count(), 0, "{code:?}");
        }
        assert_eq!(model.retry_memorial().is_ok(), retryable, "{code:?}");
    }
}

#[test]
fn cache_is_bounded_to_eight_pages_and_keeps_forward_and_newest_routes() {
    let mut model = new_model();
    open_with_entries(&mut model, page_entries(0, 32, 100_000), true);
    for page_index in 1..9_u16 {
        let request = model.load_older_memorials().unwrap();
        let has_more = page_index < 8;
        let entries = page_entries(page_index * 32, 32, 100_000 - u64::from(page_index) * 1_000);
        model
            .handle_result(&page_result(request.sequence, entries, has_more))
            .unwrap();
    }
    assert_eq!(model.memorial().cached_page_count(), 8);
    assert_eq!(model.memorial().cached_entry_count(), 256);
    assert_eq!(model.memorial().pagination_identity_count(), 288);
    assert_eq!(
        MemorialWallModel::pagination_identity_bytes(),
        MEMORIAL_IDENTITY_FILTER_BYTES
    );
    assert!(model.memorial().newest_pages_evicted());
    assert!(model.memorial().can_return_to_newest());
    assert!(!model.memorial().can_load_older());

    let refresh = model.refresh_memorial_wall().unwrap();
    model
        .handle_result(&page_result(
            refresh.sequence,
            vec![memorial_entry(999, 200_000)],
            false,
        ))
        .unwrap();
    assert!(!model.memorial().can_return_to_newest());
    assert_eq!(model.memorial().cached_page_count(), 1);
    assert_eq!(model.memorial().pagination_identity_count(), 1);
}

#[test]
fn identity_uniqueness_survives_display_page_eviction() {
    let mut model = new_model();
    let evicted_id = memorial_id(0);
    open_with_entries(&mut model, page_entries(0, 32, 100_000), true);
    for page_index in 1..9_u16 {
        let request = model.load_older_memorials().unwrap();
        model
            .handle_result(&page_result(
                request.sequence,
                page_entries(page_index * 32, 32, 100_000 - u64::from(page_index) * 1_000),
                true,
            ))
            .unwrap();
    }
    assert!(model.memorial().newest_pages_evicted());
    assert!(
        model
            .memorial()
            .entries()
            .all(|entry| entry.cursor.death_id != evicted_id)
    );

    let request = model.load_older_memorials().unwrap();
    let mut duplicate = memorial_entry(999, 80_000);
    duplicate.cursor.death_id = evicted_id;
    assert!(matches!(
        model.handle_result(&page_result(request.sequence, vec![duplicate], false)),
        Err(DeathViewClientError::Projection(
            DeathViewProjectionError::InvalidMemorialPage(
                "death identity was duplicated across pages"
            )
        ))
    ));
}

#[test]
fn stale_and_wrong_kind_results_cannot_mutate_memorial_state() {
    let mut model = new_model();
    let request = model.open_memorial_wall().unwrap();
    let stale = page_result(request.sequence + 10, vec![memorial_entry(1, 1_000)], false);
    assert_eq!(
        model.handle_result(&stale).unwrap().disposition,
        DeathViewApplyDisposition::IgnoredStale
    );
    assert_eq!(
        model.memorial().list_phase(),
        MemorialListPhase::LoadingInitial
    );

    let wrong_kind = summary_result(request.sequence, summary_page(1, 0, vec![item_loss(0)]));
    assert_eq!(
        model.handle_result(&wrong_kind).unwrap().disposition,
        DeathViewApplyDisposition::IgnoredUnexpectedKind
    );
    assert_eq!(
        model.memorial().list_phase(),
        MemorialListPhase::LoadingInitial
    );
}
