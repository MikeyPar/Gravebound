//! Production routing seams for the capacity-one Core private-life world flow.
//!
//! The normal route remains unadvertised until the owning actor and terminal authorities are
//! attached. This module makes the composition honest: read-only location queries retain the
//! durable projection authority, mutations retain the transactional coordinator, and the Bell
//! portal can be authorized only by a matching live private-life actor.

use std::{future::Future, sync::Arc};

use protocol::{
    WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest, WorldFlowResult,
    WorldTransferCommand, WorldTransferResultCode,
};

use crate::core_private_hall_runtime::{
    CorePrivateHallActorLease, CorePrivateHallDirectory, CorePrivateHallError,
};
use crate::{AuthenticatedAccount, CorePrivateLifeTransportLease, CoreWorldFlowAuthority};

/// Immutable server-side binding presented to the live actor before the fixed Bell dungeon may
/// replace the microrealm as the character's current dangerous scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBellPortalBinding {
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub mutation_id: [u8; 16],
    pub instance_lineage_id: [u8; 16],
    pub entry_restore_point_id: [u8; 16],
    pub character_version: u64,
    pub content_revision: WorldFlowContentRevisionV1,
}

/// Exclusive live-actor reservation for one exact Bell transfer. The actor generation and route
/// state version prevent approval from surviving a replacement or unrelated live-state advance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBellPortalPermit {
    pub binding: CoreBellPortalBinding,
    pub permit_id: [u8; 16],
    pub actor_generation: u64,
    pub route_state_version: u64,
}

/// Opaque, non-clone actor-generation pin. A live implementation must keep the referenced actor
/// generation and route reservation valid until this value is consumed by commit/abort or dropped
/// during timeout/cancellation. Actor replacement must fail with `TransferInProgress` while any
/// lease is alive.
pub trait CoreBellPortalPermitLease: Send {
    fn permit(&self) -> &CoreBellPortalPermit;
}

impl CoreBellPortalPermit {
    #[must_use]
    pub fn is_well_formed_for(&self, binding: &CoreBellPortalBinding) -> bool {
        self.binding == *binding
            && self.permit_id.iter().any(|byte| *byte != 0)
            && self.actor_generation != 0
            && self.route_state_version != 0
    }
}

/// Durable transition reported to the live authority after `PostgreSQL` has released every lock.
/// Reconciliation uses the same shape after response loss or process replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBellPortalTransition {
    pub binding: CoreBellPortalBinding,
    pub transfer_id: [u8; 16],
    pub destination_character_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreBellPortalAbortReason {
    PersistentStateChanged,
    PersistenceUnavailable,
    DurableRejection,
    ConcurrentResolution,
}

/// Stable versus transient Bell-portal rejections remain explicit so the transactional route can
/// durably receipt only decisions that are safe to replay forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreBellPortalRejection {
    NotCleared,
    OutOfRange,
    TransferInProgress,
    InstanceUnavailable,
    ServiceUnavailable,
}

/// Live private-life actor boundary. Implementations must bind the exact generation/lineage,
/// authoritative Cleared state, and server-owned interaction range; the wire request carries none
/// of those authorities.
pub trait CoreBellPortalAuthority: Send + Sync {
    type PermitLease: CoreBellPortalPermitLease;

    /// Reserve the exact actor generation and route state. This is always called without a
    /// `PostgreSQL` transaction so the actor may safely consult its own durable dependencies.
    /// Implementations that mark a reservation before any suspension point must immediately own a
    /// cancellation guard whose `Drop` releases it, then move that guard into the returned lease.
    /// Dropping this future at the coordinator timeout must never strand `TransferInProgress`.
    fn prepare_bell_portal(
        &self,
        binding: CoreBellPortalBinding,
    ) -> impl Future<Output = Result<Self::PermitLease, CoreBellPortalRejection>> + Send;

    /// Consume a prepared reservation after the durable transition commits. Implementations must
    /// be idempotent by `(actor_generation, permit_id, transfer_id)`.
    fn commit_bell_portal(
        &self,
        permit: Self::PermitLease,
        transition: CoreBellPortalTransition,
    ) -> impl Future<Output = Result<(), CoreBellPortalRejection>> + Send;

