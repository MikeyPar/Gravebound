//! Process owner graph for the ordinary Core private-life route.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`-`003`,
//! `TECH-015`, and `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-WORLD-001`, `CONT-HUB-001`/`002`, and `CONT-BOSS-001`/`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-08`, and the M03
//! exit gate). ADR-037 requires this all-or-nothing owner graph before a normal socket may exist.
//!
//! Normal-route composition is all-or-nothing: persistent authorities, Hall ownership, terminal-
//! first transport dispatch, and the native route client must all be present before capability
//! publication.

use std::{path::Path, sync::Arc};

use persistence::PostgresPersistence;
use thiserror::Error;

use crate::core_private_hall_runtime::{CorePrivateHallDirectory, CorePrivateHallError};
use crate::core_private_life_foundation::{
    CorePrivateLifeFoundationError, CorePrivateLifePersistentFoundation, PersistentWorldFlow,
    SystemIdentityClock, route_revision,
};
use crate::core_private_world_flow::CorePrivateHallWorldFlow;
use crate::{
    CaldusVictoryCompositionError, CoreB3RewardCompositionError, CoreCharacterCombatFactory,
    CoreCombatFactoryError, CoreExtractionActorDirectory, CorePrivateHallActorLease,
    CorePrivateLifeSessionDirectory, CorePrivateLifeSessionReport, CorePrivateLifeTickDirectory,
    CorePrivateLifeTickDirectoryReport, CorePrivateLifeTransportLease,
    CorePrivateMicrorealmBinding, CorePrivateMicrorealmRuntime, CorePrivateRouteActorLease,
    CorePrivateRouteRuntimeReport, CoreRecallActorDirectory, CoreReliableWriter,
    ProductionRecallIntentActor, ProductionRecallPendingAuthorityV1, SecretRewardEpoch,
};

type PersistentRecallDirectory =
    CoreRecallActorDirectory<SystemIdentityClock, CorePrivateLifeTickDirectory>;
type PersistentExtractionDirectory = CoreExtractionActorDirectory<
    PostgresPersistence,
    SystemIdentityClock,
    CorePrivateLifeTickDirectory,
>;
type PersistentSessionDirectory =
    CorePrivateLifeSessionDirectory<SystemIdentityClock, CorePrivateLifeTickDirectory>;
type PersistentHallWorldFlow = CorePrivateHallWorldFlow<PersistentWorldFlow>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CorePrivateLifeAdmission {
    authoritative_hall: bool,
    transport_dispatch: bool,
    native_client: bool,
}

impl CorePrivateLifeAdmission {
    const NORMAL_ROUTE_COMPOSED: Self = Self {
        authoritative_hall: true,
        transport_dispatch: true,
        native_client: true,
    };

    const fn ready(self) -> bool {
        self.authoritative_hall && self.transport_dispatch && self.native_client
    }
}

/// One process-owned graph for shared ticks, Recall, extraction, rewards, terminal resolution,
/// combat construction, and generation-safe transport sessions.
pub(crate) struct CorePrivateLifeProcess {
    foundation: Arc<CorePrivateLifePersistentFoundation>,
    persistence: PostgresPersistence,
    sessions: Arc<PersistentSessionDirectory>,
    recall: Arc<PersistentRecallDirectory>,
    extraction: Arc<PersistentExtractionDirectory>,
    ticks: Arc<CorePrivateLifeTickDirectory>,
    hall: Arc<CorePrivateHallDirectory>,
    combat: Arc<CoreCharacterCombatFactory>,
    admission: CorePrivateLifeAdmission,
}

impl std::fmt::Debug for CorePrivateLifeProcess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CorePrivateLifeProcess")
            .field("admission_ready", &self.admission_ready())
            .finish_non_exhaustive()
    }
}

