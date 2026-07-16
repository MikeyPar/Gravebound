//! Bounded, renderer-independent Memorial Wall state.
//!
//! Content `CONT-HUB-002` requires a read-only newest-first list and selection of the exact stored
//! `DTH-020` snapshot. Pages are accepted atomically, selection is anchored to a held immutable
//! entry, and the cache remains bounded even though an account's memorial history is unbounded.

use std::collections::{BTreeSet, VecDeque};

use protocol::{
    DeathMemorialCursorV1, DeathMemorialEntryV1, DeathSummaryViewV1, DeathViewContentRevisionV1,
    DeathViewResultCodeV1,
};

use super::projection::{DeathSummaryLossContinuation, project_memorial_page};
use super::{
    DeathSummaryPresentation, DeathViewClientError, DeathViewFailure, DeathViewProjectionError,
    MemorialDetailQueryIntent, MemorialEntryPresentation, MemorialPageQueryIntent,
    PendingDeathViewQuery,
};

pub const MEMORIAL_PAGE_LIMIT: u8 = protocol::DEATH_VIEW_MAX_MEMORIALS_PER_PAGE;
pub const MEMORIAL_MAX_CACHED_PAGES: usize = 8;
pub const MEMORIAL_MAX_CACHED_ENTRIES: usize =
    MEMORIAL_MAX_CACHED_PAGES * MEMORIAL_PAGE_LIMIT as usize;
/// Fixed memory for the active pagination chain's fail-closed death-ID membership filter.
pub const MEMORIAL_IDENTITY_FILTER_BYTES: usize = 256 * 1_024;
const MEMORIAL_IDENTITY_FILTER_WORDS: usize =
    MEMORIAL_IDENTITY_FILTER_BYTES / std::mem::size_of::<u64>();
const MEMORIAL_IDENTITY_FILTER_BITS: usize = MEMORIAL_IDENTITY_FILTER_BYTES * 8;
const MEMORIAL_IDENTITY_FILTER_HASHES: usize = 4;
const MEMORIAL_IDENTITY_FILTER_WORD_BITS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
struct MemorialIdentityFilter {
    words: Box<[u64]>,
    inserted_count: usize,
}

impl Default for MemorialIdentityFilter {
    fn default() -> Self {
        Self {
            words: vec![0; MEMORIAL_IDENTITY_FILTER_WORDS].into_boxed_slice(),
            inserted_count: 0,
        }
    }
}

impl MemorialIdentityFilter {
    fn from_ids(ids: &BTreeSet<[u8; 16]>) -> Self {
        let mut filter = Self::default();
        for id in ids {
            filter.insert(*id);
        }
        filter
    }

    fn might_contain(&self, id: [u8; 16]) -> bool {
        identity_filter_indices(id).all(|bit| {
            let word = bit / MEMORIAL_IDENTITY_FILTER_WORD_BITS;
            let mask = 1_u64 << (bit % MEMORIAL_IDENTITY_FILTER_WORD_BITS);
            self.words[word] & mask != 0
        })
    }

    fn insert(&mut self, id: [u8; 16]) {
        for bit in identity_filter_indices(id) {
            let word = bit / MEMORIAL_IDENTITY_FILTER_WORD_BITS;
            let mask = 1_u64 << (bit % MEMORIAL_IDENTITY_FILTER_WORD_BITS);
            self.words[word] |= mask;
        }
        self.inserted_count = self.inserted_count.saturating_add(1);
    }
}

