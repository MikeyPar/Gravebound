use std::path::{Path, PathBuf};

use client_bevy::{CorrectionClass, PredictedMovementInput, RemoteClientRuntime};
use network_harness::{AdverseNetworkProfile, Direction, NetworkHarness};
use protocol::{
    ControlEvent, InputFrame, ReliableEvent, SessionControlFrame, SessionControlRequest,
    SessionDestination, WireMessage,
};
use server_app::{InputDisposition, SessionDirectory, SessionOwnerId, SessionPhase, TransportId};
use sim_core::{MovementAction, PlayerMovementState};

const PLAYER_ENTITY_ID: u64 = 10_000;
const TRACE_SEED: u64 = 0x4752_4156_4542_4F55;
const MAX_COMBAT_TICKS: u64 = 5_000;

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn tick_micros(tick: u64) -> u64 {
    tick.saturating_mul(1_000_000) / 30
}

fn scripted_movement(tick: u64) -> (i16, i16) {
    match ((tick - 1) / 45) % 4 {
        0 => (250, 0),
        1 => (0, 250),
        2 => (-250, 0),
        _ => (0, -250),
    }
}

fn joined_directory() -> (SessionDirectory, protocol::WireText<64>) {
    let root = content_root();
    let owner = SessionOwnerId::new(1).unwrap();
    let transport = TransportId::new(1).unwrap();
    let mut directory = SessionDirectory::default();
    let response = directory
        .handle_control(
            owner,
            transport,
            &SessionControlFrame {
                sequence: 1,
                client_tick: 0,
                client_monotonic_micros: 0,
                request: SessionControlRequest::Join,
            },
            &root,
            0,
        )
        .expect("join");
    let ReliableEvent::Control(ControlEvent::SessionResult(result)) = response.event.event else {
        panic!("join result");
    };
    (directory, result.session_id)
}

#[test]
#[allow(clippy::too_many_lines)] // One continuous impaired combat/death trace preserves ordering.
fn m02_exit_profile_remains_playable_and_delivers_authoritative_death_trace() {
    let root = content_root();
    let (package, _) = sim_content::load_and_validate(&root).expect("validated content");
    let content =
        sim_content::first_playable_authority_combat_test(&package).expect("authority content");
    let arena = content.definitions.arena.clone();
    let initial = PlayerMovementState::at_arena_spawn(&arena).expect("initial movement");
    let mut client = RemoteClientRuntime::new(PLAYER_ENTITY_ID, arena, initial);

    let owner = SessionOwnerId::new(1).unwrap();
    let transport = TransportId::new(1).unwrap();
    let mut directory = SessionDirectory::default();
    directory
        .handle_control(
            owner,
            transport,
            &SessionControlFrame {
                sequence: 1,
                client_tick: 0,
                client_monotonic_micros: 0,
                request: SessionControlRequest::Join,
            },
            &root,
            0,
        )
        .expect("join");
    let mut harness =
        NetworkHarness::new(AdverseNetworkProfile::M02Exit.config(), TRACE_SEED).expect("harness");

    let mut corrections = 0_u64;
    let mut snaps = 0_u64;
    let mut accepted_inputs = 0_u64;
    let mut client_death = None;
    let mut authoritative_death_tick = None;

    for tick in 1..=MAX_COMBAT_TICKS {
        let now = tick_micros(tick);
        let due = harness.advance_to(now).expect("advance");
        for delivery in due.deliveries {
            match (delivery.direction, delivery.message) {
                (Direction::ClientToServer, WireMessage::InputFrame(frame)) => {
                    if matches!(
                        directory.session(owner).expect("session").phase(),
                        SessionPhase::Dead { .. }
                    ) {
                        continue;
                    }
                    if directory
                        .session_mut(owner)
                        .expect("session")
                        .submit_input(transport, &frame)
                        .expect("bounded input")
                        == InputDisposition::Accepted
                    {
                        accepted_inputs += 1;
                    }
                }
                (Direction::ServerToClient, WireMessage::SnapshotChunk(chunk)) => {
                    if let Some(application) = client
                        .ingest_snapshot_chunk(chunk)
                        .expect("snapshot application")
                    {
                        corrections += 1;
                        snaps += u64::from(application.correction.class == CorrectionClass::Snap);
                        if application.correction.authoritative_death {
                            client_death = Some((
                                application.snapshot.server_tick,
                                application.snapshot.state_version,
                            ));
                        }
                    }
                }
                _ => panic!("unexpected impaired message direction"),
            }
        }

        if authoritative_death_tick.is_none() {
            let movement = if tick <= 600 {
                scripted_movement(tick)
            } else {
                (0, 0)
            };
            let action = MovementAction::try_from_milli(movement.0, movement.1).unwrap();
            client
                .predict_local_movement(PredictedMovementInput {
                    sequence: u32::try_from(tick).unwrap(),
                    action,
                })
                .expect("local prediction");
            harness
                .submit(
                    Direction::ClientToServer,
                    &WireMessage::InputFrame(InputFrame {
                        sequence: u32::try_from(tick).unwrap(),
                        client_tick: tick,
                        movement_x_milli: movement.0,
                        movement_y_milli: movement.1,
                        aim_x_milli: 1_000,
                        aim_y_milli: 0,
                        held_primary: false,
                        primary_sequence: 0,
                        ability_1_sequence: 0,
                        ability_2_sequence: 0,
                    }),
                )
                .expect("input schedule");
        }

        let snapshots = directory
            .session_mut(owner)
            .expect("session")
            .tick()
            .expect("authority tick");
        for snapshot in snapshots {
            harness
                .submit(
                    Direction::ServerToClient,
                    &WireMessage::SnapshotChunk(snapshot),
                )
                .expect("snapshot schedule");
        }
        if let SessionPhase::Dead { committed_tick } =
            directory.session(owner).expect("session").phase()
        {
            authoritative_death_tick = Some(committed_tick);
        }
        if authoritative_death_tick.is_some() && harness.stats().queued_frames == 0 {
            break;
        }
    }

    let drain_at = harness
        .now_micros()
        .checked_add(1_000_000)
        .expect("drain clock");
    for delivery in harness.advance_to(drain_at).expect("drain").deliveries {
        if let (Direction::ServerToClient, WireMessage::SnapshotChunk(chunk)) =
            (delivery.direction, delivery.message)
            && let Some(application) = client
                .ingest_snapshot_chunk(chunk)
                .expect("drained snapshot")
        {
            corrections += 1;
            snaps += u64::from(application.correction.class == CorrectionClass::Snap);
            if application.correction.authoritative_death {
                client_death = Some((
                    application.snapshot.server_tick,
                    application.snapshot.state_version,
                ));
            }
        }
    }

    let session = directory.session(owner).expect("session");
    let death_tick = authoritative_death_tick.expect("authoritative death");
    let (client_death_tick, client_death_version) = client_death.expect("delivered death");
    assert_eq!(client_death_tick, death_tick);
    assert_eq!(client_death_version, session.state_version());
    assert!(matches!(session.phase(), SessionPhase::Dead { .. }));
    assert!(
        corrections >= 30,
        "insufficient delivered correction samples"
    );
    assert!(
        snaps.saturating_mul(100) <= corrections,
        "snap corrections {snaps}/{corrections} exceeded one percent"
    );
    assert!(accepted_inputs > 30, "input delivery stalled");
    assert!(
        (client.local_simulation_state().position()
            - session.authority().arena().movement().position())
        .length()
            <= 0.001
    );
    assert!(
        session
            .authority()
            .arena()
            .snapshots()
            .expect("terminal snapshots")
            .iter()
            .filter(|entity| entity.kind == sim_core::AuthorityEntityKind::Player)
            .all(|entity| !entity.alive)
    );
    let stats = harness.stats();
    assert!(stats.probabilistically_lost > 0);
    assert_eq!(stats.queued_frames, 0);
    assert_eq!(stats.queued_bytes, 0);
}