impl CorePrivateLifeProcess {
    /// Builds the complete danger-route process graph from one persistence pool and one redacted
    /// reward epoch. Callers must retain this value as a unit; no partial owner escapes on error.
    pub(crate) fn compose_normal_route(
        foundation: Arc<CorePrivateLifePersistentFoundation>,
        persistence: PostgresPersistence,
        content_root: &Path,
        reward_epoch: SecretRewardEpoch,
    ) -> Result<Self, CorePrivateLifeProcessError> {
        let ticks = Arc::new(CorePrivateLifeTickDirectory::new());
        let recall = Arc::new(PersistentRecallDirectory::new(Arc::clone(&ticks)));
        let extraction = Arc::new(PersistentExtractionDirectory::new(Arc::clone(&ticks)));
        let sessions = CorePrivateLifeSessionDirectory::with_caldus_extraction_runtime(
            Arc::clone(&recall),
            Arc::clone(&extraction),
            persistence.clone(),
            SystemIdentityClock,
        )
        .with_authoritative_tick_directory(Arc::clone(&ticks))
        .with_terminal_owner_factory(foundation.terminal_owner_factory())
        .with_persistent_b3_reward_authority(
            persistence.clone(),
            content_root,
            reward_epoch.clone(),
        )?
        .with_persistent_caldus_reward_authority(
            persistence.clone(),
            content_root,
            reward_epoch,
        )?;
        let process = Self {
            foundation,
            persistence: persistence.clone(),
            sessions: Arc::new(sessions),
            recall,
            extraction,
            ticks,
            hall: Arc::new(CorePrivateHallDirectory::load(content_root)?),
            combat: Arc::new(CoreCharacterCombatFactory::load(persistence, content_root)?),
            admission: CorePrivateLifeAdmission::NORMAL_ROUTE_COMPOSED,
        };
        process.validate_normal_composition()?;
        Ok(process)
    }

    #[must_use]
    pub(crate) const fn admission_ready(&self) -> bool {
        self.admission.ready()
    }

    #[must_use]
    pub(crate) fn sessions(&self) -> &Arc<PersistentSessionDirectory> {
        &self.sessions
    }

    #[must_use]
    pub(crate) fn identity(&self) -> Arc<crate::core_private_life_foundation::PersistentIdentity> {
        self.foundation.identity()
    }

    #[must_use]
    pub(crate) fn progression(
        &self,
    ) -> Arc<crate::ProgressionQueryService<crate::PostgresProgressionQueryRepository>> {
        self.foundation.progression()
    }

    #[must_use]
    pub(crate) fn death_views(
        &self,
    ) -> Arc<crate::DeathViewService<crate::PostgresDeathViewRepository>> {
        self.foundation.death_views()
    }

    #[must_use]
    pub(crate) fn oath(&self) -> Arc<crate::CoreOathSelectionAuthority<SystemIdentityClock>> {
        self.foundation.oath()
    }

    #[must_use]
    pub(crate) fn bargain(&self) -> Arc<crate::CoreBargainAuthority<SystemIdentityClock>> {
        self.foundation.bargain()
    }

    #[must_use]
    pub(crate) fn safe_inventory(&self) -> Arc<crate::CoreSafeInventoryAuthority> {
        self.foundation.safe_inventory()
    }

    #[must_use]
    pub(crate) fn safe_storage(&self) -> Arc<crate::CoreSafeStorageAuthority> {
        self.foundation.safe_storage()
    }

    #[must_use]
    pub(crate) fn resolution_hold(&self) -> Arc<crate::CoreResolutionHoldAuthority> {
        self.foundation.resolution_hold()
    }

    #[must_use]
    pub(crate) fn successor(&self) -> Arc<crate::CoreSuccessorAuthority> {
        self.foundation.successor()
    }

    #[must_use]
    pub(crate) fn recall(&self) -> &Arc<PersistentRecallDirectory> {
        &self.recall
    }

    #[must_use]
    pub(crate) fn extraction(&self) -> &Arc<PersistentExtractionDirectory> {
        &self.extraction
    }

    #[must_use]
    pub(crate) fn combat(&self) -> &Arc<CoreCharacterCombatFactory> {
        &self.combat
    }

    #[must_use]
    pub(crate) fn hall(&self) -> &Arc<CorePrivateHallDirectory> {
        &self.hall
    }

    #[must_use]
    pub(crate) fn hall_world_flow(
        &self,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
    ) -> PersistentHallWorldFlow {
        CorePrivateHallWorldFlow::new(
            self.foundation.world_flow(),
            Arc::clone(&self.hall),
            actor,
            transport,
        )
    }

    #[must_use]
    pub(crate) fn world_flow(&self) -> Arc<PersistentWorldFlow> {
        self.foundation.world_flow()
    }

    pub(crate) fn route_snapshot(
        &self,
        lease: crate::CorePrivateRouteActorLease,
    ) -> Result<protocol::CorePrivateRouteStateV1, CorePrivateLifeProcessError> {
        Ok(self.foundation.route_directory().snapshot(lease)?)
    }

