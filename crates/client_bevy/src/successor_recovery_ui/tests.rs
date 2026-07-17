use std::path::Path;

use protocol::{
    CORE_SUCCESSOR_FEATURE_FLAG, GRAVE_ARBALIST_CLASS_ID, M03_CORE_DEV_BUILD_ID, ManifestHash,
    PROTOCOL_MAJOR, PROTOCOL_MINOR, SIMULATION_HZ, SNAPSHOT_HZ, SUCCESSOR_CONTENT_ID_MAX_BYTES,
    SUCCESSOR_RESULT_HASH_BYTES, SUCCESSOR_SCHEMA_VERSION, SafeArrival, ServerHello,
    StoredSuccessorResultV1, SuccessorAppearanceSnapshotV1, SuccessorCreateResultV1,
    SuccessorStarterItemsV1, SuccessorVersionVectorV1, WireText, WorldFlowContentRevisionV1,
    WorldFlowResult, WorldTransferResultCode,
};
use sim_content::{CoreSuccessorRecoveryContent, load_core_successor_recovery};

use super::*;
use crate::{CoreSceneReadiness, TerminalSuccessorAuthority};

const DEATH_ID: [u8; 16] = [31; 16];
const CREATE_MUTATION_ID: [u8; 16] = [32; 16];
const SUCCESSOR_ID: [u8; 16] = [33; 16];
const HALL_MUTATION_ID: [u8; 16] = [34; 16];

fn content() -> CoreSuccessorRecoveryContent {
    load_core_successor_recovery(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"))
        .unwrap()
}

fn hello() -> ServerHello {
    ServerHello {
        session_id: WireText::new("successor-ui-test").unwrap(),
        protocol_major: PROTOCOL_MAJOR,
        protocol_minor: PROTOCOL_MINOR,
        required_client_build: WireText::new(M03_CORE_DEV_BUILD_ID).unwrap(),
        content_bundle_version: WireText::new("core-test").unwrap(),
        server_tick_rate: SIMULATION_HZ,
        snapshot_rate: SNAPSHOT_HZ,
        region_id: WireText::new("local").unwrap(),
        feature_flags: vec![WireText::new(CORE_SUCCESSOR_FEATURE_FLAG).unwrap()],
    }
}

fn revision(content: &CoreSuccessorRecoveryContent) -> WireText<SUCCESSOR_CONTENT_ID_MAX_BYTES> {
    WireText::new(content.item_content_revision().to_owned()).unwrap()
}

fn world_revision() -> WorldFlowContentRevisionV1 {
    WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
        assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
        localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
    }
}

fn ready_model(content: &CoreSuccessorRecoveryContent) -> SuccessorRecoveryClientModel {
    let mut model = SuccessorRecoveryClientModel::new(&hello(), revision(content));
    model
        .observe_terminal_summary(TerminalSuccessorAuthority { death_id: DEATH_ID })
        .unwrap();
    model
}

fn stored_result(content: &CoreSuccessorRecoveryContent) -> StoredSuccessorResultV1 {
    let mut stored = StoredSuccessorResultV1 {
        mutation_id: CREATE_MUTATION_ID,
        death_id: DEATH_ID,
        successor_id: SUCCESSOR_ID,
        receipt_id: [35; 16],
        former_roster_ordinal: 2,
        class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID).unwrap(),
        appearance: SuccessorAppearanceSnapshotV1::CoreBaseSilhouette,
        starter_items: SuccessorStarterItemsV1 {
            weapon_uid: [36; 16],
            relic_uid: [37; 16],
            tonic_unit_uids: [[38; 16], [39; 16]],
        },
        versions: SuccessorVersionVectorV1 {
            account: 12,
            character: 1,
            progression: 1,
            world: 1,
            inventory: 1,
            life_metrics: 1,
            oath_bargain: 1,
        },
        content_revision: revision(content),
        selected_character_id: SUCCESSOR_ID,
        result_hash: [0; SUCCESSOR_RESULT_HASH_BYTES],
    };
    stored.result_hash = stored.canonical_result_hash();
    stored
}

fn selected_model(content: &CoreSuccessorRecoveryContent) -> SuccessorRecoveryClientModel {
    let mut model = ready_model(content);
    model.begin_create(CREATE_MUTATION_ID).unwrap();
    model
        .apply_create_result(&SuccessorCreateResultV1::Stored {
            schema_version: SUCCESSOR_SCHEMA_VERSION,
            request_sequence: 1,
            replayed: false,
            result: Box::new(stored_result(content)),
        })
        .unwrap();
    model
}

