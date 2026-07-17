//! Disposable real-widget driver for `GB-M03-06D` native death/Memorial evidence.

use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result};
use bevy::{
    app::AppExit,
    asset::io::{AssetSourceBuilder, AssetSourceId, file::FileAssetReader},
    prelude::*,
    render::view::screenshot::Screenshot,
    window::{PresentMode, PrimaryWindow, WindowResolution},
};
use protocol::{
    DEATH_SUMMARY_REVISION, DEATH_VIEW_SCHEMA_VERSION, DeathCharacterName, DeathDamageTypeV1,
    DeathEchoOutcomeV1, DeathMemorialCursorV1, DeathMemorialEntryV1, DeathNetworkStateV1,
    DeathRecallStateV1, DeathSummaryProjectionEntryV1, DeathSummaryProjectionKindV1,
    DeathSummaryViewV1, DeathTraceEntryV1, DeathTraceStatusV1, DeathViewContentRevisionV1,
    DeathViewResultV1, LatestCommittedDeathV1, ManifestHash, WireText,
};
use sim_content::{CoreDevelopmentDeathView, load_core_development_death_view};

use crate::{
    DeathSummaryAction, DeathUiAction, DeathUiCommand, DeathUiConfig, DeathUiFocusRequest,
    DeathUiRenderReadiness, DeathUiScrollState, DeathUiSnapshot, NativeDeathView,
    NativeDeathViewPlugin, TERMINAL_SUMMARY_LOSS_PAGE_LIMIT, validate_death_ui_assets,
};

const EVIDENCE_SETTLE_FRAMES: u8 = 90;
pub(crate) const CHARACTER_ID: [u8; 16] = [0x42; 16];
const DEATH_AT_UNIX_MS: u64 = 1_784_167_200_000;
const DEATH_TICK: u64 = 12_480;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreDeathViewShowcaseState {
    Summary,
    SummaryActions,
    SummaryTrace,
    MemorialList,
    MemorialDetail,
    AwaitingCommit,
    RecoverableError,
}

#[derive(Debug, Clone)]
pub struct CoreDeathViewShowcaseConfig {
    pub content_root: PathBuf,
    pub reduced_effects: bool,
    pub ui_scale_percent: u16,
    pub state: CoreDeathViewShowcaseState,
}

#[derive(Debug, Clone, Resource)]
struct ShowcaseDeck {
    terminal: DeathUiSnapshot,
    terminal_trace: DeathUiSnapshot,
    memorial_list: DeathUiSnapshot,
    memorial_detail: DeathUiSnapshot,
    memorial_details: Vec<ShowcaseMemorialDetail>,
    awaiting_commit: DeathUiSnapshot,
    recoverable_error: DeathUiSnapshot,
}

#[derive(Debug, Clone)]
struct ShowcaseMemorialDetail {
    cursor: DeathMemorialCursorV1,
    summary: DeathUiSnapshot,
    trace: DeathUiSnapshot,
}

impl ShowcaseDeck {
    fn snapshot(&self, state: CoreDeathViewShowcaseState) -> DeathUiSnapshot {
        match state {
            CoreDeathViewShowcaseState::Summary | CoreDeathViewShowcaseState::SummaryActions => {
                self.terminal.clone()
            }
            CoreDeathViewShowcaseState::SummaryTrace => self.terminal_trace.clone(),
            CoreDeathViewShowcaseState::MemorialList => self.memorial_list.clone(),
            CoreDeathViewShowcaseState::MemorialDetail => self.memorial_detail.clone(),
            CoreDeathViewShowcaseState::AwaitingCommit => self.awaiting_commit.clone(),
            CoreDeathViewShowcaseState::RecoverableError => self.recoverable_error.clone(),
        }
    }

    fn memorial_detail(&self, cursor: DeathMemorialCursorV1) -> Option<&DeathUiSnapshot> {
        self.memorial_details
            .iter()
            .find(|detail| detail.cursor == cursor)
            .map(|detail| &detail.summary)
    }

