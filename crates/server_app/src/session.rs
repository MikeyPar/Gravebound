use std::{collections::BTreeMap, path::Path};

use protocol::{
    ActionFrame, ActionKind, ActionResultCode, ENTITY_STATE_ALIVE, ENTITY_STATE_COLLECTED,
    ENTITY_STATE_ELIGIBLE, EntityKind, EntitySnapshot, InputFrame, MAX_SNAPSHOT_ENTITIES_PER_CHUNK,
    MutationRequest, MutationResult, MutationResultCode, PickupPlacement, ReliableEvent,
    ReliableEventFrame, SnapshotChunk, WireMessage,
};
use sim_core::{
    AimDirection, AuthoritativeArena, AuthorityEntityKind, AuthorityError, AuthorityInput,
    AuthorityPhase, FieldPickupId, InventoryError, MovementAction, PickupEligibility,
    PlacementChoice, SimulationVector,
};
use thiserror::Error;

const PLAYER_ENTITY_ID: u64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDisposition {
    Accepted,
    Superseded,
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
    mutation_results: BTreeMap<[u8; 16], MutationResult>,
}

impl AuthoritativeSession {
    pub fn from_content_root(content_root: &Path) -> Result<Self, SessionError> {
        Self::from_content_root_with_eligibility(
            content_root,
            PickupEligibility {
                valid_session: true,
                reward_eligible: true,
            },
        )
    }

    fn from_content_root_with_eligibility(
        content_root: &Path,
        eligibility: PickupEligibility,
    ) -> Result<Self, SessionError> {
        let (package, _) = sim_content::load_and_validate(content_root)
            .map_err(|error| SessionError::Content(error.to_string()))?;
        let content = sim_content::first_playable_authority_combat_test(&package)
            .map_err(|error| SessionError::Content(error.to_string()))?;
        let player_entity_id =
            sim_core::EntityId::new(PLAYER_ENTITY_ID).ok_or(SessionError::InvalidPlayerIdentity)?;
        let arena = AuthoritativeArena::new(
            content.definitions,
            player_entity_id,
            content.spawns,
            eligibility,
            content.hostile_projectile_ids,
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
        })
    }

    #[must_use]
    pub const fn arena(&self) -> &AuthoritativeArena {
        &self.arena
    }

    pub fn submit_input(&mut self, frame: &InputFrame) -> Result<InputDisposition, SessionError> {
        if !matches!(self.arena.phase(), AuthorityPhase::Alive) {
            return Err(SessionError::Dead);
        }
        frame
            .validate()
            .map_err(|_| SessionError::InvalidProtocolMessage)?;
        if frame.sequence <= self.latest_input.sequence {
            return Ok(InputDisposition::Superseded);
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
        frame
            .validate()
            .map_err(|_| SessionError::InvalidProtocolMessage)?;
        if frame.sequence <= self.last_action_sequence {
            return Err(SessionError::NonMonotonicAction {
                received: frame.sequence,
                last: self.last_action_sequence,
            });
        }
        self.last_action_sequence = frame.sequence;
        let code = if matches!(self.arena.phase(), AuthorityPhase::Alive) {
            match frame.action {
                ActionKind::Ability1Press => {
                    self.ability_1_sequence = frame.sequence;
                    ActionResultCode::Accepted
                }
                ActionKind::Ability2Press => {
                    self.ability_2_sequence = frame.sequence;
                    ActionResultCode::Accepted
                }
                ActionKind::RecallStart | ActionKind::RecallCancel | ActionKind::Interact => {
                    ActionResultCode::InvalidState
                }
            }
        } else {
            ActionResultCode::InvalidState
        };
        self.reliable_event(ReliableEvent::ActionResult {
            action_sequence: frame.sequence,
            code,
        })
    }

    /// Advances one server-owned 30 Hz tick and returns a 15 Hz snapshot every second tick.
    pub fn tick(&mut self) -> Result<Vec<SnapshotChunk>, SessionError> {
        let movement = MovementAction::try_from_milli(
            self.latest_input.movement_x_milli,
            self.latest_input.movement_y_milli,
        )?;
        let aim = AimDirection::new(SimulationVector::new(
            f32::from(self.latest_input.aim_x_milli),
            f32::from(self.latest_input.aim_y_milli),
        ))?;
        let step = self.arena.step(AuthorityInput {
            movement,
            aim,
            primary_held: self.latest_input.held_primary,
            primary_sequence: self.latest_input.primary_sequence,
            ability_1_sequence: self.ability_1_sequence,
            ability_2_sequence: self.ability_2_sequence,
        })?;
        if !step.tick.0.is_multiple_of(2) {
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
        let result = if let Some(cached) = self.mutation_results.get(&request.mutation_id) {
            cached.clone()
        } else {
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
            self.mutation_results
                .insert(request.mutation_id, result.clone());
            result
        };
        self.reliable_event(ReliableEvent::MutationResult(result))
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
        self.reliable_sequence = self
            .reliable_sequence
            .checked_add(1)
            .ok_or(SessionError::SequenceExhausted)?;
        Ok(ReliableEventFrame {
            sequence: self.reliable_sequence,
            server_tick: self.arena.player().combat.tick().0,
            event,
        })
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

fn mutation_error_code(error: &AuthorityError) -> MutationResultCode {
    match error {
        AuthorityError::Dead => MutationResultCode::Dead,
        AuthorityError::Ineligible => MutationResultCode::Ineligible,
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

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("content compilation failed: {0}")]
    Content(String),
    #[error("authoritative player identity is invalid")]
    InvalidPlayerIdentity,
    #[error("protocol message failed validation")]
    InvalidProtocolMessage,
    #[error("reliable action sequence {received} is not newer than {last}")]
    NonMonotonicAction { received: u32, last: u32 },
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
    }

    #[test]
    fn hostile_simulation_commits_death_and_closes_all_client_intent_seams() {
        let mut session =
            AuthoritativeSession::from_content_root(&content_root()).expect("session content");
        session
            .submit_input(&input(1, (0, 0), (1_000, 0), false))
            .unwrap();
        let mut saw_hostile_projectile = false;
        for _ in 0..5_000 {
            session.tick().expect("alive authority tick");
            saw_hostile_projectile |= session
                .arena()
                .snapshots()
                .unwrap()
                .iter()
                .any(|entity| entity.kind == AuthorityEntityKind::HostileProjectile);
            if matches!(session.arena().phase(), AuthorityPhase::Dead { .. }) {
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
        assert!(matches!(
            session.submit_action(&ActionFrame {
                sequence: 1,
                client_tick: 2,
                action: ActionKind::Ability2Press,
            }),
            Err(SessionError::NonMonotonicAction { .. })
        ));
        assert!(matches!(
            session.handle_reliable(WireMessage::InputFrame(input(1, (0, 0), (1_000, 0), false))),
            Err(SessionError::UnexpectedReliableMessage)
        ));
    }

    #[test]
    fn ineligible_pickup_request_is_nonmutating_and_idempotent() {
        let mut session = AuthoritativeSession::from_content_root_with_eligibility(
            &content_root(),
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
