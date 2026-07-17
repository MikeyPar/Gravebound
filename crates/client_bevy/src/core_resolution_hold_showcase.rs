//! Disposable native Resolution Hold evidence surface for `GB-M03-08`.
//!
//! Every state is built through the production client model and reusable blocking UI plugin. The
//! showcase does not connect to a server and cannot enable the normal Hall or danger route.

use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use bevy::{
    asset::io::{AssetSourceBuilder, AssetSourceId, file::FileAssetReader},
    prelude::*,
    render::view::screenshot::Screenshot,
    window::{PresentMode, WindowResolution},
};
use protocol::{
    CORE_RESOLUTION_HOLD_FEATURE_FLAG, M03_CORE_DEV_BUILD_ID, ProtocolVersion,
    RESOLUTION_HOLD_SCHEMA_VERSION, ResolutionHoldActionV1, ResolutionHoldDestinationV1,
    ResolutionHoldDispositionV1, ResolutionHoldItemKindV1, ResolutionHoldItemTransitionV1,
    ResolutionHoldItemV1, ResolutionHoldMutationResultV1, ResolutionHoldQueryResultV1,
    ResolutionHoldRejectionCodeV1, ResolutionHoldStackV1, ResolutionHoldVersionAdvanceV1,
    ResolutionHoldVersionVectorV1, ResolutionHoldVersionsV1, SIMULATION_HZ, SNAPSHOT_HZ,
    ServerHello, StoredResolutionHoldMutationResultV1, WireText,
};
use sim_content::{CompiledProductionItemCatalog, load_core_development_items};

use crate::{
    NativeResolutionHoldPlugin, NativeResolutionHoldView, ResolutionHoldClientModel,
    ResolutionHoldUiConfig, ResolutionHoldUiCopy, ResolutionHoldUiSnapshot,
    save_screenshot_atomically,
};

const ICON_SOURCE_BLAKE3: &str = "19d49b684fd2b78c84b7aee67b0f94dcc9f8f061acff0ec9c81882bddd2cf9f5";
const ICON_RUNTIME_BLAKE3: &str =
    "c48daa7c1e7d7e054dd94480031e636a7a892af19d25c5b5091e0b03c55b8da7";