    fn memorial_trace(&self, death_id: [u8; 16]) -> Option<&DeathUiSnapshot> {
        self.memorial_details
            .iter()
            .find(|detail| detail.cursor.death_id == death_id)
            .map(|detail| &detail.trace)
    }
}

#[derive(Debug, Resource)]
struct ScreenshotRequest(PathBuf);

#[derive(Debug, Resource)]
struct ShowcaseInitialFocus {
    next: bool,
    issued: bool,
}

#[derive(Debug, Default)]
struct CaptureProgress {
    settled_frames: u8,
    queued: bool,
}

pub fn run_core_death_view_showcase(config: &CoreDeathViewShowcaseConfig) -> Result<()> {
    let content_root = fs::canonicalize(&config.content_root).with_context(|| {
        format!(
            "could not resolve content root {}",
            config.content_root.display()
        )
    })?;
    let repository_root = content_root
        .parent()
        .context("content root has no repository parent")?;
    let asset_root = repository_root.join("assets");
    validate_death_ui_assets(&asset_root)?;
    let catalog = load_core_development_death_view(&content_root)
        .context("unpromoted Core death presentation failed validation")?;
    let deck = build_showcase_deck(catalog)?;
    let native_view = NativeDeathView::new(
        deck.snapshot(config.state),
        DeathUiConfig {
            reduced_effects: config.reduced_effects,
            ui_scale_percent: config.ui_scale_percent,
        },
    )?;
    let (width, height) = crate::configured_window_size()?;
    let screenshot = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);
    let asset_reader_root = asset_root.clone();
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(asset_reader_root.clone()))),
    )
    .insert_resource(ClearColor(Color::srgb_u8(5, 7, 8)))
    .insert_resource(deck)
    .insert_resource(native_view)
    .insert_resource(ShowcaseInitialFocus {
        next: config.state == CoreDeathViewShowcaseState::SummaryActions,
        issued: false,
    })
    .add_plugins(
        crate::gravebound_default_plugins()
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Gravebound".to_owned(),
                    resolution: WindowResolution::new(width, height),
                    present_mode: PresentMode::AutoVsync,
                    resizable: true,
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins(NativeDeathViewPlugin)
    .add_systems(Startup, spawn_camera)
    .add_systems(
        Update,
        (handle_showcase_commands, apply_showcase_initial_focus),
    );
    if let Some(path) = screenshot {
        app.insert_resource(ScreenshotRequest(path))
            .add_systems(Update, capture_evidence);
    }
    app.run();
    Ok(())
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((Camera2d, IsDefaultUiCamera, BoxShadowSamples(6)));
}

#[allow(clippy::needless_pass_by_value)]
fn apply_showcase_initial_focus(
    readiness: Res<DeathUiRenderReadiness>,
    mut initial: ResMut<ShowcaseInitialFocus>,
    mut requests: MessageWriter<DeathUiFocusRequest>,
) {
    if initial.next && !initial.issued && readiness.is_ready() {
        requests.write(DeathUiFocusRequest::Next);
        initial.issued = true;
    }
}

