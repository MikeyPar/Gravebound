use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use protocol::{InputFrame, WireCodecError, WireMessage, decode_frame, encode_frame};
use server_app::{
    AuthoritativeSession, InputDisposition, InputRejection, SessionDirectory, SessionOwnerId,
    TransportId,
};
use sim_core::AuthorityEntityKind;

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn input(sequence: u32, primary_sequence: u32) -> InputFrame {
    InputFrame {
        sequence,
        client_tick: u64::MAX,
        movement_x_milli: 1_000,
        movement_y_milli: 0,
        aim_x_milli: 1_000,
        aim_y_milli: 0,
        held_primary: primary_sequence != 0,
        primary_sequence,
        ability_1_sequence: 0,
        ability_2_sequence: 0,
    }
}

#[test]
fn encoded_input_flood_cannot_teleport_speed_or_regress_primary_sequence() {
    let owner = SessionOwnerId::new(1).unwrap();
    let transport = TransportId::new(1).unwrap();
    let mut directory = SessionDirectory::default();
    directory
        .handle_control(
            owner,
            transport,
            &protocol::SessionControlFrame {
                sequence: 1,
                client_tick: 0,
                client_monotonic_micros: 0,
                request: protocol::SessionControlRequest::Join,
            },
            &content_root(),
            0,
        )
        .unwrap();
    let start = directory
        .session(owner)
        .unwrap()
        .authority()
        .arena()
        .movement()
        .position();
    let maximum_tick_displacement = directory
        .session(owner)
        .unwrap()
        .authority()
        .arena()
        .movement()
        .config()
        .final_speed_tiles_per_second
        / 30.0;
    for sequence in 1..=100 {
        let encoded = encode_frame(&WireMessage::InputFrame(input(sequence, 10))).unwrap();
        let WireMessage::InputFrame(decoded) = decode_frame(&encoded).unwrap() else {
            panic!("input frame");
        };
        assert_eq!(
            directory
                .session_mut(owner)
                .unwrap()
                .submit_input(transport, &decoded)
                .unwrap(),
            InputDisposition::Accepted
        );
    }
    directory.session_mut(owner).unwrap().tick().unwrap();
    let end = directory
        .session(owner)
        .unwrap()
        .authority()
        .arena()
        .movement()
        .position();
    let displacement = (end - start).length();
    assert!(
        displacement <= maximum_tick_displacement + 1.0e-5,
        "one-tick displacement {displacement} exceeded {maximum_tick_displacement}"
    );

    let regression = directory
        .session_mut(owner)
        .unwrap()
        .submit_input(transport, &input(101, 9))
        .unwrap();
    assert_eq!(
        regression,
        InputDisposition::Rejected(InputRejection::PrimarySequenceRegression)
    );
    assert_eq!(
        directory
            .session(owner)
            .unwrap()
            .authority()
            .arena()
            .movement()
            .position(),
        end
    );
}

#[test]
fn forged_results_and_malformed_intents_fail_before_authority() {
    let invalid_vector = WireMessage::InputFrame(InputFrame {
        movement_x_milli: 1_001,
        ..input(1, 0)
    });
    assert_eq!(
        encode_frame(&invalid_vector),
        Err(WireCodecError::InvalidMessage)
    );
    let ability_on_datagram = WireMessage::InputFrame(InputFrame {
        ability_1_sequence: 1,
        ..input(1, 0)
    });
    assert_eq!(
        encode_frame(&ability_on_datagram),
        Err(WireCodecError::InvalidMessage)
    );

    let mut forged_kind = encode_frame(&WireMessage::InputFrame(input(1, 0))).unwrap();
    forged_kind[8] = 7;
    assert_eq!(
        decode_frame(&forged_kind),
        Err(WireCodecError::HeaderPayloadMismatch)
    );
}

#[test]
fn changing_primary_identity_every_tick_cannot_exceed_server_fire_cadence() {
    fn replay(change_identity: bool) -> usize {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        let mut projectiles = BTreeSet::new();
        for tick in 1..=100_u32 {
            let primary_sequence = if change_identity { tick } else { 1 };
            session
                .submit_input(&input(tick, primary_sequence))
                .unwrap();
            session.tick().unwrap();
            for snapshot in session.arena().snapshots().unwrap() {
                if snapshot.kind == AuthorityEntityKind::FriendlyProjectile {
                    projectiles.insert(snapshot.entity_id);
                }
            }
        }
        projectiles.len()
    }
    let ordinary = replay(false);
    assert!(ordinary > 1);
    assert_eq!(replay(true), ordinary);
}

#[test]
fn identical_encoded_abuse_script_replays_to_identical_evidence_and_state() {
    fn replay() -> (server_app::IngressDiagnostics, u64, [u32; 4]) {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        for frame in [input(1, 5), input(1, 5), input(2, 4)] {
            let encoded = encode_frame(&WireMessage::InputFrame(frame)).unwrap();
            let WireMessage::InputFrame(decoded) = decode_frame(&encoded).unwrap() else {
                panic!("input");
            };
            session.submit_input(&decoded).unwrap();
        }
        for (sequence, action) in [
            (1, protocol::ActionKind::Ability1Press),
            (2, protocol::ActionKind::Ability1Press),
            (2, protocol::ActionKind::Ability2Press),
        ] {
            session
                .submit_action(&protocol::ActionFrame {
                    sequence,
                    client_tick: 0,
                    action,
                })
                .unwrap();
        }
        let first_mutation = protocol::MutationRequest {
            mutation_id: [1; 16],
            pickup_id: 1,
            placement: protocol::PickupPlacement::Take,
        };
        session.submit_mutation(&first_mutation).unwrap();
        session
            .submit_mutation(&protocol::MutationRequest {
                pickup_id: 2,
                ..first_mutation
            })
            .unwrap();
        session.tick().unwrap();
        let movement = session.arena().movement();
        (
            session.ingress_diagnostics().clone(),
            session.arena().state_version(),
            [
                movement.position().x.to_bits(),
                movement.position().y.to_bits(),
                movement.velocity().x.to_bits(),
                movement.velocity().y.to_bits(),
            ],
        )
    }
    assert_eq!(replay(), replay());
}