#[test]
fn projection_follows_authoritative_two_confirmation_phases() {
    let content = content();
    let mut model = ready_model(&content);
    assert_eq!(
        SuccessorRecoveryUiSnapshot::project(&model, &content),
        Err(SuccessorRecoveryUiError::PhaseNotRenderable)
    );

    model.begin_create(CREATE_MUTATION_ID).unwrap();
    let creating = SuccessorRecoveryUiSnapshot::project(&model, &content).unwrap();
    assert_eq!(creating.surface, SuccessorRecoveryUiSurface::Creating);
    assert_eq!(creating.activity, SuccessorRecoveryUiActivity::Busy);
    assert!(creating.character.is_none());
    assert!(creating.actions.is_empty());

    model
        .apply_create_result(&SuccessorCreateResultV1::Stored {
            schema_version: SUCCESSOR_SCHEMA_VERSION,
            request_sequence: 1,
            replayed: true,
            result: Box::new(stored_result(&content)),
        })
        .unwrap();
    let selected = SuccessorRecoveryUiSnapshot::project(&model, &content).unwrap();
    assert_eq!(
        selected.surface,
        SuccessorRecoveryUiSurface::CharacterSelect
    );
    assert_eq!(selected.progress_completed, 1);
    let character = selected.character.as_ref().unwrap();
    assert_eq!(character.class_name, "Grave Arbalist");
    assert_eq!(character.slot_text, "SLOT 02");
    assert_eq!(character.level_text, "LEVEL 1");
    assert_eq!(character.oath_text, "OATH: NONE");
    assert_eq!(selected.actions.len(), 1);
    assert_eq!(selected.actions[0].action, SuccessorRecoveryUiAction::Play);
}

#[test]
fn play_loading_and_hall_readiness_never_grant_early_control() {
    let content = content();
    let mut model = selected_model(&content);
    let revision = world_revision();
    model
        .begin_play(9, HALL_MUTATION_ID, 50_000, revision.clone())
        .unwrap();
    let entering = SuccessorRecoveryUiSnapshot::project(&model, &content).unwrap();
    assert_eq!(entering.surface, SuccessorRecoveryUiSurface::EnteringHall);
    assert_eq!(entering.progress_completed, 2);
    assert!(entering.actions.is_empty());

    model
        .apply_hall_result(&WorldFlowResult::Transfer {
            request_sequence: 9,
            mutation_id: HALL_MUTATION_ID,
            accepted: true,
            code: WorldTransferResultCode::Accepted,
            snapshot: Some(protocol::CharacterLocationSnapshot {
                character_id: SUCCESSOR_ID,
                character_version: 2,
                location: protocol::CharacterLocation::Safe {
                    location_id: WireText::new("hub.lantern_halls_01").unwrap(),
                    arrival: SafeArrival::HallDefault,
                },
            }),
            transfer_id: Some([40; 16]),
        })
        .unwrap();
    let loading = SuccessorRecoveryUiSnapshot::project(&model, &content).unwrap();
    assert_eq!(loading.surface, SuccessorRecoveryUiSurface::LoadingHall);
    assert_eq!(loading.status, "PREPARING LANTERN HALLS");

    model
        .mark_hall_content_ready(&CoreSceneReadiness {
            location_id: WireText::new("hub.lantern_halls_01").unwrap(),
            character_version: 2,
            content_revision: revision,
        })
        .unwrap();
    let ready = SuccessorRecoveryUiSnapshot::project(&model, &content).unwrap();
    assert_eq!(ready.surface, SuccessorRecoveryUiSurface::HallReady);
    assert_eq!(ready.status, "LANTERN HALLS — CONTROL READY");
    assert!(ready.actions.is_empty());
}

#[test]
fn certified_layouts_are_legible_and_effect_mode_is_information_invariant() {
    let minimum = SuccessorRecoveryUiMetrics::for_viewport(1_280.0, 720.0, 80).unwrap();
    assert_eq!(minimum.layout_mode, SuccessorRecoveryUiLayoutMode::Minimum);
    assert!(minimum.safe_margin_px >= 18.0);
    assert!(minimum.body_text_px >= 14.0);
    assert!(minimum.label_text_px >= 14.0);
    assert!(minimum.panel_width_px <= 1_280.0 - minimum.safe_margin_px * 2.0);
    assert!(minimum.panel_height_px <= 720.0 - minimum.safe_margin_px * 2.0);

    let reference = SuccessorRecoveryUiMetrics::for_viewport(1_920.0, 1_080.0, 100).unwrap();
    assert_eq!(
        reference.layout_mode,
        SuccessorRecoveryUiLayoutMode::Reference
    );
    assert!(reference.title_text_px > minimum.title_text_px);
    let enlarged = SuccessorRecoveryUiMetrics::for_viewport(1_280.0, 720.0, 150).unwrap();
    assert!(enlarged.panel_width_px <= 1_280.0 - enlarged.safe_margin_px * 2.0);
    assert!(enlarged.panel_height_px <= 720.0 - enlarged.safe_margin_px * 2.0);

    let content = content();
    let snapshot =
        SuccessorRecoveryUiSnapshot::project(&selected_model(&content), &content).unwrap();
    let standard =
        NativeSuccessorRecoveryView::new(snapshot.clone(), SuccessorRecoveryUiConfig::default())
            .unwrap();
    let reduced = NativeSuccessorRecoveryView::new(
        snapshot,
        SuccessorRecoveryUiConfig {
            reduced_effects: true,
            ui_scale_percent: 100,
        },
    )
    .unwrap();
    assert_eq!(
        standard.snapshot().semantic_signature(),
        reduced.snapshot().semantic_signature()
    );
}