    /// Release a reservation after any non-commit path. Abort is idempotent and best effort; actor
    /// restart also clears orphaned in-memory reservations.
    fn abort_bell_portal(
        &self,
        permit: Self::PermitLease,
        reason: CoreBellPortalAbortReason,
    ) -> impl Future<Output = ()> + Send;

    /// Converge the current actor generation on an already committed transition. This is used for
    /// exact receipt replay and for the commit-callback response-loss window.
    fn reconcile_bell_portal(
        &self,
        transition: CoreBellPortalTransition,
    ) -> impl Future<Output = Result<(), CoreBellPortalRejection>> + Send;
}

/// Fail-closed authority used until the live private-life actor directory is constructed.
#[derive(Debug, Clone, Copy, Default)]
pub struct DisabledCoreBellPortalAuthority;

#[doc(hidden)]
pub struct DisabledCoreBellPortalPermitLease {
    permit: CoreBellPortalPermit,
}

impl CoreBellPortalPermitLease for DisabledCoreBellPortalPermitLease {
    fn permit(&self) -> &CoreBellPortalPermit {
        &self.permit
    }
}

impl CoreBellPortalAuthority for DisabledCoreBellPortalAuthority {
    type PermitLease = DisabledCoreBellPortalPermitLease;

    async fn prepare_bell_portal(
        &self,
        _binding: CoreBellPortalBinding,
    ) -> Result<Self::PermitLease, CoreBellPortalRejection> {
        Err(CoreBellPortalRejection::NotCleared)
    }

    async fn commit_bell_portal(
        &self,
        _permit: Self::PermitLease,
        _transition: CoreBellPortalTransition,
    ) -> Result<(), CoreBellPortalRejection> {
        Err(CoreBellPortalRejection::InstanceUnavailable)
    }

    async fn abort_bell_portal(
        &self,
        _permit: Self::PermitLease,
        _reason: CoreBellPortalAbortReason,
    ) {
    }

    async fn reconcile_bell_portal(
        &self,
        _transition: CoreBellPortalTransition,
    ) -> Result<(), CoreBellPortalRejection> {
        Err(CoreBellPortalRejection::InstanceUnavailable)
    }
}

/// Routes location reads and transfer mutations to distinct production authorities. This prevents
/// the read-only regression gate from accidentally deciding mutations and prevents a mutation
/// coordinator from fabricating location projections when no transaction was requested.
#[derive(Debug, Clone)]
pub struct CorePrivateWorldFlowRouter<Location, Transfers> {
    location: Location,
    transfers: Transfers,
}

impl<Location, Transfers> CorePrivateWorldFlowRouter<Location, Transfers> {
    #[must_use]
    pub const fn new(location: Location, transfers: Transfers) -> Self {
        Self {
            location,
            transfers,
        }
    }

    #[must_use]
    pub const fn location_authority(&self) -> &Location {
        &self.location
    }

    #[must_use]
    pub const fn transfer_authority(&self) -> &Transfers {
        &self.transfers
    }
}

impl<Location, Transfers> CoreWorldFlowAuthority for CorePrivateWorldFlowRouter<Location, Transfers>
where
    Location: CoreWorldFlowAuthority,
    Transfers: CoreWorldFlowAuthority,
{
    async fn handle_world_flow(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &WorldFlowFrame,
    ) -> WorldFlowResult {
        match frame.request {
            WorldFlowRequest::Location { .. } => {
                self.location.handle_world_flow(authenticated, frame).await
            }
            WorldFlowRequest::Transfer(_) => {
                self.transfers.handle_world_flow(authenticated, frame).await
            }
        }
    }
}

/// Per-connection world-flow authority that makes an authenticated Hall reservation mandatory
/// before the process-wide router can see a fresh Realm Gate mutation. Other route commands keep
/// their own domain authority and pass through unchanged.
#[derive(Debug, Clone)]
#[allow(
    dead_code,
    reason = "the Hall-gated authority is consumed by the next bound normal-server dispatch slice"
)]
pub(crate) struct CorePrivateHallWorldFlow<Inner> {
    inner: Arc<Inner>,
    hall: Arc<CorePrivateHallDirectory>,
    actor: CorePrivateHallActorLease,
    transport: CorePrivateLifeTransportLease,
}

