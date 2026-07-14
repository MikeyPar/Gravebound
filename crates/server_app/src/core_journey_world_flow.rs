//! Explicitly disposable world-flow composition for `GB-M03-03F` integration journeys.
//!
//! The normal Core endpoint never constructs this adapter. It exists to route reliable test-only
//! transfers through the completed dormant entry and committed-extraction authorities while the
//! player-visible route remains fail closed.

use std::future::Future;

use protocol::{
    WorldFlowFrame, WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferMutation,
};

use crate::{
    AuthenticatedAccount, CoreWorldFlowAuthority, IdentityClock,
    PostgresCaldusHallTransferCoordinator,
};

pub trait CommittedExtractionTransferAuthority: Send + Sync {
    fn transfer_committed_extraction(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
    ) -> impl Future<Output = WorldFlowResult> + Send;
}

impl<Clock> CommittedExtractionTransferAuthority for PostgresCaldusHallTransferCoordinator<Clock>
where
    Clock: IdentityClock,
{
    async fn transfer_committed_extraction(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
    ) -> WorldFlowResult {
        self.transfer(authenticated, request_sequence, mutation)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct DisposableCoreJourneyWorldFlow<Route, Extraction> {
    route: Route,
    extraction: Extraction,
}

impl<Route, Extraction> DisposableCoreJourneyWorldFlow<Route, Extraction> {
    #[must_use]
    pub const fn new(route: Route, extraction: Extraction) -> Self {
        Self { route, extraction }
    }
}

impl<Route, Extraction> CoreWorldFlowAuthority for DisposableCoreJourneyWorldFlow<Route, Extraction>
where
    Route: CoreWorldFlowAuthority,
    Extraction: CommittedExtractionTransferAuthority,
{
    async fn handle_world_flow(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &WorldFlowFrame,
    ) -> WorldFlowResult {
        if let WorldFlowRequest::Transfer(mutation) = &frame.request
            && matches!(
                mutation.payload.command,
                WorldTransferCommand::UseCommittedExtraction { .. }
            )
        {
            return self
                .extraction
                .transfer_committed_extraction(authenticated, frame.sequence, mutation)
                .await;
        }
        self.route.handle_world_flow(authenticated, frame).await
    }
}

#[cfg(test)]
mod tests {
    use protocol::{
        CharacterLocation, CharacterLocationSnapshot, ManifestHash, SafeArrival, WireText,
        WorldFlowContentRevisionV1, WorldTransferPayload, WorldTransferResultCode,
    };

    use super::*;
    use crate::{AccountId, AuthenticatedNamespace};

    #[derive(Debug, Clone, Copy)]
    struct Route;

    impl CoreWorldFlowAuthority for Route {
        async fn handle_world_flow(
            &self,
            _authenticated: AuthenticatedAccount,
            frame: &WorldFlowFrame,
        ) -> WorldFlowResult {
            WorldFlowResult::Location {
                request_sequence: frame.sequence,
                snapshot: snapshot(),
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct Extraction;

    impl CommittedExtractionTransferAuthority for Extraction {
        async fn transfer_committed_extraction(
            &self,
            _authenticated: AuthenticatedAccount,
            request_sequence: u32,
            mutation: &WorldTransferMutation,
        ) -> WorldFlowResult {
            WorldFlowResult::Transfer {
                request_sequence,
                mutation_id: mutation.mutation_id,
                accepted: true,
                code: WorldTransferResultCode::Accepted,
                snapshot: Some(snapshot()),
                transfer_id: Some([9; 16]),
            }
        }
    }

    #[tokio::test]
    async fn only_committed_extraction_selects_the_extraction_authority() {
        let authority = DisposableCoreJourneyWorldFlow::new(Route, Extraction);
        let account = AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        let ordinary = WorldFlowFrame {
            sequence: 1,
            request: WorldFlowRequest::Location {
                character_id: [2; 16],
                content_revision: revision(),
            },
        };
        assert!(matches!(
            authority.handle_world_flow(account, &ordinary).await,
            WorldFlowResult::Location { .. }
        ));

        let payload = WorldTransferPayload {
            content_revision: revision(),
            command: WorldTransferCommand::UseCommittedExtraction {
                portal_id: WireText::new("portal.exit.dungeon.bell_sepulcher").unwrap(),
                extraction_request_id: [3; 16],
                extraction_receipt_id: [4; 16],
            },
        };
        let extraction = WorldFlowFrame {
            sequence: 2,
            request: WorldFlowRequest::Transfer(WorldTransferMutation {
                mutation_id: [5; 16],
                character_id: [2; 16],
                expected_character_version: 1,
                issued_at_unix_millis: 1,
                payload_hash: payload.canonical_hash(),
                payload,
            }),
        };
        assert!(matches!(
            authority.handle_world_flow(account, &extraction).await,
            WorldFlowResult::Transfer {
                accepted: true,
                transfer_id: Some(transfer_id),
                ..
            } if transfer_id == [9; 16]
        ));
    }

    fn revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
        }
    }

    fn snapshot() -> CharacterLocationSnapshot {
        CharacterLocationSnapshot {
            character_id: [2; 16],
            character_version: 1,
            location: CharacterLocation::Safe {
                location_id: WireText::new("hub.lantern_halls_01").unwrap(),
                arrival: SafeArrival::HallDefault,
            },
        }
    }
}
