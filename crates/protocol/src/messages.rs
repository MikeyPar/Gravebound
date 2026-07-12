use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ClientHello, HandshakeResponse, NetworkChannel, WireText};

pub const FIXED_VECTOR_SCALE: i16 = 1_000;
pub const MAX_SNAPSHOT_ENTITIES_PER_CHUNK: usize = 32;
pub const MAX_SNAPSHOT_CHUNKS: u16 = 64;
pub const CONTENT_ID_MAX_BYTES: usize = 96;
pub const ENTITY_STATE_ALIVE: u32 = 1 << 0;
pub const ENTITY_STATE_ELIGIBLE: u32 = 1 << 1;
pub const ENTITY_STATE_COLLECTED: u32 = 1 << 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    ClientHello,
    HandshakeResponse,
    InputFrame,
    ActionFrame,
    SnapshotChunk,
    ReliableEvent,
    MutationRequest,
    SessionControlFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputFrame {
    pub sequence: u32,
    pub client_tick: u64,
    pub movement_x_milli: i16,
    pub movement_y_milli: i16,
    pub aim_x_milli: i16,
    pub aim_y_milli: i16,
    pub held_primary: bool,
    pub primary_sequence: u32,
    pub ability_1_sequence: u32,
    pub ability_2_sequence: u32,
}

