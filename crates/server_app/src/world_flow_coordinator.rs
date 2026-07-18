//! Transactional world-flow coordinator for the capacity-one Core private route.
//!
//! Approved `SPEC-CONFLICT-006`/`010` and `ADR-037` keep normal admission fail closed until the
//! complete actor/terminal/client composition passes. This authority nevertheless owns the real
//! production mutation rules: durable safe arrival, one safe-to-danger restore capture, and the
//! same-lineage microrealm-to-fixed-dungeon transition authorized only by a live actor.

use std::{sync::Arc, time::Duration};

use persistence::{
    PersistenceError, PostgresPersistence, StoredDangerEntryRootV3, StoredSafeArrival,
    StoredWorldFlowRevisionV1, StoredWorldLocation, StoredWorldTransferReceipt, WorldFlowBegin,
    WorldFlowTransaction, WorldFlowTransactionState, load_world_flow_safe_inventory,
    stage_world_flow_danger_entry, stage_world_flow_safe_inventory_preflight,
};
use protocol::{
    CharacterLocationSnapshot, WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest,
    WorldFlowResult, WorldTransferCommand, WorldTransferMutation, WorldTransferResultCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    AshWalletRestoreV3, AuthenticatedAccount, AuthenticatedNamespace, CoreBellPortalAbortReason,
    CoreBellPortalAuthority, CoreBellPortalBinding, CoreBellPortalPermitLease,
    CoreBellPortalRejection, CoreBellPortalTransition, CorePrivateLifeRuntimeBootstrapAdapter,
    DisabledCoreBellPortalAuthority, EntryCaptureContext, EntryRestoreProvider, IdentityClock,
    InventorySecurityRestoreV3, LifeMetricsRestoreV3, OathBargainRestoreV3,
    PostgresProgressionRestoreProvider, RestorePointError, RestorePointProvidersV3,
    SafeInventoryServiceError, WorldFlowRepositoryError,
    safe_inventory::plan_danger_entry_safe_deposit,
    world_flow_gate::{CoreWorldFlowAuthority, stored_location_snapshot},
};

const HALL_ID: &str = "hub.lantern_halls_01";
const CHARACTER_SELECT_RETURN_SPAWN_ID: &str = "spawn.hub.character_select_return";
const REALM_GATE_ID: &str = "station.realm_gate";
const CORE_MICROREALM_ID: &str = "world.core_microrealm_01";
const BELL_DUNGEON_PORTAL_ID: &str = "portal.dungeon.bell_sepulcher";
const CORE_BELL_DUNGEON_ID: &str = "dungeon.bell_sepulcher";
const CORE_PRIVATE_LIFE_LAYOUT_ID: &str = "layout.core_private_life_01";
const CORE_BELL_PORTAL_ACTOR_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldFlowIdentityMaterial {
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub mutation_id: [u8; 16],
}

pub trait WorldFlowIdGenerator: Send + Sync {
    fn transfer_id(&self, material: WorldFlowIdentityMaterial) -> [u8; 16];
    fn lineage_id(&self, material: WorldFlowIdentityMaterial) -> [u8; 16];
    fn restore_point_id(&self, material: WorldFlowIdentityMaterial) -> [u8; 16];
}

/// Restart-stable, server-planned identifiers derived from authenticated authority plus the
/// canonical mutation identity. Domain separation makes all three IDs disjoint without trusting a
/// client-selected destination or retaining process-local counters.
#[derive(Debug, Clone, Copy, Default)]
pub struct Blake3WorldFlowIds;

impl WorldFlowIdGenerator for Blake3WorldFlowIds {
    fn transfer_id(&self, material: WorldFlowIdentityMaterial) -> [u8; 16] {
        derive_world_flow_id(b"transfer", material)
    }

    fn lineage_id(&self, material: WorldFlowIdentityMaterial) -> [u8; 16] {
        derive_world_flow_id(b"lineage", material)
    }

    fn restore_point_id(&self, material: WorldFlowIdentityMaterial) -> [u8; 16] {
        derive_world_flow_id(b"restore", material)
    }
}

fn derive_world_flow_id(domain: &[u8], material: WorldFlowIdentityMaterial) -> [u8; 16] {
    let mut input = Vec::with_capacity(39 + domain.len());
    input.extend_from_slice(b"gravebound.core-world-flow.v1\0");
    input.extend_from_slice(domain);
    input.push(0);
    input.extend_from_slice(&material.account_id);
    input.extend_from_slice(&material.character_id);
    input.extend_from_slice(&material.mutation_id);
    let hash = blake3::hash(&input);
    let mut id = [0; 16];
    id.copy_from_slice(&hash.as_bytes()[..16]);
    id
}

#[derive(Debug, Clone)]
pub struct CorePrivateWorldFlowPlanner<Generator, Clock> {
    generator: Generator,
    clock: Clock,
    required_content_revision: WorldFlowContentRevisionV1,
}