    pub(crate) async fn consumable_state(
        &self,
        transport: CorePrivateLifeTransportLease,
        route: CorePrivateRouteActorLease,
    ) -> Result<protocol::CoreConsumableStateV1, CorePrivateLifeProcessError> {
        let authority = self.sessions.consumable_danger_authority(transport).await?;
        let command = self.consumable_command(&authority, route, [1; 16], [1; 32], 1, 0)?;
        let state = self.persistence.core_consumable_state_v1(&command).await?;
        stored_consumable_state(&state)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the reservation, durable commit, replay, and live-apply transaction stays visible as one ordered authority flow"
    )]
    pub(crate) async fn use_consumable(
        &self,
        transport: CorePrivateLifeTransportLease,
        route: CorePrivateRouteActorLease,
        frame: &protocol::CoreConsumableUseFrameV1,
    ) -> Result<
        (
            protocol::CoreConsumableUseResultV1,
            Option<protocol::CoreConsumableStateV1>,
        ),
        CorePrivateLifeProcessError,
    > {
        frame
            .validate()
            .map_err(|_| CorePrivateLifeProcessError::InvalidConsumable)?;
        if frame.payload.character_id != route.character_id()
            || frame.payload.actor_generation != route.actor_generation()
            || frame.payload.instance_lineage_id
                != self
                    .route_snapshot(route)?
                    .instance_lineage_id
                    .ok_or(CorePrivateLifeProcessError::InvalidConsumable)?
        {
            return Ok((
                consumable_rejection(
                    frame,
                    protocol::CoreConsumableResultCodeV1::AuthorityMismatch,
                ),
                None,
            ));
        }
        let authority = self.sessions.consumable_danger_authority(transport).await?;
        let mut command = self.consumable_command(
            &authority,
            route,
            frame.mutation_id,
            frame.payload_hash,
            frame.payload.expected_inventory_version,
            frame.payload.slot.index(),
        )?;
        if frame.payload.content_revision.as_str()
            != command
                .content_revision
                .strip_prefix("core-dev.blake3.")
                .ok_or(CorePrivateLifeProcessError::InvalidConsumable)?
        {
            return Ok((
                consumable_rejection(frame, protocol::CoreConsumableResultCodeV1::ContentMismatch),
                None,
            ));
        }
        match self
            .persistence
            .load_core_consumable_replay_v1(&command)
            .await
        {
            Ok(Some(stored)) => return project_stored_consumable(&stored),
            Ok(None) => {}
            Err(persistence::PersistenceError::CoreConsumableIdempotencyConflict) => {
                return Ok((
                    consumable_rejection(
                        frame,
                        protocol::CoreConsumableResultCodeV1::IdempotencyConflict,
                    ),
                    None,
                ));
            }
            Err(error) => return Err(error.into()),
        }
        let live_slot = match frame.payload.slot {
            protocol::CoreConsumableSlotV1::BeltOne => crate::CorePrivateConsumableSlot::BeltOne,
            protocol::CoreConsumableSlotV1::BeltTwo => crate::CorePrivateConsumableSlot::BeltTwo,
        };
        let preparation = match self
            .sessions
            .prepare_consumable_use(transport, live_slot)
            .await
        {
            Ok(preparation) => preparation,
            Err(crate::CorePrivateLifeSessionError::MicrorealmIngress(
                crate::CorePrivateMicrorealmIngressError::RecallBlocked,
            )) => {
                return Ok((
                    consumable_rejection(
                        frame,
                        protocol::CoreConsumableResultCodeV1::RecallBlocked,
                    ),
                    None,
                ));
            }
            Err(crate::CorePrivateLifeSessionError::MicrorealmIngress(
                crate::CorePrivateMicrorealmIngressError::DriverFrozen,
            )) => {
                return Ok((
                    consumable_rejection(
                        frame,
                        protocol::CoreConsumableResultCodeV1::TerminalPending,
                    ),
                    None,
                ));
            }
            Err(error) => return Err(error.into()),
        };
        let mut reservation = None;
        command.preflight = match preparation {
            crate::core_private_microrealm_driver::CorePrivateConsumablePreparation::Prepared(
                prepared,
            ) => {
                reservation = Some(prepared);
                persistence::CoreConsumablePreflightV1::Attempt
            }
            crate::core_private_microrealm_driver::CorePrivateConsumablePreparation::Rejected(
                availability,
            ) => match availability {
                crate::core_private_combat_frame::CorePrivateConsumableAvailability::Available
                | crate::core_private_combat_frame::CorePrivateConsumableAvailability::Empty => {
                    persistence::CoreConsumablePreflightV1::Attempt
                }
                crate::core_private_combat_frame::CorePrivateConsumableAvailability::FullHealth => {
                    persistence::CoreConsumablePreflightV1::RejectFullHealth
                }
                crate::core_private_combat_frame::CorePrivateConsumableAvailability::SharedCooldown => {
                    persistence::CoreConsumablePreflightV1::RejectSharedCooldown
                }
                crate::core_private_combat_frame::CorePrivateConsumableAvailability::Inactive => {
                    persistence::CoreConsumablePreflightV1::RejectInactiveSlot
                }
            },
            crate::core_private_microrealm_driver::CorePrivateConsumablePreparation::RecallBlocked => {
                return Ok((
                    consumable_rejection(
                        frame,
                        protocol::CoreConsumableResultCodeV1::RecallBlocked,
                    ),
                    None,
                ));
            }
            crate::core_private_microrealm_driver::CorePrivateConsumablePreparation::TerminalPending => {
                return Ok((
                    consumable_rejection(
                        frame,
                        protocol::CoreConsumableResultCodeV1::TerminalPending,
                    ),
                    None,
                ));
            }
        };
        let stored = match self
            .persistence
            .commit_core_consumable_use_v1(&command)
            .await
        {
            Ok(stored) => stored,
            Err(persistence::PersistenceError::CoreConsumableIdempotencyConflict) => {
                return Ok((
                    consumable_rejection(
                        frame,
                        protocol::CoreConsumableResultCodeV1::IdempotencyConflict,
                    ),
                    None,
                ));
            }
            Err(persistence::PersistenceError::CoreConsumableInventoryVersionMismatch) => {
                return Ok((
                    consumable_rejection(
                        frame,
                        protocol::CoreConsumableResultCodeV1::InventoryVersionMismatch,
                    ),
                    None,
                ));
            }
            Err(persistence::PersistenceError::CoreConsumableContentMismatch) => {
                return Ok((
                    consumable_rejection(
                        frame,
                        protocol::CoreConsumableResultCodeV1::ContentMismatch,
                    ),
                    None,
                ));
            }
            Err(
                persistence::PersistenceError::CoreConsumableAuthorityMismatch
                | persistence::PersistenceError::ActiveDangerAuthorityBindingMismatch
                | persistence::PersistenceError::ActiveDangerAuthoritySuperseded,
            ) => {
                return Ok((
                    consumable_rejection(
                        frame,
                        protocol::CoreConsumableResultCodeV1::AuthorityMismatch,
                    ),
                    None,
                ));
            }
            Err(error) => return Err(error.into()),
        };
        let state = stored_consumable_state(&stored.state)?;
        let code = match stored.code {
            persistence::StoredCoreConsumableResultCodeV1::Accepted => {
                if !stored.replayed {
                    reservation
                        .take()
                        .ok_or(CorePrivateLifeProcessError::InvalidConsumable)?
                        .commit(state.inventory_version)
                        .await
                        .map_err(crate::CorePrivateLifeSessionError::from)?;
                }
                protocol::CoreConsumableResultCodeV1::Accepted
            }
            persistence::StoredCoreConsumableResultCodeV1::EmptySlot => {
                if let Some(reservation) = reservation.take() {
                    reservation.abort();
                }
                protocol::CoreConsumableResultCodeV1::EmptySlot
            }
            persistence::StoredCoreConsumableResultCodeV1::FullHealth => {
                if let Some(reservation) = reservation.take() {
                    reservation.abort();
                }
                protocol::CoreConsumableResultCodeV1::FullHealth
            }
            persistence::StoredCoreConsumableResultCodeV1::SharedCooldown => {
                if let Some(reservation) = reservation.take() {
                    reservation.abort();
                }
                protocol::CoreConsumableResultCodeV1::SharedCooldown
            }
            persistence::StoredCoreConsumableResultCodeV1::InactiveSlot => {
                if let Some(reservation) = reservation.take() {
                    reservation.abort();
                }
                protocol::CoreConsumableResultCodeV1::InactiveSlot
            }
        };
        let result = protocol::CoreConsumableUseResultV1 {
            schema_version: protocol::CORE_CONSUMABLE_SCHEMA_VERSION,
            mutation_id: frame.mutation_id,
            code,
            consumed_item_uid: stored.consumed_item_uid,
            state: (code == protocol::CoreConsumableResultCodeV1::Accepted)
                .then_some(state.clone()),
        };
        Ok((result, Some(state)))
    }

    fn consumable_command(
        &self,
        authority: &crate::CorePrivateDangerEntryAuthority,
        route: CorePrivateRouteActorLease,
        mutation_id: [u8; 16],
        payload_hash: [u8; 32],
        expected_inventory_version: u64,
        slot_index: u8,
    ) -> Result<persistence::CoreConsumableUseCommandV1, CorePrivateLifeProcessError> {
        let terminal = authority.terminal();
        if authority.route_lease() != route {
            return Err(CorePrivateLifeProcessError::InvalidConsumable);
        }
        Ok(persistence::CoreConsumableUseCommandV1 {
            authority: persistence::StoredActiveDangerAuthorityV1 {
                account_id: *terminal.account_id(),
                character_id: *terminal.character_id(),
                instance_lineage_id: *terminal.lineage_id(),
                entry_restore_point_id: *terminal.restore_point_id(),
            },
            mutation_id,
            payload_hash,
            actor_generation: route.actor_generation(),
            content_revision: self.combat.item_content_revision().to_owned(),
            expected_inventory_version,
            slot_index,
            preflight: persistence::CoreConsumablePreflightV1::Attempt,
        })
    }

    /// Attaches the winning authenticated transport and resolves terminal/restart state before
    /// any Hall or danger control can be published by the connection root.
    pub(crate) async fn attach_transport(
        self: &Arc<Self>,
        authenticated: crate::AuthenticatedAccount,
        connection: quinn::Connection,
        issued_at_unix_ms: u64,
    ) -> Result<CorePrivateLifeProcessAttach, CorePrivateLifeProcessError> {
        let attached = self
            .sessions
            .attach_transport(authenticated, connection, issued_at_unix_ms)
            .await?;
        if let Some(previous) = attached.invalidated_connection.as_ref() {
            crate::close_transport(
                previous,
                crate::TRANSPORT_REPLACED_CLOSE_CODE,
                b"authoritative private-life transport replaced",
            );
        }
        if let Some(microrealm) = attached.microrealm {
            return Ok(CorePrivateLifeProcessAttach {
                transport: attached.lease,
                writer: attached.writer,
                disposition: CorePrivateLifeProcessDisposition::Danger(microrealm),
            });
        }
        let bootstrap = match self
            .foundation
            .runtime_bootstrap()
            .bootstrap_process_restart(authenticated, attached.lease, self.sessions.as_ref())
            .await
        {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                let _ = self
                    .sessions
                    .detach_transport(attached.lease, issued_at_unix_ms)
                    .await;
                return Err(error.into());
            }
        };
        if !Arc::ptr_eq(&bootstrap.writer, &attached.writer) {
            let _ = self
                .sessions
                .detach_transport(attached.lease, issued_at_unix_ms)
                .await;
            return Err(CorePrivateLifeProcessError::SplitReliableWriter);
        }
        let disposition = match bootstrap.disposition {
            crate::CorePrivateLifeBootstrapDisposition::HallReady { hall, route } => {
                let actor = match self.hall.install_stored(authenticated, &hall) {
                    Ok(actor) => actor,
                    Err(error) => {
                        let _ = self
                            .sessions
                            .detach_transport(attached.lease, issued_at_unix_ms)
                            .await;
                        return Err(error.into());
                    }
                };
                if let Err(error) = self
                    .hall
                    .attach_transport(authenticated, actor, attached.lease)
                {
                    let _ = self.hall.retire(actor);
                    let _ = self
                        .sessions
                        .detach_transport(attached.lease, issued_at_unix_ms)
                        .await;
                    return Err(error.into());
                }
                CorePrivateLifeProcessDisposition::Hall { hall, route, actor }
            }
            disposition => CorePrivateLifeProcessDisposition::Bootstrap(disposition),
        };
        Ok(CorePrivateLifeProcessAttach {
            transport: attached.lease,
            writer: attached.writer,
            disposition,
        })
    }

    /// Converts one already committed Hall -> microrealm receipt into the exact live danger
    /// graph. Exact replay returns the retained binding; every partial fresh bind is retired before
    /// the error escapes.
    pub(crate) async fn enter_committed_microrealm(
        self: &Arc<Self>,
        authenticated: crate::AuthenticatedAccount,
        transport: CorePrivateLifeTransportLease,
        character_id: [u8; 16],
    ) -> Result<CorePrivateMicrorealmBinding, CorePrivateLifeProcessError> {
        if let Ok(binding) = self.sessions.microrealm_authority(transport).await {
            if binding.lease.character_id() == character_id {
                return Ok(binding);
            }
            return Err(CorePrivateLifeProcessError::InvalidRouteBinding);
        }
        let reattached = self
            .foundation
            .runtime_bootstrap()
            .reattach_within_process(authenticated, transport, self.sessions.as_ref())
            .await?;
        let route_lease = reattached
            .route
            .ok_or(CorePrivateLifeProcessError::InvalidRouteBinding)?;
        if route_lease.character_id() != character_id {
            return Err(CorePrivateLifeProcessError::InvalidRouteBinding);
        }
        let combat = self
            .combat
            .build(authenticated.account_id.as_bytes(), character_id)
            .await?;
        let content = self.foundation.content();
        let runtime = CorePrivateMicrorealmRuntime::new(
            self.foundation.route_directory(),
            route_lease,
            &route_revision(content.revision())?,
            content.microrealm_scene(),
            content.encounter_rooms().clone(),
            content.world_flow().clone(),
            combat,
        )?;
        let recall_actor = Arc::new(ProductionRecallIntentActor::new(
            SystemIdentityClock,
            authenticated.account_id.as_bytes(),
            character_id,
            ProductionRecallPendingAuthorityV1 {
                pending_item_count: 0,
                pending_material_stack_count: 0,
            },
        )?);
        self.recall
            .register_actor(authenticated, route_lease, recall_actor)
            .await?;
        if let Err(error) = self.sessions.bind_recall(transport).await {
            let _ = self.recall.retire_actor(authenticated).await;
            return Err(error.into());
        }
        match self.sessions.bind_microrealm(transport, runtime).await {
            Ok(binding) => Ok(binding),
            Err(error) => {
                let _ = self.sessions.unbind_recall(transport).await;
                Err(error.into())
            }
        }
    }

    /// Installs one already committed Bell transfer into the same danger task that was frozen
    /// before persistence began. The immutable process content is supplied here so transport code
    /// cannot select a layout, encounter pack, route revision, or boss definition.
    pub(crate) async fn commit_bell_handoff(
        &self,
        prepared: crate::CorePrivateLifePreparedBellHandoff,
        transition: crate::CoreBellPortalTransition,
    ) -> Result<crate::CorePrivateFixedDungeonDriverReady, CorePrivateLifeProcessError> {
        let content = self.foundation.content();
        Ok(prepared
            .commit_into_fixed_dungeon(
                transition,
                route_revision(content.revision())?,
                content.encounter_rooms().clone(),
                Arc::new(content.caldus().clone()),
            )
            .await?)
    }

    /// Refreshes durable Character Select/Hall/terminal state after an acknowledged identity or
    /// non-danger transition. A danger transition must use `enter_committed_microrealm` instead.
    pub(crate) async fn refresh_transport(
        &self,
        authenticated: crate::AuthenticatedAccount,
        transport: CorePrivateLifeTransportLease,
        expected_writer: &Arc<CoreReliableWriter>,
    ) -> Result<CorePrivateLifeProcessDisposition, CorePrivateLifeProcessError> {
        let bootstrap = self
            .foundation
            .runtime_bootstrap()
            .refresh_after_identity_or_transition(authenticated, transport, self.sessions.as_ref())
            .await?;
        if !Arc::ptr_eq(&bootstrap.writer, expected_writer) {
            return Err(CorePrivateLifeProcessError::SplitReliableWriter);
        }
        match bootstrap.disposition {
            crate::CorePrivateLifeBootstrapDisposition::HallReady { hall, route } => {
                let actor = self.hall.install_stored(authenticated, &hall)?;
                if let Err(error) = self.hall.attach_transport(authenticated, actor, transport) {
                    let _ = self.hall.retire(actor);
                    return Err(error.into());
                }
                Ok(CorePrivateLifeProcessDisposition::Hall { hall, route, actor })
            }
            disposition => Ok(CorePrivateLifeProcessDisposition::Bootstrap(disposition)),
        }
    }

    /// Installs Hall only after the retained stored extraction result reached this transport.
    /// Extraction replay authority is consumed after the exact Hall snapshot is live, then normal
    /// bootstrap recreates the read-only Hall route actor from durable state.
    pub(crate) async fn install_delivered_extraction_hall(
        &self,
        authenticated: crate::AuthenticatedAccount,
        transport: CorePrivateLifeTransportLease,
        writer: &Arc<CoreReliableWriter>,
    ) -> Result<Option<CorePrivateLifeProcessDisposition>, CorePrivateLifeProcessError> {
        let Some(projection) = self.sessions.delivered_extraction_hall(transport).await? else {
            return Ok(None);
        };
        let actor = self.hall.install(authenticated, projection.snapshot())?;
        if let Err(error) = self.hall.attach_transport(authenticated, actor, transport) {
            let _ = self.hall.retire(actor);
            return Err(error.into());
        }
        if let Err(error) = self
            .sessions
            .acknowledge_extraction_hall_installed(transport, projection)
            .await
        {
            let _ = self.hall.retire(actor);
            return Err(error.into());
        }
        self.refresh_transport(authenticated, transport, writer)
            .await
            .map(Some)
    }

    pub(crate) async fn detach_transport(
        &self,
        transport: CorePrivateLifeTransportLease,
        issued_at_unix_ms: u64,
    ) -> Result<crate::CorePrivateLifeTransportDetach, CorePrivateLifeProcessError> {
        Ok(self
            .sessions
            .detach_transport(transport, issued_at_unix_ms)
            .await?)
    }

    fn validate_normal_composition(&self) -> Result<(), CorePrivateLifeProcessError> {
        if !self.admission_ready() || !self.foundation.normal_route_enabled() {
            return Err(CorePrivateLifeProcessError::IncompleteAdmission);
        }
        Ok(())
    }

    /// Retires session writers before tick and route owners so no task can publish through a
    /// partially dismantled graph.
    pub(crate) async fn begin_shutdown(&self) {
        for connection in self.sessions.begin_shutdown().await {
            connection.close(
                crate::SERVER_SHUTDOWN_CLOSE_CODE.into(),
                b"private-life process shutdown",
            );
        }
        self.ticks.begin_shutdown();
        self.foundation.begin_shutdown();
    }

    pub(crate) async fn finish_shutdown(
        &self,
    ) -> Result<CorePrivateLifeProcessReport, CorePrivateLifeProcessError> {
        let sessions = self.sessions.finish_shutdown().await?;
        let ticks = self.ticks.finish_shutdown()?;
        let routes = self.foundation.finish_shutdown().await?;
        Ok(CorePrivateLifeProcessReport {
            zero_residue: sessions.zero_residue && ticks.zero_residue && routes.zero_residue,
            sessions,
            ticks,
            routes,
        })
    }
}

