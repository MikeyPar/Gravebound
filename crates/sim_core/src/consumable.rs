use thiserror::Error;

use crate::{LanternAshDefinition, Tick};

pub const RED_TONIC_CONTENT_ID: &str = "consumable.red_tonic";
pub const RED_TONIC_STACK_CAP: u8 = 6;
pub const RED_TONIC_RESTORE_BASIS_POINTS: u32 = 3_000;
pub const RED_TONIC_RESTORE_TICKS: u32 = 12;
pub const RED_TONIC_SHARED_COOLDOWN_TICKS: u32 = 60;
pub const UNDERTAKER_KNOT_RESTORE_BASIS_POINTS: u32 = 3_500;
pub const UNDERTAKER_KNOT_SHARED_COOLDOWN_TICKS: u32 = 75;
const BASIS_POINTS_PER_ONE: u32 = 10_000;
const BELT_SLOT_COUNT: usize = 2;

/// Exact fixed-point inputs compiled from `consumable.red_tonic`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedTonicDefinitionParameters {
    pub content_id: String,
    pub belt_stack_cap: u8,
    pub restore_max_health_basis_points: u32,
    pub restore_duration_ticks: u32,
    pub shared_cooldown_ticks: u32,
    pub damage_interrupts_restore: bool,
    pub consumed_on_use: bool,
}

/// Validated immutable First Playable Red Tonic definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedTonicDefinition {
    parameters: RedTonicDefinitionParameters,
}

impl RedTonicDefinition {
    pub fn new(parameters: RedTonicDefinitionParameters) -> Result<Self, RedTonicDefinitionError> {
        Self::validate_common(&parameters)?;
        if parameters.restore_max_health_basis_points != RED_TONIC_RESTORE_BASIS_POINTS {
            return Err(RedTonicDefinitionError::UnexpectedRestoreBasisPoints(
                parameters.restore_max_health_basis_points,
            ));
        }
        if parameters.shared_cooldown_ticks != RED_TONIC_SHARED_COOLDOWN_TICKS {
            return Err(RedTonicDefinitionError::UnexpectedSharedCooldown(
                parameters.shared_cooldown_ticks,
            ));
        }
        Ok(Self { parameters })
    }

    fn validate_common(
        parameters: &RedTonicDefinitionParameters,
    ) -> Result<(), RedTonicDefinitionError> {
        if parameters.content_id != RED_TONIC_CONTENT_ID {
            return Err(RedTonicDefinitionError::UnexpectedContentId(
                parameters.content_id.clone(),
            ));
        }
        if parameters.belt_stack_cap != RED_TONIC_STACK_CAP {
            return Err(RedTonicDefinitionError::UnexpectedStackCap(
                parameters.belt_stack_cap,
            ));
        }
        if parameters.restore_duration_ticks != RED_TONIC_RESTORE_TICKS {
            return Err(RedTonicDefinitionError::UnexpectedRestoreDuration(
                parameters.restore_duration_ticks,
            ));
        }
        if parameters.damage_interrupts_restore {
            return Err(RedTonicDefinitionError::DamageMustNotInterrupt);
        }
        if !parameters.consumed_on_use {
            return Err(RedTonicDefinitionError::MustConsumeOnUse);
        }
        Ok(())
    }

    /// Exact resolved definition while `item.prototype.charm.undertaker_knot` is equipped.
    pub fn with_undertaker_knot() -> Result<Self, RedTonicDefinitionError> {
        let parameters = RedTonicDefinitionParameters {
            content_id: RED_TONIC_CONTENT_ID.to_owned(),
            belt_stack_cap: RED_TONIC_STACK_CAP,
            restore_max_health_basis_points: UNDERTAKER_KNOT_RESTORE_BASIS_POINTS,
            restore_duration_ticks: RED_TONIC_RESTORE_TICKS,
            shared_cooldown_ticks: UNDERTAKER_KNOT_SHARED_COOLDOWN_TICKS,
            damage_interrupts_restore: false,
            consumed_on_use: true,
        };
        Self::validate_common(&parameters)?;
        Ok(Self { parameters })
    }

    #[must_use]
    pub fn first_playable() -> Self {
        Self {
            parameters: RedTonicDefinitionParameters {
                content_id: RED_TONIC_CONTENT_ID.to_owned(),
                belt_stack_cap: RED_TONIC_STACK_CAP,
                restore_max_health_basis_points: RED_TONIC_RESTORE_BASIS_POINTS,
                restore_duration_ticks: RED_TONIC_RESTORE_TICKS,
                shared_cooldown_ticks: RED_TONIC_SHARED_COOLDOWN_TICKS,
                damage_interrupts_restore: false,
                consumed_on_use: true,
            },
        }
    }

    #[must_use]
    pub fn content_id(&self) -> &str {
        &self.parameters.content_id
    }

    #[must_use]
    pub const fn belt_stack_cap(&self) -> u8 {
        self.parameters.belt_stack_cap
    }

    #[must_use]
    pub const fn restore_max_health_basis_points(&self) -> u32 {
        self.parameters.restore_max_health_basis_points
    }

    #[must_use]
    pub const fn restore_duration_ticks(&self) -> u32 {
        self.parameters.restore_duration_ticks
    }

    #[must_use]
    pub const fn shared_cooldown_ticks(&self) -> u32 {
        self.parameters.shared_cooldown_ticks
    }

    #[must_use]
    pub const fn damage_interrupts_restore(&self) -> bool {
        self.parameters.damage_interrupts_restore
    }

    #[must_use]
    pub const fn consumed_on_use(&self) -> bool {
        self.parameters.consumed_on_use
    }

