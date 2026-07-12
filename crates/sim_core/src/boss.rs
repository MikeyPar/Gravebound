//! Exact deterministic Bell Proctor benchmark boss scheduler (`GB-M01-04B`/`04C`).
//!
//! Health mutation, projectile simulation, damage application, rendering, and rewards are
//! downstream. This module owns the immutable FP definition, phase scheduler, and stable events.

use thiserror::Error;

use crate::{
    AimVector, Counterplay, DamageBand, DamageType, EchoMemoryFamily, HostileDisposition,
    LaneAttackDefinition, ProjectileAttackDefinition, Tick,
};

pub const BELL_PROCTOR_ID: &str = "boss.prototype.bell_proctor";
pub const BELL_PROCTOR_REWARD_ID: &str = "reward.prototype.boss";
pub const BELL_PROCTOR_FAN_ID: &str = "pattern.prototype.bell_proctor.aimed_fan";
pub const BELL_PROCTOR_RING_ID: &str = "pattern.prototype.bell_proctor.gap_ring";
pub const BELL_PROCTOR_CROSS_ID: &str = "pattern.prototype.bell_proctor.cross_lanes";

const BASIS_POINTS: u32 = 10_000;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BossCastId(u64);

impl BossCastId {
    pub const FIRST: Self = Self(1);

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    fn allocate(next: &mut Self) -> Result<Self, BossRuntimeError> {
        let allocated = *next;
        next.0 = next
            .0
            .checked_add(1)
            .ok_or(BossRuntimeError::CastIdOverflow)?;
        Ok(allocated)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BellProctorPhase {
    Phase1,
    Phase2,
    Phase3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BossCueKind {
    Fan,
    Ring,
    RingPreviewA,
    RingPreviewB,
    Cross,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BossTimelineCue {
    pub kind: BossCueKind,
    pub starts_at_offset_ticks: u32,
    pub resolves_at_offset_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellProctorDefinitionParameters {
    pub content_id: String,
    pub health: u32,
    pub armor: u32,
    pub hurtbox_radius_milli_tiles: u32,
    pub position_x_milli_tiles: i32,
    pub position_y_milli_tiles: i32,
    pub target_solo_duration_min_ticks: u32,
    pub target_solo_duration_max_ticks: u32,
    pub soft_enrage_ticks: u32,
    pub introduction_ticks: u32,
    pub break_ticks: u32,
    pub break_received_damage_multiplier_basis_points: u32,
    pub soft_enrage_downtime_multiplier_basis_points: u32,
    pub phase1_loop_ticks: u32,
    pub phase2_loop_ticks: u32,
    pub phase3_loop_ticks: u32,
    pub phase3_low_health_loop_ticks: u32,
    pub phase_two_health: u32,
    pub phase_three_health: u32,
    pub low_health_restart: u32,
    pub fan_offsets_degrees: [i16; 5],
    pub ring_index_count: u8,
    pub ring_omitted_count: u8,
    pub ring_gap_advance: u8,
    pub phase3_second_gap_advance: u8,
    pub ring_preview_ticks: u32,
    pub fan: ProjectileAttackDefinition,
    pub ring: ProjectileAttackDefinition,
    pub cross: LaneAttackDefinition,
    pub phase1_timeline: Vec<BossTimelineCue>,
    pub phase2_timeline: Vec<BossTimelineCue>,
    pub phase3_timeline: Vec<BossTimelineCue>,
    pub reward_table_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellProctorDefinition(BellProctorDefinitionParameters);

impl BellProctorDefinition {
    pub fn new(parameters: BellProctorDefinitionParameters) -> Result<Self, BossDefinitionError> {
        validate_cross_exclusion(&parameters)?;
        if parameters != bell_proctor_parameters() {
            return Err(BossDefinitionError::FirstPlayableDrift);
        }
        Ok(Self(parameters))
    }

    #[must_use]
    pub fn first_playable() -> Self {
        Self(bell_proctor_parameters())
    }

    #[must_use]
    pub const fn parameters(&self) -> &BellProctorDefinitionParameters {
        &self.0
    }
}

fn bell_proctor_parameters() -> BellProctorDefinitionParameters {
    BellProctorDefinitionParameters {
        content_id: BELL_PROCTOR_ID.to_owned(),
        health: 3_000,
        armor: 4,
        hurtbox_radius_milli_tiles: 650,
        position_x_milli_tiles: 24_000,
        position_y_milli_tiles: 12_000,
        target_solo_duration_min_ticks: 2_250,
        target_solo_duration_max_ticks: 3_300,
        soft_enrage_ticks: 5_400,
        introduction_ticks: 60,
        break_ticks: 90,
        break_received_damage_multiplier_basis_points: 12_000,
        soft_enrage_downtime_multiplier_basis_points: 8_500,
        phase1_loop_ticks: 216,
        phase2_loop_ticks: 300,
        phase3_loop_ticks: 300,
        phase3_low_health_loop_ticks: 270,
        phase_two_health: 2_100,
        phase_three_health: 1_050,
        low_health_restart: 600,
        fan_offsets_degrees: [-20, -10, 0, 10, 20],
        ring_index_count: 16,
        ring_omitted_count: 4,
        ring_gap_advance: 5,
        phase3_second_gap_advance: 4,
        ring_preview_ticks: 15,
        fan: ProjectileAttackDefinition {
            pattern_id: BELL_PROCTOR_FAN_ID,
            projectile_count: 5,
            speed_milli_tiles_per_second: 6_000,
            radius_milli_tiles: 120,
            lifetime_ticks: 90,
            raw_damage: 12,
            damage_type: DamageType::Veil,
            damage_band: DamageBand::Chip,
            threat_cost: 5,
            memory_family: EchoMemoryFamily::FanProjectile,
            counterplay: Counterplay::Strafe,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players: false,
            maximum_active_instances: 10,
        },
        ring: ProjectileAttackDefinition {
            pattern_id: BELL_PROCTOR_RING_ID,
            projectile_count: 12,
            speed_milli_tiles_per_second: 4_500,
            radius_milli_tiles: 130,
            lifetime_ticks: 120,
            raw_damage: 15,
            damage_type: DamageType::Veil,
            damage_band: DamageBand::Pressure,
            threat_cost: 12,
            memory_family: EchoMemoryFamily::RadialProjectile,
            counterplay: Counterplay::FollowGap,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players: false,
            maximum_active_instances: 24,
        },
        cross: LaneAttackDefinition {
            pattern_id: BELL_PROCTOR_CROSS_ID,
            lane_count: 2,
            width_milli_tiles: 1_000,
            active_ticks: 15,
            raw_damage: 28,
            damage_type: DamageType::Physical,
            damage_band: DamageBand::Major,
            threat_cost_per_lane: 12,
            memory_family: EchoMemoryFamily::LaneOrBeam,
            counterplay: Counterplay::LeaveTelegraph,
            disposition: HostileDisposition::ExpireAtAuthoredEnd,
            maximum_active_instances: 2,
        },
        phase1_timeline: vec![
            cue(BossCueKind::Fan, 0, 12),
            cue(BossCueKind::Fan, 72, 84),
            cue(BossCueKind::Ring, 168, 188),
        ],
        phase2_timeline: vec![
            cue(BossCueKind::Fan, 0, 12),
            cue(BossCueKind::Fan, 72, 84),
            cue(BossCueKind::Ring, 126, 146),
            cue(BossCueKind::Cross, 210, 237),
        ],
        phase3_timeline: vec![
            cue(BossCueKind::RingPreviewA, 0, 27),
            cue(BossCueKind::RingPreviewB, 30, 54),
            cue(BossCueKind::Fan, 120, 132),
            cue(BossCueKind::Cross, 195, 222),
            cue(BossCueKind::Fan, 252, 264),
        ],
        reward_table_id: BELL_PROCTOR_REWARD_ID.to_owned(),
    }
}

const fn cue(kind: BossCueKind, starts: u32, resolves: u32) -> BossTimelineCue {
    BossTimelineCue {
        kind,
        starts_at_offset_ticks: starts,
        resolves_at_offset_ticks: resolves,
    }
}

fn validate_cross_exclusion(
    parameters: &BellProctorDefinitionParameters,
) -> Result<(), BossDefinitionError> {
    for timeline in [&parameters.phase2_timeline, &parameters.phase3_timeline] {
        for cross in timeline.iter().filter(|cue| cue.kind == BossCueKind::Cross) {
            for other in timeline.iter().filter(|cue| {
                matches!(
                    cue.kind,
                    BossCueKind::Fan
                        | BossCueKind::Ring
                        | BossCueKind::RingPreviewA
                        | BossCueKind::RingPreviewB
                )
            }) {
                if cross
                    .resolves_at_offset_ticks
                    .abs_diff(other.resolves_at_offset_ticks)
                    < 15
                {
                    return Err(BossDefinitionError::CrossImpactExclusion);
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BossInput {
    pub current_health: u32,
    pub target_aim: AimVector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellProctorStateKind {
    Active(BellProctorPhase),
    Break { entering: BellProctorPhase },
    Defeated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BossEvent {
    PhaseStarted {
        tick: Tick,
        phase: BellProctorPhase,
        loop_ordinal: u32,
    },
    PhaseThresholdCrossed {
        tick: Tick,
        from: BellProctorPhase,
        to: BellProctorPhase,
    },
    TimelineCancelled {
        tick: Tick,
        from: BellProctorPhase,
        cancellation_ordinal: u8,
        cancelled_pending_casts: u32,
    },
    HostileProjectilesCleared {
        tick: Tick,
    },
    BreakStarted {
        tick: Tick,
        entering: BellProctorPhase,
        ends_at: Tick,
        received_damage_multiplier_basis_points: u32,
    },
    BreakEnded {
        tick: Tick,
        phase: BellProctorPhase,
    },
    FanTelegraph {
        tick: Tick,
        cast_id: BossCastId,
        locked_aim: AimVector,
        fires_at: Tick,
        offsets_degrees: [i16; 5],
    },
    FanFired {
        tick: Tick,
        cast_id: BossCastId,
        locked_aim: AimVector,
        offsets_degrees: [i16; 5],
        attack: ProjectileAttackDefinition,
    },
    RingTelegraph {
        tick: Tick,
        cast_id: BossCastId,
        omitted_indices: [u8; 4],
        fires_at: Tick,
    },
    RingPreview {
        tick: Tick,
        cast_id: BossCastId,
        sequence_index: u8,
        omitted_indices: [u8; 4],
        preview_ends_at: Tick,
        fires_at: Tick,
    },
    RingFired {
        tick: Tick,
        cast_id: BossCastId,
        emitted_indices: Vec<u8>,
        omitted_indices: [u8; 4],
        attack: ProjectileAttackDefinition,
    },
    CrossTelegraph {
        tick: Tick,
        cast_id: BossCastId,
        axes_degrees: [u16; 2],
        impacts_at: Tick,
        width_milli_tiles: u32,
    },
    CrossActivated {
        tick: Tick,
        cast_id: BossCastId,
        axes_degrees: [u16; 2],
        active_until: Tick,
        attack: LaneAttackDefinition,
    },
    CrossExpired {
        tick: Tick,
        cast_id: BossCastId,
    },
    LoopRestarted {
        tick: Tick,
        phase: BellProctorPhase,
        loop_ordinal: u32,
        low_health_restart: bool,
        soft_enraged: bool,
    },
    SoftEnrageStarted {
        tick: Tick,
    },
    BossDefeated {
        tick: Tick,
        reward_table_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingCastKind {
    Fan { aim: AimVector },
    Ring { omitted: [u8; 4] },
    Cross { axes: [u16; 2] },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingCast {
    id: BossCastId,
    resolves_at: Tick,
    kind: PendingCastKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveCross {
    id: BossCastId,
    expires_at: Tick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BossState {
    Active {
        phase: BellProctorPhase,
        loop_started: Tick,
        loop_ordinal: u32,
    },
    Break {
        entering: BellProctorPhase,
        ends_at: Tick,
    },
    Defeated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellProctorSimulation {
    definition: BellProctorDefinition,
    tick: Tick,
    health: u32,
    state: BossState,
    next_cast_id: BossCastId,
    pending: Vec<PendingCast>,
    active_crosses: Vec<ActiveCross>,
    next_ring_gap_start: u8,
    next_cross_diagonal: bool,
    cancellation_count: u8,
    soft_enrage_announced: bool,
    initial_phase_announced: bool,
}

impl BellProctorSimulation {
    #[must_use]
    pub fn new(definition: BellProctorDefinition) -> Self {
        let health = definition.parameters().health;
        Self {
            definition,
            tick: Tick(0),
            health,
            state: BossState::Active {
                phase: BellProctorPhase::Phase1,
                loop_started: Tick(0),
                loop_ordinal: 1,
            },
            next_cast_id: BossCastId::FIRST,
            pending: Vec::new(),
            active_crosses: Vec::new(),
            next_ring_gap_start: 0,
            next_cross_diagonal: false,
            cancellation_count: 0,
            soft_enrage_announced: false,
            initial_phase_announced: false,
        }
    }

    #[must_use]
    pub fn first_playable() -> Self {
        Self::new(BellProctorDefinition::first_playable())
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn health(&self) -> u32 {
        self.health
    }

    #[must_use]
    pub fn state(&self) -> BellProctorStateKind {
        match self.state {
            BossState::Active { phase, .. } => BellProctorStateKind::Active(phase),
            BossState::Break { entering, .. } => BellProctorStateKind::Break { entering },
            BossState::Defeated => BellProctorStateKind::Defeated,
        }
    }

    #[must_use]
    pub fn received_damage_multiplier_basis_points(&self) -> u32 {
        if matches!(self.state, BossState::Break { .. }) {
            self.definition
                .parameters()
                .break_received_damage_multiplier_basis_points
        } else {
            BASIS_POINTS
        }
    }

    pub fn advance(&mut self, input: BossInput) -> Result<Vec<BossEvent>, BossRuntimeError> {
        if input.current_health > self.health || input.current_health > self.definition.0.health {
            return Err(BossRuntimeError::HealthIncreased);
        }
        if !input.target_aim.is_valid() {
            return Err(BossRuntimeError::InvalidAim);
        }
        let mut staged = self.clone();
        let events = staged.advance_staged(input)?;
        *self = staged;
        Ok(events)
    }

    fn advance_staged(&mut self, input: BossInput) -> Result<Vec<BossEvent>, BossRuntimeError> {
        let mut events = Vec::new();
        self.health = input.current_health;
        if self.health == 0 {
            if !matches!(self.state, BossState::Defeated) {
                self.pending.clear();
                self.active_crosses.clear();
                self.state = BossState::Defeated;
                events.push(BossEvent::HostileProjectilesCleared { tick: self.tick });
                events.push(BossEvent::BossDefeated {
                    tick: self.tick,
                    reward_table_id: self.definition.0.reward_table_id.clone(),
                });
            }
            self.increment_tick()?;
            return Ok(events);
        }
        if matches!(self.state, BossState::Defeated) {
            return Err(BossRuntimeError::DefeatedBossAdvanced);
        }

        self.announce_soft_enrage(&mut events);
        self.expire_crosses(&mut events);

        match self.state.clone() {
            BossState::Break { entering, ends_at } if self.tick >= ends_at => {
                events.push(BossEvent::BreakEnded {
                    tick: self.tick,
                    phase: entering,
                });
                self.state = BossState::Active {
                    phase: entering,
                    loop_started: self.tick,
                    loop_ordinal: 1,
                };
                events.push(BossEvent::PhaseStarted {
                    tick: self.tick,
                    phase: entering,
                    loop_ordinal: 1,
                });
                self.schedule_current_tick(input.target_aim, &mut events)?;
                self.resolve_pending(&mut events)?;
            }
            BossState::Break { .. } => {}
            BossState::Active {
                phase,
                loop_started,
                loop_ordinal,
            } => {
                if let Some(next) = self.required_phase_transition(phase) {
                    self.begin_break(phase, next, &mut events)?;
                } else {
                    if !self.initial_phase_announced {
                        self.initial_phase_announced = true;
                        events.push(BossEvent::PhaseStarted {
                            tick: self.tick,
                            phase,
                            loop_ordinal,
                        });
                    }
                    self.schedule_current_tick(input.target_aim, &mut events)?;
                    self.resolve_pending(&mut events)?;
                    self.maybe_restart_loop(
                        phase,
                        loop_started,
                        loop_ordinal,
                        input.target_aim,
                        &mut events,
                    )?;
                }
            }
            BossState::Defeated => unreachable!(),
        }
        self.increment_tick()?;
        Ok(events)
    }

    fn required_phase_transition(&self, phase: BellProctorPhase) -> Option<BellProctorPhase> {
        match phase {
            BellProctorPhase::Phase1 if self.health <= self.definition.0.phase_two_health => {
                Some(BellProctorPhase::Phase2)
            }
            BellProctorPhase::Phase2 if self.health <= self.definition.0.phase_three_health => {
                Some(BellProctorPhase::Phase3)
            }
            _ => None,
        }
    }

    fn begin_break(
        &mut self,
        from: BellProctorPhase,
        to: BellProctorPhase,
        events: &mut Vec<BossEvent>,
    ) -> Result<(), BossRuntimeError> {
        self.cancellation_count = self
            .cancellation_count
            .checked_add(1)
            .ok_or(BossRuntimeError::CancellationOverflow)?;
        let cancelled_pending_casts = u32::try_from(self.pending.len())
            .map_err(|_| BossRuntimeError::PendingCastCountOverflow)?;
        self.pending.clear();
        self.active_crosses.clear();
        let ends_at = add_ticks(self.tick, self.definition.0.break_ticks)?;
        self.state = BossState::Break {
            entering: to,
            ends_at,
        };
        events.push(BossEvent::PhaseThresholdCrossed {
            tick: self.tick,
            from,
            to,
        });
        events.push(BossEvent::TimelineCancelled {
            tick: self.tick,
            from,
            cancellation_ordinal: self.cancellation_count,
            cancelled_pending_casts,
        });
        events.push(BossEvent::HostileProjectilesCleared { tick: self.tick });
        events.push(BossEvent::BreakStarted {
            tick: self.tick,
            entering: to,
            ends_at,
            received_damage_multiplier_basis_points: self
                .definition
                .0
                .break_received_damage_multiplier_basis_points,
        });
        Ok(())
    }

    fn schedule_current_tick(
        &mut self,
        target_aim: AimVector,
        events: &mut Vec<BossEvent>,
    ) -> Result<(), BossRuntimeError> {
        let BossState::Active {
            phase,
            loop_started,
            ..
        } = self.state
        else {
            return Ok(());
        };
        let relative = self
            .tick
            .0
            .checked_sub(loop_started.0)
            .ok_or(BossRuntimeError::TickOverflow)?;
        let cues = match phase {
            BellProctorPhase::Phase1 => self.definition.0.phase1_timeline.clone(),
            BellProctorPhase::Phase2 => self.definition.0.phase2_timeline.clone(),
            BellProctorPhase::Phase3 => self.definition.0.phase3_timeline.clone(),
        };
        for cue in cues
            .iter()
            .filter(|cue| u64::from(cue.starts_at_offset_ticks) == relative)
        {
            self.begin_cue(*cue, target_aim, events)?;
        }
        Ok(())
    }

    fn begin_cue(
        &mut self,
        cue: BossTimelineCue,
        target_aim: AimVector,
        events: &mut Vec<BossEvent>,
    ) -> Result<(), BossRuntimeError> {
        let id = BossCastId::allocate(&mut self.next_cast_id)?;
        let resolves_at = add_ticks(
            self.tick,
            cue.resolves_at_offset_ticks - cue.starts_at_offset_ticks,
        )?;
        match cue.kind {
            BossCueKind::Fan => {
                self.pending.push(PendingCast {
                    id,
                    resolves_at,
                    kind: PendingCastKind::Fan { aim: target_aim },
                });
                events.push(BossEvent::FanTelegraph {
                    tick: self.tick,
                    cast_id: id,
                    locked_aim: target_aim,
                    fires_at: resolves_at,
                    offsets_degrees: self.definition.0.fan_offsets_degrees,
                });
            }
            BossCueKind::Ring => {
                let omitted =
                    omitted_indices(self.next_ring_gap_start, self.definition.0.ring_index_count);
                self.next_ring_gap_start = (self.next_ring_gap_start
                    + self.definition.0.ring_gap_advance)
                    % self.definition.0.ring_index_count;
                self.pending.push(PendingCast {
                    id,
                    resolves_at,
                    kind: PendingCastKind::Ring { omitted },
                });
                events.push(BossEvent::RingTelegraph {
                    tick: self.tick,
                    cast_id: id,
                    omitted_indices: omitted,
                    fires_at: resolves_at,
                });
            }
            BossCueKind::RingPreviewA | BossCueKind::RingPreviewB => {
                let sequence_index = u8::from(cue.kind == BossCueKind::RingPreviewB);
                let start = if sequence_index == 0 {
                    self.next_ring_gap_start
                } else {
                    (self.next_ring_gap_start + self.definition.0.phase3_second_gap_advance)
                        % self.definition.0.ring_index_count
                };
                let omitted = omitted_indices(start, self.definition.0.ring_index_count);
                if sequence_index == 1 {
                    self.next_ring_gap_start = (start + self.definition.0.ring_gap_advance)
                        % self.definition.0.ring_index_count;
                }
                self.pending.push(PendingCast {
                    id,
                    resolves_at,
                    kind: PendingCastKind::Ring { omitted },
                });
                events.push(BossEvent::RingPreview {
                    tick: self.tick,
                    cast_id: id,
                    sequence_index,
                    omitted_indices: omitted,
                    preview_ends_at: add_ticks(self.tick, self.definition.0.ring_preview_ticks)?,
                    fires_at: resolves_at,
                });
            }
            BossCueKind::Cross => {
                let axes = if self.next_cross_diagonal {
                    [45, 135]
                } else {
                    [0, 90]
                };
                self.next_cross_diagonal = !self.next_cross_diagonal;
                self.pending.push(PendingCast {
                    id,
                    resolves_at,
                    kind: PendingCastKind::Cross { axes },
                });
                events.push(BossEvent::CrossTelegraph {
                    tick: self.tick,
                    cast_id: id,
                    axes_degrees: axes,
                    impacts_at: resolves_at,
                    width_milli_tiles: self.definition.0.cross.width_milli_tiles,
                });
            }
        }
        Ok(())
    }

    fn resolve_pending(&mut self, events: &mut Vec<BossEvent>) -> Result<(), BossRuntimeError> {
        let mut remaining = Vec::with_capacity(self.pending.len());
        for cast in std::mem::take(&mut self.pending) {
            if cast.resolves_at > self.tick {
                remaining.push(cast);
                continue;
            }
            match cast.kind {
                PendingCastKind::Fan { aim } => events.push(BossEvent::FanFired {
                    tick: self.tick,
                    cast_id: cast.id,
                    locked_aim: aim,
                    offsets_degrees: self.definition.0.fan_offsets_degrees,
                    attack: self.definition.0.fan.clone(),
                }),
                PendingCastKind::Ring { omitted } => events.push(BossEvent::RingFired {
                    tick: self.tick,
                    cast_id: cast.id,
                    emitted_indices: emitted_indices(omitted, self.definition.0.ring_index_count),
                    omitted_indices: omitted,
                    attack: self.definition.0.ring.clone(),
                }),
                PendingCastKind::Cross { axes } => {
                    let active_until = add_ticks(self.tick, self.definition.0.cross.active_ticks)?;
                    self.active_crosses.push(ActiveCross {
                        id: cast.id,
                        expires_at: active_until,
                    });
                    events.push(BossEvent::CrossActivated {
                        tick: self.tick,
                        cast_id: cast.id,
                        axes_degrees: axes,
                        active_until,
                        attack: self.definition.0.cross.clone(),
                    });
                }
            }
        }
        self.pending = remaining;
        Ok(())
    }

    fn expire_crosses(&mut self, events: &mut Vec<BossEvent>) {
        let mut active = Vec::with_capacity(self.active_crosses.len());
        for cross in std::mem::take(&mut self.active_crosses) {
            if cross.expires_at <= self.tick {
                events.push(BossEvent::CrossExpired {
                    tick: self.tick,
                    cast_id: cross.id,
                });
            } else {
                active.push(cross);
            }
        }
        self.active_crosses = active;
    }

    fn maybe_restart_loop(
        &mut self,
        phase: BellProctorPhase,
        loop_started: Tick,
        loop_ordinal: u32,
        target_aim: AimVector,
        events: &mut Vec<BossEvent>,
    ) -> Result<(), BossRuntimeError> {
        let normal_length = self.loop_length(phase);
        let last_event = Self::last_resolve_offset(phase);
        let enraged = self.tick.0 >= u64::from(self.definition.0.soft_enrage_ticks);
        let effective_length = if enraged {
            let downtime = normal_length - last_event;
            last_event
                + multiply_basis_points_half_up(
                    downtime,
                    self.definition
                        .0
                        .soft_enrage_downtime_multiplier_basis_points,
                )?
        } else {
            normal_length
        };
        if self.tick.0 < loop_started.0 + u64::from(effective_length) {
            return Ok(());
        }
        let next_ordinal = loop_ordinal
            .checked_add(1)
            .ok_or(BossRuntimeError::LoopOrdinalOverflow)?;
        self.state = BossState::Active {
            phase,
            loop_started: self.tick,
            loop_ordinal: next_ordinal,
        };
        events.push(BossEvent::LoopRestarted {
            tick: self.tick,
            phase,
            loop_ordinal: next_ordinal,
            low_health_restart: phase == BellProctorPhase::Phase3
                && self.health < self.definition.0.low_health_restart,
            soft_enraged: enraged,
        });
        events.push(BossEvent::PhaseStarted {
            tick: self.tick,
            phase,
            loop_ordinal: next_ordinal,
        });
        self.schedule_current_tick(target_aim, events)?;
        Ok(())
    }

    fn loop_length(&self, phase: BellProctorPhase) -> u32 {
        match phase {
            BellProctorPhase::Phase1 => self.definition.0.phase1_loop_ticks,
            BellProctorPhase::Phase2 => self.definition.0.phase2_loop_ticks,
            BellProctorPhase::Phase3 if self.health < self.definition.0.low_health_restart => {
                self.definition.0.phase3_low_health_loop_ticks
            }
            BellProctorPhase::Phase3 => self.definition.0.phase3_loop_ticks,
        }
    }

    fn last_resolve_offset(phase: BellProctorPhase) -> u32 {
        match phase {
            BellProctorPhase::Phase1 => 188,
            BellProctorPhase::Phase2 => 237,
            BellProctorPhase::Phase3 => 264,
        }
    }

    fn announce_soft_enrage(&mut self, events: &mut Vec<BossEvent>) {
        if !self.soft_enrage_announced
            && self.tick.0 >= u64::from(self.definition.0.soft_enrage_ticks)
        {
            self.soft_enrage_announced = true;
            events.push(BossEvent::SoftEnrageStarted { tick: self.tick });
        }
    }

    fn increment_tick(&mut self) -> Result<(), BossRuntimeError> {
        self.tick = self
            .tick
            .checked_next()
            .ok_or(BossRuntimeError::TickOverflow)?;
        Ok(())
    }
}

fn add_ticks(tick: Tick, count: u32) -> Result<Tick, BossRuntimeError> {
    tick.0
        .checked_add(u64::from(count))
        .map(Tick)
        .ok_or(BossRuntimeError::TickOverflow)
}

fn multiply_basis_points_half_up(value: u32, basis_points: u32) -> Result<u32, BossRuntimeError> {
    u64::from(value)
        .checked_mul(u64::from(basis_points))
        .and_then(|product| product.checked_add(5_000))
        .map(|rounded| rounded / 10_000)
        .and_then(|result| u32::try_from(result).ok())
        .ok_or(BossRuntimeError::ArithmeticOverflow)
}

fn omitted_indices(start: u8, count: u8) -> [u8; 4] {
    [
        start,
        (start + 1) % count,
        (start + 2) % count,
        (start + 3) % count,
    ]
}

fn emitted_indices(omitted: [u8; 4], count: u8) -> Vec<u8> {
    (0..count)
        .filter(|index| !omitted.contains(index))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BossDefinitionError {
    #[error("Bell Proctor differs from exact CONT-FP-005 values")]
    FirstPlayableDrift,
    #[error("fan/ring impact occurs within 500 ms of a Cross impact")]
    CrossImpactExclusion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BossRuntimeError {
    #[error("boss health cannot increase or exceed its definition")]
    HealthIncreased,
    #[error("boss target aim must be nonzero")]
    InvalidAim,
    #[error("defeated boss cannot be advanced with positive health")]
    DefeatedBossAdvanced,
    #[error("boss tick overflow")]
    TickOverflow,
    #[error("boss cast ID overflow")]
    CastIdOverflow,
    #[error("boss loop ordinal overflow")]
    LoopOrdinalOverflow,
    #[error("boss cancellation ordinal overflow")]
    CancellationOverflow,
    #[error("pending cast count exceeds u32")]
    PendingCastCountOverflow,
    #[error("boss scheduler arithmetic overflow")]
    ArithmeticOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(health: u32) -> BossInput {
        BossInput {
            current_health: health,
            target_aim: AimVector { x: 3, y: 4 },
        }
    }

    fn trace(sim: &mut BellProctorSimulation, ticks: usize, health: u32) -> Vec<BossEvent> {
        let mut events = Vec::new();
        for _ in 0..ticks {
            events.extend(sim.advance(input(health)).expect("boss tick"));
        }
        events
    }

    #[test]
    fn exact_definition_and_timelines_are_pinned_and_drift_fails() {
        let definition = BellProctorDefinition::first_playable();
        let p = definition.parameters();
        assert_eq!(
            (p.health, p.armor, p.hurtbox_radius_milli_tiles),
            (3_000, 4, 650)
        );
        assert_eq!((p.phase1_loop_ticks, p.phase2_loop_ticks), (216, 300));
        assert_eq!(
            (p.phase3_loop_ticks, p.phase3_low_health_loop_ticks),
            (300, 270)
        );
        assert_eq!(
            p.phase1_timeline,
            vec![
                cue(BossCueKind::Fan, 0, 12),
                cue(BossCueKind::Fan, 72, 84),
                cue(BossCueKind::Ring, 168, 188)
            ]
        );
        let mut changed = p.clone();
        changed.health = 2_999;
        assert_eq!(
            BellProctorDefinition::new(changed),
            Err(BossDefinitionError::FirstPlayableDrift)
        );
        let mut illegal_overlap = p.clone();
        illegal_overlap.phase2_timeline[3].resolves_at_offset_ticks = 150;
        assert_eq!(
            BellProctorDefinition::new(illegal_overlap),
            Err(BossDefinitionError::CrossImpactExclusion)
        );
    }

    #[test]
    fn phase_one_exact_golden_ticks_and_gap_are_stable() {
        let events = trace(&mut BellProctorSimulation::first_playable(), 217, 3_000);
        let compact: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                BossEvent::FanTelegraph { tick, fires_at, .. } => {
                    Some(("fan_warn", tick.0, fires_at.0))
                }
                BossEvent::FanFired { tick, .. } => Some(("fan_fire", tick.0, tick.0)),
                BossEvent::RingTelegraph { tick, fires_at, .. } => {
                    Some(("ring_warn", tick.0, fires_at.0))
                }
                BossEvent::RingFired { tick, .. } => Some(("ring_fire", tick.0, tick.0)),
                _ => None,
            })
            .collect();
        assert_eq!(
            compact,
            vec![
                ("fan_warn", 0, 12),
                ("fan_fire", 12, 12),
                ("fan_warn", 72, 84),
                ("fan_fire", 84, 84),
                ("ring_warn", 168, 188),
                ("ring_fire", 188, 188),
                ("fan_warn", 216, 228)
            ]
        );
    }

    #[test]
    fn thresholds_cancel_once_and_break_for_exact_ninety_ticks() {
        let mut sim = BellProctorSimulation::first_playable();
        trace(&mut sim, 20, 3_000);
        let start = sim.advance(input(2_100)).expect("phase2 threshold");
        assert!(matches!(
            sim.state(),
            BellProctorStateKind::Break {
                entering: BellProctorPhase::Phase2
            }
        ));
        assert_eq!(sim.received_damage_multiplier_basis_points(), 12_000);
        assert_eq!(
            start
                .iter()
                .filter(|e| matches!(e, BossEvent::TimelineCancelled { .. }))
                .count(),
            1
        );
        let Some(end_tick) = start.iter().find_map(|e| match e {
            BossEvent::BreakStarted { ends_at, .. } => Some(*ends_at),
            _ => None,
        }) else {
            panic!("break");
        };
        while sim.tick() < end_tick {
            assert!(
                trace(&mut sim, 1, 2_100)
                    .iter()
                    .all(|e| !matches!(e, BossEvent::FanTelegraph { .. }))
            );
        }
        trace(&mut sim, 1, 2_100);
        assert!(matches!(
            sim.state(),
            BellProctorStateKind::Active(BellProctorPhase::Phase2)
        ));
        let p3 = sim.advance(input(1_050)).expect("phase3 threshold");
        assert_eq!(
            p3.iter()
                .filter(|e| matches!(e, BossEvent::TimelineCancelled { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn phase_two_and_three_exact_events_previews_gaps_and_cross_exclusion() {
        let mut sim = BellProctorSimulation::first_playable();
        sim.advance(input(2_100)).expect("threshold");
        trace(&mut sim, 90, 2_100);
        let p2 = trace(&mut sim, 250, 2_100);
        assert!(p2.iter().any(|e| matches!(
            e,
            BossEvent::CrossActivated {
                tick: Tick(327),
                axes_degrees: [0, 90],
                ..
            }
        )));
        sim.advance(input(1_050)).expect("threshold3");
        trace(&mut sim, 89, 1_050);
        let p3_start = sim.tick();
        let p3 = trace(&mut sim, 265, 1_050);
        let previews: Vec<_> = p3
            .iter()
            .filter_map(|e| match e {
                BossEvent::RingPreview {
                    tick,
                    sequence_index,
                    omitted_indices,
                    fires_at,
                    ..
                } => Some((
                    tick.0 - p3_start.0,
                    *sequence_index,
                    *omitted_indices,
                    fires_at.0 - p3_start.0,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            previews,
            vec![(0, 0, [5, 6, 7, 8], 27), (30, 1, [9, 10, 11, 12], 54)]
        );
        let impacts: Vec<u64> = p3
            .iter()
            .filter_map(|e| match e {
                BossEvent::RingFired { tick, .. } | BossEvent::FanFired { tick, .. } => {
                    Some(tick.0 - p3_start.0)
                }
                _ => None,
            })
            .collect();
        assert!(impacts.iter().all(|impact| impact.abs_diff(222) >= 15));
    }

    #[test]
    fn low_health_and_soft_enrage_change_only_loop_restart_downtime() {
        let mut low = BellProctorSimulation::first_playable();
        low.advance(input(2_100)).expect("p2");
        trace(&mut low, 90, 2_100);
        low.advance(input(1_050)).expect("p3");
        trace(&mut low, 89, 1_050);
        let start = low.tick();
        let events = trace(&mut low, 271, 599);
        assert!(events.iter().any(|e| matches!(e,BossEvent::LoopRestarted{tick,low_health_restart:true,..} if tick.0==start.0+270)));

        let p = BellProctorDefinition::first_playable().parameters().clone();
        assert_eq!(
            multiply_basis_points_half_up(p.phase1_loop_ticks - 188, 8_500).expect("math"),
            24
        );
        assert_eq!(188 + 24, 212);
    }

    #[test]
    fn soft_enrage_event_preserves_cue_delays_and_shortens_only_downtime() {
        let mut sim = BellProctorSimulation::first_playable();
        let events = trace(&mut sim, 5_620, 3_000);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, BossEvent::SoftEnrageStarted { tick: Tick(5_400) }))
        );
        let enraged_restarts: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                BossEvent::LoopRestarted {
                    tick,
                    soft_enraged: true,
                    ..
                } => Some(tick.0),
                _ => None,
            })
            .collect();
        assert!(enraged_restarts.contains(&5_400));
        assert!(enraged_restarts.contains(&5_612));
        assert!(events.iter().any(|event| matches!(
            event,
            BossEvent::FanTelegraph {
                tick: Tick(5_400),
                fires_at: Tick(5_412),
                ..
            }
        )));
    }

    #[test]
    fn phase_change_cancels_pending_casts_and_old_projectiles_once() {
        let mut sim = BellProctorSimulation::first_playable();
        trace(&mut sim, 5, 3_000);
        let events = sim.advance(input(2_100)).expect("cancel pending fan");
        assert!(events.iter().any(|e| matches!(
            e,
            BossEvent::TimelineCancelled {
                cancelled_pending_casts: 1,
                ..
            }
        )));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, BossEvent::HostileProjectilesCleared { .. }))
                .count(),
            1
        );
        let during = trace(&mut sim, 90, 2_100);
        assert!(
            !during
                .iter()
                .any(|e| matches!(e,BossEvent::FanFired{cast_id,..} if cast_id.get()==1))
        );
    }

    #[test]
    fn invalid_health_and_aim_are_transactional() {
        let mut sim = BellProctorSimulation::first_playable();
        let before = sim.clone();
        assert_eq!(
            sim.advance(input(3_001)),
            Err(BossRuntimeError::HealthIncreased)
        );
        assert_eq!(sim, before);
        assert_eq!(
            sim.advance(BossInput {
                current_health: 3_000,
                target_aim: AimVector { x: 0, y: 0 }
            }),
            Err(BossRuntimeError::InvalidAim)
        );
        assert_eq!(sim, before);
    }

    #[test]
    fn twenty_complete_damage_traces_defeat_without_softlock_and_replay() {
        fn run(index: u32) -> (u64, blake3::Hash) {
            let mut sim = BellProctorSimulation::first_playable();
            let mut health = 3_000_u32;
            let mut hasher = blake3::Hasher::new();
            for step in 0..5_000_u64 {
                if step > 0 && step % u64::from(18 + index) == 0 {
                    health = health.saturating_sub(37 + index * 3);
                }
                let events = sim.advance(input(health)).expect("trace");
                hasher.update(&sim.tick().0.to_le_bytes());
                hasher.update(format!("{events:?}").as_bytes());
                if sim.state() == BellProctorStateKind::Defeated {
                    return (step, hasher.finalize());
                }
            }
            panic!("softlock")
        }
        for index in 0..20 {
            let first = run(index);
            let second = run(index);
            assert_eq!(first, second);
            assert!(first.0 < 5_000);
        }
    }
}
