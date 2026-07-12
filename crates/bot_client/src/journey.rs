use std::collections::{BTreeMap, BTreeSet};

use protocol::{
    ActionFrame, ActionKind, ActionResultCode, ControlEvent, ENTITY_STATE_ALIVE,
    ENTITY_STATE_COLLECTED, ENTITY_STATE_ELIGIBLE, EntityKind, EntitySnapshot, InputFrame,
    MutationRequest, MutationResult, MutationResultCode, PickupPlacement, ReliableEvent,
    ReliableEventFrame, SessionControlFrame, SessionControlRequest, SessionControlResult,
    SessionControlResultCode, SessionDestination, SnapshotChunk, WireText,
};
use thiserror::Error;

pub const BOT_PICKUP_POLICY_DISTANCE_MILLI_TILES: i64 = 1_100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotSnapshot {
    pub sequence: u32,
    pub server_tick: u64,
    pub state_version: u64,
    pub acknowledged_input_sequence: u32,
    pub entities: Vec<EntitySnapshot>,
}

#[derive(Debug, Clone)]
struct PendingSnapshot {
    sequence: u32,
    server_tick: u64,
    state_version: u64,
    acknowledged_input_sequence: u32,
    chunk_count: u16,
    chunks: BTreeMap<u16, Vec<EntitySnapshot>>,
}

#[derive(Debug, Clone, Default)]
pub struct BotSnapshotAssembler {
    completed_sequence: u32,
    pending: Option<PendingSnapshot>,
}