    fn scheduled_restore(
        &self,
        maximum_health: u32,
        potion_healing_multiplier_basis_points: u32,
    ) -> Result<u32, ConsumableError> {
        let numerator = u64::from(maximum_health)
            .checked_mul(u64::from(self.restore_max_health_basis_points()))
            .and_then(|value| value.checked_mul(u64::from(potion_healing_multiplier_basis_points)))
            .ok_or(ConsumableError::HealingArithmeticOverflow)?;
        let denominator = u64::from(BASIS_POINTS_PER_ONE)
            .checked_mul(u64::from(BASIS_POINTS_PER_ONE))
            .ok_or(ConsumableError::HealingArithmeticOverflow)?;
        let rounded = numerator
            .checked_add(denominator / 2)
            .ok_or(ConsumableError::HealingArithmeticOverflow)?
            / denominator;
        u32::try_from(rounded).map_err(|_| ConsumableError::HealingArithmeticOverflow)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RedTonicDefinitionError {
    #[error("expected content ID consumable.red_tonic, received {0}")]
    UnexpectedContentId(String),
    #[error("expected Red Tonic stack cap 6, received {0}")]
    UnexpectedStackCap(u8),
    #[error("expected Red Tonic restore 3000 basis points, received {0}")]
    UnexpectedRestoreBasisPoints(u32),
    #[error("expected Red Tonic restore duration 12 ticks, received {0}")]
    UnexpectedRestoreDuration(u32),
    #[error("expected Red Tonic shared cooldown 60 ticks, received {0}")]
    UnexpectedSharedCooldown(u32),
    #[error("Red Tonic damage_interrupts_restore must be false")]
    DamageMustNotInterrupt,
    #[error("Red Tonic consumed_on_use must be true")]
    MustConsumeOnUse,
}

/// Authoritative integer health. Damage saturates at zero and healing caps at maximum health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerVitals {
    current_health: u32,
    maximum_health: u32,
}

impl PlayerVitals {
    pub fn new(current_health: u32, maximum_health: u32) -> Result<Self, VitalsError> {
        if maximum_health == 0 {
            return Err(VitalsError::ZeroMaximumHealth);
        }
        if current_health > maximum_health {
            return Err(VitalsError::CurrentExceedsMaximum {
                current: current_health,
                maximum: maximum_health,
            });
        }
        Ok(Self {
            current_health,
            maximum_health,
        })
    }

    #[must_use]
    pub const fn current_health(self) -> u32 {
        self.current_health
    }

    #[must_use]
    pub const fn maximum_health(self) -> u32 {
        self.maximum_health
    }

    #[must_use]
    pub const fn is_full(self) -> bool {
        self.current_health == self.maximum_health
    }

    /// Applies damage and returns the amount actually removed.
    pub fn apply_damage(&mut self, requested: u32) -> u32 {
        let applied = requested.min(self.current_health);
        self.current_health -= applied;
        applied
    }