#[allow(clippy::needless_pass_by_value)]
fn handle_showcase_commands(
    mut commands: MessageReader<DeathUiCommand>,
    deck: Res<ShowcaseDeck>,
    mut view: ResMut<NativeDeathView>,
    mut exit: MessageWriter<AppExit>,
) {
    for command in commands.read() {
        match &command.0 {
            DeathUiAction::Summary(DeathSummaryAction::InspectTrace) => {
                let snapshot = if view.snapshot().surface == crate::DeathUiSurface::MemorialDetail {
                    view.snapshot()
                        .summary
                        .as_ref()
                        .and_then(|summary| deck.memorial_trace(summary.death_id))
                        .cloned()
                } else {
                    Some(deck.terminal_trace.clone())
                };
                if let Some(snapshot) = snapshot {
                    replace_showcase_snapshot(&mut view, snapshot);
                }
            }
            DeathUiAction::Summary(DeathSummaryAction::Memorial) => {
                replace_showcase_snapshot(&mut view, deck.memorial_list.clone());
            }
            DeathUiAction::Summary(DeathSummaryAction::CharacterSelect) => {
                exit.write(AppExit::Success);
            }
            DeathUiAction::MemorialEntry(cursor) => {
                if let Some(snapshot) = deck.memorial_detail(*cursor) {
                    replace_showcase_snapshot(&mut view, snapshot.clone());
                }
            }
            DeathUiAction::Back => match view.snapshot().surface {
                crate::DeathUiSurface::MemorialDetail => {
                    replace_showcase_snapshot(&mut view, deck.memorial_list.clone());
                }
                crate::DeathUiSurface::MemorialList => {
                    replace_showcase_snapshot(&mut view, deck.terminal.clone());
                }
                crate::DeathUiSurface::TerminalSummary => {}
            },
            DeathUiAction::Retry | DeathUiAction::Summary(DeathSummaryAction::Retry) => {
                replace_showcase_snapshot(&mut view, deck.awaiting_commit.clone());
            }
            DeathUiAction::Summary(DeathSummaryAction::CreateSuccessor)
            | DeathUiAction::LoadMoreLosses
            | DeathUiAction::LoadOlderMemorials => {}
        }
    }
}

fn replace_showcase_snapshot(view: &mut NativeDeathView, snapshot: DeathUiSnapshot) {
    if let Err(error) = view.replace_snapshot(snapshot) {
        error!(%error, "rejected invalid native death evidence snapshot");
    }
}

#[allow(clippy::needless_pass_by_value)]
fn capture_evidence(
    mut commands: Commands,
    request: Res<ScreenshotRequest>,
    readiness: Res<DeathUiRenderReadiness>,
    scroll: Res<DeathUiScrollState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut progress: Local<CaptureProgress>,
) {
    if progress.queued || !readiness.is_ready() || windows.single().is_err() {
        return;
    }
    progress.settled_frames = progress.settled_frames.saturating_add(1);
    if progress.settled_frames >= EVIDENCE_SETTLE_FRAMES {
        progress.queued = true;
        info!(
            overflow = scroll.has_overflow(),
            offset = scroll.offset(),
            max_offset = scroll.max_offset(),
            "death evidence surface settled"
        );
        commands
            .spawn(Screenshot::primary_window())
            .observe(crate::save_screenshot_atomically(request.0.clone()));
    }
}

