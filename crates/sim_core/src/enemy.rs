//! Deterministic First Playable enemy roles for `GB-M01-03A` through `GB-M01-03C`.
//!
//! This module reconciles the GDD `SIM-010`/`SIM-011` and `COM-009` contracts, the exact
//! `CONT-FP-004` overrides, and the roadmap/completion-matrix ordering. It owns simulation facts
//! only: presentation, audio, health mutation, rewards, and Bevy entities are downstream concerns.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{DamageBand, DamageType, Tick};

pub const DROWNED_PILGRIM_ID: &str = "enemy.drowned_pilgrim";
pub const BELL_REED_ID: &str = "enemy.bell_reed";
pub const CHAIN_SENTRY_ID: &str = "enemy.chain_sentry";
pub const NORMAL_ENEMY_REWARD_TABLE_ID: &str = "reward.prototype.normal_enemy";

const SPAWN_TELEGRAPH_TICKS: u32 = 27;
const MILLI_TILES_PER_TILE: i32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnemyRole {
    Fodder,
    Pressure,
    Anchor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EchoMemoryFamily {
    ChargeOrContact,
    FanProjectile,
    RotatingProjectile,
    RadialProjectile,
    LaneOrBeam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Counterplay {
    Strafe,
    FollowGap,
    LeaveTelegraph,
    MoveWithRotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostileDisposition {
    ConsumeOnPlayerOrSolid,
    ExpireAtAuthoredEnd,
    OneContactHitPerCast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttackCastId(u64);

impl AttackCastId {
    pub const FIRST: Self = Self(1);

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn from_ordinal(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    fn checked_next(self) -> Result<Self, EnemyRuntimeError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(EnemyRuntimeError::CastIdOverflow)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AimVector {
    pub x: i32,
    pub y: i32,
}

impl AimVector {
    pub const EAST: Self = Self {
        x: MILLI_TILES_PER_TILE,
        y: 0,
    };

    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.x != 0 || self.y != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectileAttackDefinition {
    pub pattern_id: &'static str,
    pub projectile_count: u8,
    pub speed_milli_tiles_per_second: u32,
    pub radius_milli_tiles: u32,
    pub lifetime_ticks: u32,
    pub raw_damage: u32,
    pub damage_type: DamageType,
    pub damage_band: DamageBand,
    pub threat_cost: u32,
    pub memory_family: EchoMemoryFamily,
    pub counterplay: Counterplay,
    pub disposition: HostileDisposition,
    pub pierces_players: bool,
    pub maximum_active_instances: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneAttackDefinition {
    pub pattern_id: &'static str,
    pub lane_count: u8,
    pub width_milli_tiles: u32,
    pub active_ticks: u32,
    pub raw_damage: u32,
    pub damage_type: DamageType,
    pub damage_band: DamageBand,
    pub threat_cost_per_lane: u32,
    pub memory_family: EchoMemoryFamily,
    pub counterplay: Counterplay,
    pub disposition: HostileDisposition,
    pub maximum_active_instances: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrownedPilgrimDefinitionParameters {
    pub content_id: String,
    pub role: EnemyRole,
    pub health: u32,
    pub armor: u32,
    pub hurtbox_radius_milli_tiles: u32,
    pub movement_speed_milli_tiles_per_second: u32,
    pub aggro_radius_milli_tiles: u32,
    pub leash_radius_milli_tiles: u32,
    pub spawn_telegraph_ticks: u32,
    pub approach_distance_milli_tiles: u32,
    pub windup_ticks: u32,
    pub recover_ticks: u32,
    pub fan_offsets_degrees: [i16; 3],
    pub origin_offset_milli_tiles: u32,
    pub attack: ProjectileAttackDefinition,
    pub reward_table_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellReedDefinitionParameters {
    pub content_id: String,
    pub role: EnemyRole,
    pub health: u32,
    pub armor: u32,
    pub hurtbox_radius_milli_tiles: u32,
    pub movement_speed_milli_tiles_per_second: u32,
    pub aggro_radius_milli_tiles: u32,
    pub leash_radius_milli_tiles: u32,
    pub spawn_telegraph_ticks: u32,
    pub dormant_ticks: u32,
    pub cycle_ticks: u32,
    pub first_telegraph_ticks: u32,
    pub repeated_telegraph_ticks: u32,
    pub ring_index_count: u8,
    pub omitted_count: u8,
    pub omitted_start_advance: u8,
    pub attack: ProjectileAttackDefinition,
    pub reward_table_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainSentryDefinitionParameters {
    pub content_id: String,
    pub role: EnemyRole,
    pub health: u32,
    pub armor: u32,
    pub hurtbox_radius_milli_tiles: u32,
    pub movement_speed_milli_tiles_per_second: u32,
    pub aggro_radius_milli_tiles: u32,
    pub leash_radius_milli_tiles: u32,
    pub spawn_telegraph_ticks: u32,
    pub dormant_ticks: u32,
    pub cycle_ticks: u32,
    pub first_telegraph_ticks: u32,
    pub repeated_telegraph_ticks: u32,
    pub attack: LaneAttackDefinition,
    pub reward_table_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrownedPilgrimDefinition(DrownedPilgrimDefinitionParameters);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellReedDefinition(BellReedDefinitionParameters);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainSentryDefinition(ChainSentryDefinitionParameters);

macro_rules! exact_definition {
    ($name:expr, $actual:expr, $expected:expr) => {
        if $actual != $expected {
            return Err(EnemyDefinitionError::FirstPlayableDrift { enemy: $name });
        }
    };
}

impl DrownedPilgrimDefinition {
    pub fn new(
        parameters: DrownedPilgrimDefinitionParameters,
    ) -> Result<Self, EnemyDefinitionError> {
        exact_definition!(DROWNED_PILGRIM_ID, parameters, drowned_pilgrim_parameters());
        Ok(Self(parameters))
    }

    #[must_use]
    pub fn first_playable() -> Self {
        Self(drowned_pilgrim_parameters())
    }

    #[must_use]
    pub const fn parameters(&self) -> &DrownedPilgrimDefinitionParameters {
        &self.0
    }
}

impl BellReedDefinition {
    pub fn new(parameters: BellReedDefinitionParameters) -> Result<Self, EnemyDefinitionError> {
        exact_definition!(BELL_REED_ID, parameters, bell_reed_parameters());
        Ok(Self(parameters))
    }

    #[must_use]
    pub fn first_playable() -> Self {
        Self(bell_reed_parameters())
    }

    #[must_use]
    pub const fn parameters(&self) -> &BellReedDefinitionParameters {
        &self.0
    }
}

impl ChainSentryDefinition {
    pub fn new(parameters: ChainSentryDefinitionParameters) -> Result<Self, EnemyDefinitionError> {
        exact_definition!(CHAIN_SENTRY_ID, parameters, chain_sentry_parameters());
        Ok(Self(parameters))
    }

    #[must_use]
    pub fn first_playable() -> Self {
        Self(chain_sentry_parameters())
    }

    #[must_use]
    pub const fn parameters(&self) -> &ChainSentryDefinitionParameters {
        &self.0
    }
}

fn drowned_pilgrim_parameters() -> DrownedPilgrimDefinitionParameters {
    DrownedPilgrimDefinitionParameters {
        content_id: DROWNED_PILGRIM_ID.to_owned(),
        role: EnemyRole::Fodder,
        health: 85,
        armor: 0,
        hurtbox_radius_milli_tiles: 340,
        movement_speed_milli_tiles_per_second: 2_200,
        aggro_radius_milli_tiles: 10_000,
        leash_radius_milli_tiles: 12_000,
        spawn_telegraph_ticks: SPAWN_TELEGRAPH_TICKS,
        approach_distance_milli_tiles: 5_000,
        windup_ticks: 9,
        recover_ticks: 57,
        fan_offsets_degrees: [-15, 0, 15],
        origin_offset_milli_tiles: 450,
        attack: ProjectileAttackDefinition {
            pattern_id: "pattern.enemy.drowned_pilgrim.fan",
            projectile_count: 3,
            speed_milli_tiles_per_second: 5_500,
            radius_milli_tiles: 120,
            lifetime_ticks: 66,
            raw_damage: 8,
            damage_type: DamageType::Physical,
            damage_band: DamageBand::Chip,
            threat_cost: 3,
            memory_family: EchoMemoryFamily::FanProjectile,
            counterplay: Counterplay::Strafe,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players: false,
            maximum_active_instances: 6,
        },
        reward_table_id: NORMAL_ENEMY_REWARD_TABLE_ID.to_owned(),
    }
}

fn bell_reed_parameters() -> BellReedDefinitionParameters {
    BellReedDefinitionParameters {
        content_id: BELL_REED_ID.to_owned(),
        role: EnemyRole::Pressure,
        health: 130,
        armor: 2,
        hurtbox_radius_milli_tiles: 420,
        movement_speed_milli_tiles_per_second: 0,
        aggro_radius_milli_tiles: 11_000,
        leash_radius_milli_tiles: 12_000,
        spawn_telegraph_ticks: SPAWN_TELEGRAPH_TICKS,
        dormant_ticks: 15,
        cycle_ticks: 90,
        first_telegraph_ticks: 14,
        repeated_telegraph_ticks: 9,
        ring_index_count: 8,
        omitted_count: 2,
        omitted_start_advance: 3,
        attack: ProjectileAttackDefinition {
            pattern_id: "pattern.enemy.bell_reed.gap_ring",
            projectile_count: 6,
            speed_milli_tiles_per_second: 4_500,
            radius_milli_tiles: 130,
            lifetime_ticks: 90,
            raw_damage: 10,
            damage_type: DamageType::Veil,
            damage_band: DamageBand::Chip,
            threat_cost: 6,
            memory_family: EchoMemoryFamily::RadialProjectile,
            counterplay: Counterplay::FollowGap,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players: false,
            maximum_active_instances: 12,
        },
        reward_table_id: NORMAL_ENEMY_REWARD_TABLE_ID.to_owned(),
    }
}

fn chain_sentry_parameters() -> ChainSentryDefinitionParameters {
    ChainSentryDefinitionParameters {
        content_id: CHAIN_SENTRY_ID.to_owned(),
        role: EnemyRole::Anchor,
        health: 300,
        armor: 5,
        hurtbox_radius_milli_tiles: 550,
        movement_speed_milli_tiles_per_second: 0,
        aggro_radius_milli_tiles: 13_000,
        leash_radius_milli_tiles: 13_000,
        spawn_telegraph_ticks: SPAWN_TELEGRAPH_TICKS,
        dormant_ticks: 21,
        cycle_ticks: 135,
        first_telegraph_ticks: 24,
        repeated_telegraph_ticks: 20,
        attack: LaneAttackDefinition {
            pattern_id: "pattern.enemy.chain_sentry.cross_lanes",
            lane_count: 2,
            width_milli_tiles: 900,
            active_ticks: 11,
            raw_damage: 22,
            damage_type: DamageType::Physical,
            damage_band: DamageBand::Pressure,
            threat_cost_per_lane: 12,
            memory_family: EchoMemoryFamily::LaneOrBeam,
            counterplay: Counterplay::LeaveTelegraph,
            disposition: HostileDisposition::ExpireAtAuthoredEnd,
            maximum_active_instances: 2,
        },
        reward_table_id: NORMAL_ENEMY_REWARD_TABLE_ID.to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EnemyDefinitionError {
    #[error("{enemy} differs from the exact fp.1.0.0 CONT-FP-004 override")]
    FirstPlayableDrift { enemy: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnemyStateKind {
    SpawnTelegraph,
    Acquire,
    Approach,
    AttackWindup,
    Dormant,
    AttackTelegraph,
    Active,
    Recover,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnemyEvent {
    SpawnTelegraph {
        enemy_id: &'static str,
        ends_at: Tick,
    },
    StateChanged {
        enemy_id: &'static str,
        state: EnemyStateKind,
    },
    ApproachIntent {
        speed_milli_tiles_per_second: u32,
        target_delta: AimVector,
        stop_distance_milli_tiles: u32,
    },
    AimLocked {
        cast_id: AttackCastId,
        direction: AimVector,
        fires_at: Tick,
    },
    FanFired {
        cast_id: AttackCastId,
        direction: AimVector,
        offsets_degrees: [i16; 3],
        origin_offset_milli_tiles: u32,
        attack: ProjectileAttackDefinition,
    },
    RingTelegraph {
        cast_id: AttackCastId,
        omitted_indices: [u8; 2],
        fires_at: Tick,
    },
    RingFired {
        cast_id: AttackCastId,
        emitted_indices: [u8; 6],
        omitted_indices: [u8; 2],
        attack: ProjectileAttackDefinition,
    },
    LaneTelegraph {
        cast_id: AttackCastId,
        axes_degrees: [u16; 2],
        impacts_at: Tick,
        width_milli_tiles: u32,
        extends_to_arena_collision: bool,
    },
    LanesActivated {
        cast_id: AttackCastId,
        axes_degrees: [u16; 2],
        active_until: Tick,
        attack: LaneAttackDefinition,
    },
    LanesExpired {
        cast_id: AttackCastId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PilgrimTargetInput {
    pub present: bool,
    pub distance_milli_tiles: u32,
    pub delta: AimVector,
}

impl PilgrimTargetInput {
    pub const ABSENT: Self = Self {
        present: false,
        distance_milli_tiles: u32::MAX,
        delta: AimVector::EAST,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PilgrimState {
    Spawn {
        ends_at: Tick,
    },
    Acquire,
    Approach,
    Windup {
        cast_id: AttackCastId,
        fires_at: Tick,
        locked_aim: AimVector,
    },
    Recover {
        ends_at: Tick,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrownedPilgrimSimulation {
    definition: DrownedPilgrimDefinition,
    tick: Tick,
    state: PilgrimState,
    next_cast_id: AttackCastId,
    last_valid_aim: AimVector,
}

impl DrownedPilgrimSimulation {
    #[must_use]
    pub fn new(definition: DrownedPilgrimDefinition) -> Self {
        let ends_at = Tick(u64::from(definition.parameters().spawn_telegraph_ticks));
        Self {
            definition,
            tick: Tick(0),
            state: PilgrimState::Spawn { ends_at },
            next_cast_id: AttackCastId::FIRST,
            last_valid_aim: AimVector::EAST,
        }
    }

    #[must_use]
    pub fn first_playable() -> Self {
        Self::new(DrownedPilgrimDefinition::first_playable())
    }

    #[must_use]
    pub const fn definition(&self) -> &DrownedPilgrimDefinition {
        &self.definition
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub fn state(&self) -> EnemyStateKind {
        match &self.state {
            PilgrimState::Spawn { .. } => EnemyStateKind::SpawnTelegraph,
            PilgrimState::Acquire => EnemyStateKind::Acquire,
            PilgrimState::Approach => EnemyStateKind::Approach,
            PilgrimState::Windup { .. } => EnemyStateKind::AttackWindup,
            PilgrimState::Recover { .. } => EnemyStateKind::Recover,
        }
    }

    pub fn advance(
        &mut self,
        input: PilgrimTargetInput,
    ) -> Result<Vec<EnemyEvent>, EnemyRuntimeError> {
        let now = self.tick;
        let mut events = Vec::with_capacity(3);
        if now == Tick(0) {
            events.push(EnemyEvent::SpawnTelegraph {
                enemy_id: DROWNED_PILGRIM_ID,
                ends_at: Tick(u64::from(
                    self.definition.parameters().spawn_telegraph_ticks,
                )),
            });
        }
        match self.state.clone() {
            PilgrimState::Spawn { ends_at } if now >= ends_at => {
                self.state = PilgrimState::Acquire;
                events.push(state_event(DROWNED_PILGRIM_ID, EnemyStateKind::Acquire));
                self.acquire(input, now, &mut events)?;
            }
            PilgrimState::Acquire => self.acquire(input, now, &mut events)?,
            PilgrimState::Approach => self.approach(input, now, &mut events)?,
            PilgrimState::Windup {
                cast_id,
                fires_at,
                locked_aim,
            } if now >= fires_at => {
                events.push(EnemyEvent::FanFired {
                    cast_id,
                    direction: locked_aim,
                    offsets_degrees: self.definition.parameters().fan_offsets_degrees,
                    origin_offset_milli_tiles: self
                        .definition
                        .parameters()
                        .origin_offset_milli_tiles,
                    attack: self.definition.parameters().attack.clone(),
                });
                let ends_at = add_ticks(now, self.definition.parameters().recover_ticks)?;
                self.state = PilgrimState::Recover { ends_at };
                events.push(state_event(DROWNED_PILGRIM_ID, EnemyStateKind::Recover));
            }
            PilgrimState::Recover { ends_at } if now >= ends_at => {
                self.state = PilgrimState::Acquire;
                events.push(state_event(DROWNED_PILGRIM_ID, EnemyStateKind::Acquire));
            }
            _ => {}
        }
        self.tick = self
            .tick
            .checked_next()
            .ok_or(EnemyRuntimeError::TickOverflow)?;
        Ok(events)
    }

    fn acquire(
        &mut self,
        input: PilgrimTargetInput,
        now: Tick,
        events: &mut Vec<EnemyEvent>,
    ) -> Result<(), EnemyRuntimeError> {
        let p = self.definition.parameters();
        if !input.present || input.distance_milli_tiles > p.aggro_radius_milli_tiles {
            return Ok(());
        }
        if input.distance_milli_tiles <= p.approach_distance_milli_tiles {
            self.begin_windup(input.delta, now, events)?;
        } else {
            self.state = PilgrimState::Approach;
            events.push(state_event(DROWNED_PILGRIM_ID, EnemyStateKind::Approach));
            events.push(EnemyEvent::ApproachIntent {
                speed_milli_tiles_per_second: p.movement_speed_milli_tiles_per_second,
                target_delta: input.delta,
                stop_distance_milli_tiles: p.approach_distance_milli_tiles,
            });
        }
        Ok(())
    }

    fn approach(
        &mut self,
        input: PilgrimTargetInput,
        now: Tick,
        events: &mut Vec<EnemyEvent>,
    ) -> Result<(), EnemyRuntimeError> {
        let p = self.definition.parameters();
        if !input.present || input.distance_milli_tiles > p.leash_radius_milli_tiles {
            self.state = PilgrimState::Acquire;
            events.push(state_event(DROWNED_PILGRIM_ID, EnemyStateKind::Acquire));
        } else if input.distance_milli_tiles <= p.approach_distance_milli_tiles {
            self.begin_windup(input.delta, now, events)?;
        } else {
            events.push(EnemyEvent::ApproachIntent {
                speed_milli_tiles_per_second: p.movement_speed_milli_tiles_per_second,
                target_delta: input.delta,
                stop_distance_milli_tiles: p.approach_distance_milli_tiles,
            });
        }
        Ok(())
    }

    fn begin_windup(
        &mut self,
        requested_aim: AimVector,
        now: Tick,
        events: &mut Vec<EnemyEvent>,
    ) -> Result<(), EnemyRuntimeError> {
        if requested_aim.is_valid() {
            self.last_valid_aim = requested_aim;
        }
        let cast_id = self.next_cast_id;
        self.next_cast_id = cast_id.checked_next()?;
        let fires_at = add_ticks(now, self.definition.parameters().windup_ticks)?;
        self.state = PilgrimState::Windup {
            cast_id,
            fires_at,
            locked_aim: self.last_valid_aim,
        };
        events.push(state_event(
            DROWNED_PILGRIM_ID,
            EnemyStateKind::AttackWindup,
        ));
        events.push(EnemyEvent::AimLocked {
            cast_id,
            direction: self.last_valid_aim,
            fires_at,
        });
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReedState {
    Spawn {
        ends_at: Tick,
    },
    Dormant {
        ends_at: Tick,
    },
    Telegraph {
        cast_id: AttackCastId,
        omitted_start: u8,
        fires_at: Tick,
        cycle_started: Tick,
    },
    Recover {
        next_cycle: Tick,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellReedSimulation {
    definition: BellReedDefinition,
    tick: Tick,
    state: ReedState,
    next_cast_id: AttackCastId,
    next_omitted_start: u8,
}

impl BellReedSimulation {
    #[must_use]
    pub fn new(definition: BellReedDefinition) -> Self {
        let ends_at = Tick(u64::from(definition.parameters().spawn_telegraph_ticks));
        Self {
            state: ReedState::Spawn { ends_at },
            definition,
            tick: Tick(0),
            next_cast_id: AttackCastId::FIRST,
            next_omitted_start: 0,
        }
    }

    #[must_use]
    pub fn first_playable() -> Self {
        Self::new(BellReedDefinition::first_playable())
    }

    #[must_use]
    pub const fn definition(&self) -> &BellReedDefinition {
        &self.definition
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    pub fn advance(&mut self) -> Result<Vec<EnemyEvent>, EnemyRuntimeError> {
        let now = self.tick;
        let mut events = Vec::with_capacity(3);
        if now == Tick(0) {
            events.push(EnemyEvent::SpawnTelegraph {
                enemy_id: BELL_REED_ID,
                ends_at: Tick(u64::from(
                    self.definition.parameters().spawn_telegraph_ticks,
                )),
            });
        }
        match self.state.clone() {
            ReedState::Spawn { ends_at } if now >= ends_at => {
                let dormant_end = add_ticks(now, self.definition.parameters().dormant_ticks)?;
                self.state = ReedState::Dormant {
                    ends_at: dormant_end,
                };
                events.push(state_event(BELL_REED_ID, EnemyStateKind::Dormant));
            }
            ReedState::Dormant { ends_at } if now >= ends_at => {
                self.begin_ring(now, true, &mut events)?;
            }
            ReedState::Telegraph {
                cast_id,
                omitted_start,
                fires_at,
                cycle_started,
            } if now >= fires_at => {
                let omitted =
                    omitted_pair(omitted_start, self.definition.parameters().ring_index_count);
                events.push(EnemyEvent::RingFired {
                    cast_id,
                    emitted_indices: emitted_ring_indices(omitted_start),
                    omitted_indices: omitted,
                    attack: self.definition.parameters().attack.clone(),
                });
                self.next_omitted_start = (omitted_start
                    + self.definition.parameters().omitted_start_advance)
                    % self.definition.parameters().ring_index_count;
                self.state = ReedState::Recover {
                    next_cycle: add_ticks(cycle_started, self.definition.parameters().cycle_ticks)?,
                };
                events.push(state_event(BELL_REED_ID, EnemyStateKind::Recover));
            }
            ReedState::Recover { next_cycle } if now >= next_cycle => {
                self.begin_ring(now, false, &mut events)?;
            }
            _ => {}
        }
        self.tick = self
            .tick
            .checked_next()
            .ok_or(EnemyRuntimeError::TickOverflow)?;
        Ok(events)
    }

    fn begin_ring(
        &mut self,
        now: Tick,
        first: bool,
        events: &mut Vec<EnemyEvent>,
    ) -> Result<(), EnemyRuntimeError> {
        let cast_id = self.next_cast_id;
        self.next_cast_id = cast_id.checked_next()?;
        let telegraph_ticks = if first {
            self.definition.parameters().first_telegraph_ticks
        } else {
            self.definition.parameters().repeated_telegraph_ticks
        };
        let fires_at = add_ticks(now, telegraph_ticks)?;
        let omitted_start = self.next_omitted_start;
        self.state = ReedState::Telegraph {
            cast_id,
            omitted_start,
            fires_at,
            cycle_started: now,
        };
        events.push(state_event(BELL_REED_ID, EnemyStateKind::AttackTelegraph));
        events.push(EnemyEvent::RingTelegraph {
            cast_id,
            omitted_indices: omitted_pair(
                omitted_start,
                self.definition.parameters().ring_index_count,
            ),
            fires_at,
        });
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SentryState {
    Spawn {
        ends_at: Tick,
    },
    Dormant {
        ends_at: Tick,
    },
    Telegraph {
        cast_id: AttackCastId,
        diagonal: bool,
        impacts_at: Tick,
        cycle_started: Tick,
    },
    Active {
        cast_id: AttackCastId,
        cycle_started: Tick,
        active_until: Tick,
    },
    Recover {
        next_cycle: Tick,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainSentrySimulation {
    definition: ChainSentryDefinition,
    tick: Tick,
    state: SentryState,
    next_cast_id: AttackCastId,
    next_diagonal: bool,
    contacted_players: BTreeSet<u64>,
}

impl ChainSentrySimulation {
    #[must_use]
    pub fn new(definition: ChainSentryDefinition) -> Self {
        let ends_at = Tick(u64::from(definition.parameters().spawn_telegraph_ticks));
        Self {
            state: SentryState::Spawn { ends_at },
            definition,
            tick: Tick(0),
            next_cast_id: AttackCastId::FIRST,
            next_diagonal: false,
            contacted_players: BTreeSet::new(),
        }
    }

    #[must_use]
    pub fn first_playable() -> Self {
        Self::new(ChainSentryDefinition::first_playable())
    }

    #[must_use]
    pub const fn definition(&self) -> &ChainSentryDefinition {
        &self.definition
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Registers one player contact for the currently active two-lane hit group.
    ///
    /// Returns `true` exactly once per `(cast, player)` even if both lanes overlap the player.
    pub fn register_player_contact(
        &mut self,
        cast_id: AttackCastId,
        player_id: u64,
    ) -> Result<bool, EnemyRuntimeError> {
        let SentryState::Active {
            cast_id: active_cast,
            ..
        } = &self.state
        else {
            return Err(EnemyRuntimeError::LaneCastNotActive);
        };
        if *active_cast != cast_id {
            return Err(EnemyRuntimeError::WrongActiveCast {
                expected: *active_cast,
                received: cast_id,
            });
        }
        Ok(self.contacted_players.insert(player_id))
    }

    pub fn advance(&mut self) -> Result<Vec<EnemyEvent>, EnemyRuntimeError> {
        let now = self.tick;
        let mut events = Vec::with_capacity(3);
        if now == Tick(0) {
            events.push(EnemyEvent::SpawnTelegraph {
                enemy_id: CHAIN_SENTRY_ID,
                ends_at: Tick(u64::from(
                    self.definition.parameters().spawn_telegraph_ticks,
                )),
            });
        }
        match self.state.clone() {
            SentryState::Spawn { ends_at } if now >= ends_at => {
                let dormant_end = add_ticks(now, self.definition.parameters().dormant_ticks)?;
                self.state = SentryState::Dormant {
                    ends_at: dormant_end,
                };
                events.push(state_event(CHAIN_SENTRY_ID, EnemyStateKind::Dormant));
            }
            SentryState::Dormant { ends_at } if now >= ends_at => {
                self.begin_lanes(now, true, &mut events)?;
            }
            SentryState::Telegraph {
                cast_id,
                diagonal,
                impacts_at,
                cycle_started,
            } if now >= impacts_at => {
                let active_until =
                    add_ticks(now, self.definition.parameters().attack.active_ticks)?;
                self.contacted_players.clear();
                self.state = SentryState::Active {
                    cast_id,
                    cycle_started,
                    active_until,
                };
                events.push(state_event(CHAIN_SENTRY_ID, EnemyStateKind::Active));
                events.push(EnemyEvent::LanesActivated {
                    cast_id,
                    axes_degrees: lane_axes(diagonal),
                    active_until,
                    attack: self.definition.parameters().attack.clone(),
                });
            }
            SentryState::Active {
                cast_id,
                cycle_started,
                active_until,
            } if now >= active_until => {
                events.push(EnemyEvent::LanesExpired { cast_id });
                self.contacted_players.clear();
                self.state = SentryState::Recover {
                    next_cycle: add_ticks(cycle_started, self.definition.parameters().cycle_ticks)?,
                };
                events.push(state_event(CHAIN_SENTRY_ID, EnemyStateKind::Recover));
            }
            SentryState::Recover { next_cycle } if now >= next_cycle => {
                self.begin_lanes(now, false, &mut events)?;
            }
            _ => {}
        }
        self.tick = self
            .tick
            .checked_next()
            .ok_or(EnemyRuntimeError::TickOverflow)?;
        Ok(events)
    }

    fn begin_lanes(
        &mut self,
        now: Tick,
        first: bool,
        events: &mut Vec<EnemyEvent>,
    ) -> Result<(), EnemyRuntimeError> {
        let cast_id = self.next_cast_id;
        self.next_cast_id = cast_id.checked_next()?;
        let diagonal = self.next_diagonal;
        self.next_diagonal = !self.next_diagonal;
        let telegraph_ticks = if first {
            self.definition.parameters().first_telegraph_ticks
        } else {
            self.definition.parameters().repeated_telegraph_ticks
        };
        let impacts_at = add_ticks(now, telegraph_ticks)?;
        self.state = SentryState::Telegraph {
            cast_id,
            diagonal,
            impacts_at,
            cycle_started: now,
        };
        events.push(state_event(
            CHAIN_SENTRY_ID,
            EnemyStateKind::AttackTelegraph,
        ));
        events.push(EnemyEvent::LaneTelegraph {
            cast_id,
            axes_degrees: lane_axes(diagonal),
            impacts_at,
            width_milli_tiles: self.definition.parameters().attack.width_milli_tiles,
            extends_to_arena_collision: true,
        });
        Ok(())
    }
}

fn state_event(enemy_id: &'static str, state: EnemyStateKind) -> EnemyEvent {
    EnemyEvent::StateChanged { enemy_id, state }
}

fn add_ticks(tick: Tick, count: u32) -> Result<Tick, EnemyRuntimeError> {
    tick.0
        .checked_add(u64::from(count))
        .map(Tick)
        .ok_or(EnemyRuntimeError::TickOverflow)
}

fn omitted_pair(start: u8, count: u8) -> [u8; 2] {
    [start, (start + 1) % count]
}

fn emitted_ring_indices(omitted_start: u8) -> [u8; 6] {
    let omitted = omitted_pair(omitted_start, 8);
    let mut emitted = [0; 6];
    let mut output_index = 0;
    for index in 0..8 {
        if !omitted.contains(&index) {
            emitted[output_index] = index;
            output_index += 1;
        }
    }
    emitted
}

fn lane_axes(diagonal: bool) -> [u16; 2] {
    if diagonal { [45, 135] } else { [0, 90] }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EnemyRuntimeError {
    #[error("authoritative enemy tick overflow")]
    TickOverflow,
    #[error("enemy attack cast ID overflow")]
    CastIdOverflow,
    #[error("lane contact was submitted outside an active cast")]
    LaneCastNotActive,
    #[error("lane contact referenced cast {received:?}, but active cast is {expected:?}")]
    WrongActiveCast {
        expected: AttackCastId,
        received: AttackCastId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events_through_pilgrim(ticks: usize, input: PilgrimTargetInput) -> Vec<(Tick, EnemyEvent)> {
        let mut simulation = DrownedPilgrimSimulation::first_playable();
        let mut trace = Vec::new();
        for _ in 0..ticks {
            let tick = simulation.tick();
            trace.extend(
                simulation
                    .advance(input)
                    .expect("valid Pilgrim tick")
                    .into_iter()
                    .map(|event| (tick, event)),
            );
        }
        trace
    }

    #[test]
    fn simulations_own_and_expose_the_supplied_validated_definitions() {
        let pilgrim_definition = DrownedPilgrimDefinition::new(
            DrownedPilgrimDefinition::first_playable()
                .parameters()
                .clone(),
        )
        .expect("compiled Pilgrim definition");
        let pilgrim_expected = pilgrim_definition.clone();
        let pilgrim = DrownedPilgrimSimulation::new(pilgrim_definition);
        assert_eq!(pilgrim.definition(), &pilgrim_expected);
        assert_eq!(
            DrownedPilgrimSimulation::first_playable().definition(),
            &pilgrim_expected
        );

        let reed_definition =
            BellReedDefinition::new(BellReedDefinition::first_playable().parameters().clone())
                .expect("compiled Reed definition");
        let reed_expected = reed_definition.clone();
        let reed = BellReedSimulation::new(reed_definition);
        assert_eq!(reed.definition(), &reed_expected);
        assert_eq!(
            BellReedSimulation::first_playable().definition(),
            &reed_expected
        );

        let sentry_definition = ChainSentryDefinition::new(
            ChainSentryDefinition::first_playable().parameters().clone(),
        )
        .expect("compiled Sentry definition");
        let sentry_expected = sentry_definition.clone();
        let sentry = ChainSentrySimulation::new(sentry_definition);
        assert_eq!(sentry.definition(), &sentry_expected);
        assert_eq!(
            ChainSentrySimulation::first_playable().definition(),
            &sentry_expected
        );
    }

    #[test]
    fn exact_fp_definitions_reject_numeric_and_semantic_drift() {
        assert_eq!(AttackCastId::FIRST.get(), 1);
        let mut idle_pilgrim = DrownedPilgrimSimulation::first_playable();
        assert_eq!(idle_pilgrim.state(), EnemyStateKind::SpawnTelegraph);
        idle_pilgrim
            .advance(PilgrimTargetInput::ABSENT)
            .expect("idle spawn tick");

        let pilgrim = DrownedPilgrimDefinition::first_playable();
        assert_eq!(pilgrim.parameters().health, 85);
        assert_eq!(pilgrim.parameters().attack.lifetime_ticks, 66);
        let mut changed = pilgrim.parameters().clone();
        changed.health = 86;
        assert!(matches!(
            DrownedPilgrimDefinition::new(changed),
            Err(EnemyDefinitionError::FirstPlayableDrift {
                enemy: DROWNED_PILGRIM_ID
            })
        ));

        let reed = BellReedDefinition::first_playable();
        assert_eq!(reed.parameters().first_telegraph_ticks, 14);
        assert_eq!(reed.parameters().repeated_telegraph_ticks, 9);
        let mut changed = reed.parameters().clone();
        changed.attack.damage_type = DamageType::Physical;
        assert!(BellReedDefinition::new(changed).is_err());

        let sentry = ChainSentryDefinition::first_playable();
        assert_eq!(sentry.parameters().attack.active_ticks, 11);
        let mut changed = sentry.parameters().clone();
        changed.attack.disposition = HostileDisposition::ConsumeOnPlayerOrSolid;
        assert!(ChainSentryDefinition::new(changed).is_err());
    }

    #[test]
    fn pilgrim_cannot_attack_early_and_locks_aim_at_windup_start() {
        let initial = PilgrimTargetInput {
            present: true,
            distance_milli_tiles: 5_000,
            delta: AimVector { x: 3_000, y: 4_000 },
        };
        let trace = events_through_pilgrim(38, initial);
        assert!(
            !trace.iter().any(|(tick, event)| {
                tick.0 < 36 && matches!(event, EnemyEvent::FanFired { .. })
            })
        );
        let fired = trace
            .iter()
            .find(|(_, event)| matches!(event, EnemyEvent::FanFired { .. }))
            .expect("fan fire");
        assert_eq!(fired.0, Tick(36));
        let EnemyEvent::FanFired {
            direction,
            offsets_degrees,
            origin_offset_milli_tiles,
            attack,
            ..
        } = &fired.1
        else {
            unreachable!();
        };
        assert_eq!(*direction, initial.delta);
        assert_eq!(*offsets_degrees, [-15, 0, 15]);
        assert_eq!(*origin_offset_milli_tiles, 450);
        assert_eq!(attack.projectile_count, 3);
        assert_eq!(attack.speed_milli_tiles_per_second, 5_500);
        assert_eq!(attack.raw_damage, 8);
    }

    #[test]
    fn pilgrim_aggro_stop_and_leash_boundaries_are_inclusive_and_stable() {
        let at_aggro = PilgrimTargetInput {
            present: true,
            distance_milli_tiles: 10_000,
            delta: AimVector::EAST,
        };
        let trace = events_through_pilgrim(29, at_aggro);
        assert!(trace.iter().any(|(tick, event)| {
            *tick == Tick(28) && matches!(event, EnemyEvent::ApproachIntent { .. })
        }));

        let mut simulation = DrownedPilgrimSimulation::first_playable();
        for _ in 0..29 {
            simulation.advance(at_aggro).expect("advance");
        }
        let beyond_leash = PilgrimTargetInput {
            distance_milli_tiles: 12_001,
            ..at_aggro
        };
        let events = simulation.advance(beyond_leash).expect("leash transition");
        assert!(events.iter().any(|event| matches!(
            event,
            EnemyEvent::StateChanged {
                state: EnemyStateKind::Acquire,
                ..
            }
        )));
    }

    #[test]
    fn bell_reed_first_and_repeat_cycles_advance_exact_adjacent_gaps() {
        let mut simulation = BellReedSimulation::first_playable();
        let mut trace = Vec::new();
        for _ in 0..150 {
            let tick = simulation.tick();
            trace.extend(
                simulation
                    .advance()
                    .expect("Reed tick")
                    .into_iter()
                    .map(|event| (tick, event)),
            );
        }
        let telegraphs: Vec<_> = trace
            .iter()
            .filter_map(|(tick, event)| match event {
                EnemyEvent::RingTelegraph {
                    omitted_indices,
                    fires_at,
                    ..
                } => Some((*tick, *omitted_indices, *fires_at)),
                _ => None,
            })
            .collect();
        assert_eq!(
            telegraphs,
            vec![(Tick(42), [0, 1], Tick(56)), (Tick(132), [3, 4], Tick(141))]
        );
        let rings: Vec<_> = trace
            .iter()
            .filter_map(|(tick, event)| match event {
                EnemyEvent::RingFired {
                    emitted_indices,
                    omitted_indices,
                    ..
                } => Some((*tick, *emitted_indices, *omitted_indices)),
                _ => None,
            })
            .collect();
        assert_eq!(
            rings,
            vec![
                (Tick(56), [2, 3, 4, 5, 6, 7], [0, 1]),
                (Tick(141), [0, 1, 2, 5, 6, 7], [3, 4])
            ]
        );
    }

    #[test]
    fn chain_sentry_alternates_axes_and_contacts_once_per_player_per_cast() {
        let mut simulation = ChainSentrySimulation::first_playable();
        let mut trace = Vec::new();
        for _ in 0..220 {
            let tick = simulation.tick();
            let events = simulation.advance().expect("Sentry tick");
            for event in events {
                if let EnemyEvent::LanesActivated { cast_id, .. } = &event {
                    assert!(
                        simulation
                            .register_player_contact(*cast_id, 44)
                            .expect("first contact")
                    );
                    assert!(
                        !simulation
                            .register_player_contact(*cast_id, 44)
                            .expect("duplicate contact")
                    );
                    assert!(
                        simulation
                            .register_player_contact(*cast_id, 45)
                            .expect("other player")
                    );
                }
                trace.push((tick, event));
            }
        }
        let warnings: Vec<_> = trace
            .iter()
            .filter_map(|(tick, event)| match event {
                EnemyEvent::LaneTelegraph {
                    axes_degrees,
                    impacts_at,
                    width_milli_tiles,
                    extends_to_arena_collision,
                    ..
                } => Some((
                    *tick,
                    *axes_degrees,
                    *impacts_at,
                    *width_milli_tiles,
                    *extends_to_arena_collision,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            warnings,
            vec![
                (Tick(48), [0, 90], Tick(72), 900, true),
                (Tick(183), [45, 135], Tick(203), 900, true)
            ]
        );
        assert!(matches!(
            simulation.register_player_contact(AttackCastId::FIRST, 44),
            Err(EnemyRuntimeError::LaneCastNotActive | EnemyRuntimeError::WrongActiveCast { .. })
        ));
    }

    #[test]
    fn fixed_enemy_traces_are_bit_identical() {
        fn replay() -> [blake3::Hash; 3] {
            let pilgrim_input = PilgrimTargetInput {
                present: true,
                distance_milli_tiles: 5_000,
                delta: AimVector { x: -7, y: 11 },
            };
            let pilgrim = events_through_pilgrim(300, pilgrim_input);

            let mut reed_sim = BellReedSimulation::first_playable();
            let mut reed = Vec::new();
            for _ in 0..300 {
                let tick = reed_sim.tick();
                reed.extend(
                    reed_sim
                        .advance()
                        .expect("Reed replay")
                        .into_iter()
                        .map(|event| (tick, event)),
                );
            }

            let mut sentry_sim = ChainSentrySimulation::first_playable();
            let mut sentry = Vec::new();
            for _ in 0..400 {
                let tick = sentry_sim.tick();
                sentry.extend(
                    sentry_sim
                        .advance()
                        .expect("Sentry replay")
                        .into_iter()
                        .map(|event| (tick, event)),
                );
            }
            [trace_hash(&pilgrim), trace_hash(&reed), trace_hash(&sentry)]
        }

        let first = replay();
        assert_eq!(
            first[0].to_string(),
            "46c5efb70afe8f9237c12e850e0d9eae4e60a264c45f680d182531073a354e39"
        );
        assert_eq!(
            first[1].to_string(),
            "fa33c935fe16283c2366ee1d2456143367d3d4c49a4ba2a58cff0b3d27685182"
        );
        assert_eq!(
            first[2].to_string(),
            "54b45fbe860364beec6b2a34ee8571b4f3209c1c265f751bc4ffaf2dcab638a4"
        );
        assert_eq!(first, replay());
    }

    fn trace_hash(trace: &[(Tick, EnemyEvent)]) -> blake3::Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"gravebound-fp-enemy-trace-v1\0");
        for (tick, event) in trace {
            hasher.update(&tick.0.to_le_bytes());
            let debug = format!("{event:?}");
            hasher.update(&(debug.len() as u64).to_le_bytes());
            hasher.update(debug.as_bytes());
        }
        hasher.finalize()
    }
}