impl<Generator, Clock> CorePrivateWorldFlowPlanner<Generator, Clock>
where
    Generator: WorldFlowIdGenerator,
    Clock: IdentityClock,
{
    pub const fn new(
        generator: Generator,
        clock: Clock,
        required_content_revision: WorldFlowContentRevisionV1,
    ) -> Self {
        Self {
            generator,
            clock,
            required_content_revision,
        }
    }

    #[cfg(test)]
    fn plan_fresh(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        state: &mut WorldFlowTransactionState,
    ) -> Result<WorldFlowResult, PersistenceError> {
        self.plan_fresh_with_bell_portal(authenticated, request_sequence, mutation, state, false)
    }

    #[cfg(test)]
    fn plan_fresh_with_bell_portal(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        state: &mut WorldFlowTransactionState,
        bell_portal_authorized: bool,
    ) -> Result<WorldFlowResult, PersistenceError> {
        let planned = self
            .validate_fresh(request_sequence, mutation, state)?
            .map_or_else(
                || {
                    self.plan_route(
                        authenticated,
                        request_sequence,
                        mutation,
                        state,
                        bell_portal_authorized,
                    )
                },
                Ok,
            )?;
        stage_receipt(authenticated, mutation, state, &planned)?;
        Ok(planned)
    }

    fn validate_fresh(
        &self,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        state: &WorldFlowTransactionState,
    ) -> Result<Option<WorldFlowResult>, PersistenceError> {
        let reject = |code| staged_result(request_sequence, mutation, code, None, None);
        let planned = if mutation.payload.content_revision != self.required_content_revision {
            Some(reject(WorldTransferResultCode::ContentMismatch))
        } else if mutation.issued_at_unix_millis > self.clock.unix_millis() {
            Some(reject(WorldTransferResultCode::IssuedAtInvalid))
        } else if state.selected_character_id.is_none() {
            Some(reject(WorldTransferResultCode::NoSelectedCharacter))
        } else if state.selected_character_id != Some(mutation.character_id) {
            Some(reject(WorldTransferResultCode::InvalidSource))
        } else if state.character.life_state != 0 {
            Some(reject(WorldTransferResultCode::CharacterDead))
        } else if state.character.security_state != 0 {
            Some(reject(WorldTransferResultCode::StorageResolutionRequired))
        } else if state.location.character_version()
            != i64::try_from(mutation.expected_character_version)
                .map_err(|_| PersistenceError::CorruptStoredWorldFlow)?
        {
            let snapshot = protocol_snapshot(mutation.character_id, &state.location)?;
            Some(staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::StateVersionMismatch,
                Some(snapshot),
                None,
            ))
        } else {
            None
        };
        Ok(planned)
    }

    fn plan_validated(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        state: &mut WorldFlowTransactionState,
        bell_portal_authorized: bool,
    ) -> Result<WorldFlowResult, PersistenceError> {
        let planned = self.plan_route(
            authenticated,
            request_sequence,
            mutation,
            state,
            bell_portal_authorized,
        )?;
        stage_receipt(authenticated, mutation, state, &planned)?;
        Ok(planned)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the dormant route matrix is intentionally kept together for fail-closed auditability"
    )]
    fn plan_route(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        state: &mut WorldFlowTransactionState,
        bell_portal_authorized: bool,
    ) -> Result<WorldFlowResult, PersistenceError> {
        let next_version = state
            .location
            .character_version()
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredWorldFlow)?;
        let next_location = match (&mutation.payload.command, &state.location) {
            (
                WorldTransferCommand::EnterHallFromCharacterSelect,
                StoredWorldLocation::CharacterSelect {
                    next_hall_arrival, ..
                },
            ) => StoredWorldLocation::Safe {
                character_version: next_version,
                location_content_id: HALL_ID.to_owned(),
                arrival: next_hall_arrival.clone(),
            },
            (
                WorldTransferCommand::ReturnToCharacterSelect,
                StoredWorldLocation::Safe {
                    location_content_id,
                    ..
                },
            ) if location_content_id == HALL_ID => StoredWorldLocation::CharacterSelect {
                character_version: next_version,
                next_hall_arrival: StoredSafeArrival::SpawnAnchor(
                    CHARACTER_SELECT_RETURN_SPAWN_ID.to_owned(),
                ),
            },
            (
                WorldTransferCommand::UsePortal { portal_id },
                StoredWorldLocation::Safe {
                    location_content_id,
                    ..
                },
            ) if portal_id.as_str() == REALM_GATE_ID && location_content_id == HALL_ID => {
                let material = WorldFlowIdentityMaterial {
                    account_id: authenticated.account_id.as_bytes(),
                    character_id: mutation.character_id,
                    mutation_id: mutation.mutation_id,
                };
                let transfer_id = self.generator.transfer_id(material);
                let lineage_id = self.generator.lineage_id(material);
                let restore_point_id = self.generator.restore_point_id(material);
                if [transfer_id, lineage_id, restore_point_id]
                    .into_iter()
                    .any(|identity| identity.iter().all(|byte| *byte == 0))
                    || transfer_id == lineage_id
                    || transfer_id == restore_point_id
                    || lineage_id == restore_point_id
                {
                    return Err(PersistenceError::CorruptStoredWorldFlow);
                }
                let next_location = StoredWorldLocation::Danger {
                    character_version: next_version,
                    location_content_id: CORE_MICROREALM_ID.to_owned(),
                    instance_lineage_id: lineage_id,
                    entry_restore_point_id: restore_point_id,
                };
                let snapshot = protocol_snapshot(mutation.character_id, &next_location)?;
                state.location = next_location;
                state.location_changed = true;
                return Ok(staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::Accepted,
                    Some(snapshot),
                    Some(transfer_id),
                ));
            }
            (
                WorldTransferCommand::UsePortal { portal_id },
                StoredWorldLocation::Danger {
                    location_content_id,
                    instance_lineage_id,
                    entry_restore_point_id,
                    ..
                },
            ) if portal_id.as_str() == BELL_DUNGEON_PORTAL_ID
                && location_content_id == CORE_MICROREALM_ID
                && bell_portal_authorized =>
            {
                StoredWorldLocation::Danger {
                    character_version: next_version,
                    location_content_id: CORE_BELL_DUNGEON_ID.to_owned(),
                    instance_lineage_id: *instance_lineage_id,
                    entry_restore_point_id: *entry_restore_point_id,
                }
            }
            (
                WorldTransferCommand::UsePortal { portal_id },
                StoredWorldLocation::Danger {
                    location_content_id,
                    ..
                },
            ) if portal_id.as_str() == BELL_DUNGEON_PORTAL_ID
                && location_content_id == CORE_MICROREALM_ID =>
            {
                return Ok(staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::DestinationDisabled,
                    protocol_snapshot(mutation.character_id, &state.location).ok(),
                    None,
                ));
            }
            (WorldTransferCommand::UsePortal { portal_id }, _)
                if matches!(portal_id.as_str(), REALM_GATE_ID | BELL_DUNGEON_PORTAL_ID) =>
            {
                return Ok(staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::InvalidSource,
                    protocol_snapshot(mutation.character_id, &state.location).ok(),
                    None,
                ));
            }
            (
                WorldTransferCommand::UsePortal { .. }
                | WorldTransferCommand::UseCommittedExtraction { .. },
                _,
            ) => {
                return Ok(staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::DestinationDisabled,
                    Some(protocol_snapshot(mutation.character_id, &state.location)?),
                    None,
                ));
            }
            _ => {
                return Ok(staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::InvalidSource,
                    Some(protocol_snapshot(mutation.character_id, &state.location)?),
                    None,
                ));
            }
        };
        let transfer_id = self.generator.transfer_id(WorldFlowIdentityMaterial {
            account_id: authenticated.account_id.as_bytes(),
            character_id: mutation.character_id,
            mutation_id: mutation.mutation_id,
        });
        if transfer_id.iter().all(|byte| *byte == 0) {
            return Err(PersistenceError::CorruptStoredWorldFlow);
        }
        let snapshot = protocol_snapshot(mutation.character_id, &next_location)?;
        state.location = next_location;
        state.location_changed = true;
        Ok(staged_result(
            request_sequence,
            mutation,
            WorldTransferResultCode::Accepted,
            Some(snapshot),
            Some(transfer_id),
        ))
    }

    fn replay(
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        receipt: &StoredWorldTransferReceipt,
    ) -> WorldFlowResult {
        if receipt.character_id != mutation.character_id
            || receipt.payload_hash != mutation.payload_hash
            || receipt.content_revision != stored_revision(&mutation.payload.content_revision)
            || receipt.expected_character_version
                != i64::try_from(mutation.expected_character_version).unwrap_or(i64::MIN)
            || receipt.issued_at_unix_millis
                != i64::try_from(mutation.issued_at_unix_millis).unwrap_or(i64::MIN)
            || receipt.command_kind != command_kind(&mutation.payload.command)
        {
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::IdempotencyConflict,
                None,
                None,
            );
        }
        postcard::from_bytes::<StoredWorldFlowOutcome>(&receipt.result_payload).map_or_else(
            |_| {
                staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                )
            },
            |outcome| outcome.into_result(request_sequence, mutation.mutation_id),
        )
    }
}

/// Compatibility name retained for completed `03B`/`03F` evidence. New production composition
/// should use [`CorePrivateWorldFlowPlanner`].
pub type DormantWorldFlowPlanner<Generator, Clock> = CorePrivateWorldFlowPlanner<Generator, Clock>;

#[derive(Debug, Clone)]
pub struct PostgresCorePrivateWorldFlowCoordinator<
    Generator,
    Clock,
    Inventory,
    OathBargains,
    LifeMetrics,
    AshWallet,
    BellPortal = DisabledCoreBellPortalAuthority,
> {
    persistence: PostgresPersistence,
    planner: CorePrivateWorldFlowPlanner<Generator, Clock>,
    bell_portal: BellPortal,
    route_transitions: Option<Arc<CorePrivateLifeRuntimeBootstrapAdapter<PostgresPersistence>>>,
    restore_providers: RestorePointProvidersV3<
        PostgresProgressionRestoreProvider,
        Inventory,
        OathBargains,
        LifeMetrics,
        AshWallet,
    >,
}

/// Compatibility name retained for completed disposable evidence. It keeps Bell admission closed;
/// the normal server must construct [`PostgresCorePrivateWorldFlowCoordinator`] with a live portal
/// authority explicitly.
pub type PostgresDormantWorldFlowCoordinator<
    Generator,
    Clock,
    Inventory,
    OathBargains,
    LifeMetrics,
    AshWallet,
> = PostgresCorePrivateWorldFlowCoordinator<
    Generator,
    Clock,
    Inventory,
    OathBargains,
    LifeMetrics,
    AshWallet,
    DisabledCoreBellPortalAuthority,
>;

impl<Generator, Clock, Inventory, OathBargains, LifeMetrics, AshWallet>
    PostgresCorePrivateWorldFlowCoordinator<
        Generator,
        Clock,
        Inventory,
        OathBargains,
        LifeMetrics,
        AshWallet,
        DisabledCoreBellPortalAuthority,
    >