fn identity_filter_indices(id: [u8; 16]) -> impl Iterator<Item = usize> {
    let digest = blake3::hash(&id);
    let mut indices = [0; MEMORIAL_IDENTITY_FILTER_HASHES];
    for (index, output) in indices.iter_mut().enumerate() {
        let start = index * std::mem::size_of::<u64>();
        let raw = u64::from_le_bytes(
            digest.as_bytes()[start..start + std::mem::size_of::<u64>()]
                .try_into()
                .expect("BLAKE3 output contains four u64 lanes"),
        );
        let bounded = raw
            % u64::try_from(MEMORIAL_IDENTITY_FILTER_BITS)
                .expect("identity-filter bit count fits u64");
        *output = usize::try_from(bounded).expect("bounded filter index fits usize");
    }
    indices.into_iter()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorialListPhase {
    Closed,
    LoadingInitial,
    Empty,
    Ready,
    LoadingContinuation,
    Refreshing,
    RecoverableError,
    ReadyWithRecoverableError,
    SurfaceDisabled,
    FatalContentError,
    FatalRecordError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorialDetailPhase {
    Closed,
    Loading,
    Refreshing,
    Ready,
    LoadingContinuation,
    RecoverableError,
    ReadyWithRecoverableError,
    FatalContentError,
    FatalRecordError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedMemorialPage {
    entries: Vec<MemorialEntryPresentation>,
    next_cursor: Option<DeathMemorialCursorV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorialWallModel {
    list_phase: MemorialListPhase,
    detail_phase: MemorialDetailPhase,
    pages: VecDeque<CachedMemorialPage>,
    /// Fixed-memory membership state for the active keyset-pagination chain. It has no false
    /// negatives, so an already-seen death ID cannot pass after display-page eviction. A filter
    /// collision rejects a legitimate page rather than risking duplicate historical identity.
    seen_death_ids: MemorialIdentityFilter,
    newest_pages_evicted: bool,
    selected: Option<DeathMemorialEntryV1>,
    detail_anchor: Option<DeathSummaryViewV1>,
    detail: Option<DeathSummaryPresentation>,
    failure: Option<DeathViewFailure>,
    retry_query: Option<PendingDeathViewQuery>,
}

impl Default for MemorialWallModel {
    fn default() -> Self {
        Self {
            list_phase: MemorialListPhase::Closed,
            detail_phase: MemorialDetailPhase::Closed,
            pages: VecDeque::new(),
            seen_death_ids: MemorialIdentityFilter::default(),
            newest_pages_evicted: false,
            selected: None,
            detail_anchor: None,
            detail: None,
            failure: None,
            retry_query: None,
        }
    }
}

impl MemorialWallModel {
    #[must_use]
    pub const fn list_phase(&self) -> MemorialListPhase {
        self.list_phase
    }

    #[must_use]
    pub const fn detail_phase(&self) -> MemorialDetailPhase {
        self.detail_phase
    }

    pub fn entries(&self) -> impl Iterator<Item = &DeathMemorialEntryV1> {
        let visible = self.list_is_visible();
        self.pages
            .iter()
            .filter(move |_| visible)
            .flat_map(|page| page.entries.iter())
            .map(|entry| &entry.authority)
    }

    pub fn presentations(&self) -> impl Iterator<Item = &MemorialEntryPresentation> {
        let visible = self.list_is_visible();
        self.pages
            .iter()
            .filter(move |_| visible)
            .flat_map(|page| page.entries.iter())
    }

    #[must_use]
    pub fn cached_page_count(&self) -> usize {
        self.pages.len()
    }

    #[must_use]
    pub fn cached_entry_count(&self) -> usize {
        self.pages.iter().map(|page| page.entries.len()).sum()
    }

    #[must_use]
    pub fn pagination_identity_count(&self) -> usize {
        self.seen_death_ids.inserted_count
    }

    #[must_use]
    pub const fn pagination_identity_bytes() -> usize {
        MEMORIAL_IDENTITY_FILTER_BYTES
    }

    #[must_use]
    pub const fn newest_pages_evicted(&self) -> bool {
        self.newest_pages_evicted
    }

    #[must_use]
    pub fn next_cursor(&self) -> Option<DeathMemorialCursorV1> {
        self.pages.back().and_then(|page| page.next_cursor)
    }

    #[must_use]
    pub fn can_load_older(&self) -> bool {
        matches!(
            self.list_phase,
            MemorialListPhase::Ready | MemorialListPhase::ReadyWithRecoverableError
        ) && self.detail_phase == MemorialDetailPhase::Closed
            && self.next_cursor().is_some()
    }

    #[must_use]
    pub const fn can_return_to_newest(&self) -> bool {
        self.newest_pages_evicted
    }

    #[must_use]
    pub fn selected_entry(&self) -> Option<&DeathMemorialEntryV1> {
        self.detail_is_visible()
            .then_some(self.selected.as_ref())
            .flatten()
    }

    #[must_use]
    pub fn detail(&self) -> Option<&DeathSummaryPresentation> {
        self.detail_is_visible()
            .then_some(self.detail.as_ref())
            .flatten()
    }

    #[must_use]
    pub const fn failure(&self) -> Option<&DeathViewFailure> {
        self.failure.as_ref()
    }

    pub(crate) fn validate_open(&self) -> Result<(), DeathViewClientError> {
        if self.list_phase != MemorialListPhase::Closed {
            return Err(DeathViewClientError::InvalidMemorialPhase);
        }
        Ok(())
    }

    pub(crate) fn validate_refresh(&self) -> Result<(), DeathViewClientError> {
        if !matches!(
            self.list_phase,
            MemorialListPhase::Empty
                | MemorialListPhase::Ready
                | MemorialListPhase::RecoverableError
                | MemorialListPhase::ReadyWithRecoverableError
        ) || !matches!(
            self.detail_phase,
            MemorialDetailPhase::Closed
                | MemorialDetailPhase::RecoverableError
                | MemorialDetailPhase::ReadyWithRecoverableError
        ) {
            return Err(DeathViewClientError::InvalidMemorialPhase);
        }
        Ok(())
    }

    pub(crate) fn continuation_query(&self) -> Result<PendingDeathViewQuery, DeathViewClientError> {
        if !self.can_load_older() {
            return if self.next_cursor().is_none() {
                Err(DeathViewClientError::NoAdditionalMemorialPage)
            } else {
                Err(DeathViewClientError::InvalidMemorialPhase)
            };
        }
        Ok(PendingDeathViewQuery::MemorialPage {
            after: self.next_cursor(),
            limit: MEMORIAL_PAGE_LIMIT,
            intent: MemorialPageQueryIntent::Continuation,
        })
    }

    pub(crate) fn selection_query(
        &self,
        cursor: DeathMemorialCursorV1,
    ) -> Result<PendingDeathViewQuery, DeathViewClientError> {
        if !matches!(
            self.list_phase,
            MemorialListPhase::Ready | MemorialListPhase::ReadyWithRecoverableError
        ) || self.detail_phase != MemorialDetailPhase::Closed
        {
            return Err(DeathViewClientError::InvalidMemorialPhase);
        }
        let anchor = self
            .entries()
            .find(|entry| entry.cursor == cursor)
            .cloned()
            .ok_or(DeathViewClientError::MemorialEntryNotHeld)?;
        Ok(PendingDeathViewQuery::MemorialSummary {
            anchor: Box::new(anchor),
            lost_start_ordinal: 0,
            lost_limit: super::TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
            intent: MemorialDetailQueryIntent::Initial,
        })
    }

    pub(crate) fn detail_continuation_query(
        &self,
    ) -> Result<PendingDeathViewQuery, DeathViewClientError> {
        if self.detail_phase != MemorialDetailPhase::Ready {
            return Err(DeathViewClientError::InvalidMemorialPhase);
        }
        let anchor = self
            .selected
            .clone()
            .ok_or(DeathViewClientError::MemorialEntryNotHeld)?;
        let next = self
            .detail
            .as_ref()
            .and_then(|detail| detail.next_lost_ordinal)
            .ok_or(DeathViewClientError::NoAdditionalLossPage)?;
        Ok(PendingDeathViewQuery::MemorialSummary {
            anchor: Box::new(anchor),
            lost_start_ordinal: next,
            lost_limit: super::TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
            intent: MemorialDetailQueryIntent::Continuation,
        })
    }

    pub(crate) fn close_detail(&mut self) -> Result<(), DeathViewClientError> {
        if !matches!(
            self.detail_phase,
            MemorialDetailPhase::Ready
                | MemorialDetailPhase::RecoverableError
                | MemorialDetailPhase::ReadyWithRecoverableError
                | MemorialDetailPhase::FatalContentError
                | MemorialDetailPhase::FatalRecordError
        ) {
            return Err(DeathViewClientError::InvalidMemorialPhase);
        }
        self.clear_detail();
        self.failure = None;
        self.retry_query = None;
        Ok(())
    }

    pub(crate) fn close(&mut self) -> Result<(), DeathViewClientError> {
        if matches!(
            self.list_phase,
            MemorialListPhase::LoadingInitial
                | MemorialListPhase::LoadingContinuation
                | MemorialListPhase::Refreshing
        ) || matches!(
            self.detail_phase,
            MemorialDetailPhase::Loading
                | MemorialDetailPhase::Refreshing
                | MemorialDetailPhase::LoadingContinuation
        ) {
            return Err(DeathViewClientError::InvalidMemorialPhase);
        }
        *self = Self::default();
        Ok(())
    }

    pub(crate) fn mark_query_issued(&mut self, query: &PendingDeathViewQuery) {
        self.failure = None;
        self.retry_query = None;
        match query {
            PendingDeathViewQuery::MemorialPage { intent, .. } => match intent {
                MemorialPageQueryIntent::Initial => {
                    self.list_phase = MemorialListPhase::LoadingInitial;
                    self.pages.clear();
                    self.seen_death_ids = MemorialIdentityFilter::default();
                    self.newest_pages_evicted = false;
                    self.clear_detail();
                }
                MemorialPageQueryIntent::Refresh => {
                    self.list_phase = MemorialListPhase::Refreshing;
                }
                MemorialPageQueryIntent::Continuation => {
                    self.list_phase = MemorialListPhase::LoadingContinuation;
                }
            },
            PendingDeathViewQuery::MemorialSummary { anchor, intent, .. } => match intent {
                MemorialDetailQueryIntent::Initial => {
                    if self.list_phase == MemorialListPhase::ReadyWithRecoverableError {
                        self.list_phase = MemorialListPhase::Ready;
                    }
                    self.selected = Some((**anchor).clone());
                    self.detail_anchor = None;
                    self.detail = None;
                    self.detail_phase = MemorialDetailPhase::Loading;
                }
                MemorialDetailQueryIntent::Refresh => {
                    self.detail_phase = MemorialDetailPhase::Refreshing;
                }
                MemorialDetailQueryIntent::Continuation => {
                    self.detail_phase = MemorialDetailPhase::LoadingContinuation;
                }
            },
            PendingDeathViewQuery::Latest { .. }
            | PendingDeathViewQuery::TerminalSummary { .. } => {}
        }
    }

    pub(crate) fn accept_page(
        &mut self,
        intent: MemorialPageQueryIntent,
        after: Option<DeathMemorialCursorV1>,
        entries: Vec<DeathMemorialEntryV1>,
        next_cursor: Option<DeathMemorialCursorV1>,
        required_revision: &DeathViewContentRevisionV1,
        catalog: &sim_content::CoreDevelopmentDeathView,
    ) -> Result<(), DeathViewProjectionError> {
        let entries = project_memorial_page(entries, required_revision, catalog)?;
        let page_ids = entries
            .iter()
            .map(|entry| entry.authority.cursor.death_id)
            .collect::<BTreeSet<_>>();
        if page_ids.len() != entries.len() {
            return Err(DeathViewProjectionError::InvalidMemorialPage(
                "death identity was duplicated within a page",
            ));
        }
        match intent {
            MemorialPageQueryIntent::Initial | MemorialPageQueryIntent::Refresh => {
                if after.is_some() {
                    return Err(DeathViewProjectionError::InvalidMemorialPage(
                        "newest page had a continuation anchor",
                    ));
                }
            }
            MemorialPageQueryIntent::Continuation => {
                let expected_after =
                    self.next_cursor()
                        .ok_or(DeathViewProjectionError::InvalidMemorialPage(
                            "continuation was not advertised by the prior page",
                        ))?;
                if after != Some(expected_after) || entries.is_empty() {
                    return Err(DeathViewProjectionError::InvalidMemorialPage(
                        "continuation did not follow its immutable cursor",
                    ));
                }
                if entries
                    .first()
                    .is_none_or(|entry| !memorial_precedes(expected_after, entry.authority.cursor))
                {
                    return Err(DeathViewProjectionError::InvalidMemorialPage(
                        "continuation was not strictly older",
                    ));
                }
                if entries.iter().any(|entry| {
                    self.seen_death_ids
                        .might_contain(entry.authority.cursor.death_id)
                }) {
                    return Err(DeathViewProjectionError::InvalidMemorialPage(
                        "death identity was duplicated across pages",
                    ));
                }
            }
        }

        let page = CachedMemorialPage {
            entries,
            next_cursor,
        };
        match intent {
            MemorialPageQueryIntent::Initial | MemorialPageQueryIntent::Refresh => {
                self.pages.clear();
                self.pages.push_back(page);
                self.seen_death_ids = MemorialIdentityFilter::from_ids(&page_ids);
                self.newest_pages_evicted = false;
                self.clear_detail();
            }
            MemorialPageQueryIntent::Continuation => {
                for id in page_ids {
                    self.seen_death_ids.insert(id);
                }
                self.pages.push_back(page);
                if self.pages.len() > MEMORIAL_MAX_CACHED_PAGES {
                    self.pages.pop_front();
                    self.newest_pages_evicted = true;
                }
            }
        }
        debug_assert!(self.pages.len() <= MEMORIAL_MAX_CACHED_PAGES);
        debug_assert!(self.cached_entry_count() <= MEMORIAL_MAX_CACHED_ENTRIES);
        self.list_phase = if self.cached_entry_count() == 0 {
            MemorialListPhase::Empty
        } else {
            MemorialListPhase::Ready
        };
        self.failure = None;
        self.retry_query = None;
        Ok(())
    }

    pub(crate) fn accept_detail(
        &mut self,
        intent: MemorialDetailQueryIntent,
        anchor: &DeathMemorialEntryV1,
        summary_anchor: DeathSummaryViewV1,
        presentation: DeathSummaryPresentation,
    ) -> Result<(), DeathViewClientError> {
        if self.selected.as_ref() != Some(anchor) || !self.entry_is_held(anchor) {
            return Err(DeathViewClientError::MemorialEntryNotHeld);
        }
        match intent {
            MemorialDetailQueryIntent::Initial | MemorialDetailQueryIntent::Refresh => {
                self.detail_anchor = Some(summary_anchor);
                self.detail = Some(presentation);
            }
            MemorialDetailQueryIntent::Continuation => {
                return Err(DeathViewClientError::InvalidMemorialPhase);
            }
        }
        self.detail_phase = MemorialDetailPhase::Ready;
        self.failure = None;
        self.retry_query = None;
        Ok(())
    }

    pub(crate) fn detail_anchor(&self) -> Result<&DeathSummaryViewV1, DeathViewClientError> {
        self.detail_anchor
            .as_ref()
            .ok_or(DeathViewClientError::MissingSummaryAnchor)
    }

    pub(crate) fn retained_detail(
        &self,
    ) -> Result<&DeathSummaryPresentation, DeathViewClientError> {
        self.detail
            .as_ref()
            .ok_or(DeathViewClientError::MissingSummaryAnchor)
    }

    pub(crate) fn accept_detail_continuation(
        &mut self,
        continuation: DeathSummaryLossContinuation,
    ) -> Result<(), DeathViewClientError> {
        let detail = self
            .detail
            .as_mut()
            .ok_or(DeathViewClientError::MissingSummaryAnchor)?;
        detail.lost.extend(continuation.additions);
        detail.next_lost_ordinal = continuation.next_lost_ordinal;
        self.detail_phase = MemorialDetailPhase::Ready;
        self.failure = None;
        self.retry_query = None;
        Ok(())
    }

    pub(crate) fn record_failure(
        &mut self,
        query: &PendingDeathViewQuery,
        failure: DeathViewFailure,
        retry_query: Option<PendingDeathViewQuery>,
    ) {
        let code = failure.code;
        match code {
            DeathViewResultCodeV1::FeatureDisabled => {
                self.list_phase = MemorialListPhase::SurfaceDisabled;
                self.detail_phase = MemorialDetailPhase::Closed;
            }
            DeathViewResultCodeV1::ContentMismatch => {
                self.record_fatal(
                    query,
                    MemorialListPhase::FatalContentError,
                    MemorialDetailPhase::FatalContentError,
                );
            }
            DeathViewResultCodeV1::CorruptStoredRecord => {
                self.record_fatal(
                    query,
                    MemorialListPhase::FatalRecordError,
                    MemorialDetailPhase::FatalRecordError,
                );
            }
            DeathViewResultCodeV1::DeathNotFound | DeathViewResultCodeV1::DeathNotOwned
                if matches!(query, PendingDeathViewQuery::MemorialSummary { .. }) =>
            {
                self.clear_detail();
                self.list_phase = if self.cached_entry_count() == 0 {
                    MemorialListPhase::RecoverableError
                } else {
                    MemorialListPhase::ReadyWithRecoverableError
                };
            }
            _ if matches!(query, PendingDeathViewQuery::MemorialPage { .. }) => {
                self.list_phase = if self.cached_entry_count() == 0 {
                    MemorialListPhase::RecoverableError
                } else {
                    MemorialListPhase::ReadyWithRecoverableError
                };
            }
            _ => {
                self.detail_phase = if self.detail.is_some() {
                    MemorialDetailPhase::ReadyWithRecoverableError
                } else {
                    MemorialDetailPhase::RecoverableError
                };
            }
        }
        self.failure = Some(failure);
        self.retry_query = retry_query;
    }

    pub(crate) fn record_unrenderable_failure(
        &mut self,
        query: &PendingDeathViewQuery,
        content_failure: bool,
        failure: Option<DeathViewFailure>,
    ) {
        self.record_fatal(
            query,
            if content_failure {
                MemorialListPhase::FatalContentError
            } else {
                MemorialListPhase::FatalRecordError
            },
            if content_failure {
                MemorialDetailPhase::FatalContentError
            } else {
                MemorialDetailPhase::FatalRecordError
            },
        );
        self.failure = failure;
        self.retry_query = None;
    }

    #[must_use]
    pub(crate) fn retry_query(&self) -> Option<PendingDeathViewQuery> {
        self.retry_query.clone()
    }

    fn record_fatal(
        &mut self,
        query: &PendingDeathViewQuery,
        list_phase: MemorialListPhase,
        detail_phase: MemorialDetailPhase,
    ) {
        if matches!(query, PendingDeathViewQuery::MemorialPage { .. }) {
            self.list_phase = list_phase;
            self.clear_detail();
        } else {
            self.detail_phase = detail_phase;
        }
    }

    fn list_is_visible(&self) -> bool {
        matches!(
            self.list_phase,
            MemorialListPhase::Empty
                | MemorialListPhase::Ready
                | MemorialListPhase::LoadingContinuation
                | MemorialListPhase::Refreshing
                | MemorialListPhase::ReadyWithRecoverableError
        )
    }

    fn detail_is_visible(&self) -> bool {
        matches!(
            self.detail_phase,
            MemorialDetailPhase::Ready
                | MemorialDetailPhase::Refreshing
                | MemorialDetailPhase::LoadingContinuation
                | MemorialDetailPhase::ReadyWithRecoverableError
        )
    }

    fn entry_is_held(&self, anchor: &DeathMemorialEntryV1) -> bool {
        self.pages
            .iter()
            .flat_map(|page| page.entries.iter())
            .any(|entry| &entry.authority == anchor)
    }

    fn clear_detail(&mut self) {
        self.selected = None;
        self.detail_anchor = None;
        self.detail = None;
        self.detail_phase = MemorialDetailPhase::Closed;
    }
}

fn memorial_precedes(left: DeathMemorialCursorV1, right: DeathMemorialCursorV1) -> bool {
    left.death_at_unix_ms > right.death_at_unix_ms
        || (left.death_at_unix_ms == right.death_at_unix_ms && left.death_id < right.death_id)
}