impl InputFrame {
    pub fn validate(&self) -> Result<(), MessageValidationError> {
        if self.sequence == 0 {
            return Err(MessageValidationError::ZeroSequence);
        }
        for component in [
            self.movement_x_milli,
            self.movement_y_milli,
            self.aim_x_milli,
            self.aim_y_milli,
        ] {
            if !(-FIXED_VECTOR_SCALE..=FIXED_VECTOR_SCALE).contains(&component) {
                return Err(MessageValidationError::VectorComponent);
            }
        }
        if self.aim_x_milli == 0 && self.aim_y_milli == 0 {
            return Err(MessageValidationError::ZeroAim);
        }
        if self.held_primary && self.primary_sequence == 0 {
            return Err(MessageValidationError::HeldPrimaryWithoutSequence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Ability1Press,
    Ability2Press,
    RecallStart,
    RecallCancel,
    Interact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionFrame {
    pub sequence: u32,
    pub client_tick: u64,
    pub action: ActionKind,
}

impl ActionFrame {
    pub const fn validate(&self) -> Result<(), MessageValidationError> {
        if self.sequence == 0 {
            return Err(MessageValidationError::ZeroSequence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Player,
    Enemy,
    Boss,
    Loot,
    Objective,
    FriendlyProjectile,
    HostileProjectile,
    PersonalPickup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitySnapshot {
    pub entity_id: u64,
    pub kind: EntityKind,
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
    pub velocity_x_milli_tiles_per_second: i32,
    pub velocity_y_milli_tiles_per_second: i32,
    pub source_input_sequence: u32,
    pub source_projectile_ordinal: u16,
    pub current_health: u32,
    pub maximum_health: u32,
    pub state_flags: u32,
}

impl EntitySnapshot {
    const fn validate(&self) -> Result<(), MessageValidationError> {
        if self.entity_id == 0 {
            return Err(MessageValidationError::ZeroEntityId);
        }
        match self.kind {
            EntityKind::Player | EntityKind::Enemy | EntityKind::Boss
                if self.maximum_health == 0 || self.current_health > self.maximum_health =>
            {
                return Err(MessageValidationError::InvalidHealth);
            }
            EntityKind::Player | EntityKind::Enemy | EntityKind::Boss => {}
            _ if self.maximum_health != 0 || self.current_health != 0 => {
                return Err(MessageValidationError::UnexpectedHealth);
            }
            _ => {}
        }
        match self.kind {
            EntityKind::FriendlyProjectile if self.source_input_sequence == 0 => {
                return Err(MessageValidationError::MissingProjectileSourceSequence);
            }
            EntityKind::FriendlyProjectile => {}
            _ if self.source_input_sequence != 0 => {
                return Err(MessageValidationError::UnexpectedProjectileSourceSequence);
            }
            _ => {}
        }
        if !matches!(self.kind, EntityKind::FriendlyProjectile)
            && self.source_projectile_ordinal != 0
        {
            return Err(MessageValidationError::UnexpectedProjectileOrdinal);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotChunk {
    pub sequence: u32,
    pub server_tick: u64,
    pub state_version: u64,
    pub acknowledged_input_sequence: u32,
    pub chunk_index: u16,
    pub chunk_count: u16,
    pub entities: Vec<EntitySnapshot>,
}

impl SnapshotChunk {
    pub fn validate(&self) -> Result<(), MessageValidationError> {
        if self.sequence == 0 {
            return Err(MessageValidationError::ZeroSequence);
        }
        if self.chunk_count == 0
            || self.chunk_count > MAX_SNAPSHOT_CHUNKS
            || self.chunk_index >= self.chunk_count
        {
            return Err(MessageValidationError::InvalidSnapshotChunk);
        }
        if self.entities.len() > MAX_SNAPSHOT_ENTITIES_PER_CHUNK {
            return Err(MessageValidationError::SnapshotEntityCount);
        }
        let mut ids = BTreeSet::new();
        for entity in &self.entities {
            entity.validate()?;
            if !ids.insert(entity.entity_id) {
                return Err(MessageValidationError::DuplicateEntityId);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionResultCode {
    Accepted,
    StaleSequence,
    Cooldown,
    InvalidState,
    RateLimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocialPingKind {
    Danger,
    Gather,
    Loot,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternDescriptor {
    pub pattern_id: WireText<CONTENT_ID_MAX_BYTES>,
    pub content_version: WireText<32>,
    pub start_tick: u64,
    pub origin_x_milli_tiles: i32,
    pub origin_y_milli_tiles: i32,
    pub parameter_hash: u64,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationResult {
    pub mutation_id: [u8; 16],
    pub accepted: bool,
    pub code: MutationResultCode,
    pub state_version: u64,
}

impl MutationResult {
    fn validate(&self) -> Result<(), MessageValidationError> {
        if self.mutation_id == [0; 16] {
            return Err(MessageValidationError::ZeroMutationId);
        }
        if self.accepted != (self.code == MutationResultCode::Accepted) {
            return Err(MessageValidationError::MutationResultMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationResultCode {
    Accepted,
    Ineligible,
    NotFound,
    AlreadyResolved,
    OutOfRange,
    InventoryRejected,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickupPlacement {
    Take,
    Equip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationRequest {
    pub mutation_id: [u8; 16],
    pub pickup_id: u64,
    pub placement: PickupPlacement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionControlRequest {
    Join,
    Reconnect { prior_session_id: WireText<64> },
    Leave,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionControlFrame {
    pub sequence: u32,
    pub client_tick: u64,
    pub client_monotonic_micros: u64,
    pub request: SessionControlRequest,
}

impl SessionControlFrame {
    pub const fn validate(&self) -> Result<(), MessageValidationError> {
        if self.sequence == 0 {
            return Err(MessageValidationError::ZeroSequence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionDestination {
    CombatInstance,
    LanternHalls,
    DeathFinal,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionControlResultCode {
    Joined,
    Reattached,
    LeaveAccepted,
    SessionNotFound,
    Unauthorized,
    StaleSequence,
    SessionResolved,
    ServerShuttingDown,
}

impl SessionControlResultCode {
    #[must_use]
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Joined | Self::Reattached | Self::LeaveAccepted)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionControlResult {
    pub request_sequence: u32,
    pub accepted: bool,
    pub code: SessionControlResultCode,
    pub session_id: WireText<64>,
    pub destination: SessionDestination,
    pub server_tick: u64,
    pub state_version: u64,
    pub server_monotonic_micros: u64,
    pub replaced_previous_transport: bool,
}

impl SessionControlResult {
    const fn validate(&self) -> Result<(), MessageValidationError> {
        if self.request_sequence == 0 {
            return Err(MessageValidationError::ZeroSequence);
        }
        if self.accepted != self.code.is_accepted() {
            return Err(MessageValidationError::SessionControlResultMismatch);
        }
        if self.replaced_previous_transport
            && !matches!(self.code, SessionControlResultCode::Reattached)
        {
            return Err(MessageValidationError::UnexpectedTransportReplacement);
        }
        Ok(())
    }
}

impl MutationRequest {
    pub const fn validate(&self) -> Result<(), MessageValidationError> {
        if all_zero(&self.mutation_id) {
            return Err(MessageValidationError::ZeroMutationId);
        }
        if self.pickup_id == 0 {
            return Err(MessageValidationError::ZeroPickupId);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlEvent {
    TimeSync {
        request_id: u32,
        server_tick: u64,
        server_monotonic_micros: u64,
    },
    ServerShuttingDown,
    SessionResult(SessionControlResult),
    Error {
        code: WireText<64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReliableEvent {
    ActionResult {
        action_sequence: u32,
        code: ActionResultCode,
    },
    PatternStarted(PatternDescriptor),
    MutationResult(MutationResult),
    Control(ControlEvent),
    SocialPing {
        ping_sequence: u32,
        kind: SocialPingKind,
        x_milli_tiles: i32,
        y_milli_tiles: i32,
    },
}

impl ReliableEvent {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        match self {
            Self::ActionResult { .. } => NetworkChannel::Action,
            Self::PatternStarted(_) => NetworkChannel::Pattern,
            Self::MutationResult(_) => NetworkChannel::Mutation,
            Self::Control(_) => NetworkChannel::Control,
            Self::SocialPing { .. } => NetworkChannel::Social,
        }
    }

    fn validate(&self) -> Result<(), MessageValidationError> {
        match self {
            Self::ActionResult {
                action_sequence, ..
            } if *action_sequence == 0 => Err(MessageValidationError::ZeroSequence),
            Self::MutationResult(result) => result.validate(),
            Self::Control(ControlEvent::SessionResult(result)) => result.validate(),
            Self::SocialPing { ping_sequence, .. } if *ping_sequence == 0 => {
                Err(MessageValidationError::ZeroSequence)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReliableEventFrame {
    pub sequence: u32,
    pub server_tick: u64,
    pub event: ReliableEvent,
}

impl ReliableEventFrame {
    pub fn validate(&self) -> Result<(), MessageValidationError> {
        if self.sequence == 0 {
            return Err(MessageValidationError::ZeroSequence);
        }
        self.event.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireMessage {
    ClientHello(ClientHello),
    HandshakeResponse(HandshakeResponse),
    InputFrame(InputFrame),
    ActionFrame(ActionFrame),
    SnapshotChunk(SnapshotChunk),
    ReliableEvent(ReliableEventFrame),
    MutationRequest(MutationRequest),
    SessionControlFrame(SessionControlFrame),
}

impl WireMessage {
    #[must_use]
    pub const fn kind(&self) -> MessageKind {
        match self {
            Self::ClientHello(_) => MessageKind::ClientHello,
            Self::HandshakeResponse(_) => MessageKind::HandshakeResponse,
            Self::InputFrame(_) => MessageKind::InputFrame,
            Self::ActionFrame(_) => MessageKind::ActionFrame,
            Self::SnapshotChunk(_) => MessageKind::SnapshotChunk,
            Self::ReliableEvent(_) => MessageKind::ReliableEvent,
            Self::MutationRequest(_) => MessageKind::MutationRequest,
            Self::SessionControlFrame(_) => MessageKind::SessionControlFrame,
        }
    }

    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        match self {
            Self::ClientHello(_) | Self::HandshakeResponse(_) | Self::SessionControlFrame(_) => {
                NetworkChannel::Control
            }
            Self::InputFrame(_) => NetworkChannel::Input,
            Self::ActionFrame(_) => NetworkChannel::Action,
            Self::SnapshotChunk(_) => NetworkChannel::Snapshot,
            Self::ReliableEvent(frame) => frame.event.channel(),
            Self::MutationRequest(_) => NetworkChannel::Mutation,
        }
    }

    #[must_use]
    pub const fn uses_datagram(&self) -> bool {
        matches!(self, Self::InputFrame(_) | Self::SnapshotChunk(_))
    }

    pub fn validate(&self) -> Result<(), MessageValidationError> {
        match self {
            Self::ClientHello(value) => value
                .validate()
                .map_err(|_| MessageValidationError::Handshake),
            Self::HandshakeResponse(value) => value
                .validate()
                .map_err(|_| MessageValidationError::Handshake),
            Self::InputFrame(value) => value.validate(),
            Self::ActionFrame(value) => value.validate(),
            Self::SnapshotChunk(value) => value.validate(),
            Self::ReliableEvent(value) => value.validate(),
            Self::MutationRequest(value) => value.validate(),
            Self::SessionControlFrame(value) => value.validate(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MessageValidationError {
    #[error("message sequence must be nonzero")]
    ZeroSequence,
    #[error("fixed-point vector component must remain within -1000..=1000")]
    VectorComponent,
    #[error("aim vector cannot be zero")]
    ZeroAim,
    #[error("held primary input requires a nonzero primary sequence")]
    HeldPrimaryWithoutSequence,
    #[error("entity ID must be nonzero")]
    ZeroEntityId,
    #[error("entity health is invalid")]
    InvalidHealth,
    #[error("non-health entity must carry zero current and maximum health")]
    UnexpectedHealth,
    #[error("friendly projectile requires a nonzero source input sequence")]
    MissingProjectileSourceSequence,
    #[error("non-friendly-projectile entity cannot carry a source input sequence")]
    UnexpectedProjectileSourceSequence,
    #[error("non-friendly-projectile entity cannot carry a projectile ordinal")]
    UnexpectedProjectileOrdinal,
    #[error("snapshot chunk index/count is invalid")]
    InvalidSnapshotChunk,
    #[error("snapshot exceeds {MAX_SNAPSHOT_ENTITIES_PER_CHUNK} entities per chunk")]
    SnapshotEntityCount,
    #[error("snapshot entity IDs must be unique inside one chunk")]
    DuplicateEntityId,
    #[error("mutation ID must be nonzero")]
    ZeroMutationId,
    #[error("pickup ID must be nonzero")]
    ZeroPickupId,
    #[error("mutation accepted flag and result code disagree")]
    MutationResultMismatch,
    #[error("session control accepted flag and result code disagree")]
    SessionControlResultMismatch,
    #[error("only a successful reattach may replace a previous transport")]
    UnexpectedTransportReplacement,
    #[error("handshake payload failed semantic validation")]
    Handshake,
}

const fn all_zero(bytes: &[u8; 16]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_bounds_and_action_sequences_fail_closed() {
        let mut input = InputFrame {
            sequence: 1,
            client_tick: 7,
            movement_x_milli: 1_000,
            movement_y_milli: 0,
            aim_x_milli: 0,
            aim_y_milli: -1_000,
            held_primary: true,
            primary_sequence: 2,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        };
        assert_eq!(input.validate(), Ok(()));
        input.movement_x_milli = 1_001;
        assert_eq!(
            input.validate(),
            Err(MessageValidationError::VectorComponent)
        );
        assert_eq!(
            ActionFrame {
                sequence: 0,
                client_tick: 1,
                action: ActionKind::Interact
            }
            .validate(),
            Err(MessageValidationError::ZeroSequence)
        );
        input.movement_x_milli = 0;
        input.held_primary = true;
        input.primary_sequence = 0;
        assert_eq!(
            input.validate(),
            Err(MessageValidationError::HeldPrimaryWithoutSequence)
        );
    }

    #[test]
    fn snapshot_chunks_reject_invalid_counts_duplicates_and_health() {
        let entity = EntitySnapshot {
            entity_id: 1,
            kind: EntityKind::Player,
            x_milli_tiles: 4_000,
            y_milli_tiles: 12_000,
            velocity_x_milli_tiles_per_second: 0,
            velocity_y_milli_tiles_per_second: 0,
            source_input_sequence: 0,
            source_projectile_ordinal: 0,
            current_health: 128,
            maximum_health: 128,
            state_flags: 0,
        };
        let mut chunk = SnapshotChunk {
            sequence: 1,
            server_tick: 2,
            state_version: 3,
            acknowledged_input_sequence: 1,
            chunk_index: 0,
            chunk_count: 1,
            entities: vec![entity.clone()],
        };
        assert_eq!(chunk.validate(), Ok(()));
        chunk.entities.push(entity);
        assert_eq!(
            chunk.validate(),
            Err(MessageValidationError::DuplicateEntityId)
        );

        let mut projectile = chunk.entities[0].clone();
        projectile.entity_id = 2;
        projectile.kind = EntityKind::FriendlyProjectile;
        projectile.current_health = 0;
        projectile.maximum_health = 0;
        assert_eq!(
            projectile.validate(),
            Err(MessageValidationError::MissingProjectileSourceSequence)
        );
        projectile.source_input_sequence = 1;
        assert_eq!(projectile.validate(), Ok(()));
        projectile.kind = EntityKind::HostileProjectile;
        projectile.source_input_sequence = 0;
        projectile.source_projectile_ordinal = 1;
        assert_eq!(
            projectile.validate(),
            Err(MessageValidationError::UnexpectedProjectileOrdinal)
        );
    }

    #[test]
    fn every_wire_message_maps_to_its_authoritative_channel() {
        let action = WireMessage::ActionFrame(ActionFrame {
            sequence: 1,
            client_tick: 2,
            action: ActionKind::RecallStart,
        });
        assert_eq!(action.channel(), NetworkChannel::Action);
        assert!(!action.uses_datagram());
        let input = WireMessage::InputFrame(InputFrame {
            sequence: 1,
            client_tick: 2,
            movement_x_milli: 0,
            movement_y_milli: 0,
            aim_x_milli: 1_000,
            aim_y_milli: 0,
            held_primary: false,
            primary_sequence: 0,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        });
        assert_eq!(input.channel(), NetworkChannel::Input);
        assert!(input.uses_datagram());

        let mutation = WireMessage::MutationRequest(MutationRequest {
            mutation_id: [1; 16],
            pickup_id: 9,
            placement: PickupPlacement::Take,
        });
        assert_eq!(mutation.channel(), NetworkChannel::Mutation);
        assert!(!mutation.uses_datagram());
        assert_eq!(mutation.validate(), Ok(()));

        let control = WireMessage::SessionControlFrame(SessionControlFrame {
            sequence: 1,
            client_tick: 2,
            client_monotonic_micros: 3,
            request: SessionControlRequest::Reconnect {
                prior_session_id: WireText::new("session-1").unwrap(),
            },
        });
        assert_eq!(control.kind(), MessageKind::SessionControlFrame);
        assert_eq!(control.channel(), NetworkChannel::Control);
        assert!(!control.uses_datagram());
        assert_eq!(control.validate(), Ok(()));
    }

    #[test]
    fn mutations_reject_zero_identity_and_inconsistent_results() {
        assert_eq!(
            MutationRequest {
                mutation_id: [0; 16],
                pickup_id: 1,
                placement: PickupPlacement::Take,
            }
            .validate(),
            Err(MessageValidationError::ZeroMutationId)
        );
        assert_eq!(
            MutationRequest {
                mutation_id: [1; 16],
                pickup_id: 0,
                placement: PickupPlacement::Take,
            }
            .validate(),
            Err(MessageValidationError::ZeroPickupId)
        );
        assert_eq!(
            MutationResult {
                mutation_id: [1; 16],
                accepted: true,
                code: MutationResultCode::OutOfRange,
                state_version: 1,
            }
            .validate(),
            Err(MessageValidationError::MutationResultMismatch)
        );
    }

    #[test]
    fn session_control_is_bounded_typed_and_consistent() {
        assert_eq!(
            SessionControlFrame {
                sequence: 0,
                client_tick: 0,
                client_monotonic_micros: 0,
                request: SessionControlRequest::Join,
            }
            .validate(),
            Err(MessageValidationError::ZeroSequence)
        );
        let mut result = SessionControlResult {
            request_sequence: 1,
            accepted: true,
            code: SessionControlResultCode::Joined,
            session_id: WireText::new("session-1").unwrap(),
            destination: SessionDestination::CombatInstance,
            server_tick: 1,
            state_version: 1,
            server_monotonic_micros: 1,
            replaced_previous_transport: false,
        };
        assert_eq!(result.validate(), Ok(()));
        result.accepted = false;
        assert_eq!(
            result.validate(),
            Err(MessageValidationError::SessionControlResultMismatch)
        );
        result.accepted = true;
        result.replaced_previous_transport = true;
        assert_eq!(
            result.validate(),
            Err(MessageValidationError::UnexpectedTransportReplacement)
        );
        result.code = SessionControlResultCode::Reattached;
        assert_eq!(result.validate(), Ok(()));
    }
}
