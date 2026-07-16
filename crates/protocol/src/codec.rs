use postcard::{from_bytes, to_stdvec};
use thiserror::Error;

use crate::{
    DEATH_VIEW_PROTOCOL_MINOR, M02_PROTOCOL_MINOR, MessageKind, PROTOCOL_MAJOR, ProtocolVersion,
    SAFE_INVENTORY_PROTOCOL_MINOR, WireMessage,
};

const MAGIC: [u8; 4] = *b"GBN1";
pub const FRAME_HEADER_BYTES: usize = 14;
pub const DATAGRAM_FRAME_LIMIT: usize = 1_200;
pub const RELIABLE_FRAME_LIMIT: usize = 64 * 1_024;

pub fn encode_frame(message: &WireMessage) -> Result<Vec<u8>, WireCodecError> {
    encode_frame_for_version(message, ProtocolVersion::current())
}

/// Reproduces canonical M02 bytes for immutable fixtures and package verification. These frames
/// are not accepted by the current exact-minor live decoder.
pub fn encode_m02_compatibility_frame(message: &WireMessage) -> Result<Vec<u8>, WireCodecError> {
    if message_uses_death_view(message)
        || message_uses_terminal_inventory(message)
        || matches!(
            message.kind(),
            MessageKind::AccountBootstrapFrame
                | MessageKind::CharacterMutationFrame
                | MessageKind::WorldFlowFrame
                | MessageKind::ProgressionQueryFrame
                | MessageKind::OathViewFrame
                | MessageKind::InitialOathSelectionFrame
                | MessageKind::BargainViewFrame
                | MessageKind::BargainDecisionFrame
                | MessageKind::SafeInventoryTransferFrame
                | MessageKind::ExtractionCommitFrame
                | MessageKind::RecallFrame
        )
    {
        return Err(WireCodecError::MessageUnavailableAtVersion);
    }
    encode_frame_for_version(
        message,
        ProtocolVersion {
            major: PROTOCOL_MAJOR,
            minor: M02_PROTOCOL_MINOR,
        },
    )
}

/// Reproduces protocol 1.12 bytes for append-only compatibility verification. Death views are
/// intentionally unavailable before their negotiated protocol generation.
pub fn encode_protocol_1_12_compatibility_frame(
    message: &WireMessage,
) -> Result<Vec<u8>, WireCodecError> {
    if message_uses_death_view(message) || message_uses_terminal_inventory(message) {
        return Err(WireCodecError::MessageUnavailableAtVersion);
    }
    encode_frame_for_version(
        message,
        ProtocolVersion {
            major: PROTOCOL_MAJOR,
            minor: SAFE_INVENTORY_PROTOCOL_MINOR,
        },
    )
}

/// Reproduces protocol 1.14 bytes for immutable death-view and earlier compatibility fixtures.
/// Extraction and Recall were appended in 1.15 and are unavailable in this generation.
pub fn encode_protocol_1_14_compatibility_frame(
    message: &WireMessage,
) -> Result<Vec<u8>, WireCodecError> {
    if message_uses_terminal_inventory(message) {
        return Err(WireCodecError::MessageUnavailableAtVersion);
    }
    encode_frame_for_version(
        message,
        ProtocolVersion {
            major: PROTOCOL_MAJOR,
            minor: DEATH_VIEW_PROTOCOL_MINOR,
        },
    )
}

const fn message_uses_death_view(message: &WireMessage) -> bool {
    matches!(
        message,
        WireMessage::DeathViewFrame(_)
            | WireMessage::ReliableEvent(crate::ReliableEventFrame {
                event: crate::ReliableEvent::DeathViewResult(_),
                ..
            })
    )
}

const fn message_uses_terminal_inventory(message: &WireMessage) -> bool {
    matches!(
        message,
        WireMessage::ExtractionCommitFrame(_)
            | WireMessage::RecallFrame(_)
            | WireMessage::ReliableEvent(crate::ReliableEventFrame {
                event: crate::ReliableEvent::ExtractionCommitResult(_)
                    | crate::ReliableEvent::RecallResult(_),
                ..
            })
    )
}

