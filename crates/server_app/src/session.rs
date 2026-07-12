use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use protocol::{
    ActionFrame, ActionKind, ActionResultCode, ControlEvent, ENTITY_STATE_ALIVE,
    ENTITY_STATE_COLLECTED, ENTITY_STATE_ELIGIBLE, EntityKind, EntitySnapshot, InputFrame,
    MAX_SNAPSHOT_ENTITIES_PER_CHUNK, MutationRequest, MutationResult, MutationResultCode,
    PickupPlacement, ReliableEvent, ReliableEventFrame, SessionControlResult, SnapshotChunk,
    WireMessage,
};
use sim_core::{
    AimDirection, AuthoritativeArena, AuthorityEntityKind, AuthorityError, AuthorityInput,
    AuthorityPhase, FieldPickupId, InventoryError, MovementAction, PickupEligibility,
    PlacementChoice, SimulationVector,
};
use thiserror::Error;

const PLAYER_ENTITY_ID: u64 = protocol::M02_ISOLATED_PLAYER_ENTITY_ID;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDisposition {
    Accepted,
    Superseded,
    Rejected(InputRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRejection {
    PrimarySequenceRegression,
}

pub const MAX_NEW_MUTATIONS_PER_TICK: u8 = 8;
pub const MAX_CACHED_MUTATIONS: usize = 1_024;
pub const MAX_RECENT_INGRESS_ANOMALIES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngressAnomalyKind {
    PrimarySequenceRegression,
    StaleReliableAction,
    ActionRateLimited,
    MutationIdempotencyConflict,
    MutationRateLimited,
    MutationRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngressAnomaly {
    pub server_tick: u64,
    pub kind: IngressAnomalyKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IngressDiagnostics {
    pub superseded_inputs: u64,
    pub rejected_primary_sequences: u64,
    pub stale_reliable_actions: u64,
    pub rate_limited_actions: u64,
    pub idempotency_conflicts: u64,
    pub rate_limited_mutations: u64,
    pub rejected_mutations: u64,
    pub anomaly_score: u64,
    pub recent_anomalies: VecDeque<IngressAnomaly>,
}

impl IngressDiagnostics {
    fn record(&mut self, server_tick: u64, kind: IngressAnomalyKind) {
        self.anomaly_score = self.anomaly_score.saturating_add(match kind {
            IngressAnomalyKind::PrimarySequenceRegression
            | IngressAnomalyKind::MutationIdempotencyConflict => 3,
            IngressAnomalyKind::ActionRateLimited | IngressAnomalyKind::MutationRateLimited => 2,
            IngressAnomalyKind::StaleReliableAction | IngressAnomalyKind::MutationRejected => 1,
        });
        if self.recent_anomalies.len() == MAX_RECENT_INGRESS_ANOMALIES {
            self.recent_anomalies.pop_front();
        }
        self.recent_anomalies
            .push_back(IngressAnomaly { server_tick, kind });
    }
}

#[derive(Debug, Clone, Copy)]
struct LatestInput {
    sequence: u32,
    movement_x_milli: i16,
    movement_y_milli: i16,
    aim_x_milli: i16,
    aim_y_milli: i16,
    held_primary: bool,
    primary_sequence: u32,
}

impl Default for LatestInput {
    fn default() -> Self {
        Self {
            sequence: 0,
            movement_x_milli: 0,
            movement_y_milli: 0,
            aim_x_milli: 1_000,
            aim_y_milli: 0,
            held_primary: false,
            primary_sequence: 0,
        }
    }
}

/// One authenticated, server-owned gameplay session. This boundary owns sequencing and wire
/// translation only; every gameplay outcome is delegated to `sim_core::AuthoritativeArena`.
#[derive(Debug)]
pub struct AuthoritativeSession {
    arena: AuthoritativeArena,
    latest_input: LatestInput,
    last_action_sequence: u32,
    ability_1_sequence: u32,
    ability_2_sequence: u32,
    snapshot_sequence: u32,
    reliable_sequence: u32,
    mutation_results: BTreeMap<[u8; 16], CachedMutation>,
    maximum_primary_sequence: u32,
    ability_1_pending: bool,
    ability_2_pending: bool,
    new_mutations_this_tick: u8,
    ingress_diagnostics: IngressDiagnostics,
}

#[derive(Debug, Clone)]
struct CachedMutation {
    request: MutationRequest,
    result: MutationResult,
}

impl AuthoritativeSession {
    pub fn from_content_root(content_root: &Path) -> Result<Self, SessionError> {
        let (package, _) = sim_content::load_and_validate(content_root)
            .map_err(|error| SessionError::Content(error.to_string()))?;
        let content = sim_content::first_playable_authority_combat_test(&package)
            .map_err(|error| SessionError::Content(error.to_string()))?;
        Self::from_compiled_content_with_eligibility(
            &content,
            PickupEligibility {
                valid_session: true,
                reward_eligible: true,
            },
        )
    }

    pub fn from_compiled_content(
        content: &sim_content::AuthorityCombatTestContent,
    ) -> Result<Self, SessionError> {
        Self::from_compiled_content_with_eligibility(
            content,
            PickupEligibility {
                valid_session: true,
                reward_eligible: true,
            },
        )
    }

    fn from_compiled_content_with_eligibility(
        content: &sim_content::AuthorityCombatTestContent,
        eligibility: PickupEligibility,
    ) -> Result<Self, SessionError> {
        let player_entity_id =
            sim_core::EntityId::new(PLAYER_ENTITY_ID).ok_or(SessionError::InvalidPlayerIdentity)?;
        let arena = AuthoritativeArena::new(
            content.definitions.clone(),
            player_entity_id,
            content.spawns.clone(),
            eligibility,
            content.hostile_projectile_ids.clone(),
        )?;
        Ok(Self {
            arena,
            latest_input: LatestInput::default(),
            last_action_sequence: 0,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
            snapshot_sequence: 0,
            reliable_sequence: 0,
            mutation_results: BTreeMap::new(),
            maximum_primary_sequence: 0,
            ability_1_pending: false,
            ability_2_pending: false,
            new_mutations_this_tick: 0,
            ingress_diagnostics: IngressDiagnostics::default(),
        })
    }

    #[must_use]
    pub const fn arena(&self) -> &AuthoritativeArena {
        &self.arena
    }

    #[must_use]
    pub const fn ingress_diagnostics(&self) -> &IngressDiagnostics {
        &self.ingress_diagnostics
    }

    /// Stops transport-carried continuous intent without changing authoritative gameplay state.
    /// The server continues stepping the vulnerable character during `LinkLost`.
    pub fn neutralize_transport_input(&mut self) {
        self.latest_input.movement_x_milli = 0;
        self.latest_input.movement_y_milli = 0;
        self.latest_input.held_primary = false;
    }

    pub(crate) fn commit_emergency_recall(
        &mut self,
    ) -> Result<sim_core::AuthorityRecallCommit, SessionError> {
        self.arena.commit_emergency_recall().map_err(Into::into)
    }

    pub fn submit_input(&mut self, frame: &InputFrame) -> Result<InputDisposition, SessionError> {
        if !matches!(self.arena.phase(), AuthorityPhase::Alive) {
            return Err(SessionError::Dead);
        }
        frame
            .validate()
            .map_err(|_| SessionError::InvalidProtocolMessage)?;
        if frame.sequence <= self.latest_input.sequence {
            self.ingress_diagnostics.superseded_inputs =
                self.ingress_diagnostics.superseded_inputs.saturating_add(1);
            return Ok(InputDisposition::Superseded);
        }
        if frame.held_primary && frame.primary_sequence < self.maximum_primary_sequence {
            self.ingress_diagnostics.rejected_primary_sequences = self
                .ingress_diagnostics
                .rejected_primary_sequences
                .saturating_add(1);
            self.record_anomaly(IngressAnomalyKind::PrimarySequenceRegression);
            return Ok(InputDisposition::Rejected(
                InputRejection::PrimarySequenceRegression,
            ));
        }
        if frame.held_primary {
            self.maximum_primary_sequence =
                self.maximum_primary_sequence.max(frame.primary_sequence);
        }
        self.latest_input = LatestInput {
            sequence: frame.sequence,
            movement_x_milli: frame.movement_x_milli,
            movement_y_milli: frame.movement_y_milli,
            aim_x_milli: frame.aim_x_milli,
            aim_y_milli: frame.aim_y_milli,
            held_primary: frame.held_primary,
            primary_sequence: frame.primary_sequence,
        };
        Ok(InputDisposition::Accepted)
    }

    pub fn submit_action(
        &mut self,
        frame: &ActionFrame,
    ) -> Result<ReliableEventFrame, SessionError> {
        self.submit_action_at(frame, self.arena.player().combat.tick().0)
    }

    pub(crate) fn submit_action_at(
        &mut self,
        frame: &ActionFrame,
        server_tick: u64,
    ) -> Result<ReliableEventFrame, SessionError> {
        frame
            .validate()
            .map_err(|_| SessionError::InvalidProtocolMessage)?;
        if frame.sequence <= self.last_action_sequence {
            self.ingress_diagnostics.stale_reliable_actions = self
                .ingress_diagnostics
                .stale_reliable_actions
                .saturating_add(1);
            self.record_anomaly(IngressAnomalyKind::StaleReliableAction);
            return self.reliable_event_at(
                ReliableEvent::ActionResult {
                    action_sequence: frame.sequence,
                    code: ActionResultCode::StaleSequence,
                },
                server_tick,
            );
        }
        self.last_action_sequence = frame.sequence;
        let code = if matches!(self.arena.phase(), AuthorityPhase::Alive) {
            match frame.action {
                ActionKind::Ability1Press => {
                    if self.arena.emergency_recall_state().is_channeling() {
                        ActionResultCode::InvalidState
                    } else if self.ability_1_pending {
                        self.ingress_diagnostics.rate_limited_actions = self
                            .ingress_diagnostics
                            .rate_limited_actions
                            .saturating_add(1);
                        self.record_anomaly(IngressAnomalyKind::ActionRateLimited);
                        ActionResultCode::RateLimited
                    } else {
                        self.ability_1_pending = true;
                        self.ability_1_sequence = frame.sequence;
                        ActionResultCode::Accepted
                    }
                }
                ActionKind::Ability2Press => {
                    if self.arena.emergency_recall_state().is_channeling() {
                        ActionResultCode::InvalidState
                    } else if self.ability_2_pending {
                        self.ingress_diagnostics.rate_limited_actions = self
                            .ingress_diagnostics
                            .rate_limited_actions
                            .saturating_add(1);
                        self.record_anomaly(IngressAnomalyKind::ActionRateLimited);
                        ActionResultCode::RateLimited
                    } else {
                        self.ability_2_pending = true;
                        self.ability_2_sequence = frame.sequence;
                        ActionResultCode::Accepted
                    }
                }
                ActionKind::RecallStart | ActionKind::RecallCancel => {
                    ActionResultCode::RecallUnavailableCombatLaboratory
                }
                ActionKind::Interact => ActionResultCode::InvalidState,
            }
        } else {
            ActionResultCode::InvalidState
        };
        self.reliable_event_at(
            ReliableEvent::ActionResult {
                action_sequence: frame.sequence,
                code,
            },
            server_tick,
        )
    }

    pub(crate) fn take_shared_authority_input(&mut self) -> Result<AuthorityInput, SessionError> {
        let movement = MovementAction::try_from_milli(
            self.latest_input.movement_x_milli,
            self.latest_input.movement_y_milli,
        )?;
        let aim = AimDirection::new(SimulationVector::new(
            f32::from(self.latest_input.aim_x_milli),
            f32::from(self.latest_input.aim_y_milli),
        ))?;
        let input = AuthorityInput {
            movement,
            aim,
            primary_held: self.latest_input.held_primary,
            primary_sequence: self.latest_input.primary_sequence,
            ability_1_sequence: self.ability_1_sequence,
            ability_2_sequence: self.ability_2_sequence,
        };
        self.ability_1_pending = false;
        self.ability_2_pending = false;
        self.new_mutations_this_tick = 0;
        Ok(input)
    }

    pub(crate) fn encode_shared_snapshots(
        &mut self,
        server_tick: u64,
        state_version: u64,
        entities: Vec<sim_core::AuthorityEntitySnapshot>,
    ) -> Result<Vec<SnapshotChunk>, SessionError> {
        self.snapshot_sequence = self
            .snapshot_sequence
            .checked_add(1)
            .ok_or(SessionError::SequenceExhausted)?;
        let entities = entities
            .into_iter()
            .map(protocol_entity_snapshot)
            .collect::<Vec<_>>();
        let chunk_count = entities.len().div_ceil(MAX_SNAPSHOT_ENTITIES_PER_CHUNK);
        let chunk_count = u16::try_from(chunk_count).map_err(|_| SessionError::SnapshotOverflow)?;
        entities
            .chunks(MAX_SNAPSHOT_ENTITIES_PER_CHUNK)
            .enumerate()
            .map(|(index, entities)| {
                let chunk = SnapshotChunk {
                    sequence: self.snapshot_sequence,
                    server_tick,
                    state_version,
                    acknowledged_input_sequence: self.latest_input.sequence,
                    chunk_index: u16::try_from(index)
                        .map_err(|_| SessionError::SnapshotOverflow)?,
                    chunk_count,
                    entities: entities.to_vec(),
                };
                chunk
                    .validate()
                    .map_err(|_| SessionError::InvalidSnapshot)?;
                Ok(chunk)
            })
            .collect()
    }

    /// Advances one server-owned 30 Hz tick and returns a 15 Hz snapshot every second tick.
    pub fn tick(&mut self) -> Result<Vec<SnapshotChunk>, SessionError> {
        let input = self.take_shared_authority_input()?;
        let step = self.arena.step(input)?;
        if !step.tick.0.is_multiple_of(2) && !step.death_committed && !step.recall_committed {
            return Ok(Vec::new());
        }
        self.snapshot_sequence = self
            .snapshot_sequence
            .checked_add(1)
            .ok_or(SessionError::SequenceExhausted)?;
        let entities = self
            .arena
            .snapshots()?
            .into_iter()
            .map(|entity| EntitySnapshot {
                entity_id: entity.entity_id,
                kind: match entity.kind {
                    AuthorityEntityKind::Player => EntityKind::Player,
                    AuthorityEntityKind::Enemy => EntityKind::Enemy,
                    AuthorityEntityKind::FriendlyProjectile => EntityKind::FriendlyProjectile,
                    AuthorityEntityKind::HostileProjectile => EntityKind::HostileProjectile,
                    AuthorityEntityKind::PersonalPickup => EntityKind::PersonalPickup,
                },
                x_milli_tiles: entity.x_milli_tiles,
                y_milli_tiles: entity.y_milli_tiles,
                velocity_x_milli_tiles_per_second: entity.velocity_x_milli_tiles_per_second,
                velocity_y_milli_tiles_per_second: entity.velocity_y_milli_tiles_per_second,
                source_entity_id: entity.source_entity_id,
                source_input_sequence: entity.source_input_sequence,
                source_projectile_ordinal: entity.source_projectile_ordinal,
                current_health: entity.current_health,
                maximum_health: entity.maximum_health,
                state_flags: entity_state_flags(entity.alive, entity.eligible, entity.collected),
            })
            .collect::<Vec<_>>();
        let chunk_count = entities.len().div_ceil(MAX_SNAPSHOT_ENTITIES_PER_CHUNK);
        let chunk_count = u16::try_from(chunk_count).map_err(|_| SessionError::SnapshotOverflow)?;
        entities
            .chunks(MAX_SNAPSHOT_ENTITIES_PER_CHUNK)
            .enumerate()
            .map(|(index, entities)| {
                let chunk = SnapshotChunk {
                    sequence: self.snapshot_sequence,
                    server_tick: step.tick.0,
                    state_version: step.state_version,
                    acknowledged_input_sequence: self.latest_input.sequence,
                    chunk_index: u16::try_from(index)
                        .map_err(|_| SessionError::SnapshotOverflow)?,
                    chunk_count,
                    entities: entities.to_vec(),
                };
                chunk
                    .validate()
                    .map_err(|_| SessionError::InvalidSnapshot)?;
                Ok(chunk)
            })
            .collect()
    }

    pub fn submit_mutation(
        &mut self,
        request: &MutationRequest,
    ) -> Result<ReliableEventFrame, SessionError> {
        request
            .validate()
            .map_err(|_| SessionError::InvalidProtocolMessage)?;
        let result = if let Some(cached) = self.mutation_results.get(&request.mutation_id).cloned()
        {
            if cached.request == *request {
                cached.result.clone()
            } else {
                self.ingress_diagnostics.idempotency_conflicts = self
                    .ingress_diagnostics
                    .idempotency_conflicts
                    .saturating_add(1);
                self.record_anomaly(IngressAnomalyKind::MutationIdempotencyConflict);
                MutationResult {
                    mutation_id: request.mutation_id,
                    accepted: false,
                    code: MutationResultCode::IdempotencyConflict,
                    state_version: self.arena.state_version(),
                }
            }
        } else if self.new_mutations_this_tick >= MAX_NEW_MUTATIONS_PER_TICK
            || self.mutation_results.len() >= MAX_CACHED_MUTATIONS
        {
            self.ingress_diagnostics.rate_limited_mutations = self
                .ingress_diagnostics
                .rate_limited_mutations
                .saturating_add(1);
            self.record_anomaly(IngressAnomalyKind::MutationRateLimited);
            MutationResult {
                mutation_id: request.mutation_id,
                accepted: false,
                code: MutationResultCode::RateLimited,
                state_version: self.arena.state_version(),
            }
        } else {
            self.new_mutations_this_tick = self
                .new_mutations_this_tick
                .checked_add(1)
                .ok_or(SessionError::SequenceExhausted)?;
            let pickup_id = FieldPickupId::new(request.pickup_id)
                .map_err(|_| SessionError::InvalidProtocolMessage)?;
            let placement = match request.placement {
                PickupPlacement::Take => PlacementChoice::Take,
                PickupPlacement::Equip => PlacementChoice::Equip,
            };
            let (accepted, code) = match self.arena.apply_pickup(pickup_id, placement) {
                Ok(_) => (true, MutationResultCode::Accepted),
                Err(error) => (false, mutation_error_code(&error)),
            };
            let result = MutationResult {
                mutation_id: request.mutation_id,
                accepted,
                code,
                state_version: self.arena.state_version(),
            };
            if !accepted {
                self.ingress_diagnostics.rejected_mutations = self
                    .ingress_diagnostics
                    .rejected_mutations
                    .saturating_add(1);
                self.record_anomaly(IngressAnomalyKind::MutationRejected);
            }
            self.mutation_results.insert(
                request.mutation_id,
                CachedMutation {
                    request: request.clone(),
                    result: result.clone(),
                },
            );
            result
        };
        self.reliable_event(ReliableEvent::MutationResult(result))
    }

    pub(crate) fn submit_shared_mutation(
        &mut self,
        request: &MutationRequest,
        arena: &mut sim_core::SharedAuthoritativeArena,
        player_id: sim_core::EntityId,
        server_tick: u64,
    ) -> Result<ReliableEventFrame, SessionError> {
        request
            .validate()
            .map_err(|_| SessionError::InvalidProtocolMessage)?;
        let result = if let Some(cached) = self.mutation_results.get(&request.mutation_id).cloned()
        {
            if cached.request == *request {
                cached.result
            } else {
                self.ingress_diagnostics.idempotency_conflicts = self
                    .ingress_diagnostics
                    .idempotency_conflicts
                    .saturating_add(1);
                MutationResult {
                    mutation_id: request.mutation_id,
                    accepted: false,
                    code: MutationResultCode::IdempotencyConflict,
                    state_version: arena.state_version(),
                }
            }
        } else if self.new_mutations_this_tick >= MAX_NEW_MUTATIONS_PER_TICK
            || self.mutation_results.len() >= MAX_CACHED_MUTATIONS
        {
            self.ingress_diagnostics.rate_limited_mutations = self
                .ingress_diagnostics
                .rate_limited_mutations
                .saturating_add(1);
            MutationResult {
                mutation_id: request.mutation_id,
                accepted: false,
                code: MutationResultCode::RateLimited,
                state_version: arena.state_version(),
            }
        } else {
            self.new_mutations_this_tick = self
                .new_mutations_this_tick
                .checked_add(1)
                .ok_or(SessionError::SequenceExhausted)?;
            let pickup_id = FieldPickupId::new(request.pickup_id)
                .map_err(|_| SessionError::InvalidProtocolMessage)?;
            let placement = match request.placement {
                PickupPlacement::Take => PlacementChoice::Take,
                PickupPlacement::Equip => PlacementChoice::Equip,
            };
            let (accepted, code) = match arena.apply_pickup(player_id, pickup_id, placement) {
                Ok(_) => (true, MutationResultCode::Accepted),
                Err(error) => (false, shared_mutation_error_code(&error)),
            };
            let result = MutationResult {
                mutation_id: request.mutation_id,
                accepted,
                code,
                state_version: arena.state_version(),
            };
            if !accepted {
                self.ingress_diagnostics.rejected_mutations = self
                    .ingress_diagnostics
                    .rejected_mutations
                    .saturating_add(1);
            }
            self.mutation_results.insert(
                request.mutation_id,
                CachedMutation {
                    request: request.clone(),
                    result: result.clone(),
                },
            );
            result
        };
        self.reliable_event_at(ReliableEvent::MutationResult(result), server_tick)
    }

    pub fn handle_reliable(&mut self, message: WireMessage) -> Result<WireMessage, SessionError> {
        let response = match message {
            WireMessage::ActionFrame(frame) => self.submit_action(&frame)?,
            WireMessage::MutationRequest(request) => self.submit_mutation(&request)?,
            _ => return Err(SessionError::UnexpectedReliableMessage),
        };
        Ok(WireMessage::ReliableEvent(response))
    }

    fn reliable_event(&mut self, event: ReliableEvent) -> Result<ReliableEventFrame, SessionError> {
        self.reliable_event_at(event, self.arena.player().combat.tick().0)
    }

    pub(crate) fn reliable_event_at(
        &mut self,
        event: ReliableEvent,
        server_tick: u64,
    ) -> Result<ReliableEventFrame, SessionError> {
        self.reliable_sequence = self
            .reliable_sequence
            .checked_add(1)
            .ok_or(SessionError::SequenceExhausted)?;
        Ok(ReliableEventFrame {
            sequence: self.reliable_sequence,
            server_tick,
            event,
        })
    }

    fn record_anomaly(&mut self, kind: IngressAnomalyKind) {
        self.ingress_diagnostics
            .record(self.arena.player().combat.tick().0, kind);
    }

    pub(crate) fn emit_control_result(
        &mut self,
        result: SessionControlResult,
    ) -> Result<ReliableEventFrame, SessionError> {
        self.reliable_event(ReliableEvent::Control(ControlEvent::SessionResult(result)))
    }

    pub(crate) fn emit_shutdown_event(&mut self) -> Result<ReliableEventFrame, SessionError> {
        self.reliable_event(ReliableEvent::Control(ControlEvent::ServerShuttingDown))
    }
}

fn entity_state_flags(alive: bool, eligible: bool, collected: bool) -> u32 {
    let mut flags = 0;
    if alive {
        flags |= ENTITY_STATE_ALIVE;
    }
    if eligible {
        flags |= ENTITY_STATE_ELIGIBLE;
    }
    if collected {
        flags |= ENTITY_STATE_COLLECTED;
    }
    flags
}

fn protocol_entity_snapshot(entity: sim_core::AuthorityEntitySnapshot) -> EntitySnapshot {
    EntitySnapshot {
        entity_id: entity.entity_id,
        kind: match entity.kind {
            AuthorityEntityKind::Player => EntityKind::Player,
            AuthorityEntityKind::Enemy => EntityKind::Enemy,
            AuthorityEntityKind::FriendlyProjectile => EntityKind::FriendlyProjectile,
            AuthorityEntityKind::HostileProjectile => EntityKind::HostileProjectile,
            AuthorityEntityKind::PersonalPickup => EntityKind::PersonalPickup,
        },
        x_milli_tiles: entity.x_milli_tiles,
        y_milli_tiles: entity.y_milli_tiles,
        velocity_x_milli_tiles_per_second: entity.velocity_x_milli_tiles_per_second,
        velocity_y_milli_tiles_per_second: entity.velocity_y_milli_tiles_per_second,
        source_entity_id: entity.source_entity_id,
        source_input_sequence: entity.source_input_sequence,
        source_projectile_ordinal: entity.source_projectile_ordinal,
        current_health: entity.current_health,
        maximum_health: entity.maximum_health,
        state_flags: entity_state_flags(entity.alive, entity.eligible, entity.collected),
    }
}

fn mutation_error_code(error: &AuthorityError) -> MutationResultCode {
    match error {
        AuthorityError::Dead => MutationResultCode::Dead,
        AuthorityError::Ineligible | AuthorityError::RecallChanneling => {
            MutationResultCode::Ineligible
        }
        AuthorityError::PickupNotFound(_) => MutationResultCode::NotFound,
        AuthorityError::PickupAlreadyResolved(_) => MutationResultCode::AlreadyResolved,
        AuthorityError::Inventory(InventoryError::PickupOutOfReach { .. }) => {
            MutationResultCode::OutOfRange
        }
        AuthorityError::Inventory(InventoryError::PickupAlreadyCollected(_)) => {
            MutationResultCode::AlreadyResolved
        }
        _ => MutationResultCode::InventoryRejected,
    }
}

fn shared_mutation_error_code(error: &sim_core::SharedAuthorityError) -> MutationResultCode {
    match error {
        sim_core::SharedAuthorityError::UnknownPlayer(_)
        | sim_core::SharedAuthorityError::PickupNotFound(_) => MutationResultCode::NotFound,
        sim_core::SharedAuthorityError::PlayerUnavailable(_) => MutationResultCode::Ineligible,
        sim_core::SharedAuthorityError::Inventory(InventoryError::PickupOutOfReach { .. }) => {
            MutationResultCode::OutOfRange
        }
        sim_core::SharedAuthorityError::Inventory(InventoryError::PickupAlreadyCollected(_)) => {
            MutationResultCode::AlreadyResolved
        }
        _ => MutationResultCode::InventoryRejected,
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("content compilation failed: {0}")]
    Content(String),
    #[error("authoritative player identity is invalid")]
    InvalidPlayerIdentity,
    #[error("protocol message failed validation")]
    InvalidProtocolMessage,
    #[error("server sequence exhausted")]
    SequenceExhausted,
    #[error("snapshot entity count exceeds protocol bounds")]
    SnapshotOverflow,
    #[error("constructed snapshot failed protocol validation")]
    InvalidSnapshot,
    #[error("unexpected message on reliable gameplay channel")]
    UnexpectedReliableMessage,
    #[error("authoritative character is dead")]
    Dead,
    #[error(transparent)]
    Authority(#[from] AuthorityError),
    #[error(transparent)]
    Movement(#[from] sim_core::MovementError),
    #[error(transparent)]
    Aim(#[from] sim_core::AimDirectionError),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use protocol::{ActionKind, MutationResultCode, PickupPlacement, ReliableEvent, WireMessage};

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn input(
        sequence: u32,
        movement: (i16, i16),
        aim: (i16, i16),
        held_primary: bool,
    ) -> InputFrame {
        InputFrame {
            sequence,
            client_tick: u64::from(sequence),
            movement_x_milli: movement.0,
            movement_y_milli: movement.1,
            aim_x_milli: aim.0,
            aim_y_milli: aim.1,
            held_primary,
            primary_sequence: 1,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        }
    }

    fn mutation_result(frame: ReliableEventFrame) -> MutationResult {
        let ReliableEvent::MutationResult(result) = frame.event else {
            panic!("expected mutation result")
        };
        result
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One linear journey makes cross-system ordering auditable.
    fn scripted_authority_session_owns_combat_snapshots_and_idempotent_pickup() {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        assert_eq!(
            session
                .submit_input(&input(1, (0, 0), (-243, -970), true))
                .unwrap(),
            InputDisposition::Accepted
        );
        assert_eq!(
            session
                .submit_input(&input(1, (1_000, 0), (1_000, 0), false))
                .unwrap(),
            InputDisposition::Superseded
        );
        let ability_two = session
            .submit_action(&ActionFrame {
                sequence: 1,
                client_tick: 1,
                action: ActionKind::Ability2Press,
            })
            .unwrap();
        assert!(matches!(
            ability_two.event,
            ReliableEvent::ActionResult {
                code: ActionResultCode::Accepted,
                ..
            }
        ));
        assert!(session.tick().expect("first authority tick").is_empty());
        assert!(
            session
                .arena()
                .player()
                .combat
                .slipstep_cooldown_remaining_ticks()
                > 0
        );

        let ability_one = session
            .submit_action(&ActionFrame {
                sequence: 2,
                client_tick: 2,
                action: ActionKind::Ability1Press,
            })
            .unwrap();
        assert!(matches!(
            ability_one.event,
            ReliableEvent::ActionResult {
                code: ActionResultCode::Accepted,
                ..
            }
        ));

        let mut snapshot_ticks = Vec::new();
        let mut saw_friendly_projectile = false;
        let mut saw_damaged_enemy = false;
        for _ in 0..399 {
            for snapshot in session.tick().expect("authority tick") {
                snapshot_ticks.push(snapshot.server_tick);
                saw_friendly_projectile |= snapshot
                    .entities
                    .iter()
                    .any(|entity| entity.kind == EntityKind::FriendlyProjectile);
                saw_damaged_enemy |= snapshot.entities.iter().any(|entity| {
                    entity.kind == EntityKind::Enemy
                        && entity.current_health < entity.maximum_health
                });
            }
            if !session.arena().pickups().is_empty() {
                break;
            }
        }
        assert!(saw_friendly_projectile);
        assert!(saw_damaged_enemy);
        assert!(!session.arena().pickups().is_empty());
        assert!(
            snapshot_ticks
                .windows(2)
                .all(|ticks| ticks[1] - ticks[0] == 2)
        );
        assert_eq!(
            session
                .arena()
                .player()
                .combat
                .last_ability_1_press_sequence(),
            2
        );
        assert_eq!(
            session
                .arena()
                .player()
                .combat
                .last_ability_2_press_sequence(),
            1
        );

        let pickup_id = session.arena().pickups()[0].pickup_id().get();
        let out_of_range = mutation_result(
            session
                .submit_mutation(&MutationRequest {
                    mutation_id: [1; 16],
                    pickup_id,
                    placement: PickupPlacement::Take,
                })
                .unwrap(),
        );
        assert_eq!(out_of_range.code, MutationResultCode::OutOfRange);
        assert!(!out_of_range.accepted);

        session
            .submit_input(&input(2, (-243, -970), (-243, -970), false))
            .unwrap();
        for _ in 0..40 {
            session.tick().expect("movement tick");
            let delta =
                session.arena().pickups()[0].position() - session.arena().movement().position();
            if delta.length() <= 1.1 {
                break;
            }
        }
        let accepted = mutation_result(
            session
                .submit_mutation(&MutationRequest {
                    mutation_id: [2; 16],
                    pickup_id,
                    placement: PickupPlacement::Take,
                })
                .unwrap(),
        );
        assert_eq!(accepted.code, MutationResultCode::Accepted);
        assert!(accepted.accepted);
        let version_after = accepted.state_version;
        let replay = mutation_result(
            session
                .submit_mutation(&MutationRequest {
                    mutation_id: [2; 16],
                    pickup_id,
                    placement: PickupPlacement::Take,
                })
                .unwrap(),
        );
        assert_eq!(replay, accepted);
        assert_eq!(session.arena().state_version(), version_after);
        let duplicate_pickup = mutation_result(
            session
                .submit_mutation(&MutationRequest {
                    mutation_id: [4; 16],
                    pickup_id,
                    placement: PickupPlacement::Take,
                })
                .unwrap(),
        );
        assert_eq!(duplicate_pickup.code, MutationResultCode::AlreadyResolved);
        assert!(!duplicate_pickup.accepted);
        assert_eq!(session.arena().state_version(), version_after);
    }

    #[test]
    fn hostile_simulation_commits_death_and_closes_all_client_intent_seams() {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        session
            .submit_input(&input(1, (0, 0), (1_000, 0), false))
            .unwrap();
        let mut saw_hostile_projectile = false;
        let mut death_snapshot = None;
        for _ in 0..5_000 {
            let emitted = session.tick().expect("alive authority tick");
            saw_hostile_projectile |= session
                .arena()
                .snapshots()
                .unwrap()
                .iter()
                .any(|entity| entity.kind == AuthorityEntityKind::HostileProjectile);
            if matches!(session.arena().phase(), AuthorityPhase::Dead { .. }) {
                death_snapshot = Some(emitted);
                break;
            }
        }
        assert!(saw_hostile_projectile);
        assert!(matches!(
            session.arena().phase(),
            AuthorityPhase::Dead { .. }
        ));
        assert_eq!(
            session
                .arena()
                .player()
                .consumables
                .vitals()
                .current_health(),
            0
        );
        let AuthorityPhase::Dead { committed_at } = session.arena().phase() else {
            panic!("death phase");
        };
        let death_snapshot = death_snapshot.expect("death snapshot capture");
        assert!(!death_snapshot.is_empty());
        assert!(
            death_snapshot
                .iter()
                .all(|snapshot| snapshot.server_tick == committed_at.0)
        );
        assert!(session.arena().snapshots().unwrap().iter().all(|entity| {
            !matches!(
                entity.kind,
                AuthorityEntityKind::FriendlyProjectile
                    | AuthorityEntityKind::HostileProjectile
                    | AuthorityEntityKind::PersonalPickup
            )
        }));
        assert!(matches!(
            session.submit_input(&input(2, (1_000, 0), (1_000, 0), true)),
            Err(SessionError::Dead)
        ));
        let mutation = mutation_result(
            session
                .submit_mutation(&MutationRequest {
                    mutation_id: [3; 16],
                    pickup_id: 1,
                    placement: PickupPlacement::Take,
                })
                .unwrap(),
        );
        assert_eq!(mutation.code, MutationResultCode::Dead);
    }

    #[test]
    fn reliable_boundary_rejects_wrong_kind_and_replayed_action() {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        session
            .submit_action(&ActionFrame {
                sequence: 1,
                client_tick: 1,
                action: ActionKind::Ability1Press,
            })
            .unwrap();
        let stale = session
            .submit_action(&ActionFrame {
                sequence: 1,
                client_tick: 2,
                action: ActionKind::Ability2Press,
            })
            .unwrap();
        assert!(matches!(
            stale.event,
            ReliableEvent::ActionResult {
                action_sequence: 1,
                code: ActionResultCode::StaleSequence
            }
        ));
        assert!(matches!(
            session.handle_reliable(WireMessage::InputFrame(input(1, (0, 0), (1_000, 0), false))),
            Err(SessionError::UnexpectedReliableMessage)
        ));
    }

    #[test]
    #[ignore = "superseded by CONT-FP-010; automatic LinkLost Recall remains lifecycle-covered"]
    #[allow(clippy::float_cmp, clippy::too_many_lines)] // Exact authored scale and one audit trail.
    fn manual_recall_pins_channel_locks_movement_scale_cancel_and_exact_completion() {
        let mut ordinary =
            AuthoritativeSession::from_content_root(&content_root()).expect("ordinary session");
        ordinary
            .submit_input(&input(1, (1_000, 0), (1_000, 0), false))
            .unwrap();
        ordinary.tick().unwrap();
        ordinary.tick().unwrap();
        let ordinary_velocity = ordinary.arena().movement().velocity().x;

        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("recall session");
        session
            .submit_input(&input(1, (1_000, 0), (1_000, 0), true))
            .unwrap();
        let started = session
            .submit_action(&ActionFrame {
                sequence: 1,
                client_tick: 0,
                action: ActionKind::RecallStart,
            })
            .unwrap();
        assert!(matches!(
            started.event,
            ReliableEvent::ActionResult {
                code: ActionResultCode::Accepted,
                ..
            }
        ));
        let redundant = session
            .submit_action(&ActionFrame {
                sequence: 2,
                client_tick: 0,
                action: ActionKind::RecallStart,
            })
            .unwrap();
        assert!(matches!(
            redundant.event,
            ReliableEvent::ActionResult {
                code: ActionResultCode::InvalidState,
                ..
            }
        ));
        let blocked_ability = session
            .submit_action(&ActionFrame {
                sequence: 3,
                client_tick: 0,
                action: ActionKind::Ability1Press,
            })
            .unwrap();
        assert!(matches!(
            blocked_ability.event,
            ReliableEvent::ActionResult {
                code: ActionResultCode::InvalidState,
                ..
            }
        ));
        let blocked_pickup = mutation_result(
            session
                .submit_mutation(&MutationRequest {
                    mutation_id: [9; 16],
                    pickup_id: 1,
                    placement: PickupPlacement::Take,
                })
                .unwrap(),
        );
        assert_eq!(blocked_pickup.code, MutationResultCode::Ineligible);

        session.tick().unwrap();
        session.tick().unwrap();
        assert_eq!(
            session.arena().movement().velocity().x,
            ordinary_velocity * 0.75
        );
        assert!(session.arena().player().combat.projectiles().is_empty());

        let cancelled = session
            .submit_action(&ActionFrame {
                sequence: 4,
                client_tick: 2,
                action: ActionKind::RecallCancel,
            })
            .unwrap();
        assert!(matches!(
            cancelled.event,
            ReliableEvent::ActionResult {
                code: ActionResultCode::Accepted,
                ..
            }
        ));
        session.tick().unwrap();
        assert!(!session.arena().player().combat.projectiles().is_empty());

        session
            .submit_action(&ActionFrame {
                sequence: 5,
                client_tick: 3,
                action: ActionKind::RecallStart,
            })
            .unwrap();
        for _ in 0..11 {
            session.tick().unwrap();
            assert!(matches!(session.arena().phase(), AuthorityPhase::Alive));
        }
        let critical = session.tick().unwrap();
        assert!(matches!(
            session.arena().phase(),
            AuthorityPhase::Recalled {
                committed_at: sim_core::Tick(15)
            }
        ));
        assert!(!critical.is_empty());
        assert!(critical.iter().all(|chunk| chunk.server_tick == 15));
    }

    #[test]
    #[ignore = "superseded by CONT-FP-010; death-versus-auto-Recall remains lifecycle-covered"]
    fn damage_does_not_cancel_manual_recall_and_death_wins_completion_tick() {
        let mut baseline =
            AuthoritativeSession::from_content_root(&content_root()).expect("baseline session");
        baseline
            .submit_input(&input(1, (0, 0), (1_000, 0), false))
            .unwrap();
        let death_tick = (1..=5_000)
            .find(|_| {
                baseline.tick().unwrap();
                matches!(baseline.arena().phase(), AuthorityPhase::Dead { .. })
            })
            .expect("deterministic hostile death");
        assert!(death_tick > sim_core::EMERGENCY_RECALL_CHANNEL_TICKS);

        let mut contested =
            AuthoritativeSession::from_content_root(&content_root()).expect("contested session");
        contested
            .submit_input(&input(1, (0, 0), (1_000, 0), false))
            .unwrap();
        for _ in 0..(death_tick - sim_core::EMERGENCY_RECALL_CHANNEL_TICKS) {
            contested.tick().unwrap();
        }
        let health_before = contested
            .arena()
            .player()
            .consumables
            .vitals()
            .current_health();
        contested
            .submit_action(&ActionFrame {
                sequence: 1,
                client_tick: death_tick - sim_core::EMERGENCY_RECALL_CHANNEL_TICKS,
                action: ActionKind::RecallStart,
            })
            .unwrap();
        for _ in 0..sim_core::EMERGENCY_RECALL_CHANNEL_TICKS - 1 {
            contested.tick().unwrap();
            assert!(contested.arena().emergency_recall_state().is_channeling());
        }
        contested.tick().unwrap();
        assert_eq!(
            contested
                .arena()
                .player()
                .consumables
                .vitals()
                .current_health(),
            0
        );
        assert!(health_before > 0);
        assert!(matches!(
            contested.arena().phase(),
            AuthorityPhase::Dead { committed_at } if committed_at.0 == death_tick
        ));
    }

    #[test]
    fn first_playable_manual_recall_is_typed_unavailable_and_nonmutating() {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        let state_version = session.arena().state_version();
        for (sequence, action) in [ActionKind::RecallStart, ActionKind::RecallCancel]
            .into_iter()
            .enumerate()
        {
            let result = session
                .submit_action(&ActionFrame {
                    sequence: u32::try_from(sequence + 1).unwrap(),
                    client_tick: 0,
                    action,
                })
                .unwrap();
            assert!(matches!(
                result.event,
                ReliableEvent::ActionResult {
                    code: ActionResultCode::RecallUnavailableCombatLaboratory,
                    ..
                }
            ));
        }
        assert_eq!(session.arena().state_version(), state_version);
        assert!(matches!(session.arena().phase(), AuthorityPhase::Alive));
        assert!(!session.arena().emergency_recall_state().is_channeling());
    }

    #[test]
    fn action_flood_is_typed_bounded_and_does_not_overwrite_first_press() {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        let first = session
            .submit_action(&ActionFrame {
                sequence: 1,
                client_tick: 0,
                action: ActionKind::Ability1Press,
            })
            .unwrap();
        let flooded = session
            .submit_action(&ActionFrame {
                sequence: 2,
                client_tick: 0,
                action: ActionKind::Ability1Press,
            })
            .unwrap();
        let other_ability = session
            .submit_action(&ActionFrame {
                sequence: 3,
                client_tick: 0,
                action: ActionKind::Ability2Press,
            })
            .unwrap();
        assert!(matches!(
            first.event,
            ReliableEvent::ActionResult {
                action_sequence: 1,
                code: ActionResultCode::Accepted
            }
        ));
        assert!(matches!(
            flooded.event,
            ReliableEvent::ActionResult {
                action_sequence: 2,
                code: ActionResultCode::RateLimited
            }
        ));
        assert!(matches!(
            other_ability.event,
            ReliableEvent::ActionResult {
                action_sequence: 3,
                code: ActionResultCode::Accepted
            }
        ));
        session.tick().unwrap();
        assert_eq!(
            session
                .arena()
                .player()
                .combat
                .last_ability_1_press_sequence(),
            1
        );
        assert_eq!(
            session
                .arena()
                .player()
                .combat
                .last_ability_2_press_sequence(),
            3
        );
        assert_eq!(session.ingress_diagnostics().rate_limited_actions, 1);
    }

    #[test]
    fn mutation_id_payload_conflict_and_per_tick_limit_are_nonmutating() {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        let original = MutationRequest {
            mutation_id: [9; 16],
            pickup_id: 1,
            placement: PickupPlacement::Take,
        };
        let first = mutation_result(session.submit_mutation(&original).unwrap());
        assert_eq!(first.code, MutationResultCode::NotFound);
        let state_version = session.arena().state_version();
        let conflict = mutation_result(
            session
                .submit_mutation(&MutationRequest {
                    mutation_id: original.mutation_id,
                    pickup_id: 2,
                    placement: PickupPlacement::Equip,
                })
                .unwrap(),
        );
        assert_eq!(conflict.code, MutationResultCode::IdempotencyConflict);
        assert_eq!(session.arena().state_version(), state_version);
        assert_eq!(
            mutation_result(session.submit_mutation(&original).unwrap()),
            first
        );

        let mut limited =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        for ordinal in 1..=MAX_NEW_MUTATIONS_PER_TICK {
            let mut mutation_id = [0; 16];
            mutation_id[0] = ordinal;
            let result = mutation_result(
                limited
                    .submit_mutation(&MutationRequest {
                        mutation_id,
                        pickup_id: u64::from(ordinal),
                        placement: PickupPlacement::Take,
                    })
                    .unwrap(),
            );
            assert_eq!(result.code, MutationResultCode::NotFound);
        }
        let limited_result = mutation_result(
            limited
                .submit_mutation(&MutationRequest {
                    mutation_id: [10; 16],
                    pickup_id: 10,
                    placement: PickupPlacement::Take,
                })
                .unwrap(),
        );
        assert_eq!(limited_result.code, MutationResultCode::RateLimited);
        assert_eq!(
            limited.mutation_results.len(),
            usize::from(MAX_NEW_MUTATIONS_PER_TICK)
        );
        limited.tick().unwrap();
        let next_tick = mutation_result(
            limited
                .submit_mutation(&MutationRequest {
                    mutation_id: [11; 16],
                    pickup_id: 11,
                    placement: PickupPlacement::Take,
                })
                .unwrap(),
        );
        assert_eq!(next_tick.code, MutationResultCode::NotFound);
    }

    #[test]
    fn mutation_cache_and_anomaly_history_are_strictly_bounded() {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        for ordinal in 0..MAX_CACHED_MUTATIONS {
            let mut mutation_id = [0; 16];
            mutation_id[..8].copy_from_slice(&u64::try_from(ordinal).unwrap().to_le_bytes());
            let request = MutationRequest {
                mutation_id,
                pickup_id: u64::try_from(ordinal).unwrap() + 1,
                placement: PickupPlacement::Take,
            };
            session.mutation_results.insert(
                mutation_id,
                CachedMutation {
                    request,
                    result: MutationResult {
                        mutation_id,
                        accepted: false,
                        code: MutationResultCode::NotFound,
                        state_version: 1,
                    },
                },
            );
        }
        let result = mutation_result(
            session
                .submit_mutation(&MutationRequest {
                    mutation_id: [0xff; 16],
                    pickup_id: u64::MAX,
                    placement: PickupPlacement::Take,
                })
                .unwrap(),
        );
        assert_eq!(result.code, MutationResultCode::RateLimited);
        assert_eq!(session.mutation_results.len(), MAX_CACHED_MUTATIONS);
        for _ in 0..100 {
            session.record_anomaly(IngressAnomalyKind::MutationRejected);
        }
        assert_eq!(
            session.ingress_diagnostics().recent_anomalies.len(),
            MAX_RECENT_INGRESS_ANOMALIES
        );
    }

    #[test]
    fn ineligible_pickup_request_is_nonmutating_and_idempotent() {
        let (package, _) = sim_content::load_and_validate(&content_root()).unwrap();
        let content = sim_content::first_playable_authority_combat_test(&package).unwrap();
        let mut session = AuthoritativeSession::from_compiled_content_with_eligibility(
            &content,
            PickupEligibility {
                valid_session: true,
                reward_eligible: false,
            },
        )
        .expect("ineligible session");
        let before = session.arena().state_version();
        let request = MutationRequest {
            mutation_id: [9; 16],
            pickup_id: 1,
            placement: PickupPlacement::Take,
        };
        let first = mutation_result(session.submit_mutation(&request).unwrap());
        let replay = mutation_result(session.submit_mutation(&request).unwrap());
        assert_eq!(first.code, MutationResultCode::Ineligible);
        assert_eq!(first, replay);
        assert_eq!(session.arena().state_version(), before);
    }

    #[test]
    fn identical_authority_sessions_replay_to_identical_snapshots() {
        fn trace() -> (Vec<SnapshotChunk>, Vec<sim_core::AuthorityEntitySnapshot>) {
            let mut session =
                AuthoritativeSession::from_content_root(&content_root()).expect("session content");
            session
                .submit_input(&input(1, (300, -700), (-243, -970), true))
                .unwrap();
            session
                .submit_action(&ActionFrame {
                    sequence: 1,
                    client_tick: 1,
                    action: ActionKind::Ability2Press,
                })
                .unwrap();
            let mut snapshots = Vec::new();
            for _ in 0..120 {
                snapshots.extend(session.tick().expect("authority tick"));
            }
            (snapshots, session.arena().snapshots().unwrap())
        }

        assert_eq!(trace(), trace());
    }
}