const EVIDENCE_SETTLE_FRAMES: u8 = 90;
const CHARACTER_ID: [u8; 16] = [1; 16];
const MUTATION_ID: [u8; 16] = [90; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreResolutionHoldShowcaseState {
    MixedDestinations,
    StorageFull,
    ConfirmDestroy,
    MutationPending,
    FinalClear,
    RecoverableError,
}

#[derive(Debug, Clone)]
pub struct CoreResolutionHoldShowcaseConfig {
    pub content_root: PathBuf,
    pub reduced_effects: bool,
    pub ui_scale_percent: u16,
    pub state: CoreResolutionHoldShowcaseState,
}

#[derive(Debug, Resource)]
struct ScreenshotRequest(PathBuf);

#[derive(Debug, Default)]
struct CaptureProgress {
    settled_frames: u8,
    queued: bool,
}

pub fn run_core_resolution_hold_showcase(config: &CoreResolutionHoldShowcaseConfig) -> Result<()> {
    let content_root = fs::canonicalize(&config.content_root).with_context(|| {
        format!(
            "could not resolve content root {}",
            config.content_root.display()
        )
    })?;
    let catalog = load_core_development_items(&content_root)
        .context("Core item catalog failed Resolution Hold validation")?;
    let repository_root = content_root
        .parent()
        .context("content root has no repository parent")?;
    let asset_root = repository_root.join("assets");
    validate_hold_assets(repository_root, &asset_root)?;
    let model = showcase_model(&catalog, config.state)?;
    let snapshot =
        ResolutionHoldUiSnapshot::from_model(&model, &catalog, ResolutionHoldUiCopy::default())?;
    let view = NativeResolutionHoldView::new(
        snapshot,
        ResolutionHoldUiConfig {
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
    .insert_resource(ClearColor(Color::srgb_u8(5, 8, 9)))
    .insert_resource(view)
    .add_plugins(
        DefaultPlugins
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Gravebound - GB-M03-08 Storage Resolution".to_owned(),
                    resolution: WindowResolution::new(width, height),
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins(NativeResolutionHoldPlugin)
    .add_systems(Startup, spawn_hall_backdrop);
    if let Some(path) = screenshot {
        app.insert_resource(ScreenshotRequest(path))
            .add_systems(Update, capture_resolution_hold_evidence);
    }
    app.run();
    Ok(())
}

fn validate_hold_assets(
    repository_root: &std::path::Path,
    asset_root: &std::path::Path,
) -> Result<()> {
    let source_path = repository_root.join("assets/core/items/core_item_icons.svg");
    let runtime_path = asset_root.join("core/items/core_item_icons.runtime.png");
    let source =
        fs::read(&source_path).with_context(|| format!("missing {}", source_path.display()))?;
    let runtime =
        fs::read(&runtime_path).with_context(|| format!("missing {}", runtime_path.display()))?;
    let source_hash = blake3::hash(&source).to_hex().to_string();
    let runtime_hash = blake3::hash(&runtime).to_hex().to_string();
    if source_hash != ICON_SOURCE_BLAKE3 || runtime_hash != ICON_RUNTIME_BLAKE3 {
        bail!(
            "Resolution Hold item-icon closure failed: source={source_hash}, runtime={runtime_hash}"
        );
    }
    crate::validate_death_ui_assets(asset_root)
        .context("Resolution Hold font closure failed validation")?;
    Ok(())
}

fn showcase_model(
    catalog: &CompiledProductionItemCatalog,
    state: CoreResolutionHoldShowcaseState,
) -> Result<ResolutionHoldClientModel> {
    let stacks = if state == CoreResolutionHoldShowcaseState::FinalClear {
        vec![showcase_stack(
            catalog,
            2,
            "item.weapon.crossbow.pine_crossbow",
            ResolutionHoldItemKindV1::Equipment,
            &[20],
            Some(ResolutionHoldDestinationV1::CharacterSafe { slot_index: 0 }),
        )?]
    } else {
        vec![
            showcase_stack(
                catalog,
                2,
                "item.weapon.crossbow.pine_crossbow",
                ResolutionHoldItemKindV1::Equipment,
                &[20],
                Some(ResolutionHoldDestinationV1::CharacterSafe { slot_index: 0 }),
            )?,
            showcase_stack(
                catalog,
                3,
                "item.armor.gravehide.t1",
                ResolutionHoldItemKindV1::Equipment,
                &[30],
                Some(ResolutionHoldDestinationV1::Vault { slot_index: 159 }),
            )?,
            showcase_stack(
                catalog,
                4,
                "consumable.red_tonic",
                ResolutionHoldItemKindV1::Consumable,
                &[40, 41, 42],
                Some(ResolutionHoldDestinationV1::Overflow { slot_index: 19 }),
            )?,
            showcase_stack(
                catalog,
                5,
                "item.charm.bell_locket.t1",
                ResolutionHoldItemKindV1::Equipment,
                &[50],
                None,
            )?,
        ]
    };
    let mut model =
        ResolutionHoldClientModel::new(WireText::new(catalog.revision_label().to_owned())?);
    model.begin_hall_query(&showcase_hello()?, CHARACTER_ID, 1)?;
    model.apply_query_result(&ResolutionHoldQueryResultV1::Stored {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        request_sequence: 1,
        character_id: CHARACTER_ID,
        versions: showcase_versions(),
        storage_resolution_required: true,
        stacks,
    })?;
    match state {
        CoreResolutionHoldShowcaseState::MixedDestinations => {}
        CoreResolutionHoldShowcaseState::StorageFull => {
            model.select_stack([5; 16], 0)?;
        }
        CoreResolutionHoldShowcaseState::ConfirmDestroy => {
            model.select_stack([4; 16], 0)?;
            model.request_destroy_confirmation()?;
        }
        CoreResolutionHoldShowcaseState::MutationPending => {
            model.begin_move(2, MUTATION_ID, 1_700_000_100_000)?;
        }
        CoreResolutionHoldShowcaseState::FinalClear => {
            let frame = model.begin_move(2, MUTATION_ID, 1_700_000_100_000)?;
            model.apply_mutation_result(&stored_final_clear(&frame))?;
        }
        CoreResolutionHoldShowcaseState::RecoverableError => {
            let frame = model.begin_move(2, MUTATION_ID, 1_700_000_100_000)?;
            model.apply_mutation_result(&ResolutionHoldMutationResultV1::Rejected {
                schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
                request_sequence: frame.sequence,
                mutation_id: frame.mutation_id,
                character_id: frame.character_id,
                extraction_id: frame.payload.extraction_id,
                stack_index: frame.payload.stack_index,
                code: ResolutionHoldRejectionCodeV1::DatabaseUnavailable,
            })?;
        }
    }
    Ok(model)
}

fn showcase_hello() -> Result<ServerHello> {
    let version = ProtocolVersion::current();
    Ok(ServerHello {
        session_id: WireText::new("hold-showcase")?,
        protocol_major: version.major,
        protocol_minor: version.minor,
        required_client_build: WireText::new(M03_CORE_DEV_BUILD_ID)?,
        content_bundle_version: WireText::new("core-test")?,
        server_tick_rate: SIMULATION_HZ,
        snapshot_rate: SNAPSHOT_HZ,
        region_id: WireText::new("local")?,
        feature_flags: vec![WireText::new(CORE_RESOLUTION_HOLD_FEATURE_FLAG)?],
    })
}

const fn showcase_versions() -> ResolutionHoldVersionsV1 {
    ResolutionHoldVersionsV1 {
        account: 10,
        character: 20,
        world: 30,
        inventory: 40,
    }
}

fn showcase_stack(
    catalog: &CompiledProductionItemCatalog,
    identity: u8,
    template_id: &str,
    item_kind: ResolutionHoldItemKindV1,
    item_identities: &[u8],
    planned_destination: Option<ResolutionHoldDestinationV1>,
) -> Result<ResolutionHoldStackV1> {
    let stack = ResolutionHoldStackV1 {
        extraction_id: [identity; 16],
        stack_index: 0,
        template_id: WireText::new(template_id)?,
        content_revision: WireText::new(catalog.revision_label().to_owned())?,
        item_kind,
        items: item_identities
            .iter()
            .copied()
            .map(|byte| ResolutionHoldItemV1 {
                item_uid: [byte; 16],
                item_version: 7,
            })
            .collect(),
        stack_digest: [identity.saturating_add(80); 32],
        extracted_at_unix_millis: 1_699_740_800_000,
        overflow_deadline_unix_millis: 1_700_000_000_000,
        planned_destination,
    };
    Ok(stack)
}

fn stored_final_clear(
    frame: &protocol::ResolutionHoldMutationFrameV1,
) -> ResolutionHoldMutationResultV1 {
    ResolutionHoldMutationResultV1::Stored {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        request_sequence: frame.sequence,
        replayed: false,
        result: Box::new(StoredResolutionHoldMutationResultV1 {
            mutation_id: frame.mutation_id,
            character_id: frame.character_id,
            extraction_id: frame.payload.extraction_id,
            stack_index: frame.payload.stack_index,
            action: ResolutionHoldActionV1::Move,
            result_hash: [99; 32],
            committed_at_unix_millis: 1_700_000_100_100,
            versions: ResolutionHoldVersionVectorV1 {
                account: ResolutionHoldVersionAdvanceV1 {
                    before: 10,
                    after: 10,
                },
                character: ResolutionHoldVersionAdvanceV1 {
                    before: 20,
                    after: 21,
                },
                world: ResolutionHoldVersionAdvanceV1 {
                    before: 30,
                    after: 31,
                },
                inventory: ResolutionHoldVersionAdvanceV1 {
                    before: 40,
                    after: 41,
                },
            },
            transitions: vec![ResolutionHoldItemTransitionV1 {
                ordinal: 0,
                item_uid: [20; 16],
                item_version: 8,
                disposition: ResolutionHoldDispositionV1::Moved {
                    destination: ResolutionHoldDestinationV1::CharacterSafe { slot_index: 0 },
                },
            }],
            remaining_hold_stack_count: 0,
            storage_resolution_required: false,
        }),
    }
}

fn spawn_hall_backdrop(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(8, 13, 14)),
            GlobalZIndex(-20),
        ))
        .with_children(|world| {
            world.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(7),
                    right: percent(7),
                    top: percent(12),
                    bottom: percent(12),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(15, 24, 24)),
                BorderColor::all(Color::srgb_u8(74, 68, 52)),
            ));
            world.spawn((
                Text::new("LANTERN HALLS  /  STORAGE ALCOVE"),
                TextFont::from_font_size(18.0),
                TextColor(Color::srgb_u8(151, 139, 105)),
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(9),
                    top: percent(15),
                    ..default()
                },
            ));
        });
}