fn encode_frame_for_version(
    message: &WireMessage,
    version: ProtocolVersion,
) -> Result<Vec<u8>, WireCodecError> {
    message
        .validate()
        .map_err(|_| WireCodecError::InvalidMessage)?;
    let payload = to_stdvec(message).map_err(|error| WireCodecError::Encode(error.to_string()))?;
    let payload_len = u32::try_from(payload.len()).map_err(|_| WireCodecError::FrameTooLarge)?;
    let total_len = FRAME_HEADER_BYTES
        .checked_add(payload.len())
        .ok_or(WireCodecError::FrameTooLarge)?;
    let limit = if message.uses_datagram() {
        DATAGRAM_FRAME_LIMIT
    } else {
        RELIABLE_FRAME_LIMIT
    };
    if total_len > limit {
        return Err(WireCodecError::FrameTooLarge);
    }
    let mut frame = Vec::with_capacity(total_len);
    frame.extend_from_slice(&MAGIC);
    frame.extend_from_slice(&version.major.to_le_bytes());
    frame.extend_from_slice(&version.minor.to_le_bytes());
    frame.push(message_kind_byte(message.kind()));
    frame.push(u8::from(message.uses_datagram()));
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame(frame: &[u8]) -> Result<WireMessage, WireCodecError> {
    if frame.len() < FRAME_HEADER_BYTES || frame.len() > RELIABLE_FRAME_LIMIT {
        return Err(WireCodecError::InvalidFrameLength);
    }
    if frame[..4] != MAGIC {
        return Err(WireCodecError::InvalidMagic);
    }
    let major = u16::from_le_bytes([frame[4], frame[5]]);
    let minor = u16::from_le_bytes([frame[6], frame[7]]);
    let received = ProtocolVersion { major, minor };
    if !ProtocolVersion::current().is_compatible_with(received) {
        return Err(WireCodecError::IncompatibleVersion(received));
    }
    let header_kind = message_kind_from_byte(frame[8])?;
    let datagram_flag = match frame[9] {
        0 => false,
        1 => true,
        _ => return Err(WireCodecError::InvalidTransportFlag),
    };
    let payload_len = usize::try_from(u32::from_le_bytes([
        frame[10], frame[11], frame[12], frame[13],
    ]))
    .map_err(|_| WireCodecError::InvalidFrameLength)?;
    if frame.len() != FRAME_HEADER_BYTES + payload_len {
        return Err(WireCodecError::InvalidFrameLength);
    }
    if datagram_flag && frame.len() > DATAGRAM_FRAME_LIMIT {
        return Err(WireCodecError::FrameTooLarge);
    }
    let message: WireMessage = from_bytes(&frame[FRAME_HEADER_BYTES..])
        .map_err(|error| WireCodecError::Decode(error.to_string()))?;
    if message.kind() != header_kind || message.uses_datagram() != datagram_flag {
        return Err(WireCodecError::HeaderPayloadMismatch);
    }
    message
        .validate()
        .map_err(|_| WireCodecError::InvalidMessage)?;
    Ok(message)
}

const fn message_kind_byte(kind: MessageKind) -> u8 {
    match kind {
        MessageKind::ClientHello => 1,
        MessageKind::HandshakeResponse => 2,
        MessageKind::InputFrame => 3,
        MessageKind::ActionFrame => 4,
        MessageKind::SnapshotChunk => 5,
        MessageKind::ReliableEvent => 6,
        MessageKind::MutationRequest => 7,
        MessageKind::SessionControlFrame => 8,
        MessageKind::AccountBootstrapFrame => 9,
        MessageKind::CharacterMutationFrame => 10,
        MessageKind::WorldFlowFrame => 11,
        MessageKind::ProgressionQueryFrame => 12,
        MessageKind::OathViewFrame => 13,
        MessageKind::InitialOathSelectionFrame => 14,
        MessageKind::BargainViewFrame => 15,
        MessageKind::BargainDecisionFrame => 16,
        MessageKind::SafeInventoryTransferFrame => 17,
        MessageKind::DeathViewFrame => 18,
        MessageKind::ExtractionCommitFrame => 19,
        MessageKind::RecallFrame => 20,
    }
}

const fn message_kind_from_byte(value: u8) -> Result<MessageKind, WireCodecError> {
    match value {
        1 => Ok(MessageKind::ClientHello),
        2 => Ok(MessageKind::HandshakeResponse),
        3 => Ok(MessageKind::InputFrame),
        4 => Ok(MessageKind::ActionFrame),
        5 => Ok(MessageKind::SnapshotChunk),
        6 => Ok(MessageKind::ReliableEvent),
        7 => Ok(MessageKind::MutationRequest),
        8 => Ok(MessageKind::SessionControlFrame),
        9 => Ok(MessageKind::AccountBootstrapFrame),
        10 => Ok(MessageKind::CharacterMutationFrame),
        11 => Ok(MessageKind::WorldFlowFrame),
        12 => Ok(MessageKind::ProgressionQueryFrame),
        13 => Ok(MessageKind::OathViewFrame),
        14 => Ok(MessageKind::InitialOathSelectionFrame),
        15 => Ok(MessageKind::BargainViewFrame),
        16 => Ok(MessageKind::BargainDecisionFrame),
        17 => Ok(MessageKind::SafeInventoryTransferFrame),
        18 => Ok(MessageKind::DeathViewFrame),
        19 => Ok(MessageKind::ExtractionCommitFrame),
        20 => Ok(MessageKind::RecallFrame),
        other => Err(WireCodecError::UnknownMessageKind(other)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WireCodecError {
    #[error("wire frame has an invalid length")]
    InvalidFrameLength,
    #[error("wire frame exceeds its transport limit")]
    FrameTooLarge,
    #[error("wire frame magic is invalid")]
    InvalidMagic,
    #[error("wire protocol version is incompatible: {0:?}")]
    IncompatibleVersion(ProtocolVersion),
    #[error("wire frame transport flag must be zero or one")]
    InvalidTransportFlag,
    #[error("wire frame uses unknown message kind {0}")]
    UnknownMessageKind(u8),
    #[error("wire header and decoded payload disagree")]
    HeaderPayloadMismatch,
    #[error("wire message failed semantic validation")]
    InvalidMessage,
    #[error("wire message is unavailable at the requested protocol generation")]
    MessageUnavailableAtVersion,
    #[error("wire message encode failed: {0}")]
    Encode(String),
    #[error("wire message decode failed: {0}")]
    Decode(String),
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;
    use crate::{
        AccountBootstrapFrame, AccountBootstrapRequest, CharacterMutationFrame,
        CharacterMutationPayload, GRAVE_ARBALIST_CLASS_ID, InputFrame, ManifestHash, WireMessage,
        WireText,
    };

    #[derive(Serialize)]
    #[allow(
        dead_code,
        reason = "variant ordinals reproduce the immutable protocol 1.13 fixture"
    )]
    enum LegacyWireMessageV1 {
        ClientHello,
        HandshakeResponse,
        InputFrame,
        ActionFrame,
        SnapshotChunk,
        ReliableEvent(LegacyReliableEventFrameV1),
    }

    #[derive(Serialize)]
    struct LegacyReliableEventFrameV1 {
        sequence: u32,
        server_tick: u64,
        event: LegacyReliableEventV1,
    }

    #[derive(Serialize)]
    #[allow(
        dead_code,
        reason = "variant ordinals reproduce the immutable protocol 1.13 fixture"
    )]
    enum LegacyReliableEventV1 {
        ActionResult,
        PatternStarted,
        MutationResult,
        Control,
        SocialPing,
        AccountBootstrapResult,
        CharacterMutationResult,
        WorldFlowResult,
        ProgressionResult,
        OathViewResult,
        InitialOathSelectionResult,
        BargainViewResult,
        BargainDecisionResult,
        SafeInventoryTransferResult,
        DeathViewResult(Box<LegacyDeathViewResultV1>),
    }

    #[derive(Serialize)]
    enum LegacyDeathViewResultV1 {
        Latest {
            schema_version: u16,
            request_sequence: u32,
            death: Option<LegacyLatestCommittedDeathV1>,
        },
    }

    #[derive(Serialize)]
    struct LegacyLatestCommittedDeathV1 {
        death_id: [u8; crate::DEATH_VIEW_ID_BYTES],
        character_id: [u8; crate::DEATH_VIEW_ID_BYTES],
        death_at_unix_ms: u64,
        death_tick: u64,
        cause: crate::DeathCauseV1,
        killer_content_id: WireText<{ crate::DEATH_VIEW_ID_MAX_BYTES }>,
        killer_pattern_id: Option<WireText<{ crate::DEATH_VIEW_ID_MAX_BYTES }>>,
        network_state: crate::DeathNetworkStateV1,
        recall_state: crate::DeathRecallStateV1,
        trace_entry_count: u16,
        trace_digest: [u8; crate::DEATH_VIEW_DIGEST_BYTES],
        destruction_entry_count: u16,
        destruction_digest: [u8; crate::DEATH_VIEW_DIGEST_BYTES],
        summary_snapshot_digest: [u8; crate::DEATH_VIEW_DIGEST_BYTES],
        content_revision: WireText<{ crate::DEATH_VIEW_ID_MAX_BYTES }>,
    }

    fn protocol_1_13_latest_success_fixture() -> Vec<u8> {
        let mut death_id = [14; crate::DEATH_VIEW_ID_BYTES];
        death_id[6] = 0x7e;
        death_id[8] = 0x8e;
        let message = LegacyWireMessageV1::ReliableEvent(LegacyReliableEventFrameV1 {
            sequence: 1,
            server_tick: 301,
            event: LegacyReliableEventV1::DeathViewResult(Box::new(
                LegacyDeathViewResultV1::Latest {
                    schema_version: 1,
                    request_sequence: 13,
                    death: Some(LegacyLatestCommittedDeathV1 {
                        death_id,
                        character_id: [15; crate::DEATH_VIEW_ID_BYTES],
                        death_at_unix_ms: 1,
                        death_tick: 301,
                        cause: crate::DeathCauseV1::DirectHit,
                        killer_content_id: WireText::new("boss.sir_caldus").unwrap(),
                        killer_pattern_id: Some(WireText::new("boss.caldus.bell_ring").unwrap()),
                        network_state: crate::DeathNetworkStateV1::Connected,
                        recall_state: crate::DeathRecallStateV1::Inactive,
                        trace_entry_count: 2,
                        trace_digest: [2; crate::DEATH_VIEW_DIGEST_BYTES],
                        destruction_entry_count: 1,
                        destruction_digest: [3; crate::DEATH_VIEW_DIGEST_BYTES],
                        summary_snapshot_digest: [4; crate::DEATH_VIEW_DIGEST_BYTES],
                        content_revision: WireText::new(format!(
                            "core-dev.blake3.{}",
                            "d".repeat(64)
                        ))
                        .unwrap(),
                    }),
                },
            )),
        });
        let payload = to_stdvec(&message).unwrap();
        let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len());
        frame.extend_from_slice(&MAGIC);
        frame.extend_from_slice(&PROTOCOL_MAJOR.to_le_bytes());
        frame.extend_from_slice(&13_u16.to_le_bytes());
        frame.push(message_kind_byte(MessageKind::ReliableEvent));
        frame.push(0);
        frame.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
        frame.extend_from_slice(&payload);
        frame
    }

    fn input_message() -> WireMessage {
        WireMessage::InputFrame(InputFrame {
            sequence: 7,
            client_tick: 42,
            movement_x_milli: 1_000,
            movement_y_milli: 0,
            aim_x_milli: 707,
            aim_y_milli: -707,
            held_primary: true,
            primary_sequence: 3,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        })
    }

    fn extraction_terminal_payload() -> crate::ExtractionCommitPayloadV1 {
        crate::ExtractionCommitPayloadV1 {
            extraction_request_id: [3; 16],
            expected_versions: crate::TerminalExpectedVersionsV1 {
                account: 1,
                character: 2,
                world: 2,
                inventory: 3,
                life_clock: 4,
            },
            content_revision: crate::WorldFlowContentRevisionV1 {
                records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
                assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
                localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
            },
        }
    }

    #[test]
    fn canonical_frame_round_trips_and_has_pinned_bytes() {
        let frame = encode_frame(&input_message()).unwrap();
        assert!(frame.len() <= DATAGRAM_FRAME_LIMIT);
        assert_eq!(decode_frame(&frame).unwrap(), input_message());
        assert_eq!(u16::from_le_bytes([frame[6], frame[7]]), 15);
        let protocol_1_14 = encode_protocol_1_14_compatibility_frame(&input_message()).unwrap();
        assert_eq!(
            blake3::hash(&protocol_1_14).to_hex().to_string(),
            "c05d1157b68f5a26ad31f70e7b61c114ae0ad6bc7b96888b1c0d127b60224832"
        );
        let protocol_1_12 = encode_protocol_1_12_compatibility_frame(&input_message()).unwrap();
        assert_eq!(
            blake3::hash(&protocol_1_12).to_hex().to_string(),
            "04b734acd84cf09bf65e76c5773ffea1892682b91600996902099aec8a7d7266"
        );
        let m02 = encode_m02_compatibility_frame(&input_message()).unwrap();
        assert_eq!(
            blake3::hash(&m02).to_hex().to_string(),
            "643b0c2d1746c2e697e2c5cb3b4fc0e352019903a951004326e808e00b5cd7ec"
        );
    }

    #[test]
    fn framing_rejects_magic_version_length_kind_and_transport_drift() {
        let valid = encode_frame(&input_message()).unwrap();
        let mut bad = valid.clone();
        bad[0] = b'X';
        assert_eq!(decode_frame(&bad), Err(WireCodecError::InvalidMagic));
        let mut bad = valid.clone();
        bad[4..6].copy_from_slice(&2_u16.to_le_bytes());
        assert!(matches!(
            decode_frame(&bad),
            Err(WireCodecError::IncompatibleVersion(_))
        ));
        let mut bad = valid.clone();
        bad[6..8].copy_from_slice(&0_u16.to_le_bytes());
        assert!(matches!(
            decode_frame(&bad),
            Err(WireCodecError::IncompatibleVersion(_))
        ));
        let mut bad = valid.clone();
        bad[8] = 2;
        assert_eq!(
            decode_frame(&bad),
            Err(WireCodecError::HeaderPayloadMismatch)
        );
        let mut bad = valid;
        bad[9] = 0;
        assert_eq!(
            decode_frame(&bad),
            Err(WireCodecError::HeaderPayloadMismatch)
        );
    }

    #[test]
    fn protocol_1_6_appends_bounded_account_message_kinds() {
        let bootstrap = WireMessage::AccountBootstrapFrame(AccountBootstrapFrame {
            sequence: 1,
            request: AccountBootstrapRequest::Bootstrap,
            content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        });
        let frame = encode_frame(&bootstrap).unwrap();
        assert_eq!(frame[8], 9);
        assert_eq!(decode_frame(&frame), Ok(bootstrap));

        let payload = CharacterMutationPayload::Create {
            class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID).unwrap(),
        };
        let mutation = WireMessage::CharacterMutationFrame(CharacterMutationFrame {
            mutation_id: [1; 16],
            expected_account_version: 1,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: 1,
            payload,
        });
        let frame = encode_frame(&mutation).unwrap();
        assert_eq!(frame[8], 10);
        assert_eq!(decode_frame(&frame), Ok(mutation.clone()));
        assert_eq!(
            encode_m02_compatibility_frame(&mutation),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
    }

    #[test]
    fn protocol_1_7_appends_bounded_world_flow_control() {
        let flow = WireMessage::WorldFlowFrame(crate::WorldFlowFrame {
            sequence: 1,
            request: crate::WorldFlowRequest::Location {
                character_id: [1; 16],
                content_revision: crate::WorldFlowContentRevisionV1 {
                    records_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
                    assets_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
                    localization_blake3: ManifestHash::new("d".repeat(64)).unwrap(),
                },
            },
        });
        let frame = encode_frame(&flow).unwrap();
        assert_eq!(frame[8], 11);
        assert_eq!(decode_frame(&frame), Ok(flow.clone()));
        assert_eq!(
            encode_m02_compatibility_frame(&flow),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
    }

    #[test]
    fn protocol_1_8_appends_read_only_progression_projection_query() {
        let query = WireMessage::ProgressionQueryFrame(crate::ProgressionQueryFrame {
            sequence: 9,
            character_id: [2; 16],
            progression_content_revision: ManifestHash::new("c".repeat(64)).unwrap(),
        });
        let frame = encode_frame(&query).unwrap();
        assert_eq!(frame[8], 12);
        assert_eq!(decode_frame(&frame), Ok(query.clone()));
        assert_eq!(
            encode_m02_compatibility_frame(&query),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
    }

    #[test]
    fn protocol_1_9_appends_bounded_oath_control_and_mutation_kinds() {
        let revision = crate::OathContentRevisionV1 {
            records_blake3: ManifestHash::new("d".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("e".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("f".repeat(64)).unwrap(),
        };
        let view = WireMessage::OathViewFrame(crate::OathViewFrame {
            sequence: 10,
            character_id: [3; 16],
            content_revision: revision.clone(),
        });
        let frame = encode_frame(&view).unwrap();
        assert_eq!(frame[8], 13);
        assert_eq!(decode_frame(&frame), Ok(view));

        let payload = crate::InitialOathSelectionPayload {
            character_id: [3; 16],
            oath_id: WireText::new(crate::LONG_VIGIL_ID).unwrap(),
            content_revision: revision,
            confirmed: true,
        };
        let selection = WireMessage::InitialOathSelectionFrame(crate::InitialOathSelectionFrame {
            mutation_id: [4; 16],
            expected_character_version: 1,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: 1,
            payload,
        });
        let frame = encode_frame(&selection).unwrap();
        assert_eq!(frame[8], 14);
        assert_eq!(decode_frame(&frame), Ok(selection.clone()));
        assert_eq!(
            encode_m02_compatibility_frame(&selection),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
    }

    #[test]
    fn protocol_1_10_appends_bounded_bargain_control_and_mutation_kinds() {
        let revision = crate::BargainContentRevisionV1 {
            records_blake3: ManifestHash::new("1".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("2".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("3".repeat(64)).unwrap(),
        };
        let view = WireMessage::BargainViewFrame(crate::BargainViewFrame {
            sequence: 11,
            character_id: [5; 16],
            content_revision: revision.clone(),
        });
        let frame = encode_frame(&view).unwrap();
        assert_eq!(frame[8], 15);
        assert_eq!(decode_frame(&frame), Ok(view));

        let payload = crate::BargainDecisionPayload {
            character_id: [5; 16],
            offer_id: [6; 16],
            decision: crate::BargainDecision::Refuse,
            content_revision: revision,
            confirmed: true,
        };
        let decision = WireMessage::BargainDecisionFrame(crate::BargainDecisionFrame {
            mutation_id: [7; 16],
            expected_oath_bargain_version: 2,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: 1,
            payload,
        });
        let frame = encode_frame(&decision).unwrap();
        assert_eq!(frame[8], 16);
        assert_eq!(decode_frame(&frame), Ok(decision.clone()));
        assert_eq!(
            encode_m02_compatibility_frame(&decision),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
    }

    #[test]
    fn protocol_1_12_preserves_committed_caldus_extraction_identities() {
        let payload = crate::WorldTransferPayload {
            content_revision: crate::WorldFlowContentRevisionV1 {
                records_blake3: ManifestHash::new("4".repeat(64)).unwrap(),
                assets_blake3: ManifestHash::new("5".repeat(64)).unwrap(),
                localization_blake3: ManifestHash::new("6".repeat(64)).unwrap(),
            },
            command: crate::WorldTransferCommand::UseCommittedExtraction {
                portal_id: WireText::new("portal.exit.dungeon.bell_sepulcher").unwrap(),
                extraction_request_id: [8; 16],
                extraction_receipt_id: [9; 16],
            },
        };
        let transfer = WireMessage::WorldFlowFrame(crate::WorldFlowFrame {
            sequence: 12,
            request: crate::WorldFlowRequest::Transfer(crate::WorldTransferMutation {
                mutation_id: [10; 16],
                character_id: [11; 16],
                expected_character_version: 3,
                issued_at_unix_millis: 1,
                payload_hash: payload.canonical_hash(),
                payload,
            }),
        });

        let frame = encode_frame(&transfer).unwrap();
        assert_eq!(u16::from_le_bytes([frame[6], frame[7]]), 15);
        assert_eq!(decode_frame(&frame), Ok(transfer.clone()));

        let compatibility = encode_protocol_1_12_compatibility_frame(&transfer).unwrap();
        assert_eq!(u16::from_le_bytes([compatibility[6], compatibility[7]]), 12);
    }

    #[test]
    fn protocol_1_12_appends_bounded_safe_inventory_mutation() {
        let payload = crate::SafeInventoryTransferPayloadV1 {
            kind: crate::SafeInventoryTransferKindV1::CharacterSafeToVault,
            source_slot_index: 7,
            expected_account_version: 4,
            expected_inventory_version: 9,
        };
        let transfer =
            WireMessage::SafeInventoryTransferFrame(crate::SafeInventoryTransferFrameV1 {
                mutation_id: [12; 16],
                character_id: [13; 16],
                issued_at_unix_millis: 1,
                payload_hash: payload.canonical_hash(),
                payload,
            });
        let frame = encode_frame(&transfer).unwrap();
        assert_eq!(u16::from_le_bytes([frame[6], frame[7]]), 15);
        assert_eq!(frame[8], 17);
        assert_eq!(decode_frame(&frame), Ok(transfer.clone()));
        let compatibility = encode_protocol_1_12_compatibility_frame(&transfer).unwrap();
        assert_eq!(u16::from_le_bytes([compatibility[6], compatibility[7]]), 12);
        assert_eq!(compatibility[8], 17);
        assert_eq!(
            encode_m02_compatibility_frame(&transfer),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );

        let rejection = WireMessage::ReliableEvent(crate::ReliableEventFrame {
            sequence: 1,
            server_tick: 1,
            event: crate::ReliableEvent::SafeInventoryTransferResult(
                crate::SafeInventoryTransferResultV1 {
                    mutation_id: [12; 16],
                    character_id: [13; 16],
                    code: crate::SafeInventoryResultCodeV1::StorageFull,
                    replayed: false,
                    result_hash: [0; 32],
                    account_version: 0,
                    inventory_version: 0,
                    placements: Vec::new(),
                },
            ),
        });
        let frame = encode_frame(&rejection).unwrap();
        assert_eq!(decode_frame(&frame), Ok(rejection));
    }

    #[test]
    fn protocol_1_14_versions_authenticated_death_views_at_kind_18() {
        let mut death_id = [14; 16];
        death_id[6] = 0x7e;
        death_id[8] = 0x8e;
        let request = WireMessage::DeathViewFrame(crate::DeathViewFrameV1 {
            schema_version: crate::DEATH_VIEW_SCHEMA_VERSION,
            sequence: 13,
            content_revision: crate::DeathViewContentRevisionV1 {
                records_blake3: ManifestHash::new("7".repeat(64)).unwrap(),
                assets_blake3: ManifestHash::new("8".repeat(64)).unwrap(),
                localization_blake3: ManifestHash::new("9".repeat(64)).unwrap(),
            },
            request: crate::DeathViewRequestV1::Summary {
                death_id,
                lost_start_ordinal: 0,
                lost_limit: 16,
            },
        });
        let frame = encode_frame(&request).unwrap();
        assert_eq!(u16::from_le_bytes([frame[6], frame[7]]), 15);
        assert_eq!(frame[8], 18);
        assert_eq!(decode_frame(&frame), Ok(request.clone()));
        let compatibility = encode_protocol_1_14_compatibility_frame(&request).unwrap();
        assert_eq!(u16::from_le_bytes([compatibility[6], compatibility[7]]), 14);
        assert_eq!(
            blake3::hash(&compatibility).to_hex().to_string(),
            "0f2c3a3d3b12a3a81b132fa4f79421cfcd44f94708d79c65e7c127bcf5df458f"
        );
        assert_eq!(
            encode_protocol_1_12_compatibility_frame(&request),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );

        let result = WireMessage::ReliableEvent(crate::ReliableEventFrame {
            sequence: 1,
            server_tick: 1,
            event: crate::ReliableEvent::DeathViewResult(Box::new(
                crate::DeathViewResultV1::Error {
                    schema_version: crate::DEATH_VIEW_SCHEMA_VERSION,
                    request_sequence: 13,
                    code: crate::DeathViewResultCodeV1::DeathNotOwned,
                },
            )),
        });
        assert_eq!(
            encode_protocol_1_12_compatibility_frame(&result),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );

        let legacy = protocol_1_13_latest_success_fixture();
        assert_eq!(u16::from_le_bytes([legacy[6], legacy[7]]), 13);
        assert_eq!(legacy[8], message_kind_byte(MessageKind::ReliableEvent));
        assert_eq!(legacy.len(), 279);
        assert_eq!(
            blake3::hash(&legacy).to_hex().to_string(),
            "aec8a61cd02890c4894abb69a98ced7a47e6a25e0ece2ade83ebf012fb595c1c"
        );
        assert_eq!(
            decode_frame(&legacy),
            Err(WireCodecError::IncompatibleVersion(ProtocolVersion {
                major: PROTOCOL_MAJOR,
                minor: 13,
            }))
        );
    }

    #[test]
    fn append_only_message_kind_bytes_one_through_eighteen_are_unchanged() {
        let legacy = [
            MessageKind::ClientHello,
            MessageKind::HandshakeResponse,
            MessageKind::InputFrame,
            MessageKind::ActionFrame,
            MessageKind::SnapshotChunk,
            MessageKind::ReliableEvent,
            MessageKind::MutationRequest,
            MessageKind::SessionControlFrame,
            MessageKind::AccountBootstrapFrame,
            MessageKind::CharacterMutationFrame,
            MessageKind::WorldFlowFrame,
            MessageKind::ProgressionQueryFrame,
            MessageKind::OathViewFrame,
            MessageKind::InitialOathSelectionFrame,
            MessageKind::BargainViewFrame,
            MessageKind::BargainDecisionFrame,
            MessageKind::SafeInventoryTransferFrame,
            MessageKind::DeathViewFrame,
        ];
        assert_eq!(
            legacy.map(message_kind_byte),
            [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18
            ]
        );
        assert_eq!(message_kind_byte(MessageKind::ExtractionCommitFrame), 19);
        assert_eq!(message_kind_byte(MessageKind::RecallFrame), 20);
    }

    #[test]
    fn protocol_1_15_appends_bounded_extraction_and_recall_frames() {
        let payload = extraction_terminal_payload();
        let extraction = WireMessage::ExtractionCommitFrame(crate::ExtractionCommitFrameV1 {
            schema_version: crate::TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence: 14,
            mutation_id: [1; 16],
            character_id: [2; 16],
            issued_at_unix_millis: 1,
            payload_hash: payload.canonical_hash(),
            payload,
        });
        let extraction_frame = encode_frame(&extraction).unwrap();
        assert_eq!(
            u16::from_le_bytes([extraction_frame[6], extraction_frame[7]]),
            15
        );
        assert_eq!(extraction_frame[8], 19);
        assert_eq!(decode_frame(&extraction_frame), Ok(extraction.clone()));
        assert_eq!(extraction.channel(), crate::NetworkChannel::Mutation);
        assert_eq!(
            encode_protocol_1_14_compatibility_frame(&extraction),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
        assert_eq!(
            encode_protocol_1_12_compatibility_frame(&extraction),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
        let extraction_hash = blake3::hash(&extraction_frame).to_hex().to_string();

        let recall = WireMessage::RecallFrame(crate::RecallFrameV1 {
            schema_version: crate::TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence: 15,
            character_id: [2; 16],
            client_tick: 99,
            intent: crate::RecallIntentV1::Start,
        });
        let recall_frame = encode_frame(&recall).unwrap();
        assert_eq!(recall_frame[8], 20);
        assert_eq!(decode_frame(&recall_frame), Ok(recall.clone()));
        assert_eq!(recall.channel(), crate::NetworkChannel::Action);
        assert_eq!(
            encode_protocol_1_14_compatibility_frame(&recall),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
        let recall_hash = blake3::hash(&recall_frame).to_hex().to_string();

        assert_eq!(
            [extraction_hash, recall_hash],
            [
                "c6a0ba1c70a34e080446b6b291a29fc48d0fe38a317a027c1a31ec83e55543f4".to_owned(),
                "1ee600a829fed22e07db06ae8b2291276f8e3cc2ff3bef50d567dabaa9c4b129".to_owned(),
            ]
        );
    }

    #[test]
    fn protocol_1_15_appends_bounded_extraction_and_recall_results() {
        let extraction_result = WireMessage::ReliableEvent(crate::ReliableEventFrame {
            sequence: 1,
            server_tick: 100,
            event: crate::ReliableEvent::ExtractionCommitResult(Box::new(
                crate::ExtractionCommitResultV1::Pending {
                    schema_version: crate::TERMINAL_INVENTORY_SCHEMA_VERSION,
                    request_sequence: 14,
                    mutation_id: [1; 16],
                    character_id: [2; 16],
                    extraction_request_id: [3; 16],
                },
            )),
        });
        let frame = encode_frame(&extraction_result).unwrap();
        assert_eq!(decode_frame(&frame), Ok(extraction_result.clone()));
        assert_eq!(extraction_result.channel(), crate::NetworkChannel::Mutation);
        assert_eq!(
            encode_protocol_1_14_compatibility_frame(&extraction_result),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
        let extraction_result_hash = blake3::hash(&frame).to_hex().to_string();

        let recall_result = WireMessage::ReliableEvent(crate::ReliableEventFrame {
            sequence: 2,
            server_tick: 100,
            event: crate::ReliableEvent::RecallResult(Box::new(crate::RecallResultV1::Pending {
                schema_version: crate::TERMINAL_INVENTORY_SCHEMA_VERSION,
                request_sequence: 15,
                character_id: [2; 16],
                started_tick: 100,
                completion_tick: 112,
                pending_item_count: 1,
                pending_material_stack_count: 0,
            })),
        });
        let frame = encode_frame(&recall_result).unwrap();
        assert_eq!(decode_frame(&frame), Ok(recall_result.clone()));
        assert_eq!(recall_result.channel(), crate::NetworkChannel::Action);
        assert_eq!(
            encode_protocol_1_14_compatibility_frame(&recall_result),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
        let recall_result_hash = blake3::hash(&frame).to_hex().to_string();

        assert_eq!(
            [extraction_result_hash, recall_result_hash],
            [
                "0c2d7228a4069c08772237bded8a330b1074325e9c72c56838b575829d85725b".to_owned(),
                "416bfae83fcd1a407e746fe7785d733b09cfda326ee026407151f5686d220ef7".to_owned(),
            ]
        );
    }

    #[test]
    fn terminal_framing_rejects_hash_kind_and_unknown_kind_drift() {
        let payload = extraction_terminal_payload();
        let mut extraction = crate::ExtractionCommitFrameV1 {
            schema_version: crate::TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence: 14,
            mutation_id: [1; 16],
            character_id: [2; 16],
            issued_at_unix_millis: 1,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        extraction.payload_hash[0] ^= 1;
        assert_eq!(
            encode_frame(&WireMessage::ExtractionCommitFrame(extraction.clone())),
            Err(WireCodecError::InvalidMessage)
        );
        extraction.payload_hash = extraction.payload.canonical_hash();
        let valid = encode_frame(&WireMessage::ExtractionCommitFrame(extraction)).unwrap();

        let mut wrong_kind = valid.clone();
        wrong_kind[8] = 20;
        assert_eq!(
            decode_frame(&wrong_kind),
            Err(WireCodecError::HeaderPayloadMismatch)
        );

        let mut unknown_kind = valid;
        unknown_kind[8] = 21;
        assert_eq!(
            decode_frame(&unknown_kind),
            Err(WireCodecError::UnknownMessageKind(21))
        );
    }

    #[test]
    fn safe_inventory_framing_rejects_malformed_and_oversized_bytes() {
        let payload = crate::SafeInventoryTransferPayloadV1 {
            kind: crate::SafeInventoryTransferKindV1::CharacterSafeToVault,
            source_slot_index: 0,
            expected_account_version: 1,
            expected_inventory_version: 2,
        };
        let transfer = crate::SafeInventoryTransferFrameV1 {
            mutation_id: [12; 16],
            character_id: [13; 16],
            issued_at_unix_millis: 1,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        let encoded = encode_frame(&WireMessage::SafeInventoryTransferFrame(transfer)).unwrap();
        assert_eq!(
            decode_frame(&encoded[..encoded.len() - 1]),
            Err(WireCodecError::InvalidFrameLength)
        );

        let mut bad_hash = transfer;
        bad_hash.payload_hash[0] ^= 1;
        assert_eq!(
            encode_frame(&WireMessage::SafeInventoryTransferFrame(bad_hash)),
            Err(WireCodecError::InvalidMessage)
        );
        assert_eq!(
            decode_frame(&vec![0; RELIABLE_FRAME_LIMIT + 1]),
            Err(WireCodecError::InvalidFrameLength)
        );
    }
}