where
    Generator: WorldFlowIdGenerator,
    Clock: IdentityClock,
    Inventory: EntryRestoreProvider<Snapshot = InventorySecurityRestoreV3>,
    OathBargains: EntryRestoreProvider<Snapshot = OathBargainRestoreV3>,
    LifeMetrics: EntryRestoreProvider<Snapshot = LifeMetricsRestoreV3>,
    AshWallet: EntryRestoreProvider<Snapshot = AshWalletRestoreV3>,
{
    #[allow(
        clippy::too_many_arguments,
        reason = "the coordinator keeps each mandatory V3 provider explicit at its composition root"
    )]
    pub fn new(
        persistence: PostgresPersistence,
        generator: Generator,
        clock: Clock,
        required_content_revision: WorldFlowContentRevisionV1,
        progression: PostgresProgressionRestoreProvider,
        inventory: Inventory,
        oath_bargains: OathBargains,
        life_metrics: LifeMetrics,
        ash_wallet: AshWallet,
    ) -> Self {
        Self::with_bell_portal_authority(
            persistence,
            generator,
            clock,
            required_content_revision,
            progression,
            inventory,
            oath_bargains,
            life_metrics,
            ash_wallet,
            DisabledCoreBellPortalAuthority,
        )
    }
}

impl<Generator, Clock, Inventory, OathBargains, LifeMetrics, AshWallet, BellPortal>
    PostgresCorePrivateWorldFlowCoordinator<
        Generator,
        Clock,
        Inventory,
        OathBargains,
        LifeMetrics,
        AshWallet,
        BellPortal,
    >
