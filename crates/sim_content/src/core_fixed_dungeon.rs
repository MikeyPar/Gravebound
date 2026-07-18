//! One-owner traversal for the exact M03 Bell Sepulcher B0-B6 chain.

use sim_core::{
    ArenaGeometry, CoreBargainKind, EnemyLabPlayer, EntityId, EntityIdAllocator, FixedRoomPhase,
    NormalWaveHandoff, Tick, TilePoint, tile_point_to_simulation,
};
use thiserror::Error;

use crate::{
    CoreB2FixedRoomSimulation, CoreB2FixedRoomStep, CoreB3FixedRoomSimulation, CoreB3FixedRoomStep,
    CoreDevelopmentEncounterRooms, CoreFixedRoomEncounterError, CoreFixedRoomEncounterPlan,
    CoreImmutableFixedRoomInput, CoreImmutableFixedRoomSimulation, CoreImmutableFixedRoomStep,
    compile_core_fixed_room_encounters,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoreFixedDungeonNode {
    BellVestibuleB0,
    BellCrossB1,
    BellNaveB2,
    BellKnightB3,
    BellRestB4,
    BellBridgeB5,
    CaldusArenaB6,
}

impl CoreFixedDungeonNode {
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::BellVestibuleB0 => "B0",
            Self::BellCrossB1 => "B1",
            Self::BellNaveB2 => "B2",
            Self::BellKnightB3 => "B3",
            Self::BellRestB4 => "B4",
            Self::BellBridgeB5 => "B5",
            Self::CaldusArenaB6 => "B6",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreFixedDungeonRestResolution {
    BargainSelected(CoreBargainKind),
    BargainRefused,
    NoOffer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreFixedDungeonRestReceipt {
    Committed,
    Replayed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreFixedDungeonAdvance {
    pub from: CoreFixedDungeonNode,
    pub to: CoreFixedDungeonNode,
    pub rest_resolution: Option<CoreFixedDungeonRestResolution>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreFixedDungeonRoomStep {
    B1(CoreImmutableFixedRoomStep),
    B2(CoreB2FixedRoomStep),
    B3(CoreB3FixedRoomStep),
    B5(CoreImmutableFixedRoomStep),
}

impl CoreFixedDungeonRoomStep {
    #[must_use]
    pub const fn node(&self) -> CoreFixedDungeonNode {
        match self {
            Self::B1(_) => CoreFixedDungeonNode::BellCrossB1,
            Self::B2(_) => CoreFixedDungeonNode::BellNaveB2,
            Self::B3(_) => CoreFixedDungeonNode::BellKnightB3,
            Self::B5(_) => CoreFixedDungeonNode::BellBridgeB5,
        }
    }

    #[must_use]
    pub const fn phase_after(&self) -> FixedRoomPhase {
        match self {
            Self::B1(step) | Self::B5(step) => step.phase_after,
            Self::B2(step) => step.phase_after,
            Self::B3(step) => step.phase_after,
        }
    }
}

#[derive(Debug, Clone)]
struct CoreFixedDungeonPlans {
    b1: CoreFixedRoomEncounterPlan,
    b2: CoreFixedRoomEncounterPlan,
    b3: CoreFixedRoomEncounterPlan,
    b5: CoreFixedRoomEncounterPlan,
}

impl CoreFixedDungeonPlans {
    fn compile(
        content: &CoreDevelopmentEncounterRooms,
        run_ordinal: u32,
    ) -> Result<Self, CoreFixedDungeonError> {
        let plans = compile_core_fixed_room_encounters(content, run_ordinal)?;
        let [b1, b2, b3, b5]: [CoreFixedRoomEncounterPlan; 4] = plans
            .try_into()
            .map_err(|_| CoreFixedDungeonError::DefinitionDrift)?;
        if [
            b1.node_id.as_str(),
            b2.node_id.as_str(),
            b3.node_id.as_str(),
            b5.node_id.as_str(),
        ] != ["B1", "B2", "B3", "B5"]
        {
            return Err(CoreFixedDungeonError::DefinitionDrift);
        }
        Ok(Self { b1, b2, b3, b5 })
    }

    const fn arena(&self, node: CoreFixedDungeonNode) -> Option<&ArenaGeometry> {
        match node {
            CoreFixedDungeonNode::BellCrossB1 => Some(self.b1.arena()),
            CoreFixedDungeonNode::BellNaveB2 => Some(self.b2.arena()),
            CoreFixedDungeonNode::BellKnightB3 => Some(self.b3.arena()),
            CoreFixedDungeonNode::BellBridgeB5 => Some(self.b5.arena()),
            CoreFixedDungeonNode::BellVestibuleB0
            | CoreFixedDungeonNode::BellRestB4
            | CoreFixedDungeonNode::CaldusArenaB6 => None,
        }
    }
}

#[derive(Debug, Clone)]
enum CoreFixedDungeonState {
    Vestibule(NormalWaveHandoff),
    B1(Box<CoreImmutableFixedRoomSimulation>),
    B2(Box<CoreB2FixedRoomSimulation>),
    B3(Box<CoreB3FixedRoomSimulation>),
    Rest {
        participant: NormalWaveHandoff,
        resolution: Option<CoreFixedDungeonRestResolution>,
    },
    B5(Box<CoreImmutableFixedRoomSimulation>),
    BossStaging(NormalWaveHandoff),
}

/// Owns one participant and one hostile-projectile allocator through the fixed M03 dungeon.
///
/// It never clones a participant to cross a room boundary. A room must reach `Cleared`, including
/// its complete quiet period, before its consuming handoff can construct the next room. B4 stores
/// an explicit authoritative Bargain outcome and makes exact retries idempotent.
#[derive(Debug, Clone)]
pub struct CoreFixedDungeonCombat {
    content: CoreDevelopmentEncounterRooms,
    plans: CoreFixedDungeonPlans,
    state: CoreFixedDungeonState,
}

impl CoreFixedDungeonCombat {
    pub fn new(
        content: CoreDevelopmentEncounterRooms,
        run_ordinal: u32,
        player: EnemyLabPlayer,
        hostile_projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CoreFixedDungeonError> {
        let plans = CoreFixedDungeonPlans::compile(&content, run_ordinal)?;
        Ok(Self {
            content,
            plans,
            state: CoreFixedDungeonState::Vestibule(NormalWaveHandoff {
                player,
                hostile_projectile_ids,
            }),
        })
    }

    pub fn from_handoff(
        content: CoreDevelopmentEncounterRooms,
        run_ordinal: u32,
        handoff: NormalWaveHandoff,
    ) -> Result<Self, CoreFixedDungeonError> {
        Self::new(
            content,
            run_ordinal,
            handoff.player,
            handoff.hostile_projectile_ids,
        )
    }

    #[must_use]
    pub const fn node(&self) -> CoreFixedDungeonNode {
        match self.state {
            CoreFixedDungeonState::Vestibule(_) => CoreFixedDungeonNode::BellVestibuleB0,
            CoreFixedDungeonState::B1(_) => CoreFixedDungeonNode::BellCrossB1,
            CoreFixedDungeonState::B2(_) => CoreFixedDungeonNode::BellNaveB2,
            CoreFixedDungeonState::B3(_) => CoreFixedDungeonNode::BellKnightB3,
            CoreFixedDungeonState::Rest { .. } => CoreFixedDungeonNode::BellRestB4,
            CoreFixedDungeonState::B5(_) => CoreFixedDungeonNode::BellBridgeB5,
            CoreFixedDungeonState::BossStaging(_) => CoreFixedDungeonNode::CaldusArenaB6,
        }
    }

    #[must_use]
    pub fn room_phase(&self) -> Option<FixedRoomPhase> {
        match &self.state {
            CoreFixedDungeonState::B1(room) | CoreFixedDungeonState::B5(room) => Some(room.phase()),
            CoreFixedDungeonState::B2(room) => Some(room.phase()),
            CoreFixedDungeonState::B3(room) => Some(room.phase()),
            CoreFixedDungeonState::Vestibule(_)
            | CoreFixedDungeonState::Rest { .. }
            | CoreFixedDungeonState::BossStaging(_) => None,
        }
    }

    #[must_use]
    pub fn arena(&self) -> Option<&ArenaGeometry> {
        self.plans.arena(self.node())
    }

    #[must_use]
    pub fn hostile_entity_ids(&self) -> Vec<EntityId> {
        let mut ids = match &self.state {
            CoreFixedDungeonState::B1(room) | CoreFixedDungeonState::B5(room) => {
                room.wave().map_or_else(Vec::new, |wave| {
                    wave.snapshots()
                        .into_iter()
                        .filter(|snapshot| snapshot.health.alive)
                        .map(|snapshot| snapshot.entity_id)
                        .collect()
                })
            }
            CoreFixedDungeonState::B2(room) => room
                .immutable_snapshots()
                .into_iter()
                .filter(|snapshot| snapshot.health.alive)
                .map(|snapshot| snapshot.entity_id)
                .chain(
                    room.authored_snapshots()
                        .into_iter()
                        .filter(|snapshot| snapshot.alive)
                        .map(|snapshot| snapshot.actor_id),
                )
                .collect(),
            CoreFixedDungeonState::B3(room) => room
                .snapshot()
                .filter(|snapshot| snapshot.alive)
                .map_or_else(Vec::new, |snapshot| vec![snapshot.actor_id]),
            CoreFixedDungeonState::Vestibule(_)
            | CoreFixedDungeonState::Rest { .. }
            | CoreFixedDungeonState::BossStaging(_) => Vec::new(),
        };
        ids.sort_unstable();
        ids
    }

    #[must_use]
    pub const fn rest_resolution(&self) -> Option<CoreFixedDungeonRestResolution> {
        match self.state {
            CoreFixedDungeonState::Rest { resolution, .. } => resolution,
            _ => None,
        }
    }

    pub fn resolve_rest(
        &mut self,
        resolution: CoreFixedDungeonRestResolution,
    ) -> Result<CoreFixedDungeonRestReceipt, CoreFixedDungeonError> {
        let CoreFixedDungeonState::Rest {
            resolution: stored, ..
        } = &mut self.state
        else {
            return Err(CoreFixedDungeonError::RestResolutionUnavailable);
        };
        match *stored {
            None => {
                *stored = Some(resolution);
                Ok(CoreFixedDungeonRestReceipt::Committed)
            }
            Some(existing) if existing == resolution => Ok(CoreFixedDungeonRestReceipt::Replayed),
            Some(_) => Err(CoreFixedDungeonError::RestResolutionConflict),
        }
    }

    pub fn step_room(
        &mut self,
        tick: Tick,
        input: &CoreImmutableFixedRoomInput,
    ) -> Result<CoreFixedDungeonRoomStep, CoreFixedDungeonError> {
        let mut staged = self.clone();
        let step = match &mut staged.state {
            CoreFixedDungeonState::B1(room) => {
                CoreFixedDungeonRoomStep::B1(room.step(tick, input)?)
            }
            CoreFixedDungeonState::B2(room) => {
                CoreFixedDungeonRoomStep::B2(room.step(tick, input)?)
            }
            CoreFixedDungeonState::B3(room) => {
                CoreFixedDungeonRoomStep::B3(room.step(tick, input)?)
            }
            CoreFixedDungeonState::B5(room) => {
                CoreFixedDungeonRoomStep::B5(room.step(tick, input)?)
            }
            CoreFixedDungeonState::Vestibule(_)
            | CoreFixedDungeonState::Rest { .. }
            | CoreFixedDungeonState::BossStaging(_) => {
                return Err(CoreFixedDungeonError::RoomStepUnavailable);
            }
        };
        *self = staged;
        Ok(step)
    }

    pub fn advance(&mut self) -> Result<CoreFixedDungeonAdvance, CoreFixedDungeonError> {
        let (staged, advance) = self.clone().advance_owned()?;
        *self = staged;
        Ok(advance)
    }

    pub fn into_boss_handoff(self) -> Result<NormalWaveHandoff, CoreFixedDungeonError> {
        match self.state {
            CoreFixedDungeonState::BossStaging(handoff) => Ok(handoff),
            _ => Err(CoreFixedDungeonError::BossHandoffUnavailable),
        }
    }

    fn advance_owned(self) -> Result<(Self, CoreFixedDungeonAdvance), CoreFixedDungeonError> {
        let Self {
            content,
            plans,
            state,
        } = self;
        let (state, advance) = match state {
            CoreFixedDungeonState::Vestibule(participant) => enter_b1(&plans, participant)?,
            CoreFixedDungeonState::B1(room) => enter_b2(&content, &plans, room)?,
            CoreFixedDungeonState::B2(room) => enter_b3(&content, &plans, room)?,
            CoreFixedDungeonState::B3(room) => enter_b4(room)?,
            CoreFixedDungeonState::Rest {
                participant,
                resolution,
            } => enter_b5(&plans, participant, resolution)?,
            CoreFixedDungeonState::B5(room) => enter_b6(room)?,
            CoreFixedDungeonState::BossStaging(_) => {
                return Err(CoreFixedDungeonError::AdvanceUnavailable {
                    node: CoreFixedDungeonNode::CaldusArenaB6,
                });
            }
        };
        Ok((
            Self {
                content,
                plans,
                state,
            },
            advance,
        ))
    }
}

type CoreFixedDungeonTransition = (CoreFixedDungeonState, CoreFixedDungeonAdvance);

fn enter_b1(
    plans: &CoreFixedDungeonPlans,
    participant: NormalWaveHandoff,
) -> Result<CoreFixedDungeonTransition, CoreFixedDungeonError> {
    let participant = relocate_participant(participant, plans.b1.arena().player_spawn);
    let room = CoreImmutableFixedRoomSimulation::new(
        plans.b1.clone(),
        participant.player,
        participant.hostile_projectile_ids,
    )?;
    Ok((
        CoreFixedDungeonState::B1(Box::new(room)),
        transition(
            CoreFixedDungeonNode::BellVestibuleB0,
            CoreFixedDungeonNode::BellCrossB1,
            None,
        ),
    ))
}

fn enter_b2(
    content: &CoreDevelopmentEncounterRooms,
    plans: &CoreFixedDungeonPlans,
    room: Box<CoreImmutableFixedRoomSimulation>,
) -> Result<CoreFixedDungeonTransition, CoreFixedDungeonError> {
    require_cleared(CoreFixedDungeonNode::BellCrossB1, room.phase())?;
    let participant = relocate_participant(room.into_handoff()?, plans.b2.arena().player_spawn);
    let room = CoreB2FixedRoomSimulation::new(
        plans.b2.clone(),
        content,
        participant.player,
        participant.hostile_projectile_ids,
    )?;
    Ok((
        CoreFixedDungeonState::B2(Box::new(room)),
        transition(
            CoreFixedDungeonNode::BellCrossB1,
            CoreFixedDungeonNode::BellNaveB2,
            None,
        ),
    ))
}

fn enter_b3(
    content: &CoreDevelopmentEncounterRooms,
    plans: &CoreFixedDungeonPlans,
    room: Box<CoreB2FixedRoomSimulation>,
) -> Result<CoreFixedDungeonTransition, CoreFixedDungeonError> {
    require_cleared(CoreFixedDungeonNode::BellNaveB2, room.phase())?;
    let participant = relocate_participant(room.into_handoff()?, plans.b3.arena().player_spawn);
    let room = CoreB3FixedRoomSimulation::new(
        plans.b3.clone(),
        content,
        participant.player,
        participant.hostile_projectile_ids,
    )?;
    Ok((
        CoreFixedDungeonState::B3(Box::new(room)),
        transition(
            CoreFixedDungeonNode::BellNaveB2,
            CoreFixedDungeonNode::BellKnightB3,
            None,
        ),
    ))
}

fn enter_b4(
    room: Box<CoreB3FixedRoomSimulation>,
) -> Result<CoreFixedDungeonTransition, CoreFixedDungeonError> {
    require_cleared(CoreFixedDungeonNode::BellKnightB3, room.phase())?;
    let participant = room.into_handoff()?;
    Ok((
        CoreFixedDungeonState::Rest {
            participant,
            resolution: None,
        },
        transition(
            CoreFixedDungeonNode::BellKnightB3,
            CoreFixedDungeonNode::BellRestB4,
            None,
        ),
    ))
}

fn enter_b5(
    plans: &CoreFixedDungeonPlans,
    participant: NormalWaveHandoff,
    resolution: Option<CoreFixedDungeonRestResolution>,
) -> Result<CoreFixedDungeonTransition, CoreFixedDungeonError> {
    let resolution = resolution.ok_or(CoreFixedDungeonError::RestResolutionRequired)?;
    let participant = relocate_participant(participant, plans.b5.arena().player_spawn);
    let room = CoreImmutableFixedRoomSimulation::new(
        plans.b5.clone(),
        participant.player,
        participant.hostile_projectile_ids,
    )?;
    Ok((
        CoreFixedDungeonState::B5(Box::new(room)),
        transition(
            CoreFixedDungeonNode::BellRestB4,
            CoreFixedDungeonNode::BellBridgeB5,
            Some(resolution),
        ),
    ))
}

/// Moves only the scene-local target position at an authored room boundary. All life-persistent
/// combat state and both identity allocators remain in the same moved allocation.
fn relocate_participant(mut participant: NormalWaveHandoff, spawn: TilePoint) -> NormalWaveHandoff {
    participant.player.target.position = tile_point_to_simulation(spawn);
    participant
}

fn enter_b6(
    room: Box<CoreImmutableFixedRoomSimulation>,
) -> Result<CoreFixedDungeonTransition, CoreFixedDungeonError> {
    require_cleared(CoreFixedDungeonNode::BellBridgeB5, room.phase())?;
    let participant = room.into_handoff()?;
    Ok((
        CoreFixedDungeonState::BossStaging(participant),
        transition(
            CoreFixedDungeonNode::BellBridgeB5,
            CoreFixedDungeonNode::CaldusArenaB6,
            None,
        ),
    ))
}

const fn transition(
    from: CoreFixedDungeonNode,
    to: CoreFixedDungeonNode,
    rest_resolution: Option<CoreFixedDungeonRestResolution>,
) -> CoreFixedDungeonAdvance {
    CoreFixedDungeonAdvance {
        from,
        to,
        rest_resolution,
    }
}

fn require_cleared(
    node: CoreFixedDungeonNode,
    phase: FixedRoomPhase,
) -> Result<(), CoreFixedDungeonError> {
    if phase == FixedRoomPhase::Cleared {
        Ok(())
    } else {
        Err(CoreFixedDungeonError::AdvanceUnavailable { node })
    }
}

#[derive(Debug, Error)]
pub enum CoreFixedDungeonError {
    #[error(transparent)]
    FixedRoom(#[from] CoreFixedRoomEncounterError),
    #[error("fixed-dungeon content differs from the exact B0-B6 contract")]
    DefinitionDrift,
    #[error("fixed-dungeon room step is unavailable outside B1, B2, B3, or B5")]
    RoomStepUnavailable,
    #[error("fixed-dungeon route cannot advance from {node:?}")]
    AdvanceUnavailable { node: CoreFixedDungeonNode },
    #[error("B4 requires a committed Bargain selection, refusal, or authoritative no-offer result")]
    RestResolutionRequired,
    #[error("B4 resolution is unavailable outside the rest room")]
    RestResolutionUnavailable,
    #[error("B4 resolution conflicts with the result already committed in this route")]
    RestResolutionConflict,
    #[error("Caldus handoff is available only after committed B5 completion")]
    BossHandoffUnavailable,
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, path::Path};

    use sim_core::{
        CollisionTarget, CombatStep, FriendlyProjectileSource, HostileTargetState, PlayerVitals,
        ProjectileCollision, RawDamageIntent, RawDamageIntentSource, RedTonicSimulation,
        SimulationVector, TonicBelt,
    };

    use super::*;

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn fixture() -> CoreFixedDungeonCombat {
        let root = content_root();
        let content = crate::load_core_development_encounter_rooms(&root).expect("room content");
        let (source, _) = crate::load_and_validate(&root).expect("FP source");
        let fixture = crate::first_playable_authority_combat_test(&source).expect("FP fixture");
        let definitions = fixture.definitions;
        let player = EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: EntityId::new(900).expect("player ID"),
                position: SimulationVector::new(3.0, 8.5),
                target_is_immune: false,
                resistance_basis_points: definitions.resistance_basis_points,
                additional_direct_damage_reductions_basis_points: Vec::new(),
                armor: definitions.starting_armor,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
            },
            consumables: RedTonicSimulation::new(
                definitions.red_tonic,
                PlayerVitals::new(definitions.maximum_health, definitions.maximum_health)
                    .expect("vitals"),
                TonicBelt::first_playable(),
            )
            .expect("tonic"),
            combat: definitions.combat,
        };
        CoreFixedDungeonCombat::new(
            content,
            1,
            player,
            EntityIdAllocator::starting_at(NonZeroU64::new(20_000).expect("projectile allocator")),
        )
        .expect("fixed dungeon")
    }

    fn room_input(
        tick: u64,
        crossed: bool,
        combat_step: CombatStep,
    ) -> CoreImmutableFixedRoomInput {
        let mut combat_step = combat_step;
        combat_step.tick = Tick(tick);
        CoreImmutableFixedRoomInput {
            crossed_activation_boundary: crossed,
            living_inside: 1,
            living_party_outside: 0,
            doorway_hurtbox_blocked: false,
            combat_step: Some(combat_step),
        }
    }

    fn lethal_step(targets: &[EntityId], tick: u64) -> CombatStep {
        let mut combat = CombatStep {
            tick: Tick(tick),
            ..CombatStep::default()
        };
        for (index, target) in targets.iter().copied().enumerate() {
            let projectile_id = EntityId::new(60_000 + u64::try_from(index).expect("index"))
                .expect("projectile ID");
            combat.collisions.push(ProjectileCollision {
                tick: Tick(tick),
                projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(target),
                final_position: SimulationVector::new(5.0, 5.0),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            });
            combat.raw_damage_intents.push(RawDamageIntent {
                tick: Tick(tick),
                projectile_id,
                source: RawDamageIntentSource::Primary,
                target,
                base_raw_damage: 10_000,
                multiplier_basis_points: 10_000,
                resolved_raw_damage: 10_000,
                contact_ordinal: 0,
            });
        }
        combat
    }

    fn clear_current_room(
        dungeon: &mut CoreFixedDungeonCombat,
        crossed_at: u64,
    ) -> (u64, Option<crate::CoreB3RewardHandoff>) {
        dungeon
            .step_room(
                Tick(crossed_at),
                &room_input(crossed_at, true, CombatStep::default()),
            )
            .expect("participant lock");
        let warning_at = crossed_at + 1;
        dungeon
            .step_room(
                Tick(warning_at),
                &room_input(warning_at, false, CombatStep::default()),
            )
            .expect("warning");
        let damage_available_after = match dungeon.node() {
            CoreFixedDungeonNode::BellNaveB2 => 57,
            CoreFixedDungeonNode::BellKnightB3 => 90,
            CoreFixedDungeonNode::BellCrossB1 | CoreFixedDungeonNode::BellBridgeB5 => 27,
            CoreFixedDungeonNode::BellVestibuleB0
            | CoreFixedDungeonNode::BellRestB4
            | CoreFixedDungeonNode::CaldusArenaB6 => panic!("not a combat room"),
        };
        for tick in warning_at + 1..warning_at + damage_available_after {
            dungeon
                .step_room(Tick(tick), &room_input(tick, false, CombatStep::default()))
                .expect("warning progression");
        }
        let clear_at = warning_at + damage_available_after;
        let targets = dungeon.hostile_entity_ids();
        assert!(!targets.is_empty());
        let clear = dungeon
            .step_room(
                Tick(clear_at),
                &room_input(clear_at, false, lethal_step(&targets, clear_at)),
            )
            .expect("authoritative clear");
        assert_eq!(
            clear.phase_after(),
            FixedRoomPhase::Quiet,
            "{} did not commit its clear",
            dungeon.node().id()
        );
        let reward = match clear {
            CoreFixedDungeonRoomStep::B3(step) => step.reward_handoff,
            CoreFixedDungeonRoomStep::B1(_)
            | CoreFixedDungeonRoomStep::B2(_)
            | CoreFixedDungeonRoomStep::B5(_) => None,
        };
        for tick in clear_at + 1..clear_at + 60 {
            dungeon
                .step_room(Tick(tick), &room_input(tick, false, CombatStep::default()))
                .expect("quiet progression");
        }
        let doors_at = clear_at + 60;
        let opened = dungeon
            .step_room(
                Tick(doors_at),
                &room_input(doors_at, false, CombatStep::default()),
            )
            .expect("doors open");
        assert_eq!(opened.phase_after(), FixedRoomPhase::Cleared);
        (doors_at, reward)
    }

    #[test]
    fn one_participant_crosses_b0_through_b6_without_early_escape_or_identity_reuse() {
        let mut dungeon = fixture();
        assert_eq!(dungeon.node(), CoreFixedDungeonNode::BellVestibuleB0);
        assert!(dungeon.arena().is_none());
        assert!(matches!(
            dungeon.step_room(Tick(0), &room_input(0, false, CombatStep::default())),
            Err(CoreFixedDungeonError::RoomStepUnavailable)
        ));

        dungeon.advance().expect("enter B1");
        assert_eq!(dungeon.node(), CoreFixedDungeonNode::BellCrossB1);
        assert!(dungeon.arena().is_some());
        assert!(matches!(
            dungeon.advance(),
            Err(CoreFixedDungeonError::AdvanceUnavailable {
                node: CoreFixedDungeonNode::BellCrossB1
            })
        ));
        let (b1_done, _) = clear_current_room(&mut dungeon, 1);
        dungeon.advance().expect("enter B2");

        let (b2_done, _) = clear_current_room(&mut dungeon, b1_done + 1);
        dungeon.advance().expect("enter B3");

        let (b3_done, reward) = clear_current_room(&mut dungeon, b2_done + 1);
        let reward = reward.expect("B3 reward handoff");
        assert_eq!(reward.reward_profile_id, "reward.miniboss_t1");
        dungeon.advance().expect("enter B4");
        assert_eq!(dungeon.node(), CoreFixedDungeonNode::BellRestB4);
        assert!(matches!(
            dungeon.advance(),
            Err(CoreFixedDungeonError::RestResolutionRequired)
        ));
        let resolution = CoreFixedDungeonRestResolution::BargainRefused;
        assert_eq!(
            dungeon.resolve_rest(resolution).expect("resolve B4"),
            CoreFixedDungeonRestReceipt::Committed
        );
        assert_eq!(
            dungeon.resolve_rest(resolution).expect("replay B4"),
            CoreFixedDungeonRestReceipt::Replayed
        );
        assert!(matches!(
            dungeon.resolve_rest(CoreFixedDungeonRestResolution::NoOffer),
            Err(CoreFixedDungeonError::RestResolutionConflict)
        ));
        let entered_b5 = dungeon.advance().expect("enter B5");
        assert_eq!(entered_b5.rest_resolution, Some(resolution));

        let (_, _) = clear_current_room(&mut dungeon, b3_done + 1);
        dungeon.advance().expect("enter B6 staging");
        assert_eq!(dungeon.node(), CoreFixedDungeonNode::CaldusArenaB6);
        assert!(matches!(
            dungeon.advance(),
            Err(CoreFixedDungeonError::AdvanceUnavailable {
                node: CoreFixedDungeonNode::CaldusArenaB6
            })
        ));
        let handoff = dungeon.into_boss_handoff().expect("Caldus handoff");
        assert_eq!(
            handoff.player.target.entity_id,
            EntityId::new(900).expect("player")
        );
        assert!(handoff.hostile_projectile_ids.peek().get() >= 20_000);
    }

    #[test]
    fn construction_rejects_zero_run_ordinal_and_boss_handoff_is_phase_bound() {
        let dungeon = fixture();
        assert!(matches!(
            dungeon.clone().into_boss_handoff(),
            Err(CoreFixedDungeonError::BossHandoffUnavailable)
        ));
        let CoreFixedDungeonState::Vestibule(handoff) = dungeon.state else {
            panic!("fixture starts at B0");
        };
        assert!(CoreFixedDungeonCombat::from_handoff(dungeon.content, 0, handoff).is_err());
    }

    #[test]
    fn room_entry_relocates_the_same_participant_and_projectile_allocator() {
        let dungeon = fixture();
        let CoreFixedDungeonState::Vestibule(participant) = dungeon.state else {
            panic!("fixture starts at B0");
        };
        let participant_id = participant.player.target.entity_id;
        let next_projectile_id = participant.hostile_projectile_ids.peek();
        let spawn = TilePoint::new(1_750, 9_250);

        let relocated = relocate_participant(participant, spawn);

        assert_eq!(relocated.player.target.entity_id, participant_id);
        assert_eq!(relocated.hostile_projectile_ids.peek(), next_projectile_id);
        assert_eq!(
            relocated.player.target.position,
            SimulationVector::new(1.75, 9.25)
        );
    }
}