fn build_showcase_deck(catalog: CoreDevelopmentDeathView) -> Result<ShowcaseDeck> {
    let terminal_model = ready_terminal_model(catalog.clone())?;
    let terminal = DeathUiSnapshot::terminal(&terminal_model)?;
    let terminal_trace = terminal.clone().with_trace_emphasis(true);

    let memorial_model = ready_memorial_model(catalog.clone(), None)?;
    let memorial_list = DeathUiSnapshot::memorial_list(&memorial_model)?;
    let memorial_details = memorial_list
        .memorial_entries
        .iter()
        .map(|entry| {
            let cursor = entry.authority.cursor;
            let model = ready_memorial_model(catalog.clone(), Some(cursor))?;
            let summary = DeathUiSnapshot::memorial_detail(&model)?;
            let trace = summary.clone().with_trace_emphasis(true);
            Ok(ShowcaseMemorialDetail {
                cursor,
                summary,
                trace,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let default_memorial = memorial_details
        .first()
        .context("Memorial evidence requires at least one stored death")?;
    let memorial_detail = default_memorial.summary.clone();

    let mut awaiting_model = crate::DeathViewClientModel::new(catalog.clone())?;
    awaiting_model.observe_local_health_zero(CHARACTER_ID)?;
    let awaiting_commit = DeathUiSnapshot::terminal(&awaiting_model)?;

    let mut error_model = crate::DeathViewClientModel::new(catalog)?;
    error_model.begin_committed_death_lookup(CHARACTER_ID)?;
    error_model.handle_response_loss()?;
    let recoverable_error = DeathUiSnapshot::terminal(&error_model)?;

    Ok(ShowcaseDeck {
        terminal,
        terminal_trace,
        memorial_list,
        memorial_detail,
        memorial_details,
        awaiting_commit,
        recoverable_error,
    })
}

fn ready_terminal_model(catalog: CoreDevelopmentDeathView) -> Result<crate::DeathViewClientModel> {
    let revision = revision(&catalog)?;
    let content_revision = catalog.item_content_revision().to_owned();
    let mut model = crate::DeathViewClientModel::new(catalog)?;
    let latest_request = model.begin_committed_death_lookup(CHARACTER_ID)?;
    let latest = latest(&revision, &content_revision);
    let outcome = model.handle_result(&DeathViewResultV1::Latest {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        request_sequence: latest_request.sequence,
        death: Some(latest),
    })?;
    let summary_request = outcome.follow_up.context("summary follow-up missing")?;
    model.handle_result(&DeathViewResultV1::Summary {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        request_sequence: summary_request.sequence,
        requested_lost_limit: TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
        summary: summary(&revision, &content_revision),
    })?;
    Ok(model)
}

fn ready_memorial_model(
    catalog: CoreDevelopmentDeathView,
    detail_cursor: Option<DeathMemorialCursorV1>,
) -> Result<crate::DeathViewClientModel> {
    let revision = revision(&catalog)?;
    let content_revision = catalog.item_content_revision().to_owned();
    let mut model = crate::DeathViewClientModel::new(catalog)?;
    let page_request = model.open_memorial_wall()?;
    let entries = memorial_entries(&revision);
    model.handle_result(&DeathViewResultV1::MemorialPage {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        request_sequence: page_request.sequence,
        requested_limit: crate::MEMORIAL_PAGE_LIMIT,
        entries: entries.clone(),
        next_cursor: None,
    })?;
    if let Some(cursor) = detail_cursor {
        let entry = entries
            .iter()
            .find(|entry| entry.cursor == cursor)
            .context("requested Memorial evidence cursor is not in the bounded page")?;
        let summary_request = model.select_memorial(cursor)?;
        model.handle_result(&DeathViewResultV1::Summary {
            schema_version: DEATH_VIEW_SCHEMA_VERSION,
            request_sequence: summary_request.sequence,
            requested_lost_limit: TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
            summary: memorial_summary(&revision, &content_revision, entry),
        })?;
    }
    Ok(model)
}

fn memorial_summary(
    revision: &DeathViewContentRevisionV1,
    content_revision: &str,
    entry: &DeathMemorialEntryV1,
) -> DeathSummaryViewV1 {
    let mut summary = summary(revision, content_revision);
    summary.death_id = entry.cursor.death_id;
    summary.character_name_snapshot = entry.character_name_snapshot.clone();
    summary.class_id = entry.class_id.clone();
    summary.level = entry.level;
    summary.echo_outcome = entry.echo_outcome;
    summary.snapshot_digest = entry.summary_snapshot_digest;
    summary
}

pub(crate) fn revision(catalog: &CoreDevelopmentDeathView) -> Result<DeathViewContentRevisionV1> {
    Ok(DeathViewContentRevisionV1 {
        records_blake3: ManifestHash::new(catalog.hashes().records_blake3.clone())?,
        assets_blake3: ManifestHash::new(catalog.hashes().assets_blake3.clone())?,
        localization_blake3: ManifestHash::new(catalog.hashes().localization_blake3.clone())?,
    })
}

fn uuid_v7(seed: u8) -> [u8; 16] {
    let mut value = [seed; 16];
    value[6] = 0x70 | (seed & 0x0f);
    value[8] = 0x80 | (seed & 0x3f);
    value
}

pub(crate) fn latest(
    revision: &DeathViewContentRevisionV1,
    content_revision: &str,
) -> LatestCommittedDeathV1 {
    LatestCommittedDeathV1 {
        death_id: uuid_v7(1),
        character_id: CHARACTER_ID,
        death_at_unix_ms: DEATH_AT_UNIX_MS,
        death_tick: DEATH_TICK,
        cause: protocol::DeathCauseV1::DirectHit,
        killer_content_id: WireText::new("miniboss.sepulcher_knight").expect("stable source"),
        killer_pattern_id: Some(
            WireText::new("miniboss.sepulcher_knight.charge_lane").expect("stable pattern"),
        ),
        network_state: DeathNetworkStateV1::Connected,
        recall_state: DeathRecallStateV1::Inactive,
        trace_entry_count: 5,
        trace_digest: [2; 32],
        destruction_entry_count: 4,
        destruction_digest: [3; 32],
        summary_snapshot_digest: [4; 32],
        content_revision: WireText::new(content_revision).expect("content revision"),
        presentation_revision: revision.clone(),
    }
}

pub(crate) fn summary(
    revision: &DeathViewContentRevisionV1,
    content_revision: &str,
) -> DeathSummaryViewV1 {
    let traces = [
        TraceFixture {
            source: "enemy.drowned_pilgrim",
            pattern: "pattern.enemy.drowned_pilgrim.fan",
            damage: 12,
            damage_type: DeathDamageTypeV1::Physical,
            pre_health: 94,
            source_position: (-3_250, 4_500),
        },
        TraceFixture {
            source: "enemy.bell_reed",
            pattern: "pattern.enemy.bell_reed.gap_ring",
            damage: 14,
            damage_type: DeathDamageTypeV1::Veil,
            pre_health: 82,
            source_position: (2_000, 5_750),
        },
        TraceFixture {
            source: "enemy.chain_sentry",
            pattern: "pattern.enemy.chain_sentry.cross_lanes",
            damage: 14,
            damage_type: DeathDamageTypeV1::Physical,
            pre_health: 68,
            source_position: (6_500, -1_250),
        },
        TraceFixture {
            source: "enemy.choir_skull",
            pattern: "pattern.enemy.choir_skull.rotor",
            damage: 20,
            damage_type: DeathDamageTypeV1::Veil,
            pre_health: 54,
            source_position: (-1_500, -4_000),
        },
        TraceFixture {
            source: "miniboss.sepulcher_knight",
            pattern: "miniboss.sepulcher_knight.charge_lane",
            damage: 34,
            damage_type: DeathDamageTypeV1::Physical,
            pre_health: 34,
            source_position: (1_000, -2_000),
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, fixture)| trace(ordinal, fixture))
    .collect();

    let lost = [
        "item.weapon.crossbow.pine_crossbow",
        "item.relic.arbalist.cracked_mark_lens",
        "item.armor.pilgrim.t1",
        "item.charm.ember_tooth.t1",
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, item_id)| lost_item(u16::try_from(ordinal).expect("ordinal"), item_id))
    .collect();

    DeathSummaryViewV1 {
        death_id: uuid_v7(1),
        summary_revision: DEATH_SUMMARY_REVISION,
        hero_label_key: WireText::new("hero.core.grave_arbalist").expect("hero label"),
        character_name_snapshot: DeathCharacterName::new("Mara Ash").expect("character name"),
        class_id: WireText::new("class.grave_arbalist").expect("class"),
        level: 10,
        oath_id: Some(WireText::new("oath.arbalist.long_vigil").expect("oath identifier")),
        bargains: vec![WireText::new("bargain.cinder_hunger").expect("bargain identifier")],
        lifetime_ms: 9_867_000,
        final_deed_id: WireText::new("deed.core.sepulcher_knight_defeated")
            .expect("deed identifier"),
        lethal_trace_ordinal: 4,
        last_five_damage: traces,
        lost_total_count: 4,
        lost_start_ordinal: 0,
        lost,
        next_lost_ordinal: None,
        preserved: fixed_preserved(),
        created: fixed_created(),
        echo_outcome: DeathEchoOutcomeV1::Available,
        death_tick: DEATH_TICK,
        content_revision: WireText::new(content_revision).expect("content revision"),
        snapshot_digest: [4; 32],
        presentation_revision: revision.clone(),
    }
}

#[derive(Debug, Clone, Copy)]
struct TraceFixture {
    source: &'static str,
    pattern: &'static str,
    damage: u32,
    damage_type: DeathDamageTypeV1,
    pre_health: u32,
    source_position: (i32, i32),
}

fn trace(ordinal: usize, fixture: TraceFixture) -> DeathTraceEntryV1 {
    let ordinal_u16 = u16::try_from(ordinal).expect("trace ordinal");
    let lethal = ordinal == 4;
    DeathTraceEntryV1 {
        ordinal: ordinal_u16,
        event_tick: DEATH_TICK - u64::try_from(4 - ordinal).expect("tick offset") * 18,
        event_ordinal: u32::try_from(ordinal).expect("event ordinal"),
        source_content_id: WireText::new(fixture.source).expect("source identifier"),
        source_entity_id: Some(uuid_v7(u8::try_from(20 + ordinal).expect("entity seed"))),
        pattern_id: Some(WireText::new(fixture.pattern).expect("pattern identifier")),
        attack_id: WireText::new(fixture.pattern).expect("attack identifier"),
        raw_damage: fixture.damage,
        final_damage: fixture.damage,
        damage_type: fixture.damage_type,
        pre_health: fixture.pre_health,
        post_health: fixture.pre_health.saturating_sub(fixture.damage),
        source_x_milli_tiles: fixture.source_position.0,
        source_y_milli_tiles: fixture.source_position.1,
        network_state: DeathNetworkStateV1::Connected,
        recall_state: DeathRecallStateV1::Inactive,
        lethal,
        statuses: if lethal {
            vec![DeathTraceStatusV1 {
                ordinal: 0,
                status_id: WireText::new("status.bleed").expect("status identifier"),
                remaining_ticks: 9,
                stack_count: 1,
            }]
        } else {
            Vec::new()
        },
    }
}

fn lost_item(ordinal: u16, item_id: &str) -> DeathSummaryProjectionEntryV1 {
    DeathSummaryProjectionEntryV1 {
        ordinal,
        kind: DeathSummaryProjectionKindV1::LostItem,
        content_id: WireText::new(item_id).expect("item identifier"),
        quantity: 1,
        item_uid: Some(uuid_v7(u8::try_from(40 + ordinal).expect("item seed"))),
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
        content_id: WireText::new(content_id).expect("projection identifier"),
        quantity: 1,
        item_uid: None,
    }
}

fn fixed_preserved() -> Vec<DeathSummaryProjectionEntryV1> {
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
    .map(|(ordinal, (kind, id))| {
        fixed_projection(
            u16::try_from(ordinal).expect("projection ordinal"),
            kind,
            id,
        )
    })
    .collect()
}

fn fixed_created() -> Vec<DeathSummaryProjectionEntryV1> {
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

fn memorial_entries(revision: &DeathViewContentRevisionV1) -> Vec<DeathMemorialEntryV1> {
    [
        (1, DEATH_AT_UNIX_MS, "Mara Ash", 10, [4; 32]),
        (2, DEATH_AT_UNIX_MS - 86_400_000, "Elian Vale", 8, [5; 32]),
        (3, DEATH_AT_UNIX_MS - 259_200_000, "Sera Mourn", 6, [6; 32]),
        (4, DEATH_AT_UNIX_MS - 604_800_000, "Orin Bell", 4, [7; 32]),
    ]
    .into_iter()
    .map(
        |(seed, death_at, name, level, snapshot_digest)| DeathMemorialEntryV1 {
            cursor: DeathMemorialCursorV1 {
                death_at_unix_ms: death_at,
                death_id: uuid_v7(seed),
            },
            summary_revision: DEATH_SUMMARY_REVISION,
            summary_snapshot_digest: snapshot_digest,
            presentation_key: WireText::new("memorial.presentation.core_default")
                .expect("presentation key"),
            presentation_digest: [u8::saturating_add(seed, 20); 32],
            character_name_snapshot: DeathCharacterName::new(name).expect("memorial name"),
            class_id: WireText::new("class.grave_arbalist").expect("class"),
            level,
            echo_outcome: if seed == 1 {
                DeathEchoOutcomeV1::Available
            } else {
                DeathEchoOutcomeV1::Dormant
            },
            presentation_revision: revision.clone(),
        },
    )
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> CoreDevelopmentDeathView {
        load_core_development_death_view(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content"),
        )
        .unwrap()
    }

    #[test]
    fn showcase_deck_uses_real_projection_and_preserves_all_surface_semantics() {
        let deck = build_showcase_deck(catalog()).unwrap();

        let terminal = deck.terminal.summary.as_ref().unwrap();
        assert_eq!(terminal.context, crate::DeathSummaryContext::Terminal);
        assert_eq!(terminal.timeline.events.len(), 5);
        assert_eq!(terminal.lost.len(), 4);
        assert!(!terminal.actions.primary.state.is_enabled());
        assert_eq!(
            deck.terminal.actions()[0].action,
            DeathUiAction::Summary(DeathSummaryAction::CreateSuccessor)
        );
        assert!(!deck.terminal.actions()[0].enabled);
        assert_eq!(
            deck.terminal_trace.trace_mode,
            crate::DeathUiTraceMode::Emphasized
        );
        assert_eq!(
            deck.snapshot(CoreDeathViewShowcaseState::SummaryActions)
                .semantic_signature(),
            deck.terminal.semantic_signature()
        );

        assert_eq!(deck.memorial_list.memorial_entries.len(), 4);
        assert!(
            deck.memorial_list
                .memorial_entries
                .windows(2)
                .all(|pair| pair[0].authority.cursor.death_at_unix_ms
                    > pair[1].authority.cursor.death_at_unix_ms)
        );
        assert_eq!(deck.memorial_details.len(), 4);
        for entry in &deck.memorial_list.memorial_entries {
            let detail = deck.memorial_detail(entry.authority.cursor).unwrap();
            let summary = detail.summary.as_ref().unwrap();
            assert_eq!(summary.death_id, entry.authority.cursor.death_id);
            assert_eq!(
                summary.hero.character_name,
                entry.authority.character_name_snapshot.as_str()
            );
            assert_eq!(
                summary.snapshot_digest,
                entry.authority.summary_snapshot_digest
            );
            assert_eq!(summary.echo_outcome, entry.echo_outcome);
            assert_eq!(
                deck.memorial_trace(summary.death_id).unwrap().trace_mode,
                crate::DeathUiTraceMode::Emphasized
            );
        }
        assert_eq!(
            deck.memorial_detail.summary.as_ref().unwrap().context,
            crate::DeathSummaryContext::Memorial
        );
    }

    #[test]
    fn loading_error_and_effect_modes_never_expose_uncommitted_losses() {
        let deck = build_showcase_deck(catalog()).unwrap();
        assert!(deck.awaiting_commit.summary.is_none());
        assert!(deck.awaiting_commit.actions().is_empty());
        assert!(deck.awaiting_commit.status.is_some());
        assert!(deck.recoverable_error.summary.is_none());
        assert!(
            deck.recoverable_error
                .actions()
                .iter()
                .any(|action| action.action == DeathUiAction::Retry)
        );

        let standard = DeathUiConfig::default();
        let reduced = DeathUiConfig {
            reduced_effects: true,
            ..standard
        };
        assert_ne!(standard, reduced);
        assert_eq!(
            deck.terminal.semantic_signature(),
            deck.terminal.clone().semantic_signature()
        );
    }
}