#[derive(Debug)]
pub(crate) struct CorePrivateLifeProcessReport {
    pub sessions: CorePrivateLifeSessionReport,
    pub ticks: CorePrivateLifeTickDirectoryReport,
    pub routes: CorePrivateRouteRuntimeReport,
    pub zero_residue: bool,
}

#[derive(Debug)]
pub(crate) struct CorePrivateLifeProcessAttach {
    pub transport: CorePrivateLifeTransportLease,
    pub writer: Arc<CoreReliableWriter>,
    pub disposition: CorePrivateLifeProcessDisposition,
}

#[derive(Debug)]
pub(crate) enum CorePrivateLifeProcessDisposition {
    Bootstrap(crate::CorePrivateLifeBootstrapDisposition),
    Hall {
        hall: persistence::StoredPrivateLifeHallV1,
        route: crate::CorePrivateRouteActorLease,
        actor: CorePrivateHallActorLease,
    },
    Danger(CorePrivateMicrorealmBinding),
}

fn stored_consumable_state(
    state: &persistence::StoredCoreConsumableStateV1,
) -> Result<protocol::CoreConsumableStateV1, CorePrivateLifeProcessError> {
    let digest = state
        .content_revision
        .strip_prefix("core-dev.blake3.")
        .ok_or(CorePrivateLifeProcessError::InvalidConsumable)?;
    Ok(protocol::CoreConsumableStateV1 {
        schema_version: protocol::CORE_CONSUMABLE_SCHEMA_VERSION,
        character_id: state.character_id,
        actor_generation: state.actor_generation,
        instance_lineage_id: state.instance_lineage_id,
        content_revision: protocol::ManifestHash::new(digest)?,
        inventory_version: state.inventory_version,
        belt_quantities: state.belt_quantities,
    })
}