impl BotSnapshotAssembler {
    pub fn ingest(&mut self, chunk: SnapshotChunk) -> Result<Option<BotSnapshot>, BotJourneyError> {
        chunk
            .validate()
            .map_err(|_| BotJourneyError::InvalidSnapshotChunk)?;
        if chunk.sequence <= self.completed_sequence {
            return Ok(None);
        }
        let replace_pending = self
            .pending
            .as_ref()
            .is_none_or(|pending| chunk.sequence > pending.sequence);
        if replace_pending {
            self.pending = Some(PendingSnapshot {
                sequence: chunk.sequence,
                server_tick: chunk.server_tick,
                state_version: chunk.state_version,
                acknowledged_input_sequence: chunk.acknowledged_input_sequence,
                chunk_count: chunk.chunk_count,
                chunks: BTreeMap::new(),
            });
        }
        let pending = self
            .pending
            .as_mut()
            .ok_or(BotJourneyError::SnapshotAssemblyMissing)?;
        if chunk.sequence < pending.sequence {
            return Ok(None);
        }
        if chunk.server_tick != pending.server_tick
            || chunk.state_version != pending.state_version
            || chunk.acknowledged_input_sequence != pending.acknowledged_input_sequence
            || chunk.chunk_count != pending.chunk_count
        {
            return Err(BotJourneyError::InconsistentSnapshotMetadata);
        }
        if let Some(existing) = pending.chunks.get(&chunk.chunk_index) {
            if existing == &chunk.entities {
                return Ok(None);
            }
            return Err(BotJourneyError::ConflictingSnapshotDuplicate);
        }
        pending.chunks.insert(chunk.chunk_index, chunk.entities);
        if pending.chunks.len() != usize::from(pending.chunk_count) {
            return Ok(None);
        }
        let pending = self
            .pending
            .take()
            .ok_or(BotJourneyError::SnapshotAssemblyMissing)?;
        let mut entity_ids = BTreeSet::new();
        let mut entities = Vec::new();
        for (_, chunk_entities) in pending.chunks {
            for entity in chunk_entities {
                if !entity_ids.insert(entity.entity_id) {
                    return Err(BotJourneyError::DuplicateSnapshotEntity);
                }
                entities.push(entity);
            }
        }
        self.completed_sequence = pending.sequence;
        Ok(Some(BotSnapshot {
            sequence: pending.sequence,
            server_tick: pending.server_tick,
            state_version: pending.state_version,
            acknowledged_input_sequence: pending.acknowledged_input_sequence,
            entities,
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotTerminalOutcome {
    Active,
    Recalled,
    Dead,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BotBehavior {
    #[default]
    FightAndCollect,
    AwaitAuthoritativeDeath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotObservation {
    pub server_tick: u64,
    pub state_version: u64,
    pub player: EntitySnapshot,
    pub nearest_enemy: Option<EntitySnapshot>,
    pub nearest_pickup: Option<EntitySnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BotJourneyEvidence {
    pub completed_snapshots: u64,
    pub inputs_sent: u64,
    pub action_results: u64,
    pub mutations_accepted: u64,
    pub reconnects_accepted: u64,
    pub saw_enemy_damage: bool,
    pub saw_friendly_projectile: bool,
    pub moved_from_first_position: bool,
}

/// Snapshot-driven headless policy. It owns intent and evidence only; no server or simulation type
/// is available in this crate's policy boundary.
#[derive(Debug, Clone)]
pub struct JourneyBot {
    behavior: BotBehavior,
    assembler: BotSnapshotAssembler,
    observation: Option<BotObservation>,
    first_player_position: Option<(i32, i32)>,
    last_aim: (i16, i16),
    input_sequence: u32,
    primary_sequence: u32,
    action_sequence: u32,
    control_sequence: u32,
    mutation_sequence: u64,
    pending_mutation: Option<[u8; 16]>,
    pending_pickup: Option<u64>,
    resolved_pickups: BTreeSet<u64>,
    logical_session_id: Option<WireText<64>>,
    terminal: BotTerminalOutcome,
    evidence: BotJourneyEvidence,
}

impl Default for JourneyBot {
    fn default() -> Self {
        Self {
            behavior: BotBehavior::default(),
            assembler: BotSnapshotAssembler::default(),
            observation: None,
            first_player_position: None,
            last_aim: (1_000, 0),
            input_sequence: 0,
            primary_sequence: 0,
            action_sequence: 0,
            control_sequence: 0,
            mutation_sequence: 0,
            pending_mutation: None,
            pending_pickup: None,
            resolved_pickups: BTreeSet::new(),
            logical_session_id: None,
            terminal: BotTerminalOutcome::Active,
            evidence: BotJourneyEvidence::default(),
        }
    }
}

impl JourneyBot {
    #[cfg(test)]
    pub(crate) fn set_input_sequence_for_test(&mut self, sequence: u32) {
        self.input_sequence = sequence;
    }

    #[must_use]
    pub fn with_behavior(behavior: BotBehavior) -> Self {
        Self {
            behavior,
            ..Self::default()
        }
    }

    #[must_use]
    pub const fn terminal_outcome(&self) -> BotTerminalOutcome {
        self.terminal
    }

    #[must_use]
    pub const fn evidence(&self) -> &BotJourneyEvidence {
        &self.evidence
    }

    #[must_use]
    pub fn logical_session_id(&self) -> Option<&WireText<64>> {
        self.logical_session_id.as_ref()
    }

    #[must_use]
    pub const fn observation(&self) -> Option<&BotObservation> {
        self.observation.as_ref()
    }

    pub fn ingest_snapshot(
        &mut self,
        chunk: SnapshotChunk,
    ) -> Result<Option<&BotObservation>, BotJourneyError> {
        let Some(snapshot) = self.assembler.ingest(chunk)? else {
            return Ok(None);
        };
        let saw_friendly_projectile = snapshot
            .entities
            .iter()
            .any(|entity| entity.kind == EntityKind::FriendlyProjectile);
        let observation = observation_from_snapshot(&snapshot)?;
        self.evidence.completed_snapshots = self.evidence.completed_snapshots.saturating_add(1);
        let position = (
            observation.player.x_milli_tiles,
            observation.player.y_milli_tiles,
        );
        if let Some(first) = self.first_player_position {
            self.evidence.moved_from_first_position |= position != first;
        } else {
            self.first_player_position = Some(position);
        }
        self.evidence.saw_enemy_damage |= observation.nearest_enemy.as_ref().is_some_and(|enemy| {
            enemy.current_health > 0 && enemy.current_health < enemy.maximum_health
        });
        self.evidence.saw_friendly_projectile |= saw_friendly_projectile;
        self.terminal = if observation.player.state_flags & ENTITY_STATE_ALIVE != 0 {
            BotTerminalOutcome::Active
        } else if observation.player.current_health == 0 {
            BotTerminalOutcome::Dead
        } else {
            BotTerminalOutcome::Recalled
        };
        self.observation = Some(observation);
        Ok(self.observation.as_ref())
    }

    pub fn next_input(&mut self) -> Result<InputFrame, BotJourneyError> {
        if self.terminal != BotTerminalOutcome::Active {
            return Err(BotJourneyError::TerminalJourney);
        }
        self.input_sequence = checked_next(self.input_sequence)?;
        let mut movement = (0, 0);
        let mut held_primary = false;
        if let Some(observation) = &self.observation {
            if self.behavior == BotBehavior::AwaitAuthoritativeDeath {
                if let Some(enemy) = &observation.nearest_enemy {
                    let delta = (
                        enemy.x_milli_tiles - observation.player.x_milli_tiles,
                        enemy.y_milli_tiles - observation.player.y_milli_tiles,
                    );
                    self.last_aim = fixed_direction(delta.0, delta.1, self.last_aim);
                }
            } else if let Some(pickup) = &observation.nearest_pickup {
                let delta = (
                    pickup.x_milli_tiles - observation.player.x_milli_tiles,
                    pickup.y_milli_tiles - observation.player.y_milli_tiles,
                );
                movement = fixed_direction(delta.0, delta.1, (0, 0));
                self.last_aim = fixed_direction(delta.0, delta.1, self.last_aim);
            } else if let Some(enemy) = &observation.nearest_enemy {
                let delta = (
                    enemy.x_milli_tiles - observation.player.x_milli_tiles,
                    enemy.y_milli_tiles - observation.player.y_milli_tiles,
                );
                self.last_aim = fixed_direction(delta.0, delta.1, self.last_aim);
                held_primary = true;
                if self.primary_sequence == 0 {
                    self.primary_sequence = 1;
                }
            }
        }
        self.evidence.inputs_sent = self.evidence.inputs_sent.saturating_add(1);
        Ok(InputFrame {
            sequence: self.input_sequence,
            client_tick: self
                .observation
                .as_ref()
                .map_or(0, |observation| observation.server_tick),
            movement_x_milli: movement.0,
            movement_y_milli: movement.1,
            aim_x_milli: self.last_aim.0,
            aim_y_milli: self.last_aim.1,
            held_primary,
            primary_sequence: self.primary_sequence,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        })
    }

    pub fn next_pickup_request(&mut self) -> Result<Option<MutationRequest>, BotJourneyError> {
        if self.pending_mutation.is_some() || self.terminal != BotTerminalOutcome::Active {
            return Ok(None);
        }
        let Some(observation) = &self.observation else {
            return Ok(None);
        };
        let Some(pickup) = &observation.nearest_pickup else {
            return Ok(None);
        };
        if self.resolved_pickups.contains(&pickup.entity_id) {
            return Ok(None);
        }
        let dx = i64::from(pickup.x_milli_tiles - observation.player.x_milli_tiles);
        let dy = i64::from(pickup.y_milli_tiles - observation.player.y_milli_tiles);
        let limit = BOT_PICKUP_POLICY_DISTANCE_MILLI_TILES;
        if dx * dx + dy * dy > limit * limit {
            return Ok(None);
        }
        self.mutation_sequence = self
            .mutation_sequence
            .checked_add(1)
            .ok_or(BotJourneyError::SequenceExhausted)?;
        let mut mutation_id = [0_u8; 16];
        mutation_id[..8].copy_from_slice(b"GBBOTM02");
        mutation_id[8..].copy_from_slice(&self.mutation_sequence.to_be_bytes());
        self.pending_mutation = Some(mutation_id);
        self.pending_pickup = Some(pickup.entity_id);
        Ok(Some(MutationRequest {
            mutation_id,
            pickup_id: pickup.entity_id,
            placement: PickupPlacement::Take,
        }))
    }

    pub fn next_action(&mut self, action: ActionKind) -> Result<ActionFrame, BotJourneyError> {
        if self.terminal != BotTerminalOutcome::Active {
            return Err(BotJourneyError::TerminalJourney);
        }
        self.action_sequence = checked_next(self.action_sequence)?;
        Ok(ActionFrame {
            sequence: self.action_sequence,
            client_tick: self
                .observation
                .as_ref()
                .map_or(0, |observation| observation.server_tick),
            action,
        })
    }

    pub fn next_join(
        &mut self,
        client_monotonic_micros: u64,
    ) -> Result<SessionControlFrame, BotJourneyError> {
        self.control_sequence = checked_next(self.control_sequence)?;
        Ok(SessionControlFrame {
            sequence: self.control_sequence,
            client_tick: self
                .observation
                .as_ref()
                .map_or(0, |value| value.server_tick),
            client_monotonic_micros,
            request: SessionControlRequest::Join,
        })
    }

    pub fn next_reconnect(
        &mut self,
        client_monotonic_micros: u64,
    ) -> Result<SessionControlFrame, BotJourneyError> {
        let prior_session_id = self
            .logical_session_id
            .clone()
            .ok_or(BotJourneyError::LogicalSessionMissing)?;
        self.control_sequence = checked_next(self.control_sequence)?;
        Ok(SessionControlFrame {
            sequence: self.control_sequence,
            client_tick: self
                .observation
                .as_ref()
                .map_or(0, |value| value.server_tick),
            client_monotonic_micros,
            request: SessionControlRequest::Reconnect { prior_session_id },
        })
    }

    pub fn apply_reliable_event(
        &mut self,
        frame: &ReliableEventFrame,
    ) -> Result<(), BotJourneyError> {
        match &frame.event {
            ReliableEvent::ActionResult { code, .. } => {
                self.evidence.action_results = self.evidence.action_results.saturating_add(1);
                if *code != ActionResultCode::Accepted {
                    return Err(BotJourneyError::ActionRejected(*code));
                }
            }
            ReliableEvent::MutationResult(result) => self.apply_mutation_result(result)?,
            ReliableEvent::Control(ControlEvent::SessionResult(result)) => {
                self.apply_control_result(result)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn apply_mutation_result(&mut self, result: &MutationResult) -> Result<(), BotJourneyError> {
        if self.pending_mutation != Some(result.mutation_id) {
            return Err(BotJourneyError::UnexpectedMutationResult);
        }
        self.pending_mutation = None;
        if result.accepted && result.code == MutationResultCode::Accepted {
            let pickup_id = self
                .pending_pickup
                .take()
                .ok_or(BotJourneyError::UnexpectedMutationResult)?;
            self.resolved_pickups.insert(pickup_id);
            self.evidence.mutations_accepted = self.evidence.mutations_accepted.saturating_add(1);
            Ok(())
        } else {
            self.pending_pickup = None;
            Err(BotJourneyError::MutationRejected(result.code))
        }
    }

    fn apply_control_result(
        &mut self,
        result: &SessionControlResult,
    ) -> Result<(), BotJourneyError> {
        if !result.accepted {
            return Err(BotJourneyError::ControlRejected(result.code));
        }
        if matches!(
            result.code,
            SessionControlResultCode::Joined | SessionControlResultCode::Reattached
        ) {
            self.logical_session_id = Some(result.session_id.clone());
        }
        if result.code == SessionControlResultCode::Reattached {
            self.evidence.reconnects_accepted = self.evidence.reconnects_accepted.saturating_add(1);
        }
        self.terminal = match result.destination {
            SessionDestination::CombatInstance => BotTerminalOutcome::Active,
            SessionDestination::LanternHalls => BotTerminalOutcome::Recalled,
            SessionDestination::DeathFinal => BotTerminalOutcome::Dead,
            SessionDestination::Closed => return Err(BotJourneyError::ClosedDestination),
        };
        Ok(())
    }
}

fn observation_from_snapshot(snapshot: &BotSnapshot) -> Result<BotObservation, BotJourneyError> {
    let players = snapshot
        .entities
        .iter()
        .filter(|entity| entity.kind == EntityKind::Player)
        .cloned()
        .collect::<Vec<_>>();
    let [player] = players.as_slice() else {
        return Err(BotJourneyError::PlayerSnapshotCardinality);
    };
    let nearest_enemy = nearest_entity(&snapshot.entities, player, |entity| {
        matches!(entity.kind, EntityKind::Enemy | EntityKind::Boss)
            && entity.state_flags & ENTITY_STATE_ALIVE != 0
            && entity.current_health > 0
    });
    let nearest_pickup = nearest_entity(&snapshot.entities, player, |entity| {
        entity.kind == EntityKind::PersonalPickup
            && entity.state_flags & ENTITY_STATE_ALIVE != 0
            && entity.state_flags & ENTITY_STATE_ELIGIBLE != 0
            && entity.state_flags & ENTITY_STATE_COLLECTED == 0
    });
    Ok(BotObservation {
        server_tick: snapshot.server_tick,
        state_version: snapshot.state_version,
        player: player.clone(),
        nearest_enemy,
        nearest_pickup,
    })
}

fn nearest_entity(
    entities: &[EntitySnapshot],
    player: &EntitySnapshot,
    include: impl Fn(&EntitySnapshot) -> bool,
) -> Option<EntitySnapshot> {
    entities
        .iter()
        .filter(|entity| include(entity))
        .min_by_key(|entity| {
            let dx = i64::from(entity.x_milli_tiles - player.x_milli_tiles);
            let dy = i64::from(entity.y_milli_tiles - player.y_milli_tiles);
            (dx * dx + dy * dy, entity.entity_id)
        })
        .cloned()
}

fn fixed_direction(x: i32, y: i32, fallback: (i16, i16)) -> (i16, i16) {
    let maximum = x.unsigned_abs().max(y.unsigned_abs());
    if maximum == 0 {
        return fallback;
    }
    let divisor = i64::from(maximum);
    let scaled_x = i64::from(x) * 1_000 / divisor;
    let scaled_y = i64::from(y) * 1_000 / divisor;
    (
        i16::try_from(scaled_x).expect("direction is bounded to fixed protocol scale"),
        i16::try_from(scaled_y).expect("direction is bounded to fixed protocol scale"),
    )
}

fn checked_next(value: u32) -> Result<u32, BotJourneyError> {
    value
        .checked_add(1)
        .ok_or(BotJourneyError::SequenceExhausted)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BotJourneyError {
    #[error("snapshot chunk failed canonical protocol validation")]
    InvalidSnapshotChunk,
    #[error("snapshot assembly state is missing")]
    SnapshotAssemblyMissing,
    #[error("snapshot chunks disagree on sequence metadata")]
    InconsistentSnapshotMetadata,
    #[error("duplicate snapshot chunk carries different entities")]
    ConflictingSnapshotDuplicate,
    #[error("one completed snapshot contains a duplicate entity identity")]
    DuplicateSnapshotEntity,
    #[error("completed snapshot must contain exactly one player")]
    PlayerSnapshotCardinality,
    #[error("bot command sequence exhausted")]
    SequenceExhausted,
    #[error("bot journey already has a terminal outcome")]
    TerminalJourney,
    #[error("logical session ID is unavailable for reconnect")]
    LogicalSessionMissing,
    #[error("server returned an unexpected mutation identity")]
    UnexpectedMutationResult,
    #[error("server rejected bot action with {0:?}")]
    ActionRejected(ActionResultCode),
    #[error("server rejected bot mutation with {0:?}")]
    MutationRejected(MutationResultCode),
    #[error("server rejected bot lifecycle request with {0:?}")]
    ControlRejected(SessionControlResultCode),
    #[error("server routed an accepted journey to a closed destination")]
    ClosedDestination,
}