#[allow(
    dead_code,
    reason = "the Hall-gated authority is consumed by the next bound normal-server dispatch slice"
)]
impl<Inner> CorePrivateHallWorldFlow<Inner> {
    #[must_use]
    pub(crate) fn new(
        inner: Arc<Inner>,
        hall: Arc<CorePrivateHallDirectory>,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
    ) -> Self {
        Self {
            inner,
            hall,
            actor,
            transport,
        }
    }
}

impl<Inner> CoreWorldFlowAuthority for CorePrivateHallWorldFlow<Inner>
where
    Inner: CoreWorldFlowAuthority,
{
    async fn handle_world_flow(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &WorldFlowFrame,
    ) -> WorldFlowResult {
        let WorldFlowRequest::Transfer(mutation) = &frame.request else {
            return self.inner.handle_world_flow(authenticated, frame).await;
        };
        let WorldTransferCommand::UsePortal { portal_id } = &mutation.payload.command else {
            return self.inner.handle_world_flow(authenticated, frame).await;
        };
        if portal_id.as_str() != "station.realm_gate" {
            return self.inner.handle_world_flow(authenticated, frame).await;
        }
        if self.hall.is_committed_realm_gate(
            authenticated,
            mutation.character_id,
            mutation.expected_character_version,
            mutation.mutation_id,
        ) {
            return self.inner.handle_world_flow(authenticated, frame).await;
        }
        let permit = match self.hall.prepare_realm_gate(
            authenticated,
            self.actor,
            self.transport,
            mutation.mutation_id,
            mutation.expected_character_version,
        ) {
            Ok(permit) => permit,
            Err(error) => return hall_rejection(frame.sequence, mutation, error),
        };
        let result = self.inner.handle_world_flow(authenticated, frame).await;
        if matches!(
            &result,
            WorldFlowResult::Transfer {
                accepted: true,
                code: WorldTransferResultCode::Accepted,
                mutation_id,
                ..
            } if *mutation_id == mutation.mutation_id
        ) {
            // A durable accepted result remains authoritative even if local retirement detects a
            // fault. The connection bootstrap must reconcile before publishing player control.
            let _ = self.hall.commit_realm_gate(permit);
        } else {
            let _ = self.hall.abort_realm_gate(permit);
        }
        result
    }
}