fn consumable_rejection(
    frame: &protocol::CoreConsumableUseFrameV1,
    code: protocol::CoreConsumableResultCodeV1,
) -> protocol::CoreConsumableUseResultV1 {
    protocol::CoreConsumableUseResultV1 {
        schema_version: protocol::CORE_CONSUMABLE_SCHEMA_VERSION,
        mutation_id: frame.mutation_id,
        code,
        consumed_item_uid: None,
        state: None,
    }
}

fn project_stored_consumable(
    stored: &persistence::StoredCoreConsumableUseResultV1,
) -> Result<
    (
        protocol::CoreConsumableUseResultV1,
        Option<protocol::CoreConsumableStateV1>,
    ),
    CorePrivateLifeProcessError,
> {
    let state = stored_consumable_state(&stored.state)?;
    let code = match stored.code {
        persistence::StoredCoreConsumableResultCodeV1::Accepted => {
            protocol::CoreConsumableResultCodeV1::Accepted
        }
        persistence::StoredCoreConsumableResultCodeV1::EmptySlot => {
            protocol::CoreConsumableResultCodeV1::EmptySlot
        }
        persistence::StoredCoreConsumableResultCodeV1::FullHealth => {
            protocol::CoreConsumableResultCodeV1::FullHealth
        }
        persistence::StoredCoreConsumableResultCodeV1::SharedCooldown => {
            protocol::CoreConsumableResultCodeV1::SharedCooldown
        }
        persistence::StoredCoreConsumableResultCodeV1::InactiveSlot => {
            protocol::CoreConsumableResultCodeV1::InactiveSlot
        }
    };
    Ok((
        protocol::CoreConsumableUseResultV1 {
            schema_version: protocol::CORE_CONSUMABLE_SCHEMA_VERSION,
            mutation_id: stored.mutation_id,
            code,
            consumed_item_uid: stored.consumed_item_uid,
            state: (code == protocol::CoreConsumableResultCodeV1::Accepted)
                .then_some(state.clone()),
        },
        Some(state),
    ))
}