    /// Applies healing and returns the amount actually restored. Overhealing is discarded.
    pub fn apply_healing(&mut self, requested: u32) -> u32 {
        let missing = self.maximum_health - self.current_health;
        let applied = requested.min(missing);
        self.current_health += applied;
        applied
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum VitalsError {
    #[error("maximum health must be positive")]
    ZeroMaximumHealth,
    #[error("current health {current} exceeds maximum health {maximum}")]
    CurrentExceedsMaximum { current: u32, maximum: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeltSlot {
    Empty,
    RedTonic(u8),
}

impl BeltSlot {
    #[must_use]
    pub const fn tonic_count(self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::RedTonic(count) => count,
        }
    }
}

/// The exact two-slot First Playable consumable belt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TonicBelt {
    slots: [BeltSlot; BELT_SLOT_COUNT],
}

impl TonicBelt {
    #[must_use]
    pub const fn first_playable() -> Self {
        Self {
            slots: [BeltSlot::RedTonic(2), BeltSlot::Empty],
        }
    }

    pub fn from_slots(slots: [BeltSlot; BELT_SLOT_COUNT]) -> Result<Self, BeltError> {
        let belt = Self { slots };
        belt.validate()?;
        Ok(belt)
    }

    #[must_use]
    pub const fn slots(&self) -> &[BeltSlot; BELT_SLOT_COUNT] {
        &self.slots
    }

    #[must_use]
    pub const fn slot(&self, index: usize) -> Option<BeltSlot> {
        if index < BELT_SLOT_COUNT {
            Some(self.slots[index])
        } else {
            None
        }
    }

    /// Merges into slot 1 first, then an existing Tonic stack in slot 2.
    ///
    /// An empty slot 1 becomes a Tonic stack. Empty slot 2 is intentionally not filled by this
    /// merge operation; the returned remainder belongs to the caller's backpack/ground policy.
    pub fn merge_red_tonics(&mut self, quantity: u32) -> TonicMergeResult {
        let mut remaining = quantity;
        let slot_one_added = merge_slot_one(&mut self.slots[0], &mut remaining);
        let slot_two_added = merge_existing_tonic(&mut self.slots[1], &mut remaining);
        TonicMergeResult {
            requested: quantity,
            slot_one_added,
            slot_two_added,
            remainder: remaining,
        }
    }

    fn consume_slot(&mut self, index: usize) -> Result<(), TonicUseRejection> {
        let slot = self
            .slots
            .get_mut(index)
            .ok_or(TonicUseRejection::InvalidBeltSlot { index })?;
        match *slot {
            BeltSlot::Empty | BeltSlot::RedTonic(0) => Err(TonicUseRejection::EmptyQSlot),
            BeltSlot::RedTonic(1) => {
                *slot = BeltSlot::Empty;
                Ok(())
            }
            BeltSlot::RedTonic(count) => {
                *slot = BeltSlot::RedTonic(count - 1);
                Ok(())
            }
        }
    }

    fn validate(self) -> Result<(), BeltError> {
        for (index, slot) in self.slots.iter().copied().enumerate() {
            if let BeltSlot::RedTonic(count) = slot {
                if count == 0 {
                    return Err(BeltError::ZeroSizedStack { index });
                }
                if count > RED_TONIC_STACK_CAP {
                    return Err(BeltError::StackExceedsCap { index, count });
                }
            }
        }
        Ok(())
    }
}

impl Default for TonicBelt {
    fn default() -> Self {
        Self::first_playable()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TonicMergeResult {
    pub requested: u32,
    pub slot_one_added: u8,
    pub slot_two_added: u8,
    pub remainder: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BeltError {
    #[error("belt slot {index} contains a zero-sized Red Tonic stack")]
    ZeroSizedStack { index: usize },
    #[error("belt slot {index} contains {count} Red Tonics, exceeding cap 6")]
    StackExceedsCap { index: usize, count: u8 },
}

/// Latest compact Q-style consumable action sampled by the client.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ConsumableAction {
    pub use_q_press_sequence: u32,
    pub use_second_slot_press_sequence: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TonicBeltPolicy {
    active_slots: [bool; BELT_SLOT_COUNT],
    potion_healing_multiplier_basis_points: u32,
}

impl TonicBeltPolicy {
    #[must_use]
    pub const fn normal() -> Self {
        Self {
            active_slots: [true, true],
            potion_healing_multiplier_basis_points: BASIS_POINTS_PER_ONE,
        }
    }

    pub fn lantern_ash(definition: LanternAshDefinition) -> Result<Self, ConsumableError> {
        if definition.active_belt_slot_count != 1
            || definition.active_belt_index != 0
            || !definition.inactive_slot_remains_stored_visible_locked
            || definition.potion_healing_multiplier_basis_points < BASIS_POINTS_PER_ONE
        {
            return Err(ConsumableError::InvalidBeltPolicy);
        }
        Ok(Self {
            active_slots: [true, false],
            potion_healing_multiplier_basis_points: definition
                .potion_healing_multiplier_basis_points,
        })
    }

    pub fn with_potion_output_multiplier(
        mut self,
        equipment_multiplier_basis_points: u32,
    ) -> Result<Self, ConsumableError> {
        if equipment_multiplier_basis_points < BASIS_POINTS_PER_ONE {
            return Err(ConsumableError::InvalidBeltPolicy);
        }
        let equipment_bonus = equipment_multiplier_basis_points - BASIS_POINTS_PER_ONE;
        self.potion_healing_multiplier_basis_points = self
            .potion_healing_multiplier_basis_points
            .checked_add(equipment_bonus)
            .ok_or(ConsumableError::HealingArithmeticOverflow)?;
        Ok(self)
    }

    #[must_use]
    pub const fn is_active(self, index: usize) -> bool {
        index < BELT_SLOT_COUNT && self.active_slots[index]
    }

    #[must_use]
    pub const fn potion_healing_multiplier_basis_points(self) -> u32 {
        self.potion_healing_multiplier_basis_points
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveRestore {
    source_press_sequence: u32,
    elapsed_ticks: u32,
    scheduled_total: u32,
    scheduled_so_far: u32,
}

/// Renderer-independent Red Tonic, belt, vitals, cooldown, and healing state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedTonicSimulation {
    definition: RedTonicDefinition,
    vitals: PlayerVitals,
    belt: TonicBelt,
    tick: Tick,
    last_use_q_press_sequence: u32,
    last_use_second_slot_press_sequence: u32,
    belt_policy: TonicBeltPolicy,
    shared_cooldown_remaining_ticks: u32,
    active_restore: Option<ActiveRestore>,
    cumulative_damage_taken: u64,
    accepted_tonic_uses: u32,
}

impl RedTonicSimulation {
    pub fn new(
        definition: RedTonicDefinition,
        vitals: PlayerVitals,
        belt: TonicBelt,
    ) -> Result<Self, ConsumableError> {
        Self::with_policy(definition, vitals, belt, TonicBeltPolicy::normal())
    }

    pub fn with_policy(
        definition: RedTonicDefinition,
        vitals: PlayerVitals,
        belt: TonicBelt,
        belt_policy: TonicBeltPolicy,
    ) -> Result<Self, ConsumableError> {
        belt.validate()?;
        Ok(Self {
            definition,
            vitals,
            belt,
            tick: Tick(0),
            last_use_q_press_sequence: 0,
            last_use_second_slot_press_sequence: 0,
            belt_policy,
            shared_cooldown_remaining_ticks: 0,
            active_restore: None,
            cumulative_damage_taken: 0,
            accepted_tonic_uses: 0,
        })
    }

    pub fn first_playable(vitals: PlayerVitals) -> Result<Self, ConsumableError> {
        Self::new(
            RedTonicDefinition::first_playable(),
            vitals,
            TonicBelt::first_playable(),
        )
    }

    /// Destroys every prior-run consumable state and reconstructs the exact fresh-run baseline.
    pub fn restart_first_playable(&mut self, vitals: PlayerVitals) -> Result<(), ConsumableError> {
        let next = Self::first_playable(vitals)?;
        *self = next;
        Ok(())
    }

    #[must_use]
    pub const fn definition(&self) -> &RedTonicDefinition {
        &self.definition
    }

    #[must_use]
    pub const fn vitals(&self) -> PlayerVitals {
        self.vitals
    }

    #[must_use]
    pub const fn belt(&self) -> &TonicBelt {
        &self.belt
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn last_use_q_press_sequence(&self) -> u32 {
        self.last_use_q_press_sequence
    }

    #[must_use]
    pub const fn belt_policy(&self) -> TonicBeltPolicy {
        self.belt_policy
    }

    #[must_use]
    pub const fn shared_cooldown_remaining_ticks(&self) -> u32 {
        self.shared_cooldown_remaining_ticks
    }

    #[must_use]
    pub const fn active_restore_remaining_ticks(&self) -> u32 {
        match self.active_restore {
            Some(restore) => self
                .definition
                .restore_duration_ticks()
                .saturating_sub(restore.elapsed_ticks),
            None => 0,
        }
    }

    #[must_use]
    pub const fn cumulative_damage_taken(&self) -> u64 {
        self.cumulative_damage_taken
    }

    #[must_use]
    pub const fn accepted_tonic_uses(&self) -> u32 {
        self.accepted_tonic_uses
    }

    /// Applies damage without disturbing an active Red Tonic restore.
    pub fn apply_damage(&mut self, requested: u32) -> DamageAppliedEvent {
        let before = self.vitals.current_health();
        let applied = self.vitals.apply_damage(requested);
        self.cumulative_damage_taken = self
            .cumulative_damage_taken
            .saturating_add(u64::from(applied));
        DamageAppliedEvent {
            tick: self.tick,
            requested,
            applied,
            health_before: before,
            health_after: self.vitals.current_health(),
            restore_continues: self.active_restore.is_some(),
        }
    }

    /// Advances one transactionally committed authoritative consumable tick.
    pub fn step(&mut self, action: ConsumableAction) -> Result<ConsumableStep, ConsumableError> {
        let mut next = self.clone();
        let result = next.step_inner(action)?;
        *self = next;
        Ok(result)
    }

    fn step_inner(&mut self, action: ConsumableAction) -> Result<ConsumableStep, ConsumableError> {
        if action.use_q_press_sequence < self.last_use_q_press_sequence {
            return Err(ConsumableError::StaleUseSequence {
                received: action.use_q_press_sequence,
                last: self.last_use_q_press_sequence,
            });
        }
        if action.use_second_slot_press_sequence < self.last_use_second_slot_press_sequence {
            return Err(ConsumableError::StaleUseSequence {
                received: action.use_second_slot_press_sequence,
                last: self.last_use_second_slot_press_sequence,
            });
        }
        self.belt.validate()?;
        self.tick = self
            .tick
            .checked_next()
            .ok_or(ConsumableError::TickExhausted)?;
        let mut result = ConsumableStep {
            tick: self.tick,
            events: Vec::new(),
        };

        if self.shared_cooldown_remaining_ticks > 0 {
            self.shared_cooldown_remaining_ticks -= 1;
            if self.shared_cooldown_remaining_ticks == 0 {
                result
                    .events
                    .push(ConsumableEvent::SharedCooldownReady { tick: self.tick });
            }
        }
        self.advance_restore(&mut result.events)?;

        if action.use_q_press_sequence > self.last_use_q_press_sequence {
            self.last_use_q_press_sequence = action.use_q_press_sequence;
            self.attempt_use(0, action.use_q_press_sequence, &mut result.events)?;
        }
        if action.use_second_slot_press_sequence > self.last_use_second_slot_press_sequence {
            self.last_use_second_slot_press_sequence = action.use_second_slot_press_sequence;
            self.attempt_use(1, action.use_second_slot_press_sequence, &mut result.events)?;
        }
        Ok(result)
    }

    fn attempt_use(
        &mut self,
        slot_index: usize,
        press_sequence: u32,
        events: &mut Vec<ConsumableEvent>,
    ) -> Result<(), ConsumableError> {
        let rejection = if !self.belt_policy.is_active(slot_index) {
            Some(TonicUseRejection::InactiveBeltSlot { index: slot_index })
        } else if self.belt.slots[slot_index].tonic_count() == 0 {
            Some(TonicUseRejection::EmptyQSlot)
        } else if self.shared_cooldown_remaining_ticks > 0 {
            Some(TonicUseRejection::SharedCooldown {
                remaining_ticks: self.shared_cooldown_remaining_ticks,
            })
        } else if self.vitals.is_full() {
            Some(TonicUseRejection::FullHealth)
        } else {
            None
        };

        if let Some(reason) = rejection {
            events.push(ConsumableEvent::UseRejected {
                tick: self.tick,
                press_sequence,
                reason,
            });
            return Ok(());
        }

        let scheduled_total = self.definition.scheduled_restore(
            self.vitals.maximum_health(),
            self.belt_policy.potion_healing_multiplier_basis_points(),
        )?;
        if scheduled_total == 0 {
            events.push(ConsumableEvent::UseRejected {
                tick: self.tick,
                press_sequence,
                reason: TonicUseRejection::NoEffectiveHealing,
            });
            return Ok(());
        }
        self.belt
            .consume_slot(slot_index)
            .map_err(ConsumableError::InvariantUseRejection)?;
        self.shared_cooldown_remaining_ticks = self.definition.shared_cooldown_ticks();
        self.active_restore = Some(ActiveRestore {
            source_press_sequence: press_sequence,
            elapsed_ticks: 0,
            scheduled_total,
            scheduled_so_far: 0,
        });
        self.accepted_tonic_uses = self.accepted_tonic_uses.saturating_add(1);
        events.push(ConsumableEvent::UseAccepted {
            tick: self.tick,
            press_sequence,
            consumed_from_slot: slot_index,
            slot_remaining: self.belt.slots[slot_index].tonic_count(),
            scheduled_healing: scheduled_total,
            restore_ticks: self.definition.restore_duration_ticks(),
            shared_cooldown_ticks: self.definition.shared_cooldown_ticks(),
        });
        Ok(())
    }

    fn advance_restore(
        &mut self,
        events: &mut Vec<ConsumableEvent>,
    ) -> Result<(), ConsumableError> {
        let Some(mut restore) = self.active_restore else {
            return Ok(());
        };
        restore.elapsed_ticks = restore
            .elapsed_ticks
            .checked_add(1)
            .ok_or(ConsumableError::HealingArithmeticOverflow)?;
        let cumulative = cumulative_half_up(
            restore.scheduled_total,
            restore.elapsed_ticks,
            self.definition.restore_duration_ticks(),
        )?;
        let requested = cumulative
            .checked_sub(restore.scheduled_so_far)
            .ok_or(ConsumableError::HealingScheduleRegressed)?;
        let health_before = self.vitals.current_health();
        let applied = self.vitals.apply_healing(requested);
        restore.scheduled_so_far = cumulative;
        let completed = restore.elapsed_ticks == self.definition.restore_duration_ticks();
        events.push(ConsumableEvent::HealingTick {
            tick: self.tick,
            source_press_sequence: restore.source_press_sequence,
            restore_tick: restore.elapsed_ticks,
            requested,
            applied,
            cumulative_scheduled: cumulative,
            health_before,
            health_after: self.vitals.current_health(),
        });
        if completed {
            events.push(ConsumableEvent::RestoreCompleted {
                tick: self.tick,
                source_press_sequence: restore.source_press_sequence,
                scheduled_total: restore.scheduled_total,
            });
            self.active_restore = None;
        } else {
            self.active_restore = Some(restore);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumableStep {
    pub tick: Tick,
    pub events: Vec<ConsumableEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumableEvent {
    UseAccepted {
        tick: Tick,
        press_sequence: u32,
        consumed_from_slot: usize,
        slot_remaining: u8,
        scheduled_healing: u32,
        restore_ticks: u32,
        shared_cooldown_ticks: u32,
    },
    UseRejected {
        tick: Tick,
        press_sequence: u32,
        reason: TonicUseRejection,
    },
    HealingTick {
        tick: Tick,
        source_press_sequence: u32,
        restore_tick: u32,
        requested: u32,
        applied: u32,
        cumulative_scheduled: u32,
        health_before: u32,
        health_after: u32,
    },
    RestoreCompleted {
        tick: Tick,
        source_press_sequence: u32,
        scheduled_total: u32,
    },
    SharedCooldownReady {
        tick: Tick,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TonicUseRejection {
    #[error("Q belt slot is empty")]
    EmptyQSlot,
    #[error("belt slot {index} is stored and visible but inactive")]
    InactiveBeltSlot { index: usize },
    #[error("belt slot {index} does not exist")]
    InvalidBeltSlot { index: usize },
    #[error("shared potion cooldown has {remaining_ticks} ticks remaining")]
    SharedCooldown { remaining_ticks: u32 },
    #[error("health is already full")]
    FullHealth,
    #[error("Red Tonic would schedule zero healing for this maximum health")]
    NoEffectiveHealing,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConsumableError {
    #[error("consumable simulation tick exhausted u64")]
    TickExhausted,
    #[error("stale Q-use sequence {received}; last observed is {last}")]
    StaleUseSequence { received: u32, last: u32 },
    #[error("belt invariant failed: {0}")]
    Belt(#[from] BeltError),
    #[error("accepted use violated a prevalidated belt invariant: {0}")]
    InvariantUseRejection(TonicUseRejection),
    #[error("consumable belt policy is invalid")]
    InvalidBeltPolicy,
    #[error("healing arithmetic overflowed")]
    HealingArithmeticOverflow,
    #[error("cumulative healing schedule regressed")]
    HealingScheduleRegressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageAppliedEvent {
    pub tick: Tick,
    pub requested: u32,
    pub applied: u32,
    pub health_before: u32,
    pub health_after: u32,
    pub restore_continues: bool,
}

fn merge_slot_one(slot: &mut BeltSlot, remaining: &mut u32) -> u8 {
    match *slot {
        BeltSlot::Empty => {
            let added = u8::try_from((*remaining).min(u32::from(RED_TONIC_STACK_CAP)))
                .expect("quantity was capped to a u8 stack size");
            if added > 0 {
                *slot = BeltSlot::RedTonic(added);
                *remaining -= u32::from(added);
            }
            added
        }
        BeltSlot::RedTonic(_) => merge_existing_tonic(slot, remaining),
    }
}

fn merge_existing_tonic(slot: &mut BeltSlot, remaining: &mut u32) -> u8 {
    let BeltSlot::RedTonic(count) = *slot else {
        return 0;
    };
    let capacity = RED_TONIC_STACK_CAP.saturating_sub(count);
    let added = u8::try_from((*remaining).min(u32::from(capacity)))
        .expect("quantity was capped to a u8 slot capacity");
    *slot = BeltSlot::RedTonic(count + added);
    *remaining -= u32::from(added);
    added
}

fn cumulative_half_up(total: u32, elapsed: u32, duration: u32) -> Result<u32, ConsumableError> {
    let numerator = u64::from(total)
        .checked_mul(u64::from(elapsed))
        .and_then(|product| product.checked_add(u64::from(duration / 2)))
        .ok_or(ConsumableError::HealingArithmeticOverflow)?;
    u32::try_from(numerator / u64::from(duration))
        .map_err(|_| ConsumableError::HealingArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hurt_simulation(current: u32, maximum: u32) -> RedTonicSimulation {
        RedTonicSimulation::first_playable(
            PlayerVitals::new(current, maximum).expect("valid vitals"),
        )
        .expect("First Playable simulation")
    }

    fn press(sequence: u32) -> ConsumableAction {
        ConsumableAction {
            use_q_press_sequence: sequence,
            ..ConsumableAction::default()
        }
    }

    fn no_press(sequence: u32) -> ConsumableAction {
        ConsumableAction {
            use_q_press_sequence: sequence,
            ..ConsumableAction::default()
        }
    }

    fn lantern_policy() -> TonicBeltPolicy {
        TonicBeltPolicy::lantern_ash(LanternAshDefinition {
            potion_healing_multiplier_basis_points: 14_000,
            active_belt_slot_count: 1,
            active_belt_index: 0,
            inactive_slot_remains_stored_visible_locked: true,
        })
        .unwrap()
    }

    #[test]
    fn lantern_healing_rounds_once_after_item_effect_and_keeps_authored_timing() {
        let mut simulation = RedTonicSimulation::with_policy(
            RedTonicDefinition::first_playable(),
            PlayerVitals::new(1, 4).unwrap(),
            TonicBelt::from_slots([BeltSlot::RedTonic(2), BeltSlot::Empty]).unwrap(),
            lantern_policy(),
        )
        .unwrap();
        let step = simulation.step(press(1)).unwrap();
        assert!(step.events.contains(&ConsumableEvent::UseAccepted {
            tick: Tick(1),
            press_sequence: 1,
            consumed_from_slot: 0,
            slot_remaining: 1,
            scheduled_healing: 2,
            restore_ticks: 12,
            shared_cooldown_ticks: 60,
        }));
    }

    #[test]
    fn lantern_keeps_second_slot_visible_locked_and_never_falls_through() {
        let belt = TonicBelt::from_slots([BeltSlot::RedTonic(2), BeltSlot::RedTonic(2)]).unwrap();
        let mut simulation = RedTonicSimulation::with_policy(
            RedTonicDefinition::first_playable(),
            PlayerVitals::new(60, 120).unwrap(),
            belt,
            lantern_policy(),
        )
        .unwrap();
        let rejected = simulation
            .step(ConsumableAction {
                use_second_slot_press_sequence: 1,
                ..ConsumableAction::default()
            })
            .unwrap();
        assert!(rejected.events.contains(&ConsumableEvent::UseRejected {
            tick: Tick(1),
            press_sequence: 1,
            reason: TonicUseRejection::InactiveBeltSlot { index: 1 },
        }));
        assert_eq!(simulation.belt().slot(1), Some(BeltSlot::RedTonic(2)));
        assert!(!simulation.belt_policy().is_active(1));

        let mut empty_first = RedTonicSimulation::with_policy(
            RedTonicDefinition::first_playable(),
            PlayerVitals::new(60, 120).unwrap(),
            TonicBelt::from_slots([BeltSlot::Empty, BeltSlot::RedTonic(2)]).unwrap(),
            lantern_policy(),
        )
        .unwrap();
        let rejected = empty_first.step(press(1)).unwrap();
        assert!(rejected.events.contains(&ConsumableEvent::UseRejected {
            tick: Tick(1),
            press_sequence: 1,
            reason: TonicUseRejection::EmptyQSlot,
        }));
        assert_eq!(empty_first.belt().slot(1), Some(BeltSlot::RedTonic(2)));
    }

    #[test]
    fn exact_first_playable_definition_is_immutable_and_visible() {
        let definition = RedTonicDefinition::first_playable();
        assert_eq!(definition.content_id(), RED_TONIC_CONTENT_ID);
        assert_eq!(definition.belt_stack_cap(), 6);
        assert_eq!(definition.restore_max_health_basis_points(), 3_000);
        assert_eq!(definition.restore_duration_ticks(), 12);
        assert_eq!(definition.shared_cooldown_ticks(), 60);
        assert!(!definition.damage_interrupts_restore());
        assert!(definition.consumed_on_use());
    }

    #[test]
    fn undertaker_knot_resolves_exact_authored_override() {
        let definition =
            RedTonicDefinition::with_undertaker_knot().expect("exact authored override");
        assert_eq!(
            definition.restore_max_health_basis_points(),
            UNDERTAKER_KNOT_RESTORE_BASIS_POINTS
        );
        assert_eq!(
            definition.shared_cooldown_ticks(),
            UNDERTAKER_KNOT_SHARED_COOLDOWN_TICKS
        );
        assert_eq!(definition.restore_duration_ticks(), RED_TONIC_RESTORE_TICKS);

        let mut simulation = RedTonicSimulation::new(
            definition,
            PlayerVitals::new(50, 100).expect("vitals"),
            TonicBelt::first_playable(),
        )
        .expect("simulation");
        let accepted = simulation.step(press(1)).expect("accepted");
        assert!(matches!(
            accepted.events.as_slice(),
            [ConsumableEvent::UseAccepted {
                scheduled_healing: 35,
                shared_cooldown_ticks: 75,
                ..
            }]
        ));
    }

    #[test]
    fn restart_destroys_restore_cooldown_sequence_and_old_belt() {
        let definition =
            RedTonicDefinition::with_undertaker_knot().expect("exact authored override");
        let mut simulation = RedTonicSimulation::new(
            definition,
            PlayerVitals::new(50, 100).expect("vitals"),
            TonicBelt::first_playable(),
        )
        .expect("simulation");
        simulation.step(press(7)).expect("accepted");
        simulation.step(no_press(7)).expect("healing");

        simulation
            .restart_first_playable(PlayerVitals::new(128, 128).expect("fresh vitals"))
            .expect("restart");
        assert_eq!(simulation.tick(), Tick(0));
        assert_eq!(simulation.last_use_q_press_sequence(), 0);
        assert_eq!(simulation.shared_cooldown_remaining_ticks(), 0);
        assert_eq!(simulation.active_restore_remaining_ticks(), 0);
        assert_eq!(simulation.belt(), &TonicBelt::first_playable());
        assert_eq!(simulation.vitals().current_health(), 128);
        assert_eq!(
            simulation.definition(),
            &RedTonicDefinition::first_playable()
        );
    }

    #[test]
    fn every_definition_drift_fails_closed() {
        let base = RedTonicDefinition::first_playable().parameters;
        let mut changed = base.clone();
        changed.content_id = "consumable.other".to_owned();
        assert!(matches!(
            RedTonicDefinition::new(changed),
            Err(RedTonicDefinitionError::UnexpectedContentId(_))
        ));
        let mut changed = base.clone();
        changed.belt_stack_cap = 5;
        assert_eq!(
            RedTonicDefinition::new(changed),
            Err(RedTonicDefinitionError::UnexpectedStackCap(5))
        );
        let mut changed = base.clone();
        changed.restore_max_health_basis_points = 2_999;
        assert_eq!(
            RedTonicDefinition::new(changed),
            Err(RedTonicDefinitionError::UnexpectedRestoreBasisPoints(2_999))
        );
        let mut changed = base.clone();
        changed.restore_duration_ticks = 11;
        assert_eq!(
            RedTonicDefinition::new(changed),
            Err(RedTonicDefinitionError::UnexpectedRestoreDuration(11))
        );
        let mut changed = base.clone();
        changed.shared_cooldown_ticks = 59;
        assert_eq!(
            RedTonicDefinition::new(changed),
            Err(RedTonicDefinitionError::UnexpectedSharedCooldown(59))
        );
        let mut changed = base.clone();
        changed.damage_interrupts_restore = true;
        assert_eq!(
            RedTonicDefinition::new(changed),
            Err(RedTonicDefinitionError::DamageMustNotInterrupt)
        );
        let mut changed = base;
        changed.consumed_on_use = false;
        assert_eq!(
            RedTonicDefinition::new(changed),
            Err(RedTonicDefinitionError::MustConsumeOnUse)
        );
    }

    #[test]
    fn vitals_damage_and_healing_are_capped() {
        assert_eq!(PlayerVitals::new(0, 0), Err(VitalsError::ZeroMaximumHealth));
        assert!(matches!(
            PlayerVitals::new(11, 10),
            Err(VitalsError::CurrentExceedsMaximum { .. })
        ));
        let mut vitals = PlayerVitals::new(80, 120).expect("vitals");
        assert_eq!(vitals.apply_damage(200), 80);
        assert_eq!(vitals.current_health(), 0);
        assert_eq!(vitals.apply_healing(200), 120);
        assert_eq!(vitals.current_health(), 120);
        assert_eq!(vitals.apply_healing(1), 0);
    }

    #[test]
    fn belt_starts_with_two_tonics_in_q_slot_and_exactly_two_slots() {
        let belt = TonicBelt::first_playable();
        assert_eq!(belt.slots().len(), 2);
        assert_eq!(belt.slot(0), Some(BeltSlot::RedTonic(2)));
        assert_eq!(belt.slot(1), Some(BeltSlot::Empty));
        assert_eq!(belt.slot(2), None);
    }

    #[test]
    fn invalid_belt_stacks_fail_closed() {
        assert_eq!(
            TonicBelt::from_slots([BeltSlot::RedTonic(0), BeltSlot::Empty]),
            Err(BeltError::ZeroSizedStack { index: 0 })
        );
        assert_eq!(
            TonicBelt::from_slots([BeltSlot::RedTonic(2), BeltSlot::RedTonic(7)]),
            Err(BeltError::StackExceedsCap { index: 1, count: 7 })
        );
    }

    #[test]
    fn pickup_merges_slot_one_then_existing_tonic_slot_two_and_returns_remainder() {
        let mut belt =
            TonicBelt::from_slots([BeltSlot::RedTonic(5), BeltSlot::RedTonic(4)]).expect("belt");
        assert_eq!(
            belt.merge_red_tonics(6),
            TonicMergeResult {
                requested: 6,
                slot_one_added: 1,
                slot_two_added: 2,
                remainder: 3,
            }
        );
        assert_eq!(
            belt.slots(),
            &[BeltSlot::RedTonic(6), BeltSlot::RedTonic(6)]
        );
    }

    #[test]
    fn pickup_refills_empty_slot_one_but_does_not_implicitly_fill_empty_slot_two() {
        let mut belt = TonicBelt::from_slots([BeltSlot::Empty, BeltSlot::Empty]).expect("belt");
        let result = belt.merge_red_tonics(8);
        assert_eq!(result.slot_one_added, 6);
        assert_eq!(result.slot_two_added, 0);
        assert_eq!(result.remainder, 2);
        assert_eq!(belt.slots(), &[BeltSlot::RedTonic(6), BeltSlot::Empty]);
    }

    #[test]
    fn accepted_use_consumes_one_starts_cooldown_and_defers_healing() {
        let mut simulation = hurt_simulation(60, 120);
        let step = simulation.step(press(1)).expect("accepted step");
        assert_eq!(step.tick, Tick(1));
        assert_eq!(simulation.vitals().current_health(), 60);
        assert_eq!(simulation.belt().slot(0), Some(BeltSlot::RedTonic(1)));
        assert_eq!(simulation.shared_cooldown_remaining_ticks(), 60);
        assert_eq!(simulation.active_restore_remaining_ticks(), 12);
        assert_eq!(
            step.events,
            vec![ConsumableEvent::UseAccepted {
                tick: Tick(1),
                press_sequence: 1,
                consumed_from_slot: 0,
                slot_remaining: 1,
                scheduled_healing: 36,
                restore_ticks: 12,
                shared_cooldown_ticks: 60,
            }]
        );
    }

    #[test]
    fn healing_uses_cumulative_half_up_over_exactly_twelve_following_ticks() {
        let mut simulation = hurt_simulation(60, 123);
        simulation.step(press(1)).expect("use");
        let mut requested = Vec::new();
        for _ in 0..12 {
            let step = simulation.step(no_press(1)).expect("healing step");
            let event = step
                .events
                .iter()
                .find_map(|event| match event {
                    ConsumableEvent::HealingTick { requested, .. } => Some(*requested),
                    _ => None,
                })
                .expect("healing event");
            requested.push(event);
        }
        assert_eq!(requested, vec![3, 3, 3, 3, 3, 4, 3, 3, 3, 3, 3, 3]);
        assert_eq!(requested.iter().sum::<u32>(), 37);
        assert_eq!(simulation.vitals().current_health(), 97);
        assert_eq!(simulation.active_restore_remaining_ticks(), 0);
    }

    #[test]
    fn damage_does_not_interrupt_and_later_healing_continues() {
        let mut simulation = hurt_simulation(100, 120);
        simulation.step(press(1)).expect("use");
        simulation.step(no_press(1)).expect("first heal");
        let damage = simulation.apply_damage(11);
        assert_eq!(damage.applied, 11);
        assert!(damage.restore_continues);
        assert_eq!(simulation.active_restore_remaining_ticks(), 11);
        for _ in 0..11 {
            simulation.step(no_press(1)).expect("continued healing");
        }
        assert_eq!(simulation.vitals().current_health(), 120);
        assert_eq!(simulation.active_restore_remaining_ticks(), 0);
    }

    #[test]
    fn overheal_is_discarded_per_tick_without_banking() {
        let mut simulation = hurt_simulation(119, 120);
        simulation.step(press(1)).expect("use");
        simulation.step(no_press(1)).expect("first heal");
        assert_eq!(simulation.vitals().current_health(), 120);
        simulation.apply_damage(10);
        for _ in 0..11 {
            simulation.step(no_press(1)).expect("remaining heals");
        }
        // First scheduled 3 healed only 1; the two overheal points were discarded.
        assert_eq!(simulation.vitals().current_health(), 120);
    }

    #[test]
    fn full_health_empty_and_cooldown_are_typed_committed_rejections() {
        let mut full = hurt_simulation(120, 120);
        let step = full.step(press(1)).expect("typed rejection");
        assert!(matches!(
            step.events.as_slice(),
            [ConsumableEvent::UseRejected {
                reason: TonicUseRejection::FullHealth,
                ..
            }]
        ));
        assert_eq!(full.last_use_q_press_sequence(), 1);
        assert_eq!(full.belt().slot(0), Some(BeltSlot::RedTonic(2)));

        let mut empty = RedTonicSimulation::new(
            RedTonicDefinition::first_playable(),
            PlayerVitals::new(60, 120).expect("vitals"),
            TonicBelt::from_slots([BeltSlot::Empty, BeltSlot::RedTonic(2)]).expect("belt"),
        )
        .expect("simulation");
        let step = empty.step(press(1)).expect("typed rejection");
        assert!(matches!(
            step.events.as_slice(),
            [ConsumableEvent::UseRejected {
                reason: TonicUseRejection::EmptyQSlot,
                ..
            }]
        ));

        let mut cooldown = hurt_simulation(60, 120);
        cooldown.step(press(1)).expect("use");
        let step = cooldown.step(press(2)).expect("typed rejection");
        assert!(matches!(
            step.events.as_slice(),
            [
                ConsumableEvent::HealingTick { .. },
                ConsumableEvent::UseRejected {
                    reason: TonicUseRejection::SharedCooldown {
                        remaining_ticks: 59
                    },
                    ..
                }
            ]
        ));
        assert_eq!(cooldown.belt().slot(0), Some(BeltSlot::RedTonic(1)));
    }

    #[test]
    fn stale_sequence_failure_is_transactional() {
        let mut simulation = hurt_simulation(60, 120);
        simulation.step(press(2)).expect("first press");
        let before = simulation.clone();
        assert_eq!(
            simulation.step(press(1)),
            Err(ConsumableError::StaleUseSequence {
                received: 1,
                last: 2,
            })
        );
        assert_eq!(simulation, before);
    }

    #[test]
    fn same_sequence_does_not_repeat_use() {
        let mut simulation = hurt_simulation(60, 120);
        simulation.step(press(1)).expect("use");
        let step = simulation.step(no_press(1)).expect("next tick");
        assert_eq!(
            step.events
                .iter()
                .filter(|event| matches!(event, ConsumableEvent::UseAccepted { .. }))
                .count(),
            0
        );
        assert_eq!(simulation.belt().slot(0), Some(BeltSlot::RedTonic(1)));
    }

    #[test]
    fn cooldown_is_ready_on_the_sixtieth_tick_after_activation() {
        let mut simulation = hurt_simulation(60, 120);
        simulation.step(press(1)).expect("use");
        let mut ready_tick = None;
        for _ in 0..60 {
            let step = simulation.step(no_press(1)).expect("tick");
            if step
                .events
                .contains(&ConsumableEvent::SharedCooldownReady { tick: step.tick })
            {
                ready_tick = Some(step.tick);
            }
        }
        assert_eq!(ready_tick, Some(Tick(61)));
        assert_eq!(simulation.shared_cooldown_remaining_ticks(), 0);
    }

    #[test]
    fn deterministic_replay_produces_identical_events_and_state() {
        fn replay() -> (Vec<ConsumableStep>, RedTonicSimulation) {
            let mut simulation = hurt_simulation(40, 123);
            let mut trace = vec![simulation.step(press(1)).expect("use")];
            for index in 0..70 {
                if index == 4 {
                    simulation.apply_damage(17);
                }
                let sequence = if index >= 20 { 2 } else { 1 };
                trace.push(simulation.step(press(sequence)).expect("step"));
            }
            (trace, simulation)
        }
        assert_eq!(replay(), replay());
    }

    #[test]
    fn one_health_character_rejects_zero_effect_without_consuming() {
        let mut simulation = hurt_simulation(0, 1);
        let step = simulation.step(press(1)).expect("typed rejection");
        assert!(matches!(
            step.events.as_slice(),
            [ConsumableEvent::UseRejected {
                reason: TonicUseRejection::NoEffectiveHealing,
                ..
            }]
        ));
        assert_eq!(simulation.belt().slot(0), Some(BeltSlot::RedTonic(2)));
        assert_eq!(simulation.shared_cooldown_remaining_ticks(), 0);
    }
}