#[allow(
    dead_code,
    reason = "the Hall-gated authority is consumed by the next bound normal-server dispatch slice"
)]
fn hall_rejection(
    request_sequence: u32,
    mutation: &protocol::WorldTransferMutation,
    error: CorePrivateHallError,
) -> WorldFlowResult {
    let code = match error {
        CorePrivateHallError::OutOfRange => WorldTransferResultCode::OutOfRange,
        CorePrivateHallError::VersionMismatch => WorldTransferResultCode::StateVersionMismatch,
        CorePrivateHallError::TransferInProgress => WorldTransferResultCode::TransferInProgress,
        CorePrivateHallError::Retired | CorePrivateHallError::ActorUnavailable => {
            WorldTransferResultCode::InstanceUnavailable
        }
        CorePrivateHallError::InvalidMutation | CorePrivateHallError::InvalidInput => {
            WorldTransferResultCode::InvalidSource
        }
        CorePrivateHallError::Content => WorldTransferResultCode::ContentDisabled,
        CorePrivateHallError::GenerationExhausted
        | CorePrivateHallError::InvalidSnapshot
        | CorePrivateHallError::ForeignAuthority
        | CorePrivateHallError::StaleActor
        | CorePrivateHallError::StaleTransport
        | CorePrivateHallError::StaleInput
        | CorePrivateHallError::UnsafeAction => WorldTransferResultCode::InvalidSource,
    };
    WorldFlowResult::Transfer {
        request_sequence,
        mutation_id: mutation.mutation_id,
        accepted: false,
        code,
        snapshot: None,
        transfer_id: None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use protocol::{
        CharacterLocation, CharacterLocationSnapshot, ManifestHash, SafeArrival, WireText,
        WorldFlowContentRevisionV1, WorldTransferCommand, WorldTransferMutation,
        WorldTransferPayload, WorldTransferResultCode,
    };

    use super::*;
    use sim_core::TilePoint;

    use crate::{AccountId, AuthenticatedNamespace, CorePrivateLifeTransportLease};

    #[derive(Debug, Clone, Copy)]
    struct LocationAuthority;

    impl CoreWorldFlowAuthority for LocationAuthority {
        async fn handle_world_flow(
            &self,
            authenticated: AuthenticatedAccount,
            frame: &WorldFlowFrame,
        ) -> WorldFlowResult {
            assert_eq!(authenticated.account_id.as_bytes(), [1; 16]);
            let WorldFlowRequest::Location {
                character_id,
                content_revision,
            } = &frame.request
            else {
                panic!("location authority received a transfer mutation")
            };
            assert_eq!(*character_id, [2; 16]);
            assert_eq!(*content_revision, revision());
            WorldFlowResult::Location {
                request_sequence: frame.sequence,
                snapshot: CharacterLocationSnapshot {
                    character_id: [2; 16],
                    character_version: 4,
                    location: CharacterLocation::Safe {
                        location_id: WireText::new("hub.lantern_halls_01").unwrap(),
                        arrival: SafeArrival::HallDefault,
                    },
                },
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct TransferAuthority;

    impl CoreWorldFlowAuthority for TransferAuthority {
        async fn handle_world_flow(
            &self,
            authenticated: AuthenticatedAccount,
            frame: &WorldFlowFrame,
        ) -> WorldFlowResult {
            assert_eq!(authenticated.account_id.as_bytes(), [1; 16]);
            let WorldFlowRequest::Transfer(mutation) = &frame.request else {
                panic!("transfer authority received a location query")
            };
            assert_eq!(mutation.mutation_id, [3; 16]);
            assert_eq!(mutation.character_id, [2; 16]);
            assert_eq!(mutation.payload.content_revision, revision());
            assert_eq!(mutation.payload_hash, mutation.payload.canonical_hash());
            WorldFlowResult::Transfer {
                request_sequence: frame.sequence,
                mutation_id: mutation.mutation_id,
                accepted: false,
                code: WorldTransferResultCode::DestinationDisabled,
                snapshot: None,
                transfer_id: None,
            }
        }
    }

    #[derive(Debug, Default)]
    struct AcceptingRealmGate {
        calls: AtomicUsize,
    }

    impl CoreWorldFlowAuthority for AcceptingRealmGate {
        async fn handle_world_flow(
            &self,
            _authenticated: AuthenticatedAccount,
            frame: &WorldFlowFrame,
        ) -> WorldFlowResult {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let WorldFlowRequest::Transfer(mutation) = &frame.request else {
                panic!("Gate authority requires transfer")
            };
            WorldFlowResult::Transfer {
                request_sequence: frame.sequence,
                mutation_id: mutation.mutation_id,
                accepted: true,
                code: WorldTransferResultCode::Accepted,
                snapshot: Some(CharacterLocationSnapshot {
                    character_id: mutation.character_id,
                    character_version: mutation.expected_character_version + 1,
                    location: CharacterLocation::Danger {
                        location_id: WireText::new("world.core_microrealm_01").unwrap(),
                        instance_lineage_id: [4; 16],
                        entry_restore_point_id: [5; 16],
                    },
                }),
                transfer_id: Some([6; 16]),
            }
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
        }
    }

    fn content_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("content")
    }

    fn realm_gate_frame() -> WorldFlowFrame {
        let payload = WorldTransferPayload {
            content_revision: revision(),
            command: WorldTransferCommand::UsePortal {
                portal_id: WireText::new("station.realm_gate").unwrap(),
            },
        };
        WorldFlowFrame {
            sequence: 9,
            request: WorldFlowRequest::Transfer(WorldTransferMutation {
                mutation_id: [3; 16],
                character_id: [2; 16],
                expected_character_version: 4,
                issued_at_unix_millis: 9_000,
                payload_hash: payload.canonical_hash(),
                payload,
            }),
        }
    }

    #[tokio::test]
    async fn router_preserves_durable_location_and_transactional_transfer_ownership() {
        let router = CorePrivateWorldFlowRouter::new(LocationAuthority, TransferAuthority);
        let location = router
            .handle_world_flow(
                authenticated(),
                &WorldFlowFrame {
                    sequence: 7,
                    request: WorldFlowRequest::Location {
                        character_id: [2; 16],
                        content_revision: revision(),
                    },
                },
            )
            .await;
        assert!(matches!(
            location,
            WorldFlowResult::Location {
                request_sequence: 7,
                snapshot: CharacterLocationSnapshot {
                    character_version: 4,
                    ..
                },
            }
        ));

        let payload = WorldTransferPayload {
            content_revision: revision(),
            command: WorldTransferCommand::UsePortal {
                portal_id: WireText::new("portal.dungeon.bell_sepulcher").unwrap(),
            },
        };
        let transfer = router
            .handle_world_flow(
                authenticated(),
                &WorldFlowFrame {
                    sequence: 8,
                    request: WorldFlowRequest::Transfer(WorldTransferMutation {
                        mutation_id: [3; 16],
                        character_id: [2; 16],
                        expected_character_version: 4,
                        issued_at_unix_millis: 9_000,
                        payload_hash: payload.canonical_hash(),
                        payload,
                    }),
                },
            )
            .await;
        assert!(matches!(
            transfer,
            WorldFlowResult::Transfer {
                request_sequence: 8,
                mutation_id,
                code: WorldTransferResultCode::DestinationDisabled,
                ..
            } if mutation_id == [3; 16]
        ));
    }

    #[tokio::test]
    async fn disabled_bell_portal_authority_is_explicitly_fail_closed() {
        let binding = CoreBellPortalBinding {
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            instance_lineage_id: [4; 16],
            entry_restore_point_id: [5; 16],
            character_version: 6,
            content_revision: revision(),
        };
        assert!(matches!(
            DisabledCoreBellPortalAuthority
                .prepare_bell_portal(binding)
                .await,
            Err(CoreBellPortalRejection::NotCleared)
        ));
    }

    #[tokio::test]
    async fn hall_gate_blocks_out_of_range_before_durable_authority() {
        let hall = Arc::new(CorePrivateHallDirectory::load(&content_root()).unwrap());
        let transport = CorePrivateLifeTransportLease::test_only([1; 16], 3);
        let actor = hall.install_at(
            authenticated(),
            [2; 16],
            4,
            TilePoint::new(32_000, 42_000),
            transport.generation(),
        );
        let inner = Arc::new(AcceptingRealmGate::default());
        let authority = CorePrivateHallWorldFlow::new(Arc::clone(&inner), hall, actor, transport);
        assert!(matches!(
            authority
                .handle_world_flow(authenticated(), &realm_gate_frame())
                .await,
            WorldFlowResult::Transfer {
                accepted: false,
                code: WorldTransferResultCode::OutOfRange,
                ..
            }
        ));
        assert_eq!(inner.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn hall_gate_commits_once_and_allows_exact_response_loss_replay() {
        let hall = Arc::new(CorePrivateHallDirectory::load(&content_root()).unwrap());
        let transport = CorePrivateLifeTransportLease::test_only([1; 16], 3);
        let actor = hall.install_at(
            authenticated(),
            [2; 16],
            4,
            TilePoint::new(32_000, 4_500),
            transport.generation(),
        );
        let inner = Arc::new(AcceptingRealmGate::default());
        let authority =
            CorePrivateHallWorldFlow::new(Arc::clone(&inner), Arc::clone(&hall), actor, transport);
        let frame = realm_gate_frame();
        for expected_calls in 1..=2 {
            assert!(matches!(
                authority.handle_world_flow(authenticated(), &frame).await,
                WorldFlowResult::Transfer {
                    accepted: true,
                    code: WorldTransferResultCode::Accepted,
                    ..
                }
            ));
            assert_eq!(inner.calls.load(Ordering::Relaxed), expected_calls);
        }
    }
}