#[derive(Debug, Error)]
pub(crate) enum CorePrivateLifeProcessError {
    #[error("private-life process admission is incomplete")]
    IncompleteAdmission,
    #[error("private-life session composition failed: {0}")]
    B3(#[from] CoreB3RewardCompositionError),
    #[error("private-life Caldus composition failed: {0}")]
    Caldus(#[from] CaldusVictoryCompositionError),
    #[error("private-life combat composition failed: {0}")]
    Combat(#[from] CoreCombatFactoryError),
    #[error("private-life Hall composition failed: {0}")]
    Hall(#[from] CorePrivateHallError),
    #[error("private-life route binding is invalid")]
    InvalidRouteBinding,
    #[error("private-life route runtime failed: {0}")]
    Route(#[from] crate::CorePrivateRouteRuntimeError),
    #[error("private-life microrealm composition failed: {0}")]
    Microrealm(#[from] crate::CorePrivateMicrorealmRuntimeError),
    #[error("private-life Recall composition failed: {0}")]
    Recall(#[from] crate::CoreRecallRuntimeError),
    #[error("private-life Recall actor is invalid: {0}")]
    RecallActor(#[from] crate::ProductionRecallChannelError),
    #[error("private-life runtime bootstrap failed: {0}")]
    Bootstrap(#[from] crate::CorePrivateLifeBootstrapError),
    #[error("private-life connection resolved more than one reliable writer")]
    SplitReliableWriter,
    #[error("private-life session runtime failed: {0}")]
    Session(#[from] crate::CorePrivateLifeSessionError),
    #[error("private-life tick runtime failed: {0}")]
    Tick(#[from] crate::CorePrivateLifeTickError),
    #[error("private-life foundation failed: {0}")]
    Foundation(#[from] CorePrivateLifeFoundationError),
    #[error("private-life consumable authority is invalid")]
    InvalidConsumable,
    #[error("private-life consumable persistence failed: {0}")]
    Persistence(#[from] persistence::PersistenceError),
    #[error("private-life consumable content revision is invalid: {0}")]
    ProtocolValue(#[from] protocol::BoundedValueError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_owner_graph_is_required_for_normal_admission() {
        assert!(CorePrivateLifeAdmission::NORMAL_ROUTE_COMPOSED.ready());
    }
}
