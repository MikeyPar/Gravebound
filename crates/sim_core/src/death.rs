//! Atomic, local-only First Playable death and restart ownership for `GB-M01-06A`.
//!
//! This deliberately excludes durable memorial/Echo/account behavior. It freezes the current run,
//! retains a bounded authoritative combat trace, destroys every local inventory/belt stack, and
//! authorizes an explicit fresh run with qualified identities and the default seed.

use std::collections::VecDeque;

use thiserror::Error;

use crate::{
    BellLaboratoryEncounter, DamageEvent, DamageType, EncounterAction, EncounterError,
    EncounterInput, EncounterState, EncounterStep, EntityId, InventoryError, PrototypeInventory,
    RestartCleanup, SimulationVector, Tick,
};

#[allow(clippy::cast_lossless)]
pub const LOCAL_DEATH_TRACE_TICKS: u64 = (10 * crate::TICKS_PER_SECOND) as u64;
#[allow(clippy::cast_lossless)]
pub const LOCAL_RESTART_DEADLINE_TICKS: u64 = (3 * crate::TICKS_PER_SECOND) as u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalDeathId(u64);

impl LocalDeathId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunEntityCounts {
    pub enemies: u32,
    pub hostile_projectiles: u32,
    pub hostile_hazards: u32,
    pub friendly_projectiles: u32,
    pub field_pickups: u32,
    pub reward_entities: u32,
    pub transient_effects: u32,
}