#[allow(clippy::needless_pass_by_value)]
fn capture_resolution_hold_evidence(
    mut commands: Commands,
    request: Res<ScreenshotRequest>,
    mut progress: Local<CaptureProgress>,
) {
    if progress.queued {
        return;
    }
    progress.settled_frames = progress.settled_frames.saturating_add(1);
    if progress.settled_frames >= EVIDENCE_SETTLE_FRAMES {
        progress.queued = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_screenshot_atomically(request.0.clone()));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn catalog() -> CompiledProductionItemCatalog {
        load_core_development_items(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"))
            .unwrap()
    }

    #[test]
    fn every_showcase_state_uses_the_real_model_and_projection() {
        let catalog = catalog();
        for state in [
            CoreResolutionHoldShowcaseState::MixedDestinations,
            CoreResolutionHoldShowcaseState::StorageFull,
            CoreResolutionHoldShowcaseState::ConfirmDestroy,
            CoreResolutionHoldShowcaseState::MutationPending,
            CoreResolutionHoldShowcaseState::FinalClear,
            CoreResolutionHoldShowcaseState::RecoverableError,
        ] {
            let model = showcase_model(&catalog, state).unwrap();
            let snapshot = ResolutionHoldUiSnapshot::from_model(
                &model,
                &catalog,
                ResolutionHoldUiCopy::default(),
            )
            .unwrap();
            assert!(!snapshot.copy.title.is_empty(), "{state:?}");
        }
    }
}
