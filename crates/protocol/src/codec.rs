use postcard::{from_bytes, to_stdvec};
use thiserror::Error;

use crate::{M02_PROTOCOL_MINOR, MessageKind, PROTOCOL_MAJOR, ProtocolVersion, WireMessage};

const MAGIC: [u8; 4] = *b"GBN1";
pub const FRAME_HEADER_BYTES: usize = 14;
pub const DATAGRAM_FRAME_LIMIT: usize = 1_200;
pub const RELIABLE_FRAME_LIMIT: usize = 64 * 1_024;

pub fn encode_frame(message: &WireMessage) -> Result<Vec<u8>, WireCodecError> {
    encode_frame_for_version(message, ProtocolVersion::current())
}

/// Reproduces canonical M02 bytes for immutable fixtures and package verification. These frames
/// are not accepted by the exact-minor 1.6 live decoder.
pub fn encode_m02_compatibility_frame(message: &WireMessage) -> Result<Vec<u8>, WireCodecError> {
    if matches!(
        message.kind(),
        MessageKind::AccountBootstrapFrame
            | MessageKind::CharacterMutationFrame
            | MessageKind::WorldFlowFrame
            | MessageKind::ProgressionQueryFrame
    ) {
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
    use super::*;
    use crate::{
        AccountBootstrapFrame, AccountBootstrapRequest, CharacterMutationFrame,
        CharacterMutationPayload, GRAVE_ARBALIST_CLASS_ID, InputFrame, ManifestHash, WireMessage,
        WireText,
    };

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

    #[test]
    fn canonical_frame_round_trips_and_has_pinned_bytes() {
        let frame = encode_frame(&input_message()).unwrap();
        assert!(frame.len() <= DATAGRAM_FRAME_LIMIT);
        assert_eq!(decode_frame(&frame).unwrap(), input_message());
        assert_eq!(
            blake3::hash(&frame).to_hex().to_string(),
            "ece8840cb3fb69b07847973f8b50ff81c39c7e72c900db3845bfc9aee5c9634f"
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
}
