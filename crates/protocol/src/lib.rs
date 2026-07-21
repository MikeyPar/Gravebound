//! Versioned Gravebound network contracts.
//!
//! This crate owns wire-facing message primitives, protocol versions, channel semantics, and
//! typed protocol errors. It never owns gameplay rules, rendering, transport sockets, sessions,
//! or persistence. `GB-M02-01` supplies the bounded handshake, gameplay envelopes, and codec.

mod account;
mod bargain;
mod bounded;
mod codec;
mod core_pending_inventory;
mod core_private_route;
mod death_view;
mod field_equipment;
mod handshake;
mod messages;
mod oath;
mod progression;
mod reliable_inbox;
mod resolution_hold;
mod safe_inventory;
mod successor;
mod terminal_inventory;
mod world_flow;

pub use account::{
    AccountBootstrapFrame, AccountBootstrapRequest, AccountBootstrapResult, AccountErrorCode,
    AccountMessageValidationError, AccountNamespace, AccountSnapshot, CHARACTER_ID_BYTES,
    CLASS_ID_MAX_BYTES, CORE_CHARACTER_SLOT_CAPACITY, CharacterLifeState, CharacterMutationFrame,
    CharacterMutationPayload, CharacterMutationResult, CharacterSecurityState, CharacterSnapshot,
    GRAVE_ARBALIST_CLASS_ID, MAX_ACCOUNT_CHARACTERS, MAX_CORE_CHARACTER_LEVEL, MUTATION_ID_BYTES,
    PAYLOAD_HASH_BYTES,
};
pub use bargain::{
    BARGAIN_CHARACTER_ID_BYTES, BARGAIN_ID_BYTES, BARGAIN_MUTATION_ID_BYTES,
    BARGAIN_OFFER_ID_BYTES, BARGAIN_PAYLOAD_HASH_BYTES, BELL_DEBT_ID, BargainContentRevisionV1,
    BargainDecision, BargainDecisionFrame, BargainDecisionPayload, BargainDecisionResult,
    BargainOfferCell, BargainOfferProjection, BargainOfferState, BargainProjection,
    BargainResultCode, BargainStatComparison, BargainValidationError, BargainViewFrame,
    BargainViewResult, CINDER_HUNGER_ID, LANTERN_ASH_ID,
};
pub use bounded::{AuthTicket, BoundedValueError, ManifestHash, WireText};
pub use codec::{
    DATAGRAM_FRAME_LIMIT, FRAME_HEADER_BYTES, RELIABLE_FRAME_LIMIT, WireCodecError, decode_frame,
    encode_frame, encode_m02_compatibility_frame, encode_protocol_1_12_compatibility_frame,
    encode_protocol_1_14_compatibility_frame, encode_protocol_1_15_compatibility_frame,
    encode_protocol_1_16_compatibility_frame, encode_protocol_1_17_compatibility_frame,
    encode_protocol_1_18_compatibility_frame,
};
pub use core_pending_inventory::{
    CORE_PENDING_BACKPACK_CAPACITY, CORE_PENDING_INVENTORY_FEATURE_FLAG,
    CORE_PENDING_INVENTORY_SCHEMA_VERSION, CORE_PENDING_ITEM_CAPACITY,
    CORE_PENDING_MATERIAL_CAPACITY, CoreExtractionReadyStateV1, CorePendingInventoryStateV1,
    CorePendingInventoryValidationError, CorePendingItemKindV1, CorePendingItemLocationV1,
    CorePendingItemV1, CorePendingMaterialV1,
};
pub use core_private_route::{
    CORE_PRIVATE_ROUTE_SCHEMA_VERSION, CorePrivateRouteAvailabilityV1,
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteReadinessV1,
    CorePrivateRouteRoomV1, CorePrivateRouteSceneV1, CorePrivateRouteStateV1,
    CorePrivateRouteValidationError,
};
pub use death_view::{
    DEATH_SUMMARY_REVISION, DEATH_VIEW_CHARACTER_NAME_MAX_BYTES, DEATH_VIEW_DIGEST_BYTES,
    DEATH_VIEW_ID_BYTES, DEATH_VIEW_ID_MAX_BYTES, DEATH_VIEW_MAX_BARGAINS,
    DEATH_VIEW_MAX_LOST_PROJECTIONS, DEATH_VIEW_MAX_LOST_PROJECTIONS_PER_PAGE,
    DEATH_VIEW_MAX_MEMORIALS_PER_PAGE, DEATH_VIEW_MAX_STATUSES_PER_TRACE_ENTRY,
    DEATH_VIEW_MAX_SUMMARY_DAMAGE_ENTRIES, DEATH_VIEW_MAX_TRACE_ENTRIES,
    DEATH_VIEW_MAX_TRACE_ENTRIES_PER_PAGE, DEATH_VIEW_SCHEMA_VERSION,
    DEATH_VIEW_TRACE_WINDOW_TICKS, DeathCauseV1, DeathCharacterName, DeathDamageTypeV1,
    DeathEchoOutcomeV1, DeathMemorialCursorV1, DeathMemorialEntryV1, DeathNetworkStateV1,
    DeathRecallStateV1, DeathSummaryProjectionEntryV1, DeathSummaryProjectionKindV1,
    DeathSummaryViewV1, DeathTraceEntryV1, DeathTracePageV1, DeathTraceStatusV1,
    DeathViewContentRevisionV1, DeathViewFrameV1, DeathViewRequestV1, DeathViewResultCodeV1,
    DeathViewResultV1, DeathViewValidationError, LatestCommittedDeathV1,
};
pub use field_equipment::{
    FIELD_EQUIPMENT_CHANGE_CAPACITY, FIELD_EQUIPMENT_ID_MAX_BYTES, FIELD_EQUIPMENT_ITEM_UID_BYTES,
    FIELD_EQUIPMENT_PICKUP_ID_BYTES, FIELD_EQUIPMENT_PREVIEW_HASH_BYTES,
    FieldEquipmentComparisonAxisV1, FieldEquipmentComparisonChangeV1,
    FieldEquipmentComparisonPreferenceV1, FieldEquipmentConfirmFrameV1,
    FieldEquipmentConfirmPayloadV1, FieldEquipmentItemV1, FieldEquipmentPreviewFrameV1,
    FieldEquipmentPreviewProjectionV1, FieldEquipmentRarityV1,
    FieldEquipmentReplacementDestinationV1, FieldEquipmentResultCodeV1, FieldEquipmentSlotV1,
    FieldEquipmentSourceV1, FieldEquipmentValidationError,
};
pub use handshake::{
    ClientHello, Compression, HandshakeRejection, HandshakeResponse, Platform, ServerHello,
};
pub use messages::{
    ActionFrame, ActionKind, ActionResultCode, ControlEvent, ENTITY_STATE_ALIVE,
    ENTITY_STATE_COLLECTED, ENTITY_STATE_ELIGIBLE, EntityKind, EntitySnapshot, InputFrame,
    MAX_SNAPSHOT_CHUNKS, MAX_SNAPSHOT_ENTITIES_PER_CHUNK, MessageKind, MessageValidationError,
    MutationRequest, MutationResult, MutationResultCode, PatternDescriptor, PickupPlacement,
    ReliableEvent, ReliableEventFrame, SessionControlFrame, SessionControlRequest,
    SessionControlResult, SessionControlResultCode, SessionDestination, SnapshotChunk,
    SocialPingKind, WireMessage,
};
pub use oath::{
    InitialOathSelectionFrame, InitialOathSelectionPayload, InitialOathSelectionResult,
    LONG_VIGIL_ID, NAILKEEPER_ID, OATH_CHARACTER_ID_BYTES, OATH_ID_BYTES, OATH_MUTATION_ID_BYTES,
    OATH_PAYLOAD_HASH_BYTES, OathContentRevisionV1, OathProjection, OathResultCode,
    OathSelectionState, OathValidationError, OathViewFrame, OathViewResult,
};
pub use progression::{
    PROGRESSION_REWARD_EVENT_ID_BYTES, ProgressionCapState, ProgressionProjection,
    ProgressionQueryFrame, ProgressionResult, ProgressionResultCode, ProgressionValidationError,
};
pub use reliable_inbox::{
    RELIABLE_EVENT_REORDER_CAPACITY, ReliableEventInbox, ReliableEventInboxError,
};
pub use resolution_hold::{
    RESOLUTION_HOLD_DIGEST_BYTES, RESOLUTION_HOLD_ID_BYTES, RESOLUTION_HOLD_ID_MAX_BYTES,
    RESOLUTION_HOLD_MAX_ITEMS, RESOLUTION_HOLD_MAX_STACKS, RESOLUTION_HOLD_SCHEMA_VERSION,
    ResolutionHoldActionV1, ResolutionHoldDestinationV1, ResolutionHoldDispositionV1,
    ResolutionHoldItemKindV1, ResolutionHoldItemTransitionV1, ResolutionHoldItemV1,
    ResolutionHoldMutationFrameV1, ResolutionHoldMutationPayloadV1, ResolutionHoldMutationResultV1,
    ResolutionHoldQueryFrameV1, ResolutionHoldQueryResultV1, ResolutionHoldRejectionCodeV1,
    ResolutionHoldStackV1, ResolutionHoldValidationError, ResolutionHoldVersionAdvanceV1,
    ResolutionHoldVersionVectorV1, ResolutionHoldVersionsV1, StoredResolutionHoldMutationResultV1,
};
pub use safe_inventory::{
    SAFE_INVENTORY_ITEM_UID_BYTES, SAFE_INVENTORY_PLACEMENT_CAPACITY,
    SAFE_INVENTORY_RESULT_HASH_BYTES, SafeInventoryDestinationV1, SafeInventoryPlacementV1,
    SafeInventoryResultCodeV1, SafeInventoryTransferFrameV1, SafeInventoryTransferKindV1,
    SafeInventoryTransferPayloadV1, SafeInventoryTransferResultV1, SafeInventoryValidationError,
};
pub use successor::{
    CORE_SUCCESSOR_BASE_SILHOUETTE_ID, SUCCESSOR_CONTENT_ID_MAX_BYTES, SUCCESSOR_ID_BYTES,
    SUCCESSOR_RESULT_HASH_BYTES, SUCCESSOR_SCHEMA_VERSION, SUCCESSOR_STARTER_ITEM_COUNT,
    StoredSuccessorResultV1, SuccessorAppearanceSnapshotV1, SuccessorCreateFrameV1,
    SuccessorCreatePayloadV1, SuccessorCreateResultV1, SuccessorRejectionCodeV1,
    SuccessorStarterItemsV1, SuccessorValidationError, SuccessorVersionVectorV1,
};
pub use terminal_inventory::{
    EXTRACTION_PLACEMENT_CAPACITY, ExtractionCommitFrameV1, ExtractionCommitPayloadV1,
    ExtractionCommitResultV1, ExtractionDestinationV1, ExtractionMaterialCreditV1,
    ExtractionPlacementV1, RECALL_CHANNEL_TICKS, RecallFrameV1, RecallIntentV1, RecallResultV1,
    RecallTerminalTriggerV1, StoredExtractionTerminalResultV1, StoredRecallTerminalResultV1,
    TERMINAL_HALL_CONTENT_ID, TERMINAL_INVENTORY_DIGEST_BYTES, TERMINAL_INVENTORY_ID_BYTES,
    TERMINAL_INVENTORY_SCHEMA_VERSION, TERMINAL_MATERIAL_CAPACITY, TERMINAL_PENDING_ITEM_CAPACITY,
    TERMINAL_STABILIZED_ITEM_CAPACITY, TerminalExpectedVersionsV1, TerminalInventoryCapabilityV1,
    TerminalInventoryRejectionCodeV1, TerminalInventoryValidationError, TerminalVersionAdvanceV1,
    TerminalVersionVectorV1,
};
pub use world_flow::{
    CharacterLocation, CharacterLocationSnapshot, INSTANCE_LINEAGE_ID_BYTES, SafeArrival,
    TRANSFER_ID_BYTES, WORLD_FLOW_ID_MAX_BYTES, WorldFlowContentRevisionV1, WorldFlowFrame,
    WorldFlowRequest, WorldFlowResult, WorldFlowValidationError, WorldTransferCommand,
    WorldTransferMutation, WorldTransferPayload, WorldTransferResultCode,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// First incompatible protocol generation.
pub const PROTOCOL_MAJOR: u16 = 1;
/// Backward-compatible feature generation within [`PROTOCOL_MAJOR`].
pub const PROTOCOL_MINOR: u16 = CORE_PENDING_INVENTORY_PROTOCOL_MINOR;
/// Exact pending-at-risk inventory projection generation.
pub const CORE_PENDING_INVENTORY_PROTOCOL_MINOR: u16 = 19;
/// Exact ordinary Core private-route projection generation.
pub const CORE_PRIVATE_ROUTE_PROTOCOL_MINOR: u16 = 18;
/// Exact M03 successor recovery generation.
pub const SUCCESSOR_PROTOCOL_MINOR: u16 = 17;
/// Exact minimum `ResolutionHold` recovery generation.
pub const RESOLUTION_HOLD_PROTOCOL_MINOR: u16 = 16;
/// Exact successful-extraction and Emergency Recall generation.
pub const TERMINAL_INVENTORY_PROTOCOL_MINOR: u16 = 15;
/// Exact authenticated durable-death view generation.
pub const DEATH_VIEW_PROTOCOL_MINOR: u16 = 14;
/// Exact safe-inventory reliable mutation generation.
pub const SAFE_INVENTORY_PROTOCOL_MINOR: u16 = 12;
/// Exact committed-Caldus-extraction command generation.
pub const CALDUS_EXTRACTION_PROTOCOL_MINOR: u16 = 11;
/// Exact Oath generation retained while Bargain messages are appended.
pub const OATH_PROTOCOL_MINOR: u16 = 9;
/// Exact progression projection generation retained while Oath messages are appended.
pub const PROGRESSION_PROTOCOL_MINOR: u16 = 8;
/// Exact world-flow generation retained while progression projection is appended.
pub const WORLD_FLOW_PROTOCOL_MINOR: u16 = 7;
/// Exact Core identity wire generation retained while world-flow messages are appended.
pub const CORE_IDENTITY_PROTOCOL_MINOR: u16 = 6;
/// Exact final M02 wire generation retained for byte-for-byte compatibility fixtures.
pub const M02_PROTOCOL_MINOR: u16 = 5;
/// Authoritative simulation and client-input cadence from GDD `TECH-012`.
pub const SIMULATION_HZ: u16 = 30;
/// Baseline world snapshot cadence from GDD `TECH-012`.
pub const SNAPSHOT_HZ: u16 = 15;
/// Optional local-critical snapshot ceiling from GDD `TECH-012`.
pub const CRITICAL_SNAPSHOT_HZ: u16 = 20;
/// Default remote interpolation delay from GDD `TECH-012`.
pub const INTERPOLATION_DELAY_MS: u16 = 100;
/// Time-sync refresh cadence from GDD `TECH-012`.
pub const TIME_SYNC_INTERVAL_MS: u16 = 5_000;
/// Realm interest-grid cell edge from GDD `TECH-013`.
pub const INTEREST_CELL_TILES: u16 = 8;
/// Camera interest safety margin from GDD `TECH-013`.
pub const INTEREST_SAFETY_MARGIN_TILES: u16 = 4;
/// Exact executable build ID for the nonpersistent M02 local network gate.
pub const M02_LOCAL_BUILD_ID: &str = "m02-local-1";
/// TLS server name used by the generated loopback certificate.
pub const M02_LOCAL_SERVER_NAME: &str = "localhost";
/// Region label reported by the nonpersistent M02 local server.
pub const M02_LOCAL_REGION_ID: &str = "local-playtest";
/// Explicit feature flag enabling the wipeable Core identity/select surface.
pub const CORE_TEST_IDENTITY_FEATURE_FLAG: &str = "core_test_identity_character_select";
/// Advertised only after every owning package makes the normal Core route honest.
pub const CORE_WORLD_FLOW_FEATURE_FLAG: &str = "core_world_flow_integration";
/// Advertised only by the disposable integrated item/Vault lifecycle harness.
pub const CORE_SAFE_INVENTORY_FEATURE_FLAG: &str = "core_safe_inventory_integration";
/// Advertises the authenticated, read-only durable-death query surface.
pub const CORE_DEATH_VIEW_FEATURE_FLAG: &str = "core_death_views";
/// Advertises the authenticated production extraction-commit mutation surface.
pub const CORE_EXTRACTION_TERMINAL_FEATURE_FLAG: &str = "core_extraction_terminal_v1";
/// Advertises the authenticated Emergency Recall intent and stored-result surface.
pub const CORE_RECALL_TERMINAL_FEATURE_FLAG: &str = "core_emergency_recall_v1";
/// Advertises the authenticated minimum `ResolutionHold` query and mutation surface.
pub const CORE_RESOLUTION_HOLD_FEATURE_FLAG: &str = "core_resolution_hold_v1";
/// Advertises authenticated, exactly-once M03 successor creation.
pub const CORE_SUCCESSOR_FEATURE_FLAG: &str = "core_successor_v1";
/// Build admitted by the explicit wipeable Core identity development endpoint.
pub const M03_CORE_DEV_BUILD_ID: &str = "m03-core-dev-identity-1";
/// Non-promotable content target label advertised by the Core identity endpoint.
pub const M03_CORE_DEV_CONTENT_TARGET: &str = "core-dev";
/// First entity ID in the M02 hosted-arena player namespace.
pub const M02_PLAYER_ENTITY_ID_BASE: u64 = 10_000;
/// Compatibility alias retained while the single-player authority facade remains under test.
pub const M02_ISOLATED_PLAYER_ENTITY_ID: u64 = M02_PLAYER_ENTITY_ID_BASE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    #[must_use]
    pub const fn current() -> Self {
        Self {
            major: PROTOCOL_MAJOR,
            minor: PROTOCOL_MINOR,
        }
    }

    #[must_use]
    pub const fn is_compatible_with(self, required: Self) -> bool {
        self.major == required.major && self.minor == required.minor
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkChannel {
    Input,
    Action,
    Snapshot,
    Pattern,
    Mutation,
    Control,
    Social,
}

impl NetworkChannel {
    pub const ALL: [Self; 7] = [
        Self::Input,
        Self::Action,
        Self::Snapshot,
        Self::Pattern,
        Self::Mutation,
        Self::Control,
        Self::Social,
    ];

    #[must_use]
    pub const fn reliability(self) -> ChannelReliability {
        match self {
            Self::Input => ChannelReliability::SequencedLatestStateDatagram,
            Self::Snapshot => ChannelReliability::LatestStateDatagram,
            Self::Action | Self::Pattern | Self::Mutation | Self::Control | Self::Social => {
                ChannelReliability::ReliableOrdered
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelReliability {
    SequencedLatestStateDatagram,
    LatestStateDatagram,
    ReliableOrdered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateRates {
    pub simulation_hz: u16,
    pub input_hz: u16,
    pub snapshot_hz: u16,
    pub critical_snapshot_hz: u16,
    pub interpolation_delay_ms: u16,
    pub time_sync_interval_ms: u16,
}

impl UpdateRates {
    #[must_use]
    pub const fn canonical() -> Self {
        Self {
            simulation_hz: SIMULATION_HZ,
            input_hz: SIMULATION_HZ,
            snapshot_hz: SNAPSHOT_HZ,
            critical_snapshot_hz: CRITICAL_SNAPSHOT_HZ,
            interpolation_delay_ms: INTERPOLATION_DELAY_MS,
            time_sync_interval_ms: TIME_SYNC_INTERVAL_MS,
        }
    }

    pub const fn validate(self) -> Result<(), ProtocolFoundationError> {
        if self.simulation_hz != SIMULATION_HZ || self.input_hz != SIMULATION_HZ {
            return Err(ProtocolFoundationError::AuthoritativeCadence);
        }
        if self.snapshot_hz != SNAPSHOT_HZ || self.critical_snapshot_hz != CRITICAL_SNAPSHOT_HZ {
            return Err(ProtocolFoundationError::SnapshotCadence);
        }
        if self.interpolation_delay_ms != INTERPOLATION_DELAY_MS
            || self.time_sync_interval_ms != TIME_SYNC_INTERVAL_MS
        {
            return Err(ProtocolFoundationError::ClientTiming);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProtocolFoundationError {
    #[error("simulation and input rates must remain at the authoritative 30 Hz cadence")]
    AuthoritativeCadence,
    #[error("snapshot rates must remain at the documented 15/20 Hz values")]
    SnapshotCadence,
    #[error("interpolation and time-sync timings must match TECH-012")]
    ClientTiming,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_requires_exact_minor_until_an_adapter_exists() {
        let current = ProtocolVersion::current();
        assert!(current.is_compatible_with(current));
        assert!(
            !ProtocolVersion { major: 1, minor: 3 }
                .is_compatible_with(ProtocolVersion { major: 1, minor: 2 })
        );
        assert!(
            !ProtocolVersion { major: 1, minor: 0 }
                .is_compatible_with(ProtocolVersion { major: 1, minor: 1 })
        );
        assert!(
            ProtocolVersion { major: 1, minor: 3 }
                .is_compatible_with(ProtocolVersion { major: 1, minor: 3 })
        );
        assert!(!ProtocolVersion { major: 2, minor: 0 }.is_compatible_with(current));
        assert!(!current.is_compatible_with(ProtocolVersion { major: 1, minor: 0 }));
    }

    #[test]
    fn every_tech_011_channel_has_exact_reliability() {
        assert_eq!(NetworkChannel::ALL.len(), 7);
        assert_eq!(
            NetworkChannel::Input.reliability(),
            ChannelReliability::SequencedLatestStateDatagram
        );
        assert_eq!(
            NetworkChannel::Snapshot.reliability(),
            ChannelReliability::LatestStateDatagram
        );
        for channel in [
            NetworkChannel::Action,
            NetworkChannel::Pattern,
            NetworkChannel::Mutation,
            NetworkChannel::Control,
            NetworkChannel::Social,
        ] {
            assert_eq!(channel.reliability(), ChannelReliability::ReliableOrdered);
        }
    }

    #[test]
    fn canonical_rates_and_interest_constants_match_gdd() {
        assert_eq!(UpdateRates::canonical().validate(), Ok(()));
        assert_eq!(INTEREST_CELL_TILES, 8);
        assert_eq!(INTEREST_SAFETY_MARGIN_TILES, 4);
        assert_eq!(SIMULATION_HZ, 30);
        assert_eq!(SNAPSHOT_HZ, 15);
        assert_eq!(CRITICAL_SNAPSHOT_HZ, 20);
        assert_eq!(INTERPOLATION_DELAY_MS, 100);
        assert_eq!(TIME_SYNC_INTERVAL_MS, 5_000);
    }

    #[test]
    fn timing_drift_fails_closed() {
        let mut rates = UpdateRates::canonical();
        rates.simulation_hz = 60;
        assert_eq!(
            rates.validate(),
            Err(ProtocolFoundationError::AuthoritativeCadence)
        );
        let mut rates = UpdateRates::canonical();
        rates.snapshot_hz = 10;
        assert_eq!(
            rates.validate(),
            Err(ProtocolFoundationError::SnapshotCadence)
        );
        let mut rates = UpdateRates::canonical();
        rates.interpolation_delay_ms = 80;
        assert_eq!(rates.validate(), Err(ProtocolFoundationError::ClientTiming));
    }

    #[test]
    fn pending_inventory_appends_protocol_1_19_with_explicit_negotiation() {
        assert_eq!(PROTOCOL_MINOR, 19);
        assert_eq!(CORE_PENDING_INVENTORY_PROTOCOL_MINOR, 19);
        assert_eq!(CORE_PRIVATE_ROUTE_PROTOCOL_MINOR, 18);
        assert_eq!(SUCCESSOR_PROTOCOL_MINOR, 17);
        assert_eq!(RESOLUTION_HOLD_PROTOCOL_MINOR, 16);
        assert_eq!(TERMINAL_INVENTORY_PROTOCOL_MINOR, 15);
        assert_eq!(DEATH_VIEW_PROTOCOL_MINOR, 14);
        assert_eq!(SAFE_INVENTORY_PROTOCOL_MINOR, 12);
        for feature in [
            CORE_DEATH_VIEW_FEATURE_FLAG,
            CORE_EXTRACTION_TERMINAL_FEATURE_FLAG,
            CORE_RECALL_TERMINAL_FEATURE_FLAG,
            CORE_RESOLUTION_HOLD_FEATURE_FLAG,
            CORE_SUCCESSOR_FEATURE_FLAG,
            CORE_WORLD_FLOW_FEATURE_FLAG,
            CORE_PENDING_INVENTORY_FEATURE_FLAG,
        ] {
            assert!(WireText::<{ crate::handshake::FEATURE_FLAG_MAX_BYTES }>::new(feature).is_ok());
        }
    }
}
