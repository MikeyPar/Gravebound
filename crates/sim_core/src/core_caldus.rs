//! Exact renderer-independent Sir Caldus phase and pattern scheduler.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::{CoreBossParticipant, CoreBossParticipantLock, Tick};

pub const CALDUS_PHASE_BREAK_TICKS: u64 = 120;
pub const CALDUS_SOFT_ENRAGE_TICKS: u64 = 10_800;
pub const CALDUS_SHIELD_WARNING_TICKS: u64 = 20;
pub const CALDUS_RING_WARNING_TICKS: u64 = 24;
pub const CALDUS_PHASE_ONE_LOOP_TICKS: u64 = 234;
pub const CALDUS_PHASE_TWO_LOOP_TICKS: u64 = 450;
pub const CALDUS_PHASE_THREE_LOOP_TICKS: u64 = 240;
pub const CALDUS_PHASE_THREE_LOW_HEALTH_LOOP_TICKS: u64 = 216;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCaldusPhase {
    Phase1,
    Phase2,
    Phase3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCaldusTargetInput {
    pub participant: CoreBossParticipant,
    pub position_x_milli_tiles: i32,
    pub position_y_milli_tiles: i32,
    pub squared_distance_to_boss: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusInput {
    pub tick: Tick,
    pub current_health: u32,
    pub living_targets: Vec<CoreCaldusTargetInput>,
}

/// Fully locked projectile release consumed by the shared hostile allocator.
#[derive(Debug, Clone, PartialEq)]
pub enum CoreCaldusProjectileRelease {
    ShieldArc {
        tick: Tick,
        cast_id: u64,
        origin: crate::SimulationVector,
        target_x_milli_tiles: i32,
        target_y_milli_tiles: i32,
    },
    BellRing {
        tick: Tick,
        cast_id: u64,
        origin: crate::SimulationVector,
        gap_start_index: u8,
    },
    ChargeStopRing {
        tick: Tick,
        cast_id: u64,
        origin: crate::SimulationVector,
        omitted_start_index: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreCaldusState {
    Active {
        phase: CoreCaldusPhase,
        phase_tick: u64,
        loop_tick: u64,
        loop_length: u64,
    },
    Break {
        entering: CoreCaldusPhase,
        ends_at: Tick,
    },
    Defeated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreCaldusEvent {
    PhaseTimelineCancelled {
        tick: Tick,
        leaving: CoreCaldusPhase,
    },
    HostilesCleared {
        tick: Tick,
    },
    BreakStarted {
        tick: Tick,
        entering: CoreCaldusPhase,
        ends_at: Tick,
        incoming_damage_basis_points: u16,
    },
    PhaseStarted {
        tick: Tick,
        phase: CoreCaldusPhase,
    },
    SoftEnrageStarted {
        tick: Tick,
        local_tick: u64,
    },
    LoopRestarted {
        tick: Tick,
        phase: CoreCaldusPhase,
        loop_length: u64,
        low_health: bool,
        soft_enraged: bool,
    },
    ShieldTelegraph {
        tick: Tick,
        cast_id: u64,
        target: CoreBossParticipant,
        target_x_milli_tiles: i32,
        target_y_milli_tiles: i32,
        fires_at: Tick,
    },
    ShieldFired {
        tick: Tick,
        cast_id: u64,
        target: CoreBossParticipant,
        target_x_milli_tiles: i32,
        target_y_milli_tiles: i32,
    },
    BellRingTelegraph {
        tick: Tick,
        cast_id: u64,
        gap_start_index: u8,
        fires_at: Tick,
    },
    BellRingPreview {
        tick: Tick,
        cast_id: u64,
        preview_ordinal: u8,
        gap_start_index: u8,
        ends_at: Tick,
    },
    BellRingFired {
        tick: Tick,
        cast_id: u64,
        gap_start_index: u8,
        child_without_ordinary_warning: bool,
    },
    ChargeTelegraph {
        tick: Tick,
        cast_id: u64,
        target: CoreBossParticipant,
        target_x_milli_tiles: i32,
        target_y_milli_tiles: i32,
    },
    ChargeDirectionLocked {
        tick: Tick,
        cast_id: u64,
        target: CoreBossParticipant,
        target_x_milli_tiles: i32,
        target_y_milli_tiles: i32,
    },
    ChargeMovementStarted {
        tick: Tick,
        cast_id: u64,
    },
    ChargeEnded {
        tick: Tick,
        cast_id: u64,
    },
    ChargeStopRingFired {
        tick: Tick,
        cast_id: u64,
    },
    BossDefeated {
        tick: Tick,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScheduledShield {
    cast_id: u64,
    target: CoreBossParticipant,
    target_x_milli_tiles: i32,
    target_y_milli_tiles: i32,
    telegraph_at: Tick,
    fires_at: Tick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScheduledCharge {
    cast_id: u64,
    target: CoreBossParticipant,
    target_x_milli_tiles: i32,
    target_y_milli_tiles: i32,
    direction_locks_at: Tick,
    movement_starts_at: Tick,
    ends_at: Tick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewedRing {
    cast_id: u64,
    gap_start_index: u8,
    fires_at: Tick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusSimulation {
    lock: CoreBossParticipantLock,
    tick: Tick,
    local_tick: u64,
    current_health: u32,
    state: CoreCaldusState,
    next_cast_id: u64,
    ordinary_gap_start: u8,
    shield_cursor: usize,
    scheduled_shields: Vec<ScheduledShield>,
    scheduled_charges: Vec<ScheduledCharge>,
    previewed_rings: Vec<PreviewedRing>,
    soft_enraged: bool,
}

impl CoreCaldusSimulation {
    pub fn new(lock: CoreBossParticipantLock) -> Result<Self, CoreCaldusError> {
        validate_lock(&lock)?;
        Ok(Self {
            current_health: lock.maximum_health,
            lock,
            tick: Tick(0),
            local_tick: 0,
            state: CoreCaldusState::Active {
                phase: CoreCaldusPhase::Phase1,
                phase_tick: 0,
                loop_tick: 0,
                loop_length: CALDUS_PHASE_ONE_LOOP_TICKS,
            },
            next_cast_id: 1,
            ordinary_gap_start: 0,
            shield_cursor: 0,
            scheduled_shields: Vec::new(),
            scheduled_charges: Vec::new(),
            previewed_rings: Vec::new(),
            soft_enraged: false,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn local_tick(&self) -> u64 {
        self.local_tick
    }

    #[must_use]
    pub const fn state(&self) -> &CoreCaldusState {
        &self.state
    }

    #[must_use]
    pub const fn current_health(&self) -> u32 {
        self.current_health
    }

    pub fn advance(
        &mut self,
        input: &CoreCaldusInput,
    ) -> Result<Vec<CoreCaldusEvent>, CoreCaldusError> {
        let mut staged = self.clone();
        let events = staged.advance_inner(input)?;
        *self = staged;
        Ok(events)
    }

    #[allow(clippy::too_many_lines)] // Equal-tick transition and pattern priority remains explicit.
    fn advance_inner(
        &mut self,
        input: &CoreCaldusInput,
    ) -> Result<Vec<CoreCaldusEvent>, CoreCaldusError> {
        if input.tick != self.tick {
            return Err(CoreCaldusError::TickMismatch {
                expected: self.tick,
                received: input.tick,
            });
        }
        if input.current_health > self.current_health {
            return Err(CoreCaldusError::HealthIncreased);
        }
        let targets = validate_targets(&self.lock, &input.living_targets)?;
        self.current_health = input.current_health;
        let mut events = Vec::new();
        if matches!(self.state, CoreCaldusState::Defeated) {
            return Err(CoreCaldusError::DefeatedBossAdvanced);
        }
        if self.current_health == 0 {
            self.cancel_timeline(current_phase(&self.state), &mut events);
            events.push(CoreCaldusEvent::BossDefeated { tick: self.tick });
            self.state = CoreCaldusState::Defeated;
            self.increment_global_tick()?;
            return Ok(events);
        }
        if let CoreCaldusState::Break { entering, ends_at } = self.state {
            if self.tick < ends_at {
                self.increment_global_tick()?;
                return Ok(events);
            }
            self.start_phase(entering, &mut events);
        }
        if self.maybe_start_phase_break(&mut events)? {
            self.increment_global_tick()?;
            return Ok(events);
        }
        if !self.soft_enraged && self.local_tick >= CALDUS_SOFT_ENRAGE_TICKS {
            self.soft_enraged = true;
            events.push(CoreCaldusEvent::SoftEnrageStarted {
                tick: self.tick,
                local_tick: self.local_tick,
            });
        }
        let CoreCaldusState::Active {
            phase, loop_tick, ..
        } = self.state
        else {
            unreachable!("break and defeat returned above")
        };
        self.schedule_pattern_starts(phase, loop_tick, &targets, &mut events)?;
        self.emit_due_scheduled(&mut events);
        self.advance_active_clock(&mut events)?;
        self.increment_global_tick()?;
        Ok(events)
    }

    fn maybe_start_phase_break(
        &mut self,
        events: &mut Vec<CoreCaldusEvent>,
    ) -> Result<bool, CoreCaldusError> {
        let CoreCaldusState::Active { phase, .. } = self.state else {
            return Ok(false);
        };
        let entering = match phase {
            CoreCaldusPhase::Phase1
                if threshold_reached(self.current_health, self.lock.maximum_health, 70) =>
            {
                Some(CoreCaldusPhase::Phase2)
            }
            CoreCaldusPhase::Phase2
                if threshold_reached(self.current_health, self.lock.maximum_health, 35) =>
            {
                Some(CoreCaldusPhase::Phase3)
            }
            _ => None,
        };
        let Some(entering) = entering else {
            return Ok(false);
        };
        self.cancel_timeline(Some(phase), events);
        let ends_at = add_ticks(self.tick, CALDUS_PHASE_BREAK_TICKS)?;
        events.push(CoreCaldusEvent::BreakStarted {
            tick: self.tick,
            entering,
            ends_at,
            incoming_damage_basis_points: 12_500,
        });
        self.state = CoreCaldusState::Break { entering, ends_at };
        Ok(true)
    }

    fn start_phase(&mut self, phase: CoreCaldusPhase, events: &mut Vec<CoreCaldusEvent>) {
        self.state = CoreCaldusState::Active {
            phase,
            phase_tick: 0,
            loop_tick: 0,
            loop_length: base_loop_length(phase),
        };
        events.push(CoreCaldusEvent::PhaseStarted {
            tick: self.tick,
            phase,
        });
    }

    fn cancel_timeline(
        &mut self,
        phase: Option<CoreCaldusPhase>,
        events: &mut Vec<CoreCaldusEvent>,
    ) {
        self.scheduled_shields.clear();
        self.scheduled_charges.clear();
        self.previewed_rings.clear();
        if let Some(leaving) = phase {
            events.push(CoreCaldusEvent::PhaseTimelineCancelled {
                tick: self.tick,
                leaving,
            });
            events.push(CoreCaldusEvent::HostilesCleared { tick: self.tick });
        }
    }

    fn schedule_pattern_starts(
        &mut self,
        phase: CoreCaldusPhase,
        loop_tick: u64,
        targets: &BTreeMap<u8, CoreCaldusTargetInput>,
        events: &mut Vec<CoreCaldusEvent>,
    ) -> Result<(), CoreCaldusError> {
        match phase {
            CoreCaldusPhase::Phase1 => {
                if [0, 54, 108].contains(&loop_tick) {
                    self.schedule_shield(targets)?;
                }
                if loop_tick == 180 {
                    self.schedule_ordinary_ring(events)?;
                }
            }
            CoreCaldusPhase::Phase2 => {
                if [0, 225].contains(&loop_tick) {
                    self.schedule_charge(targets, events)?;
                }
                if [90, 156, 315, 381].contains(&loop_tick) {
                    self.schedule_shield(targets)?;
                }
            }
            CoreCaldusPhase::Phase3 => {
                if loop_tick == 0 {
                    self.schedule_previewed_rings(events)?;
                }
                if loop_tick == 18 || loop_tick == 36 {
                    let ordinal = u8::try_from(loop_tick / 18)
                        .map_err(|_| CoreCaldusError::ArithmeticOverflow)?;
                    let preview = &self.previewed_rings[usize::from(ordinal)];
                    events.push(CoreCaldusEvent::BellRingPreview {
                        tick: self.tick,
                        cast_id: preview.cast_id,
                        preview_ordinal: ordinal,
                        gap_start_index: preview.gap_start_index,
                        ends_at: add_ticks(self.tick, 18)?,
                    });
                }
                if loop_tick == 180 {
                    self.schedule_shield(targets)?;
                }
            }
        }
        Ok(())
    }

    fn schedule_shield(
        &mut self,
        targets: &BTreeMap<u8, CoreCaldusTargetInput>,
    ) -> Result<(), CoreCaldusError> {
        if targets.is_empty() {
            return Ok(());
        }
        let selected = self.select_shield_targets(targets);
        for (ordinal, target) in selected.into_iter().enumerate() {
            let telegraph_at = add_ticks(
                self.tick,
                u64::try_from(ordinal)
                    .map_err(|_| CoreCaldusError::ArithmeticOverflow)?
                    .checked_mul(12)
                    .ok_or(CoreCaldusError::ArithmeticOverflow)?,
            )?;
            let fires_at = add_ticks(telegraph_at, CALDUS_SHIELD_WARNING_TICKS)?;
            let cast_id = self.allocate_cast_id()?;
            self.scheduled_shields.push(ScheduledShield {
                cast_id,
                target: target.participant,
                target_x_milli_tiles: target.position_x_milli_tiles,
                target_y_milli_tiles: target.position_y_milli_tiles,
                telegraph_at,
                fires_at,
            });
        }
        Ok(())
    }

    fn select_shield_targets(
        &mut self,
        targets: &BTreeMap<u8, CoreCaldusTargetInput>,
    ) -> Vec<CoreCaldusTargetInput> {
        let locked_count = self.lock.participants.len();
        let target_count = if locked_count >= 7 {
            3
        } else if locked_count >= 4 {
            2
        } else {
            1
        };
        if target_count == 1 {
            return nearest_target(targets)
                .copied()
                .map(|target| vec![target])
                .unwrap_or_default();
        }
        let mut selected = Vec::new();
        for offset in 0..self.lock.participants.len() {
            let index = (self.shield_cursor + offset) % self.lock.participants.len();
            let participant = self.lock.participants[index];
            if let Some(target) = targets.get(&participant.party_slot) {
                selected.push(*target);
                if selected.len() == target_count {
                    break;
                }
            }
        }
        if let Some(final_target) = selected.last()
            && let Some(index) = self
                .lock
                .participants
                .iter()
                .position(|participant| participant == &final_target.participant)
        {
            self.shield_cursor = (index + 1) % self.lock.participants.len();
        }
        selected
    }

    fn schedule_ordinary_ring(
        &mut self,
        events: &mut Vec<CoreCaldusEvent>,
    ) -> Result<(), CoreCaldusError> {
        let cast_id = self.allocate_cast_id()?;
        let gap_start_index = self.consume_gap();
        let fires_at = add_ticks(self.tick, CALDUS_RING_WARNING_TICKS)?;
        events.push(CoreCaldusEvent::BellRingTelegraph {
            tick: self.tick,
            cast_id,
            gap_start_index,
            fires_at,
        });
        self.previewed_rings.push(PreviewedRing {
            cast_id,
            gap_start_index,
            fires_at,
        });
        Ok(())
    }

    fn schedule_previewed_rings(
        &mut self,
        events: &mut Vec<CoreCaldusEvent>,
    ) -> Result<(), CoreCaldusError> {
        self.previewed_rings.clear();
        for (ordinal, fire_offset) in [66_u64, 90, 114].into_iter().enumerate() {
            let cast_id = self.allocate_cast_id()?;
            let gap_start_index = self.consume_gap();
            self.previewed_rings.push(PreviewedRing {
                cast_id,
                gap_start_index,
                fires_at: add_ticks(self.tick, fire_offset)?,
            });
            if ordinal == 0 {
                events.push(CoreCaldusEvent::BellRingPreview {
                    tick: self.tick,
                    cast_id,
                    preview_ordinal: 0,
                    gap_start_index,
                    ends_at: add_ticks(self.tick, 18)?,
                });
            }
        }
        Ok(())
    }

    fn schedule_charge(
        &mut self,
        targets: &BTreeMap<u8, CoreCaldusTargetInput>,
        events: &mut Vec<CoreCaldusEvent>,
    ) -> Result<(), CoreCaldusError> {
        let Some(target) = nearest_target(targets).copied() else {
            return Ok(());
        };
        let cast_id = self.allocate_cast_id()?;
        events.push(CoreCaldusEvent::ChargeTelegraph {
            tick: self.tick,
            cast_id,
            target: target.participant,
            target_x_milli_tiles: target.position_x_milli_tiles,
            target_y_milli_tiles: target.position_y_milli_tiles,
        });
        self.scheduled_charges.push(ScheduledCharge {
            cast_id,
            target: target.participant,
            target_x_milli_tiles: target.position_x_milli_tiles,
            target_y_milli_tiles: target.position_y_milli_tiles,
            direction_locks_at: add_ticks(self.tick, 21)?,
            movement_starts_at: add_ticks(self.tick, 30)?,
            ends_at: add_ticks(self.tick, 47)?,
        });
        Ok(())
    }

    fn emit_due_scheduled(&mut self, events: &mut Vec<CoreCaldusEvent>) {
        for shield in &self.scheduled_shields {
            if shield.telegraph_at == self.tick {
                events.push(CoreCaldusEvent::ShieldTelegraph {
                    tick: self.tick,
                    cast_id: shield.cast_id,
                    target: shield.target,
                    target_x_milli_tiles: shield.target_x_milli_tiles,
                    target_y_milli_tiles: shield.target_y_milli_tiles,
                    fires_at: shield.fires_at,
                });
            }
            if shield.fires_at == self.tick {
                events.push(CoreCaldusEvent::ShieldFired {
                    tick: self.tick,
                    cast_id: shield.cast_id,
                    target: shield.target,
                    target_x_milli_tiles: shield.target_x_milli_tiles,
                    target_y_milli_tiles: shield.target_y_milli_tiles,
                });
            }
        }
        self.scheduled_shields
            .retain(|shield| shield.fires_at > self.tick);
        for charge in &self.scheduled_charges {
            if charge.direction_locks_at == self.tick {
                events.push(CoreCaldusEvent::ChargeDirectionLocked {
                    tick: self.tick,
                    cast_id: charge.cast_id,
                    target: charge.target,
                    target_x_milli_tiles: charge.target_x_milli_tiles,
                    target_y_milli_tiles: charge.target_y_milli_tiles,
                });
            }
            if charge.movement_starts_at == self.tick {
                events.push(CoreCaldusEvent::ChargeMovementStarted {
                    tick: self.tick,
                    cast_id: charge.cast_id,
                });
            }
            if charge.ends_at == self.tick {
                events.push(CoreCaldusEvent::ChargeEnded {
                    tick: self.tick,
                    cast_id: charge.cast_id,
                });
                events.push(CoreCaldusEvent::ChargeStopRingFired {
                    tick: self.tick,
                    cast_id: charge.cast_id,
                });
            }
        }
        self.scheduled_charges
            .retain(|charge| charge.ends_at > self.tick);
        for ring in &self.previewed_rings {
            if ring.fires_at == self.tick {
                let phase_three = matches!(
                    self.state,
                    CoreCaldusState::Active {
                        phase: CoreCaldusPhase::Phase3,
                        ..
                    }
                );
                events.push(CoreCaldusEvent::BellRingFired {
                    tick: self.tick,
                    cast_id: ring.cast_id,
                    gap_start_index: ring.gap_start_index,
                    child_without_ordinary_warning: phase_three,
                });
            }
        }
        self.previewed_rings
            .retain(|ring| ring.fires_at > self.tick);
    }

    fn advance_active_clock(
        &mut self,
        events: &mut Vec<CoreCaldusEvent>,
    ) -> Result<(), CoreCaldusError> {
        let CoreCaldusState::Active {
            phase,
            phase_tick,
            loop_tick,
            loop_length,
        } = &mut self.state
        else {
            return Ok(());
        };
        *phase_tick = phase_tick
            .checked_add(1)
            .ok_or(CoreCaldusError::ArithmeticOverflow)?;
        *loop_tick = loop_tick
            .checked_add(1)
            .ok_or(CoreCaldusError::ArithmeticOverflow)?;
        self.local_tick = self
            .local_tick
            .checked_add(1)
            .ok_or(CoreCaldusError::ArithmeticOverflow)?;
        if *loop_tick >= *loop_length {
            *loop_tick = 0;
            *loop_length = resolved_loop_length(
                *phase,
                self.current_health,
                self.lock.maximum_health,
                self.soft_enraged,
            );
            events.push(CoreCaldusEvent::LoopRestarted {
                tick: add_ticks(self.tick, 1)?,
                phase: *phase,
                loop_length: *loop_length,
                low_health: *phase == CoreCaldusPhase::Phase3
                    && threshold_reached(self.current_health, self.lock.maximum_health, 20),
                soft_enraged: self.soft_enraged,
            });
        }
        Ok(())
    }

    fn allocate_cast_id(&mut self) -> Result<u64, CoreCaldusError> {
        let value = self.next_cast_id;
        self.next_cast_id = value
            .checked_add(1)
            .ok_or(CoreCaldusError::CastIdOverflow)?;
        Ok(value)
    }

    fn consume_gap(&mut self) -> u8 {
        let value = self.ordinary_gap_start;
        self.ordinary_gap_start = (value + 5) % 18;
        value
    }

    fn increment_global_tick(&mut self) -> Result<(), CoreCaldusError> {
        self.tick = add_ticks(self.tick, 1)?;
        Ok(())
    }
}

fn validate_lock(lock: &CoreBossParticipantLock) -> Result<(), CoreCaldusError> {
    if lock.participants.is_empty() || lock.participants.len() > 8 || lock.attempt_ordinal == 0 {
        return Err(CoreCaldusError::InvalidParticipantLock);
    }
    let mut slots = BTreeSet::new();
    let mut entities = BTreeSet::new();
    for participant in &lock.participants {
        if participant.party_slot >= 8
            || !slots.insert(participant.party_slot)
            || !entities.insert(participant.entity_id)
        {
            return Err(CoreCaldusError::InvalidParticipantLock);
        }
    }
    if lock.participants.windows(2).any(|pair| {
        (pair[0].party_slot, pair[0].entity_id) >= (pair[1].party_slot, pair[1].entity_id)
    }) {
        return Err(CoreCaldusError::InvalidParticipantLock);
    }
    Ok(())
}

fn validate_targets(
    lock: &CoreBossParticipantLock,
    targets: &[CoreCaldusTargetInput],
) -> Result<BTreeMap<u8, CoreCaldusTargetInput>, CoreCaldusError> {
    let locked = lock
        .participants
        .iter()
        .map(|participant| (participant.party_slot, participant.entity_id))
        .collect::<BTreeSet<_>>();
    let mut by_slot = BTreeMap::new();
    let mut entities = BTreeSet::new();
    for target in targets {
        if !locked.contains(&(target.participant.party_slot, target.participant.entity_id))
            || !entities.insert(target.participant.entity_id)
            || by_slot
                .insert(target.participant.party_slot, *target)
                .is_some()
        {
            return Err(CoreCaldusError::InvalidTargetSet);
        }
    }
    Ok(by_slot)
}

fn nearest_target(targets: &BTreeMap<u8, CoreCaldusTargetInput>) -> Option<&CoreCaldusTargetInput> {
    targets.values().min_by_key(|target| {
        (
            target.squared_distance_to_boss,
            target.participant.party_slot,
            target.participant.entity_id,
        )
    })
}

fn threshold_reached(health: u32, maximum: u32, percent: u32) -> bool {
    u64::from(health) * 100 <= u64::from(maximum) * u64::from(percent)
}

const fn base_loop_length(phase: CoreCaldusPhase) -> u64 {
    match phase {
        CoreCaldusPhase::Phase1 => CALDUS_PHASE_ONE_LOOP_TICKS,
        CoreCaldusPhase::Phase2 => CALDUS_PHASE_TWO_LOOP_TICKS,
        CoreCaldusPhase::Phase3 => CALDUS_PHASE_THREE_LOOP_TICKS,
    }
}

fn resolved_loop_length(
    phase: CoreCaldusPhase,
    health: u32,
    maximum: u32,
    soft_enraged: bool,
) -> u64 {
    let ordinary = match (phase, soft_enraged) {
        (CoreCaldusPhase::Phase1, false) | (CoreCaldusPhase::Phase3, true) => 234,
        (CoreCaldusPhase::Phase1, true) => 230,
        (CoreCaldusPhase::Phase2, false) => 450,
        (CoreCaldusPhase::Phase2, true) => 443,
        (CoreCaldusPhase::Phase3, false) => 240,
    };
    if phase == CoreCaldusPhase::Phase3 && threshold_reached(health, maximum, 20) {
        216
    } else {
        ordinary
    }
}

const fn current_phase(state: &CoreCaldusState) -> Option<CoreCaldusPhase> {
    match state {
        CoreCaldusState::Active { phase, .. } => Some(*phase),
        CoreCaldusState::Break { entering, .. } => Some(*entering),
        CoreCaldusState::Defeated => None,
    }
}

fn add_ticks(tick: Tick, count: u64) -> Result<Tick, CoreCaldusError> {
    tick.0
        .checked_add(count)
        .map(Tick)
        .ok_or(CoreCaldusError::TickOverflow)
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreCaldusError {
    #[error("Caldus expected tick {expected:?}, received {received:?}")]
    TickMismatch { expected: Tick, received: Tick },
    #[error("Caldus participant lock is invalid")]
    InvalidParticipantLock,
    #[error("Caldus living target set is invalid")]
    InvalidTargetSet,
    #[error("Caldus health cannot increase")]
    HealthIncreased,
    #[error("defeated Caldus cannot advance")]
    DefeatedBossAdvanced,
    #[error("Caldus cast identity overflowed")]
    CastIdOverflow,
    #[error("Caldus tick overflowed")]
    TickOverflow,
    #[error("Caldus arithmetic overflowed")]
    ArithmeticOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntityId;

    fn participant(id: u64, slot: u8) -> CoreBossParticipant {
        CoreBossParticipant {
            entity_id: EntityId::new(id).expect("entity"),
            party_slot: slot,
        }
    }

    fn lock(count: u8) -> CoreBossParticipantLock {
        CoreBossParticipantLock {
            attempt_ordinal: 1,
            participants: (0..count)
                .map(|slot| participant(u64::from(slot) + 1, slot))
                .collect(),
            maximum_health: crate::scaled_caldus_health(count).expect("health"),
        }
    }

    fn targets(count: u8) -> Vec<CoreCaldusTargetInput> {
        (0..count)
            .map(|slot| CoreCaldusTargetInput {
                participant: participant(u64::from(slot) + 1, slot),
                position_x_milli_tiles: 2_500 + i32::from(slot) * 100,
                position_y_milli_tiles: 9_000,
                squared_distance_to_boss: u64::from(slot) + 1,
            })
            .collect()
    }

    fn advance(
        simulation: &mut CoreCaldusSimulation,
        health: u32,
        count: u8,
    ) -> Vec<CoreCaldusEvent> {
        simulation
            .advance(&CoreCaldusInput {
                tick: simulation.tick(),
                current_health: health,
                living_targets: targets(count),
            })
            .expect("advance")
    }

    #[test]
    fn phase_one_exact_starts_releases_and_gap_advance() {
        let mut simulation = CoreCaldusSimulation::new(lock(1)).expect("simulation");
        let mut events = Vec::new();
        for _ in 0..234 {
            events.extend(advance(&mut simulation, 7_200, 1));
        }
        let shields = events
            .iter()
            .filter_map(|event| match event {
                CoreCaldusEvent::ShieldFired { tick, .. } => Some(tick.0),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(shields, [20, 74, 128]);
        assert!(events.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::BellRingTelegraph {
                tick: Tick(180),
                gap_start_index: 0,
                fires_at: Tick(204),
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::BellRingFired {
                tick: Tick(204),
                gap_start_index: 0,
                child_without_ordinary_warning: false,
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::LoopRestarted {
                tick: Tick(234),
                loop_length: 234,
                ..
            }
        )));
    }

    #[test]
    fn threshold_break_pauses_local_time_and_overkill_enters_next_break_first() {
        let mut simulation = CoreCaldusSimulation::new(lock(1)).expect("simulation");
        for _ in 0..10 {
            advance(&mut simulation, 7_200, 1);
        }
        let transition = advance(&mut simulation, 2_000, 1);
        assert!(transition.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::BreakStarted {
                entering: CoreCaldusPhase::Phase2,
                ends_at: Tick(130),
                ..
            }
        )));
        assert_eq!(simulation.local_tick(), 10);
        for _ in 11..130 {
            advance(&mut simulation, 2_000, 1);
        }
        let chained = advance(&mut simulation, 2_000, 1);
        assert!(chained.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::PhaseStarted {
                phase: CoreCaldusPhase::Phase2,
                ..
            }
        )));
        assert!(chained.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::BreakStarted {
                entering: CoreCaldusPhase::Phase3,
                ends_at: Tick(250),
                ..
            }
        )));
    }

    #[test]
    fn phase_two_charge_and_shield_ticks_are_exact() {
        let mut simulation = CoreCaldusSimulation::new(lock(1)).expect("simulation");
        advance(&mut simulation, 5_000, 1);
        for _ in 1..=120 {
            advance(&mut simulation, 5_000, 1);
        }
        let mut events = Vec::new();
        for _ in 0..450 {
            events.extend(advance(&mut simulation, 5_000, 1));
        }
        let phase_start = 120_u64;
        assert!(events.iter().any(|event| matches!(event, CoreCaldusEvent::ChargeDirectionLocked { tick, .. } if tick.0 == phase_start + 21)));
        assert!(events.iter().any(|event| matches!(event, CoreCaldusEvent::ChargeStopRingFired { tick, .. } if tick.0 == phase_start + 47)));
        let shields = events
            .iter()
            .filter_map(|event| match event {
                CoreCaldusEvent::ShieldFired { tick, .. } => Some(tick.0 - phase_start),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(shields, [110, 176, 335, 401]);
    }

    #[test]
    fn phase_three_previews_children_and_group_shields_are_deterministic() {
        let mut simulation = CoreCaldusSimulation::new(lock(8)).expect("simulation");
        advance(&mut simulation, 20_000, 8);
        for _ in 1..=120 {
            advance(&mut simulation, 20_000, 8);
        }
        advance(&mut simulation, 10_000, 8);
        for _ in 122..=241 {
            advance(&mut simulation, 10_000, 8);
        }
        let mut events = Vec::new();
        for _ in 0..230 {
            events.extend(advance(&mut simulation, 10_000, 8));
        }
        let fired = events
            .iter()
            .filter_map(|event| match event {
                CoreCaldusEvent::BellRingFired {
                    tick,
                    gap_start_index,
                    child_without_ordinary_warning: true,
                    ..
                } => Some((tick.0, *gap_start_index)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(fired.len(), 3);
        assert_eq!(
            fired.iter().map(|(_, gap)| *gap).collect::<Vec<_>>(),
            [0, 5, 10]
        );
        let shields = events
            .iter()
            .filter(|event| matches!(event, CoreCaldusEvent::ShieldFired { .. }))
            .count();
        assert_eq!(shields, 3);
    }

    #[test]
    fn phase_change_and_defeat_cancel_pending_actions_transactionally() {
        let mut simulation = CoreCaldusSimulation::new(lock(1)).expect("simulation");
        advance(&mut simulation, 7_200, 1);
        let transition = advance(&mut simulation, 5_000, 1);
        assert!(
            transition
                .iter()
                .any(|event| matches!(event, CoreCaldusEvent::HostilesCleared { .. }))
        );
        for _ in 2..=121 {
            advance(&mut simulation, 5_000, 1);
        }
        let defeated = advance(&mut simulation, 0, 1);
        assert!(
            defeated
                .iter()
                .any(|event| matches!(event, CoreCaldusEvent::BossDefeated { .. }))
        );
        let snapshot = simulation.clone();
        assert_eq!(
            advance_error(&mut simulation, 0, 1),
            CoreCaldusError::DefeatedBossAdvanced
        );
        assert_eq!(simulation, snapshot);
    }

    #[test]
    fn low_health_and_soft_enrage_change_only_future_loop_lengths() {
        let mut simulation = CoreCaldusSimulation::new(lock(1)).expect("simulation");
        simulation.state = CoreCaldusState::Active {
            phase: CoreCaldusPhase::Phase3,
            phase_tick: 239,
            loop_tick: 239,
            loop_length: 240,
        };
        let health = 1_440;
        let low = advance(&mut simulation, health, 1);
        assert!(low.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::LoopRestarted {
                loop_length: 216,
                low_health: true,
                soft_enraged: false,
                ..
            }
        )));

        simulation.local_tick = CALDUS_SOFT_ENRAGE_TICKS;
        simulation.state = CoreCaldusState::Active {
            phase: CoreCaldusPhase::Phase3,
            phase_tick: 455,
            loop_tick: 215,
            loop_length: 216,
        };
        let enraged = advance(&mut simulation, health, 1);
        assert!(enraged.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::SoftEnrageStarted {
                local_tick: CALDUS_SOFT_ENRAGE_TICKS,
                ..
            }
        )));
        assert!(enraged.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::LoopRestarted {
                loop_length: 216,
                low_health: true,
                soft_enraged: true,
                ..
            }
        )));
    }

    fn advance_error(
        simulation: &mut CoreCaldusSimulation,
        health: u32,
        count: u8,
    ) -> CoreCaldusError {
        simulation
            .advance(&CoreCaldusInput {
                tick: simulation.tick(),
                current_health: health,
                living_targets: targets(count),
            })
            .expect_err("error")
    }

    #[test]
    fn invalid_health_target_and_tick_inputs_roll_back() {
        let mut simulation = CoreCaldusSimulation::new(lock(1)).expect("simulation");
        let before = simulation.clone();
        assert_eq!(
            advance_error(&mut simulation, 7_201, 1),
            CoreCaldusError::HealthIncreased
        );
        assert_eq!(simulation, before);
        let error = simulation
            .advance(&CoreCaldusInput {
                tick: Tick(1),
                current_health: 7_200,
                living_targets: targets(1),
            })
            .expect_err("tick");
        assert!(matches!(error, CoreCaldusError::TickMismatch { .. }));
        assert_eq!(simulation, before);
    }
}