impl RunEntityCounts {
    pub fn total(self) -> Result<u32, LocalDeathError> {
        [
            self.enemies,
            self.hostile_projectiles,
            self.hostile_hazards,
            self.friendly_projectiles,
            self.field_pickups,
            self.reward_entities,
            self.transient_effects,
        ]
        .into_iter()
        .try_fold(0_u32, |sum, value| {
            sum.checked_add(value)
                .ok_or(LocalDeathError::EntityCountOverflow)
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalCombatTraceEntry {
    pub tick: Tick,
    pub source: EntityId,
    pub pattern_id: String,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub damage_type: DamageType,
    pub health_before: u32,
    pub health_after: u32,
    pub source_position: SimulationVector,
    pub lethal: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalDamageObservation {
    pub tick: Tick,
    pub pattern_id: String,
    pub source_position: SimulationVector,
    pub damage: DamageEvent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalDeathCause {
    pub death_id: LocalDeathId,
    pub lethal: LocalCombatTraceEntry,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LocalRunPhase {
    Alive,
    Dead(LocalDeathCause),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalDeathCommit {
    pub cause: LocalDeathCause,
    pub trace: Vec<LocalCombatTraceEntry>,
    pub inventory_cleanup: RestartCleanup,
    pub cleared_entities: RunEntityCounts,
    pub cleared_entity_total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRestartCommit {
    pub previous_death_id: LocalDeathId,
    pub new_run_ordinal: u32,
    pub seed: u64,
    pub restart_elapsed_ticks: u64,
    pub equipped_starter_items: u8,
    pub starting_tonics: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalVictoryRestartCommit {
    pub inventory_cleanup: RestartCleanup,
    pub new_run_ordinal: u32,
    pub seed: u64,
    pub restart_elapsed_ticks: u64,
    pub equipped_starter_items: u8,
    pub starting_tonics: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalRunLifecycle {
    encounter: BellLaboratoryEncounter,
    inventory: PrototypeInventory,
    phase: LocalRunPhase,
    trace: VecDeque<LocalCombatTraceEntry>,
    next_death_ordinal: u32,
}

impl LocalRunLifecycle {
    pub fn first_playable() -> Result<Self, LocalDeathError> {
        let encounter = BellLaboratoryEncounter::new_default_seed();
        Ok(Self {
            inventory: PrototypeInventory::first_playable_loadout(encounter.run_ordinal())?,
            encounter,
            phase: LocalRunPhase::Alive,
            trace: VecDeque::new(),
            next_death_ordinal: 1,
        })
    }

    #[must_use]
    pub const fn encounter(&self) -> &BellLaboratoryEncounter {
        &self.encounter
    }

    #[must_use]
    pub const fn inventory(&self) -> &PrototypeInventory {
        &self.inventory
    }

    /// Mutable seam for the local pickup/reward transaction owner while the run is alive.
    pub fn inventory_mut(&mut self) -> Result<&mut PrototypeInventory, LocalDeathError> {
        if !self.is_alive() {
            return Err(LocalDeathError::InventoryUnavailableAfterDeath);
        }
        Ok(&mut self.inventory)
    }

    #[must_use]
    pub const fn phase(&self) -> &LocalRunPhase {
        &self.phase
    }

    #[must_use]
    pub fn trace(&self) -> Vec<LocalCombatTraceEntry> {
        self.trace.iter().cloned().collect()
    }

    #[must_use]
    pub const fn is_alive(&self) -> bool {
        matches!(self.phase, LocalRunPhase::Alive)
    }

    /// Advances the authoritative encounter while the local character is alive.
    ///
    /// Death and restart remain dedicated atomic transactions; callers cannot smuggle either
    /// action through the ordinary encounter seam. Invalid input leaves this lifecycle unchanged.
    pub fn advance_encounter(
        &mut self,
        input: EncounterInput,
    ) -> Result<EncounterStep, LocalDeathError> {
        if !self.is_alive() {
            return Err(LocalDeathError::EncounterUnavailableAfterDeath);
        }
        if matches!(
            input.action,
            EncounterAction::PlayerDied | EncounterAction::RunAgain
        ) {
            return Err(LocalDeathError::ReservedEncounterAction);
        }
        let mut staged = self.clone();
        let step = staged.encounter.step(input)?;
        *self = staged;
        Ok(step)
    }

    /// Records one authoritative damage event and atomically freezes/cleans up on lethality.
    pub fn observe_damage(
        &mut self,
        observation: LocalDamageObservation,
        entities: RunEntityCounts,
    ) -> Result<Option<LocalDeathCommit>, LocalDeathError> {
        let mut staged = self.clone();
        let entry = staged.validate_and_compile_observation(observation)?;
        staged.push_trace(entry.clone());
        if !entry.lethal {
            *self = staged;
            return Ok(None);
        }
        if !staged.is_alive() {
            return Err(LocalDeathError::DeathAlreadyCommitted);
        }
        let death_id = staged.allocate_death_id()?;
        staged.encounter.step(EncounterInput {
            action: EncounterAction::PlayerDied,
            ..EncounterInput::default()
        })?;
        if staged.encounter.state() != EncounterState::DeathFrozen {
            return Err(LocalDeathError::EncounterDidNotFreeze);
        }
        let inventory_cleanup = staged.inventory.clear_for_restart();
        let cause = LocalDeathCause {
            death_id,
            lethal: entry,
        };
        staged.phase = LocalRunPhase::Dead(cause.clone());
        let cleared_entity_total = entities.total()?;
        let commit = LocalDeathCommit {
            cause,
            trace: staged.trace(),
            inventory_cleanup,
            cleared_entities: entities,
            cleared_entity_total,
        };
        *self = staged;
        Ok(Some(commit))
    }

    /// Executes the explicit Run Again transaction. Invalid/duplicate requests are nonmutating.
    pub fn restart(&mut self, requested_at: Tick) -> Result<LocalRestartCommit, LocalDeathError> {
        let LocalRunPhase::Dead(cause) = &self.phase else {
            return Err(LocalDeathError::RestartUnavailable);
        };
        let previous_death_id = cause.death_id;
        let mut staged = self.clone();
        let before_tick = staged.encounter.tick();
        if requested_at != before_tick {
            return Err(LocalDeathError::RestartTickMismatch {
                expected: before_tick,
                actual: requested_at,
            });
        }
        staged.encounter.step(EncounterInput {
            action: EncounterAction::RunAgain,
            ..EncounterInput::default()
        })?;
        let elapsed = staged.encounter.tick().0 - requested_at.0;
        if elapsed > LOCAL_RESTART_DEADLINE_TICKS {
            return Err(LocalDeathError::RestartDeadlineExceeded {
                elapsed_ticks: elapsed,
            });
        }
        if staged.encounter.tick() <= before_tick
            || staged.encounter.state() != EncounterState::AwaitingFirstActivity
        {
            return Err(LocalDeathError::EncounterDidNotRestart);
        }
        let run_ordinal = staged.encounter.run_ordinal();
        staged.inventory = PrototypeInventory::first_playable_loadout(run_ordinal)?;
        staged.phase = LocalRunPhase::Alive;
        staged.trace.clear();
        let equipped_starter_items = u8::try_from(
            staged
                .inventory
                .equipped()
                .iter()
                .filter(|slot| slot.is_some())
                .count(),
        )
        .map_err(|_| LocalDeathError::StarterCountOverflow)?;
        let starting_tonics = staged
            .inventory
            .belt()
            .slots()
            .iter()
            .copied()
            .map(crate::BeltSlot::tonic_count)
            .try_fold(0_u8, u8::checked_add)
            .ok_or(LocalDeathError::StarterCountOverflow)?;
        let commit = LocalRestartCommit {
            previous_death_id,
            new_run_ordinal: run_ordinal,
            seed: staged.encounter.seed(),
            restart_elapsed_ticks: elapsed,
            equipped_starter_items,
            starting_tonics,
        };
        *self = staged;
        Ok(commit)
    }

    /// Executes Run Again from the completion summary or retained cleared arena.
    pub fn restart_after_victory(&mut self) -> Result<LocalVictoryRestartCommit, LocalDeathError> {
        if !self.is_alive()
            || !matches!(
                self.encounter.state(),
                EncounterState::CompletionSummary | EncounterState::ClearedArena
            )
        {
            return Err(LocalDeathError::VictoryRestartUnavailable);
        }
        let mut staged = self.clone();
        let before_tick = staged.encounter.tick();
        let inventory_cleanup = staged.inventory.clear_for_restart();
        staged.encounter.step(EncounterInput {
            action: EncounterAction::RunAgain,
            ..EncounterInput::default()
        })?;
        let elapsed = staged.encounter.tick().0 - before_tick.0;
        if elapsed > LOCAL_RESTART_DEADLINE_TICKS {
            return Err(LocalDeathError::RestartDeadlineExceeded {
                elapsed_ticks: elapsed,
            });
        }
        if staged.encounter.state() != EncounterState::AwaitingFirstActivity {
            return Err(LocalDeathError::EncounterDidNotRestart);
        }
        let run_ordinal = staged.encounter.run_ordinal();
        staged.inventory = PrototypeInventory::first_playable_loadout(run_ordinal)?;
        staged.trace.clear();
        let equipped_starter_items = u8::try_from(
            staged
                .inventory
                .equipped()
                .iter()
                .filter(|slot| slot.is_some())
                .count(),
        )
        .map_err(|_| LocalDeathError::StarterCountOverflow)?;
        let starting_tonics = staged
            .inventory
            .belt()
            .slots()
            .iter()
            .copied()
            .map(crate::BeltSlot::tonic_count)
            .try_fold(0_u8, u8::checked_add)
            .ok_or(LocalDeathError::StarterCountOverflow)?;
        let commit = LocalVictoryRestartCommit {
            inventory_cleanup,
            new_run_ordinal: run_ordinal,
            seed: staged.encounter.seed(),
            restart_elapsed_ticks: elapsed,
            equipped_starter_items,
            starting_tonics,
        };
        *self = staged;
        Ok(commit)
    }

    fn validate_and_compile_observation(
        &self,
        observation: LocalDamageObservation,
    ) -> Result<LocalCombatTraceEntry, LocalDeathError> {
        if !self.is_alive() {
            return Err(LocalDeathError::DeathAlreadyCommitted);
        }
        if !valid_pattern_id(&observation.pattern_id) {
            return Err(LocalDeathError::InvalidPatternId);
        }
        if !observation.source_position.is_finite() {
            return Err(LocalDeathError::InvalidSourcePosition);
        }
        if observation.damage.lethal != (observation.damage.health_after == 0) {
            return Err(LocalDeathError::LethalFlagMismatch);
        }
        if let Some(previous) = self.trace.back()
            && observation.tick < previous.tick
        {
            return Err(LocalDeathError::TraceTickRegressed);
        }
        Ok(LocalCombatTraceEntry {
            tick: observation.tick,
            source: observation.damage.source,
            pattern_id: observation.pattern_id,
            raw_damage: observation.damage.raw_damage,
            final_damage: observation.damage.health_damage_applied,
            damage_type: observation.damage.damage_type,
            health_before: observation.damage.health_before,
            health_after: observation.damage.health_after,
            source_position: observation.source_position,
            lethal: observation.damage.lethal,
        })
    }

    fn push_trace(&mut self, entry: LocalCombatTraceEntry) {
        let oldest_allowed = entry.tick.0.saturating_sub(LOCAL_DEATH_TRACE_TICKS);
        while self
            .trace
            .front()
            .is_some_and(|existing| existing.tick.0 < oldest_allowed)
        {
            self.trace.pop_front();
        }
        self.trace.push_back(entry);
    }

    fn allocate_death_id(&mut self) -> Result<LocalDeathId, LocalDeathError> {
        let id = u64::from(self.encounter.run_ordinal())
            .checked_shl(32)
            .and_then(|prefix| prefix.checked_add(u64::from(self.next_death_ordinal)))
            .ok_or(LocalDeathError::DeathIdOverflow)?;
        self.next_death_ordinal = self
            .next_death_ordinal
            .checked_add(1)
            .ok_or(LocalDeathError::DeathIdOverflow)?;
        Ok(LocalDeathId(id))
    }
}

fn valid_pattern_id(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LocalDeathError {
    #[error("local death was already committed")]
    DeathAlreadyCommitted,
    #[error("Run Again is unavailable while the local run is alive")]
    RestartUnavailable,
    #[error("victory Run Again requires the completion summary or cleared arena")]
    VictoryRestartUnavailable,
    #[error("run inventory is unavailable after local death")]
    InventoryUnavailableAfterDeath,
    #[error("encounter progression is unavailable after local death")]
    EncounterUnavailableAfterDeath,
    #[error("death and restart must use their dedicated lifecycle transactions")]
    ReservedEncounterAction,
    #[error("pattern ID is invalid")]
    InvalidPatternId,
    #[error("lethal source position is nonfinite or outside fixed range")]
    InvalidSourcePosition,
    #[error("damage lethal flag disagrees with final health")]
    LethalFlagMismatch,
    #[error("combat trace tick regressed")]
    TraceTickRegressed,
    #[error("local death ID overflowed")]
    DeathIdOverflow,
    #[error("run entity count overflowed")]
    EntityCountOverflow,
    #[error("encounter did not enter the frozen death state")]
    EncounterDidNotFreeze,
    #[error("encounter did not return to a fresh awaiting-activity state")]
    EncounterDidNotRestart,
    #[error("restart exceeded 90 fixed ticks: {elapsed_ticks}")]
    RestartDeadlineExceeded { elapsed_ticks: u64 },
    #[error("Run Again tick mismatch: expected {expected:?}, received {actual:?}")]
    RestartTickMismatch { expected: Tick, actual: Tick },
    #[error("starter item/Tonic count overflowed")]
    StarterCountOverflow,
    #[error(transparent)]
    Encounter(#[from] EncounterError),
    #[error(transparent)]
    Inventory(#[from] InventoryError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FIRST_PLAYABLE_DEFAULT_SEED;
    use crate::{
        DamageBand, DirectHitParameters, DirectHitRequest, EquipmentItem, EquipmentSlot,
        FieldPickup, FieldPickupId, ItemContentId, ItemInstanceId, PlacementChoice,
        resolve_direct_hit,
    };

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero")
    }

    fn damage(source: u64, health: u32, raw_damage: u32) -> DamageEvent {
        resolve_direct_hit(
            &DirectHitRequest::new(DirectHitParameters {
                source: id(source),
                target: id(99),
                collision_confirmed: true,
                target_is_immune: false,
                raw_damage,
                damage_type: DamageType::Physical,
                attacker_multiplier_basis_points: 10_000,
                target_resistance_basis_points: 0,
                direct_damage_reductions_basis_points: Vec::new(),
                armor: 0,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
                current_health: health,
                max_health: 128,
            })
            .expect("request"),
        )
        .expect("damage")
    }

    fn observation(tick: u64, health: u32, raw_damage: u32) -> LocalDamageObservation {
        LocalDamageObservation {
            tick: Tick(tick),
            pattern_id: "pattern.enemy.test.hit".to_owned(),
            source_position: SimulationVector::new(8.0, 12.0),
            damage: damage(10, health, raw_damage),
        }
    }

    fn drive_to_completion(lifecycle: &mut LocalRunLifecycle) {
        for _ in 0..2_000 {
            let state = lifecycle.encounter().state();
            if state == EncounterState::CompletionSummary {
                return;
            }
            let input = match state {
                EncounterState::AwaitingFirstActivity => EncounterInput {
                    player_moved: true,
                    ..EncounterInput::default()
                },
                EncounterState::Active { .. } => EncounterInput {
                    defeated: lifecycle.encounter().active_instances(),
                    ..EncounterInput::default()
                },
                EncounterState::RewardOpen { .. } => EncounterInput {
                    action: EncounterAction::CloseRewardPanel,
                    ..EncounterInput::default()
                },
                _ => EncounterInput::default(),
            };
            lifecycle.advance_encounter(input).expect("journey step");
        }
        panic!("completion summary did not open");
    }

    #[test]
    fn trace_retains_exact_prior_ten_second_window() {
        let mut lifecycle = LocalRunLifecycle::first_playable().expect("lifecycle");
        for tick in [1, 100, 301, 302] {
            assert!(
                lifecycle
                    .observe_damage(observation(tick, 128, 1), RunEntityCounts::default())
                    .expect("nonlethal")
                    .is_none()
            );
        }
        assert_eq!(
            lifecycle
                .trace()
                .iter()
                .map(|entry| entry.tick)
                .collect::<Vec<_>>(),
            vec![Tick(100), Tick(301), Tick(302)]
        );
    }

    #[test]
    fn victory_run_again_is_atomic_from_summary_or_cleared_arena() {
        let mut lifecycle = LocalRunLifecycle::first_playable().expect("lifecycle");
        drive_to_completion(&mut lifecycle);
        let first = lifecycle
            .restart_after_victory()
            .expect("summary Run Again");
        assert_eq!(first.new_run_ordinal, 2);
        assert_eq!(first.seed, FIRST_PLAYABLE_DEFAULT_SEED);
        assert_eq!(first.restart_elapsed_ticks, 1);
        assert_eq!(first.equipped_starter_items, 3);
        assert_eq!(first.starting_tonics, 2);
        assert_eq!(
            lifecycle.encounter().state(),
            EncounterState::AwaitingFirstActivity
        );
        assert!(matches!(
            lifecycle.restart_after_victory(),
            Err(LocalDeathError::VictoryRestartUnavailable)
        ));

        drive_to_completion(&mut lifecycle);
        lifecycle
            .advance_encounter(EncounterInput {
                action: EncounterAction::CloseCompletionSummary,
                ..EncounterInput::default()
            })
            .expect("Escape retains cleared arena");
        assert_eq!(lifecycle.encounter().state(), EncounterState::ClearedArena);
        let second = lifecycle
            .restart_after_victory()
            .expect("pause/cleared Run Again");
        assert_eq!(second.new_run_ordinal, 3);
    }

    #[test]
    fn ordinary_encounter_progression_is_owned_and_transactional() {
        let mut lifecycle = LocalRunLifecycle::first_playable().expect("lifecycle");
        let step = lifecycle
            .advance_encounter(EncounterInput {
                player_moved: true,
                ..EncounterInput::default()
            })
            .expect("first activity");
        assert_eq!(step.tick, Tick(1));
        assert!(matches!(
            lifecycle.encounter().state(),
            EncounterState::FirstWaveDelay { .. }
        ));

        let before = lifecycle.clone();
        assert_eq!(
            lifecycle.advance_encounter(EncounterInput {
                action: EncounterAction::PlayerDied,
                ..EncounterInput::default()
            }),
            Err(LocalDeathError::ReservedEncounterAction)
        );
        assert_eq!(lifecycle, before);
    }

    #[test]
    fn lethal_commit_freezes_encounter_and_clears_all_owned_inventory_once() {
        let mut lifecycle = LocalRunLifecycle::first_playable().expect("lifecycle");
        let mut pickup = FieldPickup::new(
            FieldPickupId::new(50).expect("pickup"),
            crate::InventoryStack::Equipment(EquipmentItem::new(
                ItemInstanceId::new(50).expect("item"),
                ItemContentId::new("item.prototype.charm.test").expect("content"),
                EquipmentSlot::Charm,
            )),
            SimulationVector::new(0.0, 0.0),
            Tick(1),
        )
        .expect("field pickup");
        lifecycle
            .inventory
            .apply_field_pickup(
                &mut pickup,
                PlacementChoice::Take,
                SimulationVector::new(0.0, 0.0),
                crate::FieldPickupAccess::Automatic,
                Tick(1),
            )
            .expect("take");
        let entities = RunEntityCounts {
            enemies: 3,
            hostile_projectiles: 7,
            hostile_hazards: 1,
            friendly_projectiles: 2,
            field_pickups: 1,
            reward_entities: 1,
            transient_effects: 4,
        };
        let commit = lifecycle
            .observe_damage(observation(30, 8, 8), entities)
            .expect("lethal")
            .expect("death commit");
        assert_eq!(commit.cleared_entity_total, 19);
        assert_eq!(commit.inventory_cleanup.removed_stacks.len(), 4);
        assert_eq!(commit.inventory_cleanup.cleared_belt_tonics, 2);
        assert_eq!(lifecycle.encounter().state(), EncounterState::DeathFrozen);
        assert!(lifecycle.inventory().equipped().iter().all(Option::is_none));
        assert!(lifecycle.inventory().backpack().iter().all(Option::is_none));
        assert_eq!(
            lifecycle
                .inventory()
                .belt()
                .slots()
                .iter()
                .copied()
                .map(crate::BeltSlot::tonic_count)
                .sum::<u8>(),
            0
        );
        assert!(matches!(lifecycle.phase(), LocalRunPhase::Dead(_)));
        let frozen = lifecycle.clone();
        assert_eq!(
            lifecycle.observe_damage(observation(31, 8, 8), entities),
            Err(LocalDeathError::DeathAlreadyCommitted)
        );
        assert_eq!(lifecycle, frozen);
    }

    #[test]
    fn explicit_restart_is_fresh_qualified_and_duplicate_safe() {
        let mut lifecycle = LocalRunLifecycle::first_playable().expect("lifecycle");
        let death = lifecycle
            .observe_damage(
                observation(30, 8, 8),
                RunEntityCounts {
                    enemies: 3,
                    ..RunEntityCounts::default()
                },
            )
            .expect("lethal")
            .expect("commit");
        let restart = lifecycle
            .restart(lifecycle.encounter().tick())
            .expect("restart");
        assert_eq!(restart.previous_death_id, death.cause.death_id);
        assert_eq!(restart.new_run_ordinal, 2);
        assert_eq!(restart.seed, FIRST_PLAYABLE_DEFAULT_SEED);
        assert!(restart.restart_elapsed_ticks <= LOCAL_RESTART_DEADLINE_TICKS);
        assert_eq!(restart.equipped_starter_items, 3);
        assert_eq!(restart.starting_tonics, 2);
        assert!(lifecycle.is_alive());
        assert!(lifecycle.trace().is_empty());
        assert_eq!(
            lifecycle.restart(lifecycle.encounter().tick()),
            Err(LocalDeathError::RestartUnavailable)
        );
    }

    #[test]
    fn invalid_observation_and_entity_overflow_are_transactional() {
        let mut lifecycle = LocalRunLifecycle::first_playable().expect("lifecycle");
        let before = lifecycle.clone();
        let mut invalid = observation(1, 128, 8);
        invalid.pattern_id = "BAD PATTERN".to_owned();
        assert_eq!(
            lifecycle.observe_damage(invalid, RunEntityCounts::default()),
            Err(LocalDeathError::InvalidPatternId)
        );
        assert_eq!(lifecycle, before);
        assert_eq!(
            lifecycle.observe_damage(
                observation(1, 8, 8),
                RunEntityCounts {
                    enemies: u32::MAX,
                    hostile_projectiles: 1,
                    ..RunEntityCounts::default()
                }
            ),
            Err(LocalDeathError::EntityCountOverflow)
        );
        assert_eq!(lifecycle, before);
    }

    #[test]
    fn fixed_death_restart_replay_is_identical() {
        fn replay() -> (LocalDeathCommit, LocalRestartCommit, LocalRunLifecycle) {
            let mut lifecycle = LocalRunLifecycle::first_playable().expect("lifecycle");
            lifecycle
                .observe_damage(observation(10, 128, 10), RunEntityCounts::default())
                .expect("chip");
            let death = lifecycle
                .observe_damage(
                    observation(20, 8, 8),
                    RunEntityCounts {
                        enemies: 3,
                        hostile_projectiles: 4,
                        friendly_projectiles: 1,
                        ..RunEntityCounts::default()
                    },
                )
                .expect("lethal")
                .expect("commit");
            let restart = lifecycle
                .restart(lifecycle.encounter().tick())
                .expect("restart");
            (death, restart, lifecycle)
        }
        assert_eq!(replay(), replay());
        assert_eq!(DamageBand::Chip, DamageBand::Chip);
    }
}