#[test]
fn outage_transitions_drive_existing_link_lost_reconnect_and_recall_authority() {
    let root = content_root();
    let owner = SessionOwnerId::new(1).unwrap();
    let first_transport = TransportId::new(1).unwrap();
    let second_transport = TransportId::new(2).unwrap();
    let mut short_config = AdverseNetworkProfile::Baseline.config();
    short_config.outage_windows = vec![network_harness::OutageWindow {
        start_micros: 1_000_000,
        duration_micros: 500_000,
    }];
    let mut short_harness = NetworkHarness::new(short_config, TRACE_SEED).unwrap();
    let (mut directory, session_id) = joined_directory();
    let down = short_harness.advance_to(1_000_000).unwrap();
    assert_eq!(down.transitions.len(), 1);
    directory
        .session_mut(owner)
        .unwrap()
        .transport_lost(first_transport)
        .unwrap();
    for _ in 0..15 {
        directory.session_mut(owner).unwrap().tick().unwrap();
    }
    let up = short_harness.advance_to(1_500_000).unwrap();
    assert_eq!(up.transitions.len(), 1);
    let response = directory
        .handle_control(
            owner,
            second_transport,
            &SessionControlFrame {
                sequence: 1,
                client_tick: 15,
                client_monotonic_micros: 1_500_000,
                request: SessionControlRequest::Reconnect {
                    prior_session_id: session_id,
                },
            },
            &root,
            1_500_000,
        )
        .unwrap();
    let ReliableEvent::Control(ControlEvent::SessionResult(result)) = response.event.event else {
        panic!("reconnect result");
    };
    assert_eq!(result.destination, SessionDestination::CombatInstance);
    assert_eq!(
        directory.session(owner).unwrap().phase(),
        SessionPhase::Connected
    );

    let (mut recalled, recalled_id) = joined_directory();
    recalled
        .session_mut(owner)
        .unwrap()
        .transport_lost(first_transport)
        .unwrap();
    for _ in 0..90 {
        recalled.session_mut(owner).unwrap().tick().unwrap();
    }
    let response = recalled
        .handle_control(
            owner,
            second_transport,
            &SessionControlFrame {
                sequence: 1,
                client_tick: 90,
                client_monotonic_micros: 3_000_000,
                request: SessionControlRequest::Reconnect {
                    prior_session_id: recalled_id,
                },
            },
            &root,
            3_000_000,
        )
        .unwrap();
    let ReliableEvent::Control(ControlEvent::SessionResult(result)) = response.event.event else {
        panic!("recall result");
    };
    assert_eq!(result.destination, SessionDestination::LanternHalls);
}