where
    Generator: WorldFlowIdGenerator,
    Clock: IdentityClock,
    Inventory: EntryRestoreProvider<Snapshot = InventorySecurityRestoreV3>,
    OathBargains: EntryRestoreProvider<Snapshot = OathBargainRestoreV3>,
    LifeMetrics: EntryRestoreProvider<Snapshot = LifeMetricsRestoreV3>,
    AshWallet: EntryRestoreProvider<Snapshot = AshWalletRestoreV3>,
    BellPortal: CoreBellPortalAuthority,
{
    #[allow(
        clippy::too_many_arguments,
        reason = "the production composition keeps every restore provider and live portal authority explicit"
    )]
    pub fn with_bell_portal_authority(
        persistence: PostgresPersistence,
        generator: Generator,
        clock: Clock,
        required_content_revision: WorldFlowContentRevisionV1,
        progression: PostgresProgressionRestoreProvider,
        inventory: Inventory,
        oath_bargains: OathBargains,
        life_metrics: LifeMetrics,
        ash_wallet: AshWallet,
        bell_portal: BellPortal,
    ) -> Self {
        let restore_providers = RestorePointProvidersV3::new(
            progression,
            inventory,
            oath_bargains,
            life_metrics,
            ash_wallet,
        );
        Self {
            persistence,
            planner: CorePrivateWorldFlowPlanner::new(generator, clock, required_content_revision),
            bell_portal,
            route_transitions: None,
            restore_providers,
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the production composition keeps every restore provider and runtime authority explicit"
    )]
    pub(crate) fn with_runtime_authorities(
        persistence: PostgresPersistence,
        generator: Generator,
        clock: Clock,
        required_content_revision: WorldFlowContentRevisionV1,
        progression: PostgresProgressionRestoreProvider,
        inventory: Inventory,
        oath_bargains: OathBargains,
        life_metrics: LifeMetrics,
        ash_wallet: AshWallet,
        bell_portal: BellPortal,
        route_transitions: Arc<CorePrivateLifeRuntimeBootstrapAdapter<PostgresPersistence>>,
    ) -> Self {
        let mut coordinator = Self::with_bell_portal_authority(
            persistence,
            generator,
            clock,
            required_content_revision,
            progression,
            inventory,
            oath_bargains,
            life_metrics,
            ash_wallet,
            bell_portal,
        );
        coordinator.route_transitions = Some(route_transitions);
        coordinator
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the serializable validation, preflight, route, restore, receipt, and commit order stays contiguous for auditability"
    )]
    pub async fn handle(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &WorldFlowFrame,
    ) -> WorldFlowResult {
        let WorldFlowRequest::Transfer(mutation) = &frame.request else {
            return error(frame.sequence, WorldTransferResultCode::ServiceUnavailable);
        };
        if frame.validate().is_err()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        {
            return error(frame.sequence, WorldTransferResultCode::ServiceUnavailable);
        }
        if mutation.payload_hash != mutation.payload.canonical_hash() {
            return staged_result(
                frame.sequence,
                mutation,
                WorldTransferResultCode::PayloadHashMismatch,
                None,
                None,
            );
        }
        if is_bell_portal_request(mutation) {
            return self
                .handle_bell_portal(authenticated, frame.sequence, mutation)
                .await;
        }
        let begin = self
            .persistence
            .begin_world_flow(
                authenticated.account_id.as_bytes(),
                mutation.character_id,
                mutation.mutation_id,
            )
            .await;
        let mut write = match begin {
            Ok(WorldFlowBegin::Replayed(receipt)) => {
                let result = DormantWorldFlowPlanner::<Generator, Clock>::replay(
                    frame.sequence,
                    mutation,
                    &receipt,
                );
                return self
                    .reconcile_non_bell_transition(authenticated, frame.sequence, mutation, result)
                    .await;
            }
            Ok(WorldFlowBegin::Fresh(write)) => *write,
            Err(PersistenceError::WorldFlowCharacterNotFound) => {
                let code = match self
                    .persistence
                    .identity_character_owner(mutation.character_id)
                    .await
                {
                    Ok(Some(owner)) if owner != authenticated.account_id.as_bytes() => {
                        WorldTransferResultCode::CharacterNotOwned
                    }
                    Ok(_) => WorldTransferResultCode::CharacterNotFound,
                    Err(_) => WorldTransferResultCode::ServiceUnavailable,
                };
                return staged_result(frame.sequence, mutation, code, None, None);
            }
            Err(PersistenceError::WorldFlowCharacterDead) => {
                return staged_result(
                    frame.sequence,
                    mutation,
                    WorldTransferResultCode::CharacterDead,
                    None,
                    None,
                );
            }
            Err(_) => {
                return staged_result(
                    frame.sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                );
            }
        };
        let Ok(validation) = self
            .planner
            .validate_fresh(frame.sequence, mutation, write.state())
        else {
            return staged_result(
                frame.sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        };
        let captures_danger_entry = requires_safe_preflight(mutation, write.state());
        let mut safe_placement_count = 0_u16;
        let result = if let Some(rejection) = validation {
            if stage_receipt(authenticated, mutation, write.state_mut(), &rejection).is_err() {
                return staged_result(
                    frame.sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                );
            }
            rejection
        } else {
            if captures_danger_entry {
                let preflight = self
                    .preflight_safe_inventory(&mut write, authenticated, mutation)
                    .await;
                match preflight {
                    Ok(count) => safe_placement_count = count,
                    Err(code) => {
                        if code != WorldTransferResultCode::StorageResolutionRequired {
                            return staged_result(frame.sequence, mutation, code, None, None);
                        }
                        let rejection = staged_result(frame.sequence, mutation, code, None, None);
                        if stage_receipt(authenticated, mutation, write.state_mut(), &rejection)
                            .is_err()
                        {
                            return staged_result(
                                frame.sequence,
                                mutation,
                                WorldTransferResultCode::ServiceUnavailable,
                                None,
                                None,
                            );
                        }
                        return commit_world_flow_rejection(
                            write,
                            rejection,
                            frame.sequence,
                            mutation,
                        )
                        .await;
                    }
                }
            }
            match self.planner.plan_validated(
                authenticated,
                frame.sequence,
                mutation,
                write.state_mut(),
                false,
            ) {
                Ok(result) => result,
                Err(_) => {
                    return staged_result(
                        frame.sequence,
                        mutation,
                        WorldTransferResultCode::ServiceUnavailable,
                        None,
                        None,
                    );
                }
            }
        };
        if let Err(code) = self
            .capture_danger_entry(
                &mut write,
                mutation,
                safe_placement_count,
                captures_danger_entry,
            )
            .await
        {
            return staged_result(frame.sequence, mutation, code, None, None);
        }
        match write.commit(result).await {
            Ok(WorldFlowTransaction::Committed(result)) => {
                self.reconcile_non_bell_transition(authenticated, frame.sequence, mutation, result)
                    .await
            }
            Ok(WorldFlowTransaction::Replayed(_)) => unreachable!("fresh write cannot replay"),
            Err(_) => staged_result(
                frame.sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            ),
        }
    }

    async fn reconcile_non_bell_transition(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        result: WorldFlowResult,
    ) -> WorldFlowResult {
        let Some(route_transitions) = &self.route_transitions else {
            return result;
        };
        if route_transitions
            .reconcile_committed_world_transition(authenticated, mutation, &result)
            .await
            .is_err()
        {
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        }
        result
    }

    /// Bell admission is a two-phase cross-authority operation. The first serializable read
    /// validates durable identity and is explicitly rolled back before the actor is awaited. The
    /// actor then reserves one generation/version-bound permit. A second serializable transaction
    /// revalidates the exact binding and commits the location; only after every database lock is
    /// released does the actor receive commit/abort/reconcile notification.
    #[allow(
        clippy::too_many_lines,
        reason = "the preview, explicit lock release, actor prepare, and second durable phase remain contiguous for cross-authority auditability"
    )]
    async fn handle_bell_portal(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
    ) -> WorldFlowResult {
        let begin = self
            .persistence
            .begin_world_flow(
                authenticated.account_id.as_bytes(),
                mutation.character_id,
                mutation.mutation_id,
            )
            .await;
        let mut preview = match begin {
            Ok(WorldFlowBegin::Replayed(receipt)) => {
                return self
                    .replay_and_reconcile_bell(authenticated, request_sequence, mutation, &receipt)
                    .await;
            }
            Ok(WorldFlowBegin::Fresh(write)) => *write,
            Err(error) => {
                return world_flow_begin_failure(
                    &self.persistence,
                    authenticated,
                    request_sequence,
                    mutation,
                    error,
                )
                .await;
            }
        };
        let Ok(validation) =
            self.planner
                .validate_fresh(request_sequence, mutation, preview.state())
        else {
            let _ = preview.rollback().await;
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        };
        if let Some(rejection) = validation {
            if stage_receipt(authenticated, mutation, preview.state_mut(), &rejection).is_err() {
                let _ = preview.rollback().await;
                return staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                );
            }
            return commit_world_flow_rejection(preview, rejection, request_sequence, mutation)
                .await;
        }
        let Some(binding) = bell_portal_binding(authenticated, mutation, preview.state()) else {
            let Ok(result) = self.planner.plan_validated(
                authenticated,
                request_sequence,
                mutation,
                preview.state_mut(),
                false,
            ) else {
                let _ = preview.rollback().await;
                return staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                );
            };
            return commit_world_flow_rejection(preview, result, request_sequence, mutation).await;
        };
        let snapshot = protocol_snapshot(mutation.character_id, &preview.state().location).ok();
        if preview.rollback().await.is_err() {
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        }

        match self.prepare_bell_portal(binding.clone()).await {
            Ok(permit) if permit.permit().is_well_formed_for(&binding) => {
                self.commit_prepared_bell(authenticated, request_sequence, mutation, permit)
                    .await
            }
            Ok(permit) => {
                self.abort_bell_portal(permit, CoreBellPortalAbortReason::PersistentStateChanged)
                    .await;
                staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    snapshot,
                    None,
                )
            }
            Err(rejection) if bell_portal_rejection_is_durable(rejection) => {
                self.commit_bell_denial(
                    authenticated,
                    request_sequence,
                    mutation,
                    binding,
                    rejection,
                )
                .await
            }
            Err(rejection) => staged_result(
                request_sequence,
                mutation,
                bell_portal_rejection_code(rejection),
                snapshot,
                None,
            ),
        }
    }

    async fn commit_bell_denial(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        binding: CoreBellPortalBinding,
        actor_rejection: CoreBellPortalRejection,
    ) -> WorldFlowResult {
        let begin = self
            .persistence
            .begin_world_flow(
                authenticated.account_id.as_bytes(),
                mutation.character_id,
                mutation.mutation_id,
            )
            .await;
        let mut write = match begin {
            Ok(WorldFlowBegin::Replayed(receipt)) => {
                return self
                    .replay_and_reconcile_bell(authenticated, request_sequence, mutation, &receipt)
                    .await;
            }
            Ok(WorldFlowBegin::Fresh(write)) => *write,
            Err(error) => {
                return world_flow_begin_failure(
                    &self.persistence,
                    authenticated,
                    request_sequence,
                    mutation,
                    error,
                )
                .await;
            }
        };
        let Ok(validation) = self
            .planner
            .validate_fresh(request_sequence, mutation, write.state())
        else {
            let _ = write.rollback().await;
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        };
        let rejection = if let Some(rejection) = validation {
            rejection
        } else if bell_portal_binding(authenticated, mutation, write.state()).as_ref()
            == Some(&binding)
        {
            staged_result(
                request_sequence,
                mutation,
                bell_portal_rejection_code(actor_rejection),
                protocol_snapshot(mutation.character_id, &write.state().location).ok(),
                None,
            )
        } else {
            let _ = write.rollback().await;
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::StateVersionMismatch,
                None,
                None,
            );
        };
        if stage_receipt(authenticated, mutation, write.state_mut(), &rejection).is_err() {
            let _ = write.rollback().await;
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        }
        commit_world_flow_rejection(write, rejection, request_sequence, mutation).await
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the revalidation, durable commit, and post-lock permit outcome remain contiguous for cross-authority auditability"
    )]
    async fn commit_prepared_bell(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        permit: BellPortal::PermitLease,
    ) -> WorldFlowResult {
        let begin = self
            .persistence
            .begin_world_flow(
                authenticated.account_id.as_bytes(),
                mutation.character_id,
                mutation.mutation_id,
            )
            .await;
        let mut write = match begin {
            Ok(WorldFlowBegin::Replayed(receipt)) => {
                let result = DormantWorldFlowPlanner::<Generator, Clock>::replay(
                    request_sequence,
                    mutation,
                    &receipt,
                );
                return self.finish_prepared_bell(permit, result).await;
            }
            Ok(WorldFlowBegin::Fresh(write)) => *write,
            Err(error) => {
                self.abort_bell_portal(permit, CoreBellPortalAbortReason::PersistenceUnavailable)
                    .await;
                return world_flow_begin_failure(
                    &self.persistence,
                    authenticated,
                    request_sequence,
                    mutation,
                    error,
                )
                .await;
            }
        };
        let Ok(validation) = self
            .planner
            .validate_fresh(request_sequence, mutation, write.state())
        else {
            let _ = write.rollback().await;
            self.abort_bell_portal(permit, CoreBellPortalAbortReason::PersistentStateChanged)
                .await;
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        };
        if let Some(rejection) = validation {
            if stage_receipt(authenticated, mutation, write.state_mut(), &rejection).is_err() {
                let _ = write.rollback().await;
                self.abort_bell_portal(permit, CoreBellPortalAbortReason::PersistentStateChanged)
                    .await;
                return staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                );
            }
            let result =
                commit_world_flow_rejection(write, rejection, request_sequence, mutation).await;
            self.abort_bell_portal(permit, CoreBellPortalAbortReason::DurableRejection)
                .await;
            return result;
        }
        if bell_portal_binding(authenticated, mutation, write.state()).as_ref()
            != Some(&permit.permit().binding)
        {
            let _ = write.rollback().await;
            self.abort_bell_portal(permit, CoreBellPortalAbortReason::PersistentStateChanged)
                .await;
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::StateVersionMismatch,
                None,
                None,
            );
        }
        let Ok(result) = self.planner.plan_validated(
            authenticated,
            request_sequence,
            mutation,
            write.state_mut(),
            true,
        ) else {
            let _ = write.rollback().await;
            self.abort_bell_portal(permit, CoreBellPortalAbortReason::PersistentStateChanged)
                .await;
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        };
        let result = match write.commit(result).await {
            Ok(WorldFlowTransaction::Committed(result)) => result,
            Ok(WorldFlowTransaction::Replayed(_)) => unreachable!("fresh write cannot replay"),
            Err(_) => {
                self.abort_bell_portal(permit, CoreBellPortalAbortReason::PersistenceUnavailable)
                    .await;
                return staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                );
            }
        };
        self.finish_prepared_bell(permit, result).await
    }

    async fn prepare_bell_portal(
        &self,
        binding: CoreBellPortalBinding,
    ) -> Result<BellPortal::PermitLease, CoreBellPortalRejection> {
        tokio::time::timeout(
            CORE_BELL_PORTAL_ACTOR_TIMEOUT,
            self.bell_portal.prepare_bell_portal(binding),
        )
        .await
        .map_err(|_| CoreBellPortalRejection::ServiceUnavailable)?
    }

    async fn commit_bell_portal(
        &self,
        permit: BellPortal::PermitLease,
        transition: CoreBellPortalTransition,
    ) -> Result<(), CoreBellPortalRejection> {
        tokio::time::timeout(
            CORE_BELL_PORTAL_ACTOR_TIMEOUT,
            self.bell_portal.commit_bell_portal(permit, transition),
        )
        .await
        .map_err(|_| CoreBellPortalRejection::ServiceUnavailable)?
    }

    async fn abort_bell_portal(
        &self,
        permit: BellPortal::PermitLease,
        reason: CoreBellPortalAbortReason,
    ) {
        let _ = tokio::time::timeout(
            CORE_BELL_PORTAL_ACTOR_TIMEOUT,
            self.bell_portal.abort_bell_portal(permit, reason),
        )
        .await;
    }

    async fn reconcile_bell_portal(
        &self,
        transition: CoreBellPortalTransition,
    ) -> Result<(), CoreBellPortalRejection> {
        tokio::time::timeout(
            CORE_BELL_PORTAL_ACTOR_TIMEOUT,
            self.bell_portal.reconcile_bell_portal(transition),
        )
        .await
        .map_err(|_| CoreBellPortalRejection::ServiceUnavailable)?
    }

    async fn finish_prepared_bell(
        &self,
        permit: BellPortal::PermitLease,
        result: WorldFlowResult,
    ) -> WorldFlowResult {
        let Some(transition) = bell_portal_transition(&permit.permit().binding, &result) else {
            self.abort_bell_portal(permit, CoreBellPortalAbortReason::ConcurrentResolution)
                .await;
            return result;
        };
        if self
            .commit_bell_portal(permit, transition.clone())
            .await
            .is_err()
        {
            let _ = self.reconcile_bell_portal(transition).await;
        }
        result
    }

    async fn replay_and_reconcile_bell(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        receipt: &StoredWorldTransferReceipt,
    ) -> WorldFlowResult {
        let result = DormantWorldFlowPlanner::<Generator, Clock>::replay(
            request_sequence,
            mutation,
            receipt,
        );
        if let Some(binding) = bell_portal_binding_from_result(authenticated, mutation, &result)
            && let Some(transition) = bell_portal_transition(&binding, &result)
        {
            let _ = self.reconcile_bell_portal(transition).await;
        }
        result
    }

    async fn preflight_safe_inventory(
        &self,
        write: &mut persistence::WorldFlowWrite<'_>,
        authenticated: AuthenticatedAccount,
        mutation: &WorldTransferMutation,
    ) -> Result<u16, WorldTransferResultCode> {
        let account_id = authenticated.account_id.as_bytes();
        let account_version = u64::try_from(write.state().account_version)
            .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?;
        let snapshot = load_world_flow_safe_inventory(
            write.transaction_mut(),
            account_id,
            mutation.character_id,
            account_version,
        )
        .await
        .map_err(|error| preflight_persistence_code(&error))?;
        let placements = plan_danger_entry_safe_deposit(&snapshot)
            .map_err(|error| preflight_plan_code(&error))?;
        let staged = stage_world_flow_safe_inventory_preflight(
            write.transaction_mut(),
            account_id,
            mutation.character_id,
            mutation.mutation_id,
            &snapshot,
            &placements,
        )
        .await
        .map_err(|error| preflight_persistence_code(&error))?;
        write.state_mut().account_version = i64::try_from(staged.account_version)
            .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?;
        u16::try_from(staged.moved_item_count)
            .map_err(|_| WorldTransferResultCode::ServiceUnavailable)
    }

    async fn capture_danger_entry(
        &self,
        write: &mut persistence::WorldFlowWrite<'_>,
        mutation: &WorldTransferMutation,
        safe_placement_count: u16,
        captures_danger_entry: bool,
    ) -> Result<(), WorldTransferResultCode> {
        if !captures_danger_entry || !write.state().location_changed {
            return Ok(());
        }
        let (
            account_id,
            account_version,
            character_version,
            lineage_id,
            restore_point_id,
            danger_location_id,
        ) = match &write.state().location {
            StoredWorldLocation::Danger {
                character_version: post_version,
                location_content_id,
                instance_lineage_id,
                entry_restore_point_id,
            } => (
                write
                    .state()
                    .new_receipt
                    .as_ref()
                    .ok_or(WorldTransferResultCode::ServiceUnavailable)?
                    .account_id,
                write.state().account_version,
                post_version
                    .checked_sub(1)
                    .ok_or(WorldTransferResultCode::ServiceUnavailable)?,
                *instance_lineage_id,
                *entry_restore_point_id,
                location_content_id.clone(),
            ),
            _ => return Ok(()),
        };
        let account_version = u64::try_from(account_version)
            .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?;
        let character_version = u64::try_from(character_version)
            .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?;
        let snapshot = self
            .restore_providers
            .capture_v3(
                write.transaction_mut(),
                EntryCaptureContext {
                    account_id,
                    character_id: mutation.character_id,
                    restore_point_id,
                    mutation_id: mutation.mutation_id,
                    safe_placement_count,
                },
                mutation.payload.content_revision.clone(),
                account_version,
                character_version,
            )
            .await
            .map_err(restore_capture_code)?;
        let root = StoredDangerEntryRootV3 {
            account_id,
            character_id: mutation.character_id,
            lineage_id,
            restore_point_id,
            source_location_id: HALL_ID.to_owned(),
            danger_location_id,
            layout_id: CORE_PRIVATE_LIFE_LAYOUT_ID.to_owned(),
            content_revision: stored_revision(&snapshot.content_revision),
            account_version: i64::try_from(snapshot.versions.account_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            character_version: i64::try_from(snapshot.versions.character_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            progression_version: i64::try_from(snapshot.versions.progression_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            inventory_version: i64::try_from(snapshot.versions.inventory_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            oath_bargain_version: i64::try_from(snapshot.versions.oath_bargain_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            life_metrics_version: i64::try_from(snapshot.versions.life_metrics_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            ash_wallet_version: i64::try_from(snapshot.versions.ash_wallet_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            composite_digest: snapshot.composite_digest().map_err(restore_capture_code)?,
        };
        stage_world_flow_danger_entry(write.transaction_mut(), &root)
            .await
            .map_err(|_| WorldTransferResultCode::ServiceUnavailable)
    }
}

async fn commit_world_flow_rejection(
    write: persistence::WorldFlowWrite<'_>,
    rejection: WorldFlowResult,
    request_sequence: u32,
    mutation: &WorldTransferMutation,
) -> WorldFlowResult {
    match write.commit(rejection).await {
        Ok(WorldFlowTransaction::Committed(result)) => result,
        Ok(WorldFlowTransaction::Replayed(_)) => unreachable!("fresh write cannot replay"),
        Err(_) => staged_result(
            request_sequence,
            mutation,
            WorldTransferResultCode::ServiceUnavailable,
            None,
            None,
        ),
    }
}

async fn world_flow_begin_failure(
    persistence: &PostgresPersistence,
    authenticated: AuthenticatedAccount,
    request_sequence: u32,
    mutation: &WorldTransferMutation,
    error: PersistenceError,
) -> WorldFlowResult {
    let code = match error {
        PersistenceError::WorldFlowCharacterNotFound => {
            match persistence
                .identity_character_owner(mutation.character_id)
                .await
            {
                Ok(Some(owner)) if owner != authenticated.account_id.as_bytes() => {
                    WorldTransferResultCode::CharacterNotOwned
                }
                Ok(_) => WorldTransferResultCode::CharacterNotFound,
                Err(_) => WorldTransferResultCode::ServiceUnavailable,
            }
        }
        PersistenceError::WorldFlowCharacterDead => WorldTransferResultCode::CharacterDead,
        _ => WorldTransferResultCode::ServiceUnavailable,
    };
    staged_result(request_sequence, mutation, code, None, None)
}

fn is_bell_portal_request(mutation: &WorldTransferMutation) -> bool {
    matches!(
        &mutation.payload.command,
        WorldTransferCommand::UsePortal { portal_id }
            if portal_id.as_str() == BELL_DUNGEON_PORTAL_ID
    )
}

fn bell_portal_binding(
    authenticated: AuthenticatedAccount,
    mutation: &WorldTransferMutation,
    state: &WorldFlowTransactionState,
) -> Option<CoreBellPortalBinding> {
    let StoredWorldLocation::Danger {
        character_version,
        location_content_id,
        instance_lineage_id,
        entry_restore_point_id,
    } = &state.location
    else {
        return None;
    };
    if location_content_id != CORE_MICROREALM_ID {
        return None;
    }
    let character_version = u64::try_from(*character_version).ok()?;
    Some(CoreBellPortalBinding {
        account_id: authenticated.account_id.as_bytes(),
        character_id: mutation.character_id,
        mutation_id: mutation.mutation_id,
        instance_lineage_id: *instance_lineage_id,
        entry_restore_point_id: *entry_restore_point_id,
        character_version,
        content_revision: mutation.payload.content_revision.clone(),
    })
}

fn bell_portal_binding_from_result(
    authenticated: AuthenticatedAccount,
    mutation: &WorldTransferMutation,
    result: &WorldFlowResult,
) -> Option<CoreBellPortalBinding> {
    let WorldFlowResult::Transfer {
        accepted: true,
        code: WorldTransferResultCode::Accepted,
        snapshot:
            Some(CharacterLocationSnapshot {
                location:
                    protocol::CharacterLocation::Danger {
                        location_id,
                        instance_lineage_id,
                        entry_restore_point_id,
                    },
                ..
            }),
        ..
    } = result
    else {
        return None;
    };
    (location_id.as_str() == CORE_BELL_DUNGEON_ID).then(|| CoreBellPortalBinding {
        account_id: authenticated.account_id.as_bytes(),
        character_id: mutation.character_id,
        mutation_id: mutation.mutation_id,
        instance_lineage_id: *instance_lineage_id,
        entry_restore_point_id: *entry_restore_point_id,
        character_version: mutation.expected_character_version,
        content_revision: mutation.payload.content_revision.clone(),
    })
}

fn bell_portal_transition(
    binding: &CoreBellPortalBinding,
    result: &WorldFlowResult,
) -> Option<CoreBellPortalTransition> {
    let WorldFlowResult::Transfer {
        accepted: true,
        code: WorldTransferResultCode::Accepted,
        snapshot:
            Some(CharacterLocationSnapshot {
                character_id,
                character_version,
                location:
                    protocol::CharacterLocation::Danger {
                        location_id,
                        instance_lineage_id,
                        entry_restore_point_id,
                    },
            }),
        transfer_id: Some(transfer_id),
        ..
    } = result
    else {
        return None;
    };
    (*character_id == binding.character_id
        && location_id.as_str() == CORE_BELL_DUNGEON_ID
        && *instance_lineage_id == binding.instance_lineage_id
        && *entry_restore_point_id == binding.entry_restore_point_id
        && *character_version == binding.character_version.checked_add(1)?)
    .then(|| CoreBellPortalTransition {
        binding: binding.clone(),
        transfer_id: *transfer_id,
        destination_character_version: *character_version,
    })
}

fn requires_safe_preflight(
    mutation: &WorldTransferMutation,
    state: &WorldFlowTransactionState,
) -> bool {
    matches!(
        (&mutation.payload.command, &state.location),
        (
            WorldTransferCommand::UsePortal { portal_id },
            StoredWorldLocation::Safe {
                location_content_id,
                ..
            }
        ) if portal_id.as_str() == REALM_GATE_ID && location_content_id == HALL_ID
    )
}

const fn bell_portal_rejection_code(rejection: CoreBellPortalRejection) -> WorldTransferResultCode {
    match rejection {
        CoreBellPortalRejection::NotCleared => WorldTransferResultCode::DestinationDisabled,
        CoreBellPortalRejection::OutOfRange => WorldTransferResultCode::OutOfRange,
        CoreBellPortalRejection::TransferInProgress => WorldTransferResultCode::TransferInProgress,
        CoreBellPortalRejection::InstanceUnavailable => {
            WorldTransferResultCode::InstanceUnavailable
        }
        CoreBellPortalRejection::ServiceUnavailable => WorldTransferResultCode::ServiceUnavailable,
    }
}

const fn bell_portal_rejection_is_durable(rejection: CoreBellPortalRejection) -> bool {
    matches!(
        rejection,
        CoreBellPortalRejection::NotCleared | CoreBellPortalRejection::OutOfRange
    )
}

const fn preflight_plan_code(error: &SafeInventoryServiceError) -> WorldTransferResultCode {
    match error {
        SafeInventoryServiceError::StorageFull => {
            WorldTransferResultCode::StorageResolutionRequired
        }
        _ => WorldTransferResultCode::ServiceUnavailable,
    }
}

fn preflight_persistence_code(error: &PersistenceError) -> WorldTransferResultCode {
    match error {
        PersistenceError::SafeInventoryStorageFull
        | PersistenceError::SafeInventoryUnresolvedMutation => {
            WorldTransferResultCode::StorageResolutionRequired
        }
        _ => WorldTransferResultCode::ServiceUnavailable,
    }
}

impl<Generator, Clock, Inventory, OathBargains, LifeMetrics, AshWallet, BellPortal>
    CoreWorldFlowAuthority
    for PostgresCorePrivateWorldFlowCoordinator<
        Generator,
        Clock,
        Inventory,
        OathBargains,
        LifeMetrics,
        AshWallet,
        BellPortal,
    >
where
    Generator: WorldFlowIdGenerator,
    Clock: IdentityClock,
    Inventory: EntryRestoreProvider<Snapshot = InventorySecurityRestoreV3>,
    OathBargains: EntryRestoreProvider<Snapshot = OathBargainRestoreV3>,
    LifeMetrics: EntryRestoreProvider<Snapshot = LifeMetricsRestoreV3>,
    AshWallet: EntryRestoreProvider<Snapshot = AshWalletRestoreV3>,
    BellPortal: CoreBellPortalAuthority,
{
    async fn handle_world_flow(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &WorldFlowFrame,
    ) -> WorldFlowResult {
        self.handle(authenticated, frame).await
    }
}

const fn restore_capture_code(error: RestorePointError) -> WorldTransferResultCode {
    match error {
        RestorePointError::Persistence => WorldTransferResultCode::ServiceUnavailable,
        RestorePointError::ZeroItemUid
        | RestorePointError::ZeroCharacterId
        | RestorePointError::ZeroContextIdentity
        | RestorePointError::InvalidProgression
        | RestorePointError::InvalidInventory
        | RestorePointError::InvalidBeltStack
        | RestorePointError::DuplicateItemUid
        | RestorePointError::InvalidOathBargains
        | RestorePointError::InvalidLifeMetrics
        | RestorePointError::ZeroAggregateVersion
        | RestorePointError::AggregateVersionMismatch
        | RestorePointError::Encoding
        | RestorePointError::IncompleteRestorePoint
        | RestorePointError::RestoreSuperseded => WorldTransferResultCode::IncompleteRestorePoint,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredWorldFlowOutcome {
    code: WorldTransferResultCode,
    snapshot: Option<CharacterLocationSnapshot>,
    transfer_id: Option<[u8; 16]>,
}

impl StoredWorldFlowOutcome {
    fn into_result(self, request_sequence: u32, mutation_id: [u8; 16]) -> WorldFlowResult {
        WorldFlowResult::Transfer {
            request_sequence,
            mutation_id,
            accepted: self.code == WorldTransferResultCode::Accepted,
            code: self.code,
            snapshot: self.snapshot,
            transfer_id: self.transfer_id,
        }
    }
}

fn stage_receipt(
    authenticated: AuthenticatedAccount,
    mutation: &WorldTransferMutation,
    state: &mut WorldFlowTransactionState,
    result: &WorldFlowResult,
) -> Result<(), PersistenceError> {
    let WorldFlowResult::Transfer {
        code,
        snapshot,
        transfer_id,
        ..
    } = result
    else {
        return Err(PersistenceError::CorruptStoredWorldFlow);
    };
    let result_payload = postcard::to_stdvec(&StoredWorldFlowOutcome {
        code: *code,
        snapshot: snapshot.clone(),
        transfer_id: *transfer_id,
    })
    .map_err(|_| PersistenceError::CorruptStoredWorldFlow)?;
    state.new_receipt = Some(StoredWorldTransferReceipt {
        account_id: authenticated.account_id.as_bytes(),
        character_id: mutation.character_id,
        mutation_id: mutation.mutation_id,
        payload_hash: mutation.payload_hash,
        content_revision: stored_revision(&mutation.payload.content_revision),
        expected_character_version: i64::try_from(mutation.expected_character_version)
            .map_err(|_| PersistenceError::CorruptStoredWorldFlow)?,
        issued_at_unix_millis: i64::try_from(mutation.issued_at_unix_millis)
            .map_err(|_| PersistenceError::CorruptStoredWorldFlow)?,
        command_kind: command_kind(&mutation.payload.command),
        transfer_id: *transfer_id,
        pre_character_version: state.character.character_version,
        post_character_version: state.location.character_version(),
        result_code: result_code(*code),
        result_payload,
    });
    Ok(())
}

fn protocol_snapshot(
    character_id: [u8; 16],
    location: &StoredWorldLocation,
) -> Result<CharacterLocationSnapshot, PersistenceError> {
    stored_location_snapshot(character_id, location.clone()).map_err(|error| match error {
        WorldFlowRepositoryError::Unavailable | WorldFlowRepositoryError::Corrupt => {
            PersistenceError::CorruptStoredWorldFlow
        }
    })
}

fn stored_revision(revision: &WorldFlowContentRevisionV1) -> StoredWorldFlowRevisionV1 {
    StoredWorldFlowRevisionV1 {
        records_blake3: revision.records_blake3.as_str().to_owned(),
        assets_blake3: revision.assets_blake3.as_str().to_owned(),
        localization_blake3: revision.localization_blake3.as_str().to_owned(),
    }
}

const fn command_kind(command: &WorldTransferCommand) -> i16 {
    match command {
        WorldTransferCommand::EnterHallFromCharacterSelect => 0,
        WorldTransferCommand::ReturnToCharacterSelect => 1,
        WorldTransferCommand::UsePortal { .. } => 2,
        WorldTransferCommand::UseCommittedExtraction { .. } => 3,
    }
}

const fn result_code(code: WorldTransferResultCode) -> i16 {
    match code {
        WorldTransferResultCode::Accepted => 0,
        WorldTransferResultCode::StageDisabled => 1,
        WorldTransferResultCode::StateVersionMismatch => 2,
        WorldTransferResultCode::CharacterNotFound => 3,
        WorldTransferResultCode::NoSelectedCharacter => 4,
        WorldTransferResultCode::CharacterNotOwned => 5,
        WorldTransferResultCode::CharacterDead => 6,
        WorldTransferResultCode::InvalidSource => 7,
        WorldTransferResultCode::OutOfRange => 8,
        WorldTransferResultCode::ContentDisabled => 9,
        WorldTransferResultCode::DestinationDisabled => 10,
        WorldTransferResultCode::TransferInProgress => 11,
        WorldTransferResultCode::ContentMismatch => 12,
        WorldTransferResultCode::IdempotencyConflict => 13,
        WorldTransferResultCode::PayloadHashMismatch => 14,
        WorldTransferResultCode::IssuedAtInvalid => 15,
        WorldTransferResultCode::IncompleteRestorePoint => 16,
        WorldTransferResultCode::StorageResolutionRequired => 17,
        WorldTransferResultCode::InstanceUnavailable => 18,
        WorldTransferResultCode::RateLimited => 19,
        WorldTransferResultCode::ServiceUnavailable => 20,
    }
}

fn staged_result(
    request_sequence: u32,
    mutation: &WorldTransferMutation,
    code: WorldTransferResultCode,
    snapshot: Option<CharacterLocationSnapshot>,
    transfer_id: Option<[u8; 16]>,
) -> WorldFlowResult {
    WorldFlowResult::Transfer {
        request_sequence,
        mutation_id: mutation.mutation_id,
        accepted: code == WorldTransferResultCode::Accepted,
        code,
        snapshot,
        transfer_id,
    }
}

const fn error(request_sequence: u32, code: WorldTransferResultCode) -> WorldFlowResult {
    WorldFlowResult::Error {
        request_sequence,
        code,
        snapshot: None,
    }
}

#[cfg(test)]
mod tests {
    use protocol::{CharacterLocation, ManifestHash, SafeArrival, WireText, WorldTransferPayload};

    use super::*;
    use crate::AccountId;

    #[derive(Debug, Clone, Copy)]
    struct FixedIds;

    impl WorldFlowIdGenerator for FixedIds {
        fn transfer_id(&self, _material: WorldFlowIdentityMaterial) -> [u8; 16] {
            [8; 16]
        }

        fn lineage_id(&self, _material: WorldFlowIdentityMaterial) -> [u8; 16] {
            [9; 16]
        }

        fn restore_point_id(&self, _material: WorldFlowIdentityMaterial) -> [u8; 16] {
            [10; 16]
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct FixedClock;

    impl IdentityClock for FixedClock {
        fn unix_millis(&self) -> u64 {
            10_000
        }
    }

    fn revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn mutation(command: WorldTransferCommand, version: u64) -> WorldTransferMutation {
        let payload = WorldTransferPayload {
            content_revision: revision(),
            command,
        };
        WorldTransferMutation {
            mutation_id: [3; 16],
            character_id: [2; 16],
            expected_character_version: version,
            issued_at_unix_millis: 9_000,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    fn state(location: StoredWorldLocation) -> WorldFlowTransactionState {
        WorldFlowTransactionState {
            account_version: 1,
            selected_character_id: Some([2; 16]),
            character: persistence::StoredWorldFlowCharacter {
                life_state: 0,
                security_state: 0,
                character_version: location.character_version(),
            },
            location,
            new_receipt: None,
            location_changed: false,
        }
    }

    #[test]
    fn safe_route_consumes_default_then_preserves_character_select_return_arrival() {
        let planner = DormantWorldFlowPlanner::new(FixedIds, FixedClock, revision());
        let mut initial = state(StoredWorldLocation::CharacterSelect {
            character_version: 1,
            next_hall_arrival: StoredSafeArrival::HallDefault,
        });
        let enter = mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 1);
        let result = planner
            .plan_fresh(authenticated(), 1, &enter, &mut initial)
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::Accepted,
                snapshot: Some(CharacterLocationSnapshot {
                    location: CharacterLocation::Safe {
                        arrival: SafeArrival::HallDefault,
                        ..
                    },
                    ..
                }),
                ..
            }
        ));
        assert_eq!(initial.location.character_version(), 2);

        initial.character.character_version = 2;
        initial.new_receipt = None;
        initial.location_changed = false;
        let return_to_select = mutation(WorldTransferCommand::ReturnToCharacterSelect, 2);
        planner
            .plan_fresh(authenticated(), 2, &return_to_select, &mut initial)
            .unwrap();
        assert!(matches!(
            &initial.location,
            StoredWorldLocation::CharacterSelect {
                next_hall_arrival: StoredSafeArrival::SpawnAnchor(spawn),
                ..
            } if spawn == CHARACTER_SELECT_RETURN_SPAWN_ID
        ));

        initial.character.character_version = 3;
        initial.new_receipt = None;
        initial.location_changed = false;
        let reenter = mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 3);
        let result = planner
            .plan_fresh(authenticated(), 3, &reenter, &mut initial)
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                snapshot: Some(CharacterLocationSnapshot {
                    location: CharacterLocation::Safe {
                        arrival: SafeArrival::SpawnAnchor { ref spawn_id },
                        ..
                    },
                    ..
                }),
                ..
            } if spawn_id.as_str() == CHARACTER_SELECT_RETURN_SPAWN_ID
        ));
    }

    #[test]
    fn stale_dead_unselected_and_invalid_source_results_are_stored_without_mutation() {
        let planner = DormantWorldFlowPlanner::new(FixedIds, FixedClock, revision());
        let base = StoredWorldLocation::CharacterSelect {
            character_version: 1,
            next_hall_arrival: StoredSafeArrival::HallDefault,
        };
        let mut stale = state(base.clone());
        let stale_mutation = mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 2);
        let result = planner
            .plan_fresh(authenticated(), 1, &stale_mutation, &mut stale)
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::StateVersionMismatch,
                ..
            }
        ));
        assert!(!stale.location_changed);
        assert!(stale.new_receipt.is_some());

        let mut dead = state(base.clone());
        dead.character.life_state = 1;
        let result = planner
            .plan_fresh(
                authenticated(),
                1,
                &mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 1),
                &mut dead,
            )
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::CharacterDead,
                ..
            }
        ));

        let mut unselected = state(base.clone());
        unselected.selected_character_id = None;
        let result = planner
            .plan_fresh(
                authenticated(),
                1,
                &mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 1),
                &mut unselected,
            )
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::NoSelectedCharacter,
                ..
            }
        ));

        let mut portal = state(base);
        let result = planner
            .plan_fresh(
                authenticated(),
                1,
                &mutation(
                    WorldTransferCommand::UsePortal {
                        portal_id: WireText::new("station.realm_gate").unwrap(),
                    },
                    1,
                ),
                &mut portal,
            )
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::InvalidSource,
                ..
            }
        ));
        assert!(!portal.location_changed);

        let mut later_portal = state(StoredWorldLocation::CharacterSelect {
            character_version: 1,
            next_hall_arrival: StoredSafeArrival::HallDefault,
        });
        let result = planner
            .plan_fresh(
                authenticated(),
                2,
                &mutation(
                    WorldTransferCommand::UsePortal {
                        portal_id: WireText::new("portal.later_stage").unwrap(),
                    },
                    1,
                ),
                &mut later_portal,
            )
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::DestinationDisabled,
                ..
            }
        ));
        assert!(!later_portal.location_changed);
    }

    #[test]
    fn exact_realm_gate_stages_distinct_danger_identities_after_safe_preflight() {
        let planner = DormantWorldFlowPlanner::new(FixedIds, FixedClock, revision());
        let mut state = state(StoredWorldLocation::Safe {
            character_version: 4,
            location_content_id: HALL_ID.to_owned(),
            arrival: StoredSafeArrival::HallDefault,
        });
        state.character.character_version = 4;
        let mutation = mutation(
            WorldTransferCommand::UsePortal {
                portal_id: WireText::new(REALM_GATE_ID).unwrap(),
            },
            4,
        );
        let result = planner
            .plan_fresh(authenticated(), 7, &mutation, &mut state)
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::Accepted,
                transfer_id: Some(transfer_id),
                snapshot: Some(CharacterLocationSnapshot {
                    location: CharacterLocation::Danger {
                        ref location_id,
                        instance_lineage_id,
                        entry_restore_point_id,
                    },
                    ..
                }),
                ..
            } if transfer_id == [8; 16]
                && instance_lineage_id == [9; 16]
                && entry_restore_point_id == [10; 16]
                && location_id.as_str() == CORE_MICROREALM_ID
        ));
        assert!(state.location_changed);
    }

    #[test]
    fn bell_portal_requires_live_clearance_and_preserves_the_entry_authority() {
        let planner = CorePrivateWorldFlowPlanner::new(FixedIds, FixedClock, revision());
        let original = StoredWorldLocation::Danger {
            character_version: 6,
            location_content_id: CORE_MICROREALM_ID.to_owned(),
            instance_lineage_id: [41; 16],
            entry_restore_point_id: [42; 16],
        };
        let request = mutation(
            WorldTransferCommand::UsePortal {
                portal_id: WireText::new(BELL_DUNGEON_PORTAL_ID).unwrap(),
            },
            6,
        );

        let mut uncleared = state(original.clone());
        let rejected = planner
            .plan_fresh(authenticated(), 8, &request, &mut uncleared)
            .unwrap();
        assert!(matches!(
            rejected,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::DestinationDisabled,
                transfer_id: None,
                ..
            }
        ));
        assert_eq!(uncleared.location, original);
        assert!(!uncleared.location_changed);

        let mut cleared = state(original);
        let accepted = planner
            .plan_fresh_with_bell_portal(authenticated(), 9, &request, &mut cleared, true)
            .unwrap();
        assert!(matches!(
            accepted,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::Accepted,
                transfer_id: Some(transfer_id),
                snapshot: Some(CharacterLocationSnapshot {
                    character_version: 7,
                    location: CharacterLocation::Danger {
                        ref location_id,
                        instance_lineage_id,
                        entry_restore_point_id,
                    },
                    ..
                }),
                ..
            } if transfer_id == [8; 16]
                && location_id.as_str() == CORE_BELL_DUNGEON_ID
                && instance_lineage_id == [41; 16]
                && entry_restore_point_id == [42; 16]
        ));
        assert!(matches!(
            cleared.location,
            StoredWorldLocation::Danger {
                character_version: 7,
                ref location_content_id,
                instance_lineage_id,
                entry_restore_point_id,
            } if location_content_id == CORE_BELL_DUNGEON_ID
                && instance_lineage_id == [41; 16]
                && entry_restore_point_id == [42; 16]
        ));
    }

    #[test]
    fn production_world_flow_ids_are_disjoint_restart_stable_and_authority_scoped() {
        let generator = Blake3WorldFlowIds;
        let material = WorldFlowIdentityMaterial {
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
        };
        let ids = [
            generator.transfer_id(material),
            generator.lineage_id(material),
            generator.restore_point_id(material),
        ];
        assert!(ids.iter().all(|id| id.iter().any(|byte| *byte != 0)));
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[2]);
        assert_ne!(ids[1], ids[2]);
        assert_eq!(Blake3WorldFlowIds.transfer_id(material), ids[0]);

        let mut foreign = material;
        foreign.account_id = [4; 16];
        assert_ne!(generator.transfer_id(foreign), ids[0]);
        let mut changed = material;
        changed.mutation_id = [5; 16];
        assert_ne!(generator.transfer_id(changed), ids[0]);
    }

    #[test]
    fn exact_replay_resequences_and_changed_binding_conflicts() {
        let planner = DormantWorldFlowPlanner::new(FixedIds, FixedClock, revision());
        let mut state = state(StoredWorldLocation::CharacterSelect {
            character_version: 1,
            next_hall_arrival: StoredSafeArrival::HallDefault,
        });
        let mutation = mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 1);
        planner
            .plan_fresh(authenticated(), 1, &mutation, &mut state)
            .unwrap();
        let receipt = state.new_receipt.unwrap();
        assert!(matches!(
            DormantWorldFlowPlanner::<FixedIds, FixedClock>::replay(9, &mutation, &receipt),
            WorldFlowResult::Transfer {
                request_sequence: 9,
                code: WorldTransferResultCode::Accepted,
                ..
            }
        ));
        let mut changed = mutation.clone();
        changed.payload.content_revision.records_blake3 =
            ManifestHash::new("f".repeat(64)).unwrap();
        changed.payload_hash = changed.payload.canonical_hash();
        assert!(matches!(
            DormantWorldFlowPlanner::<FixedIds, FixedClock>::replay(10, &changed, &receipt),
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::IdempotencyConflict,
                ..
            }
        ));
    }
}
