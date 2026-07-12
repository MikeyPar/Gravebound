//! Deterministic First Playable Bell Laboratory encounter director.
//!
//! This module owns only authored encounter flow and stable events. Enemy AI, rendering, rewards,
//! inventory mutation, and death presentation consume these events in later integration tickets.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::Tick;

pub const FIRST_PLAYABLE_DEFAULT_SEED: u64 = 0xB311_A501;
pub const FIRST_WAVE_DELAY_TICKS: u64 = 45;
pub const SPAWN_TELEGRAPH_TICKS: u64 = 27;
pub const REWARD_DELAY_TICKS: u64 = 45;
pub const BOSS_INTRODUCTION_TICKS: u64 = 60;

pub const DROWNED_PILGRIM_ID: &str = "enemy.drowned_pilgrim";
pub const BELL_REED_ID: &str = "enemy.bell_reed";
pub const CHAIN_SENTRY_ID: &str = "enemy.chain_sentry";
pub const BELL_PROCTOR_ID: &str = "boss.prototype.bell_proctor";

pub const WAVE_1_REWARD_ID: &str = "reward.prototype.wave_1";
pub const WAVE_2_REWARD_ID: &str = "reward.prototype.wave_2";
pub const WAVE_3_REWARD_ID: &str = "reward.prototype.wave_3";
pub const BOSS_REWARD_ID: &str = "reward.prototype.boss";

/// Authored wave or benchmark boss stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EncounterStage {
    Wave1,
    Wave2,
    Wave3,
    Boss,
}

impl EncounterStage {
    #[must_use]
    pub const fn budget(self) -> u32 {
        match self {
            Self::Wave1 => 4,
            Self::Wave2 => 10,
            Self::Wave3 => 15,
            Self::Boss => 0,
        }
    }

    #[must_use]
    pub const fn reward_id(self) -> &'static str {
        match self {
            Self::Wave1 => WAVE_1_REWARD_ID,
            Self::Wave2 => WAVE_2_REWARD_ID,
            Self::Wave3 => WAVE_3_REWARD_ID,
            Self::Boss => BOSS_REWARD_ID,
        }
    }
}

/// Run-qualified stable spawn identity. Ordinals are authored and repeat only in a new run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpawnInstanceId {
    pub run_ordinal: u32,
    pub spawn_ordinal: u16,
}

/// Authored anchor or exact arena-local point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpawnLocation {
    Anchor(&'static str),
    PointMilliTiles { x: i32, y: i32 },
}

/// Stable data-only spawn request emitted after its ground telegraph begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EncounterSpawnSpec {
    pub instance_id: SpawnInstanceId,
    pub content_id: &'static str,
    pub location: SpawnLocation,
    pub budget_cost: u32,
}

/// Externally visible encounter phase without leaking presentation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncounterState {
    AwaitingFirstActivity,
    FirstWaveDelay {
        starts_at: Tick,
    },
    SpawnTelegraph {
        stage: EncounterStage,
        activates_at: Tick,
    },
    Active {
        stage: EncounterStage,
        remaining_hostiles: u16,
    },
    RewardDelay {
        completed_stage: EncounterStage,
        opens_at: Tick,
    },
    RewardOpen {
        completed_stage: EncounterStage,
    },
    BossIntroduction {
        activates_at: Tick,
    },
    DeathFrozen,
    CompletionSummary,
    ClearedArena,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum EncounterAction {
    #[default]
    None,
    CloseRewardPanel,
    Recall,
    CloseCompletionSummary,
    RunAgain,
    PlayerDied,
}

/// One fixed-tick input. Defeat IDs must be strictly sorted and unique.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EncounterInput {
    pub player_moved: bool,
    pub player_fired: bool,
    pub defeated: Vec<SpawnInstanceId>,
    pub action: EncounterAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartReason {
    Death,
    RunAgain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallRejection {
    CombatLaboratoryUnavailable,
}

impl RecallRejection {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::CombatLaboratoryUnavailable => "recall_unavailable_combat_laboratory",
        }
    }
}

/// Stable authoritative event stream consumed by enemy, reward, UI, and trace integrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncounterEvent {
    FirstActivityObserved {
        tick: Tick,
        wave_starts_at: Tick,
    },
    SpawnTelegraphStarted {
        tick: Tick,
        stage: EncounterStage,
        activates_at: Tick,
        spawns: Vec<EncounterSpawnSpec>,
    },
    BossIntroductionStarted {
        tick: Tick,
        activates_at: Tick,
        spawn: EncounterSpawnSpec,
    },
    HostilesActivated {
        tick: Tick,
        stage: EncounterStage,
        instances: Vec<SpawnInstanceId>,
    },
    HostileDefeatAccepted {
        tick: Tick,
        instance: SpawnInstanceId,
        remaining_hostiles: u16,
    },
    HostileProjectilesCleared {
        tick: Tick,
        completed_stage: EncounterStage,
    },
    RewardDelayStarted {
        tick: Tick,
        completed_stage: EncounterStage,
        reward_id: &'static str,
        opens_at: Tick,
    },
    RewardPanelOpened {
        tick: Tick,
        completed_stage: EncounterStage,
        reward_id: &'static str,
    },
    RewardPanelClosed {
        tick: Tick,
        completed_stage: EncounterStage,
    },
    RecallRejected {
        tick: Tick,
        reason: RecallRejection,
    },
    CompletionSummaryOpened {
        tick: Tick,
        reward_id: &'static str,
        clear_ticks: u64,
        best_clear_ticks: u64,
    },
    CompletionSummaryClosed {
        tick: Tick,
    },
    PlayerDeathAccepted {
        tick: Tick,
        run_ordinal: u32,
        cleared_instances: Vec<SpawnInstanceId>,
    },
    RunRestarted {
        tick: Tick,
        reason: RestartReason,
        run_ordinal: u32,
        seed: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncounterStep {
    pub tick: Tick,
    pub events: Vec<EncounterEvent>,
}

/// Simulation-owned run director. Clone-before-step provides transactional fixed-tick mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellLaboratoryEncounter {
    tick: Tick,
    state: EncounterState,
    run_ordinal: u32,
    seed: u64,
    first_activity_tick: Option<Tick>,
    best_clear_ticks: Option<u64>,
    active_instances: BTreeSet<SpawnInstanceId>,
}

impl BellLaboratoryEncounter {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            tick: Tick(0),
            state: EncounterState::AwaitingFirstActivity,
            run_ordinal: 1,
            seed,
            first_activity_tick: None,
            best_clear_ticks: None,
            active_instances: BTreeSet::new(),
        }
    }

    #[must_use]
    pub const fn new_default_seed() -> Self {
        Self::new(FIRST_PLAYABLE_DEFAULT_SEED)
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn state(&self) -> EncounterState {
        self.state
    }

    #[must_use]
    pub const fn run_ordinal(&self) -> u32 {
        self.run_ordinal
    }

    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    #[must_use]
    pub const fn best_clear_ticks(&self) -> Option<u64> {
        self.best_clear_ticks
    }

    #[must_use]
    pub fn active_instances(&self) -> Vec<SpawnInstanceId> {
        self.active_instances.iter().copied().collect()
    }

    /// Advances one fixed tick. Invalid input leaves the encounter bit-for-bit unchanged.
    pub fn step(&mut self, input: EncounterInput) -> Result<EncounterStep, EncounterError> {
        validate_input(&input)?;
        let mut staged = self.clone();
        let result = staged.step_staged(input)?;
        *self = staged;
        Ok(result)
    }

    fn step_staged(&mut self, input: EncounterInput) -> Result<EncounterStep, EncounterError> {
        let EncounterInput {
            player_moved,
            player_fired,
            defeated,
            action,
        } = input;
        self.tick = self
            .tick
            .checked_next()
            .ok_or(EncounterError::TickOverflow)?;
        let mut events = Vec::new();
        self.advance_deadline(&mut events)?;

        if action == EncounterAction::PlayerDied {
            if !defeated.is_empty() {
                return Err(EncounterError::DeathCombinedWithDefeats);
            }
            if self.state == EncounterState::DeathFrozen {
                return Err(EncounterError::DeathAlreadyAccepted);
            }
            let cleared_instances = self.active_instances();
            self.active_instances.clear();
            self.state = EncounterState::DeathFrozen;
            events.push(EncounterEvent::PlayerDeathAccepted {
                tick: self.tick,
                run_ordinal: self.run_ordinal,
                cleared_instances,
            });
            return Ok(EncounterStep {
                tick: self.tick,
                events,
            });
        }

        self.accept_defeats(&defeated, &mut events)?;

        if self.state == EncounterState::AwaitingFirstActivity && (player_moved || player_fired) {
            let starts_at = checked_tick_add(self.tick, FIRST_WAVE_DELAY_TICKS)?;
            self.first_activity_tick = Some(self.tick);
            self.state = EncounterState::FirstWaveDelay { starts_at };
            events.push(EncounterEvent::FirstActivityObserved {
                tick: self.tick,
                wave_starts_at: starts_at,
            });
        }

        self.process_action(action, &mut events)?;
        self.validate_invariants()?;
        Ok(EncounterStep {
            tick: self.tick,
            events,
        })
    }

    fn advance_deadline(&mut self, events: &mut Vec<EncounterEvent>) -> Result<(), EncounterError> {
        match self.state {
            EncounterState::FirstWaveDelay { starts_at } if self.tick >= starts_at => {
                self.start_wave(EncounterStage::Wave1, events)?;
            }
            EncounterState::SpawnTelegraph {
                stage,
                activates_at,
            } if self.tick >= activates_at => self.activate(stage, events)?,
            EncounterState::RewardDelay {
                completed_stage,
                opens_at,
            } if self.tick >= opens_at => {
                self.state = EncounterState::RewardOpen { completed_stage };
                events.push(EncounterEvent::RewardPanelOpened {
                    tick: self.tick,
                    completed_stage,
                    reward_id: completed_stage.reward_id(),
                });
            }
            EncounterState::BossIntroduction { activates_at } if self.tick >= activates_at => {
                self.activate(EncounterStage::Boss, events)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn start_wave(
        &mut self,
        stage: EncounterStage,
        events: &mut Vec<EncounterEvent>,
    ) -> Result<(), EncounterError> {
        debug_assert!(stage != EncounterStage::Boss);
        let spawns = spawn_specs(stage, self.run_ordinal);
        validate_spawn_budget(stage, &spawns)?;
        let activates_at = checked_tick_add(self.tick, SPAWN_TELEGRAPH_TICKS)?;
        self.state = EncounterState::SpawnTelegraph {
            stage,
            activates_at,
        };
        events.push(EncounterEvent::SpawnTelegraphStarted {
            tick: self.tick,
            stage,
            activates_at,
            spawns,
        });
        Ok(())
    }

    fn start_boss_introduction(
        &mut self,
        events: &mut Vec<EncounterEvent>,
    ) -> Result<(), EncounterError> {
        let spawn = spawn_specs(EncounterStage::Boss, self.run_ordinal)
            .into_iter()
            .next()
            .ok_or(EncounterError::MissingAuthoredSpawn)?;
        let activates_at = checked_tick_add(self.tick, BOSS_INTRODUCTION_TICKS)?;
        self.state = EncounterState::BossIntroduction { activates_at };
        events.push(EncounterEvent::BossIntroductionStarted {
            tick: self.tick,
            activates_at,
            spawn,
        });
        Ok(())
    }

    fn activate(
        &mut self,
        stage: EncounterStage,
        events: &mut Vec<EncounterEvent>,
    ) -> Result<(), EncounterError> {
        let spawns = spawn_specs(stage, self.run_ordinal);
        if spawns.is_empty() {
            return Err(EncounterError::MissingAuthoredSpawn);
        }
        self.active_instances = spawns.iter().map(|spawn| spawn.instance_id).collect();
        let remaining_hostiles = u16::try_from(self.active_instances.len())
            .map_err(|_| EncounterError::HostileCountOverflow)?;
        self.state = EncounterState::Active {
            stage,
            remaining_hostiles,
        };
        events.push(EncounterEvent::HostilesActivated {
            tick: self.tick,
            stage,
            instances: self.active_instances(),
        });
        Ok(())
    }

    fn accept_defeats(
        &mut self,
        defeated: &[SpawnInstanceId],
        events: &mut Vec<EncounterEvent>,
    ) -> Result<(), EncounterError> {
        if defeated.is_empty() {
            return Ok(());
        }
        let EncounterState::Active { stage, .. } = self.state else {
            return Err(EncounterError::DefeatOutsideActiveCombat);
        };
        if defeated
            .iter()
            .any(|instance| !self.active_instances.contains(instance))
        {
            return Err(EncounterError::UnknownActiveHostile);
        }
        for instance in defeated {
            self.active_instances.remove(instance);
            let remaining_hostiles = u16::try_from(self.active_instances.len())
                .map_err(|_| EncounterError::HostileCountOverflow)?;
            events.push(EncounterEvent::HostileDefeatAccepted {
                tick: self.tick,
                instance: *instance,
                remaining_hostiles,
            });
        }
        if self.active_instances.is_empty() {
            events.push(EncounterEvent::HostileProjectilesCleared {
                tick: self.tick,
                completed_stage: stage,
            });
            if stage == EncounterStage::Boss {
                self.complete_run(events)?;
            } else {
                let opens_at = checked_tick_add(self.tick, REWARD_DELAY_TICKS)?;
                self.state = EncounterState::RewardDelay {
                    completed_stage: stage,
                    opens_at,
                };
                events.push(EncounterEvent::RewardDelayStarted {
                    tick: self.tick,
                    completed_stage: stage,
                    reward_id: stage.reward_id(),
                    opens_at,
                });
            }
        } else {
            self.state = EncounterState::Active {
                stage,
                remaining_hostiles: u16::try_from(self.active_instances.len())
                    .map_err(|_| EncounterError::HostileCountOverflow)?,
            };
        }
        Ok(())
    }

    fn process_action(
        &mut self,
        action: EncounterAction,
        events: &mut Vec<EncounterEvent>,
    ) -> Result<(), EncounterError> {
        match action {
            EncounterAction::None => Ok(()),
            EncounterAction::Recall => {
                events.push(EncounterEvent::RecallRejected {
                    tick: self.tick,
                    reason: RecallRejection::CombatLaboratoryUnavailable,
                });
                Ok(())
            }
            EncounterAction::CloseRewardPanel => {
                let EncounterState::RewardOpen { completed_stage } = self.state else {
                    return Err(EncounterError::RewardPanelNotOpen);
                };
                events.push(EncounterEvent::RewardPanelClosed {
                    tick: self.tick,
                    completed_stage,
                });
                match completed_stage {
                    EncounterStage::Wave1 => self.start_wave(EncounterStage::Wave2, events),
                    EncounterStage::Wave2 => self.start_wave(EncounterStage::Wave3, events),
                    EncounterStage::Wave3 => self.start_boss_introduction(events),
                    EncounterStage::Boss => Err(EncounterError::InvalidCompletedStage),
                }
            }
            EncounterAction::CloseCompletionSummary => {
                if self.state != EncounterState::CompletionSummary {
                    return Err(EncounterError::CompletionSummaryNotOpen);
                }
                self.state = EncounterState::ClearedArena;
                events.push(EncounterEvent::CompletionSummaryClosed { tick: self.tick });
                Ok(())
            }
            EncounterAction::RunAgain => {
                if !matches!(
                    self.state,
                    EncounterState::DeathFrozen
                        | EncounterState::CompletionSummary
                        | EncounterState::ClearedArena
                ) {
                    return Err(EncounterError::RunAgainUnavailable);
                }
                self.restart(RestartReason::RunAgain, events)
            }
            EncounterAction::PlayerDied => unreachable!("handled before defeat processing"),
        }
    }

    fn complete_run(&mut self, events: &mut Vec<EncounterEvent>) -> Result<(), EncounterError> {
        let first_activity = self
            .first_activity_tick
            .ok_or(EncounterError::MissingFirstActivity)?;
        let clear_ticks = self
            .tick
            .0
            .checked_sub(first_activity.0)
            .ok_or(EncounterError::TickOverflow)?;
        let best_clear_ticks = self
            .best_clear_ticks
            .map_or(clear_ticks, |best| best.min(clear_ticks));
        self.best_clear_ticks = Some(best_clear_ticks);
        self.state = EncounterState::CompletionSummary;
        events.push(EncounterEvent::CompletionSummaryOpened {
            tick: self.tick,
            reward_id: BOSS_REWARD_ID,
            clear_ticks,
            best_clear_ticks,
        });
        Ok(())
    }

    fn restart(
        &mut self,
        reason: RestartReason,
        events: &mut Vec<EncounterEvent>,
    ) -> Result<(), EncounterError> {
        self.run_ordinal = self
            .run_ordinal
            .checked_add(1)
            .ok_or(EncounterError::RunOrdinalOverflow)?;
        self.seed = FIRST_PLAYABLE_DEFAULT_SEED;
        self.first_activity_tick = None;
        self.active_instances.clear();
        self.state = EncounterState::AwaitingFirstActivity;
        events.push(EncounterEvent::RunRestarted {
            tick: self.tick,
            reason,
            run_ordinal: self.run_ordinal,
            seed: self.seed,
        });
        Ok(())
    }

    fn validate_invariants(&self) -> Result<(), EncounterError> {
        match self.state {
            EncounterState::Active {
                remaining_hostiles, ..
            } if usize::from(remaining_hostiles) != self.active_instances.len()
                || self.active_instances.is_empty() =>
            {
                Err(EncounterError::ActiveHostileStateMismatch)
            }
            EncounterState::Active { .. } => Ok(()),
            _ if !self.active_instances.is_empty() => {
                Err(EncounterError::HostilesOutsideActiveCombat)
            }
            _ => Ok(()),
        }
    }
}

fn validate_input(input: &EncounterInput) -> Result<(), EncounterError> {
    if input.defeated.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(EncounterError::DefeatIdsNotStrictlySorted);
    }
    Ok(())
}

fn checked_tick_add(tick: Tick, duration: u64) -> Result<Tick, EncounterError> {
    tick.0
        .checked_add(duration)
        .map(Tick)
        .ok_or(EncounterError::TickOverflow)
}

fn validate_spawn_budget(
    stage: EncounterStage,
    spawns: &[EncounterSpawnSpec],
) -> Result<(), EncounterError> {
    let actual = spawns.iter().try_fold(0_u32, |total, spawn| {
        total
            .checked_add(spawn.budget_cost)
            .ok_or(EncounterError::WaveBudgetOverflow)
    })?;
    if actual != stage.budget() {
        return Err(EncounterError::WaveBudgetMismatch {
            stage,
            expected: stage.budget(),
            actual,
        });
    }
    Ok(())
}

fn spawn_specs(stage: EncounterStage, run_ordinal: u32) -> Vec<EncounterSpawnSpec> {
    let spawn = |spawn_ordinal, content_id, location, budget_cost| EncounterSpawnSpec {
        instance_id: SpawnInstanceId {
            run_ordinal,
            spawn_ordinal,
        },
        content_id,
        location,
        budget_cost,
    };
    match stage {
        EncounterStage::Wave1 => vec![
            spawn(1, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("N1"), 1),
            spawn(2, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("N3"), 1),
            spawn(3, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("S1"), 1),
            spawn(4, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("S3"), 1),
        ],
        EncounterStage::Wave2 => vec![
            spawn(5, BELL_REED_ID, SpawnLocation::Anchor("N2"), 3),
            spawn(6, BELL_REED_ID, SpawnLocation::Anchor("S2"), 3),
            spawn(7, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("W1"), 1),
            spawn(8, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("W2"), 1),
            spawn(9, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("E1"), 1),
            spawn(10, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("E2"), 1),
        ],
        EncounterStage::Wave3 => vec![
            spawn(11, CHAIN_SENTRY_ID, SpawnLocation::Anchor("C"), 6),
            spawn(
                12,
                BELL_REED_ID,
                SpawnLocation::PointMilliTiles { x: 8_000, y: 6_000 },
                3,
            ),
            spawn(
                13,
                BELL_REED_ID,
                SpawnLocation::PointMilliTiles {
                    x: 8_000,
                    y: 18_000,
                },
                3,
            ),
            spawn(14, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("E1"), 1),
            spawn(15, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("E2"), 1),
            spawn(16, DROWNED_PILGRIM_ID, SpawnLocation::Anchor("N3"), 1),
        ],
        EncounterStage::Boss => vec![spawn(
            17,
            BELL_PROCTOR_ID,
            SpawnLocation::PointMilliTiles {
                x: 24_000,
                y: 12_000,
            },
            0,
        )],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EncounterError {
    #[error("encounter tick arithmetic overflowed")]
    TickOverflow,
    #[error("run ordinal overflowed")]
    RunOrdinalOverflow,
    #[error("defeat instance IDs must be strictly sorted and unique")]
    DefeatIdsNotStrictlySorted,
    #[error("hostile defeat was reported outside active combat")]
    DefeatOutsideActiveCombat,
    #[error("defeat referenced an instance not active in this run/stage")]
    UnknownActiveHostile,
    #[error("death input cannot be combined with hostile defeats")]
    DeathCombinedWithDefeats,
    #[error("reward panel close was requested while no reward panel was open")]
    RewardPanelNotOpen,
    #[error("completion summary close was requested while it was not open")]
    CompletionSummaryNotOpen,
    #[error("Run Again is available only after death or from a completed run")]
    RunAgainUnavailable,
    #[error("player death was already accepted for this frozen run")]
    DeathAlreadyAccepted,
    #[error("boss stage cannot be used as a completed normal-wave reward stage")]
    InvalidCompletedStage,
    #[error("authored encounter stage has no spawn")]
    MissingAuthoredSpawn,
    #[error("authored wave budget arithmetic overflowed")]
    WaveBudgetOverflow,
    #[error("{stage:?} budget expected {expected}, got {actual}")]
    WaveBudgetMismatch {
        stage: EncounterStage,
        expected: u32,
        actual: u32,
    },
    #[error("active hostile count exceeds u16")]
    HostileCountOverflow,
    #[error("active hostile state does not match tracked instances")]
    ActiveHostileStateMismatch,
    #[error("hostile instances exist outside active combat")]
    HostilesOutsideActiveCombat,
    #[error("completed run has no recorded first activity")]
    MissingFirstActivity,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> EncounterInput {
        EncounterInput::default()
    }

    fn step_until(encounter: &mut BellLaboratoryEncounter, tick: u64) -> EncounterStep {
        let mut last = None;
        while encounter.tick().0 < tick {
            last = Some(encounter.step(idle()).expect("idle step"));
        }
        last.expect("at least one step")
    }

    fn defeat_all(encounter: &mut BellLaboratoryEncounter) -> EncounterStep {
        encounter
            .step(EncounterInput {
                defeated: encounter.active_instances(),
                ..idle()
            })
            .expect("defeat active stage")
    }

    fn open_and_close_reward(encounter: &mut BellLaboratoryEncounter) {
        let EncounterState::RewardDelay { opens_at, .. } = encounter.state() else {
            panic!("reward delay expected");
        };
        step_until(encounter, opens_at.0);
        assert!(matches!(
            encounter.state(),
            EncounterState::RewardOpen { .. }
        ));
        encounter
            .step(EncounterInput {
                action: EncounterAction::CloseRewardPanel,
                ..idle()
            })
            .expect("close reward");
    }

    #[test]
    fn authored_durations_compile_to_exact_fixed_ticks() {
        assert_eq!(
            crate::duration_ms_to_ticks_ceil(1_500),
            FIRST_WAVE_DELAY_TICKS
        );
        assert_eq!(crate::duration_ms_to_ticks_ceil(900), SPAWN_TELEGRAPH_TICKS);
        assert_eq!(crate::duration_ms_to_ticks_ceil(1_500), REWARD_DELAY_TICKS);
        assert_eq!(
            crate::duration_ms_to_ticks_ceil(2_000),
            BOSS_INTRODUCTION_TICKS
        );
    }

    #[test]
    fn first_activity_waits_exactly_then_wave_one_telegraphs_and_activates() {
        let mut encounter = BellLaboratoryEncounter::new_default_seed();
        let observed = encounter
            .step(EncounterInput {
                player_moved: true,
                ..idle()
            })
            .expect("first activity");
        assert_eq!(observed.tick, Tick(1));
        assert_eq!(
            encounter.state(),
            EncounterState::FirstWaveDelay {
                starts_at: Tick(46)
            }
        );
        step_until(&mut encounter, 45);
        assert!(matches!(
            encounter.state(),
            EncounterState::FirstWaveDelay { .. }
        ));
        let telegraph = step_until(&mut encounter, 46);
        let EncounterEvent::SpawnTelegraphStarted {
            stage,
            activates_at,
            spawns,
            ..
        } = &telegraph.events[0]
        else {
            panic!("wave telegraph event");
        };
        assert_eq!(*stage, EncounterStage::Wave1);
        assert_eq!(*activates_at, Tick(73));
        assert_eq!(spawns.len(), 4);
        assert_eq!(spawns.iter().map(|spawn| spawn.budget_cost).sum::<u32>(), 4);
        step_until(&mut encounter, 72);
        assert!(encounter.active_instances().is_empty());
        let activated = step_until(&mut encounter, 73);
        assert!(matches!(
            activated.events[0],
            EncounterEvent::HostilesActivated {
                stage: EncounterStage::Wave1,
                ..
            }
        ));
        assert_eq!(encounter.active_instances().len(), 4);
    }

    #[test]
    fn all_normal_waves_have_exact_spawns_anchors_and_budgets() {
        let wave1 = spawn_specs(EncounterStage::Wave1, 7);
        let wave2 = spawn_specs(EncounterStage::Wave2, 7);
        let wave3 = spawn_specs(EncounterStage::Wave3, 7);
        validate_spawn_budget(EncounterStage::Wave1, &wave1).expect("wave1 budget");
        validate_spawn_budget(EncounterStage::Wave2, &wave2).expect("wave2 budget");
        validate_spawn_budget(EncounterStage::Wave3, &wave3).expect("wave3 budget");
        assert_eq!(wave1.len(), 4);
        assert_eq!(wave2.len(), 6);
        assert_eq!(wave3.len(), 6);
        assert_eq!(wave1[0].location, SpawnLocation::Anchor("N1"));
        assert_eq!(wave1[3].location, SpawnLocation::Anchor("S3"));
        assert_eq!(wave2[0].content_id, BELL_REED_ID);
        assert_eq!(wave2[5].location, SpawnLocation::Anchor("E2"));
        assert_eq!(wave3[0].content_id, CHAIN_SENTRY_ID);
        assert_eq!(
            wave3[1].location,
            SpawnLocation::PointMilliTiles { x: 8_000, y: 6_000 }
        );
        assert_eq!(wave3[5].location, SpawnLocation::Anchor("N3"));
    }

    #[test]
    fn completion_clears_projectiles_waits_and_opens_hostile_free_reward() {
        let mut encounter = BellLaboratoryEncounter::new_default_seed();
        encounter
            .step(EncounterInput {
                player_fired: true,
                ..idle()
            })
            .expect("first fire");
        step_until(&mut encounter, 73);
        let completed = defeat_all(&mut encounter);
        assert!(completed.events.iter().any(|event| matches!(
            event,
            EncounterEvent::HostileProjectilesCleared {
                completed_stage: EncounterStage::Wave1,
                ..
            }
        )));
        let EncounterState::RewardDelay { opens_at, .. } = encounter.state() else {
            panic!("reward delay");
        };
        assert_eq!(opens_at.0, completed.tick.0 + 45);
        assert!(encounter.active_instances().is_empty());
        let opened = step_until(&mut encounter, opens_at.0);
        assert!(matches!(
            opened.events[0],
            EncounterEvent::RewardPanelOpened {
                reward_id: WAVE_1_REWARD_ID,
                ..
            }
        ));
        assert!(encounter.active_instances().is_empty());
    }

    #[test]
    fn reward_close_starts_wave_two_and_three_immediately() {
        let mut encounter = BellLaboratoryEncounter::new_default_seed();
        encounter
            .step(EncounterInput {
                player_moved: true,
                ..idle()
            })
            .expect("activity");
        step_until(&mut encounter, 73);
        defeat_all(&mut encounter);
        open_and_close_reward(&mut encounter);
        let EncounterState::SpawnTelegraph {
            stage,
            activates_at,
        } = encounter.state()
        else {
            panic!("wave2 telegraph");
        };
        assert_eq!(stage, EncounterStage::Wave2);
        step_until(&mut encounter, activates_at.0);
        assert_eq!(encounter.active_instances().len(), 6);
        defeat_all(&mut encounter);
        open_and_close_reward(&mut encounter);
        assert!(matches!(
            encounter.state(),
            EncounterState::SpawnTelegraph {
                stage: EncounterStage::Wave3,
                ..
            }
        ));
    }

    #[test]
    fn wave_three_reward_close_runs_exact_boss_introduction() {
        let mut encounter = encounter_at_active_wave_three();
        defeat_all(&mut encounter);
        open_and_close_reward(&mut encounter);
        let EncounterState::BossIntroduction { activates_at } = encounter.state() else {
            panic!("boss introduction");
        };
        let close_tick = encounter.tick().0;
        assert_eq!(activates_at.0, close_tick + 60);
        assert!(encounter.active_instances().is_empty());
        step_until(&mut encounter, activates_at.0 - 1);
        assert!(encounter.active_instances().is_empty());
        let activated = step_until(&mut encounter, activates_at.0);
        assert!(matches!(
            activated.events[0],
            EncounterEvent::HostilesActivated {
                stage: EncounterStage::Boss,
                ..
            }
        ));
        assert_eq!(encounter.active_instances().len(), 1);
    }

    #[test]
    fn recall_is_always_a_typed_nonmutating_gameplay_rejection() {
        let mut encounter = BellLaboratoryEncounter::new_default_seed();
        let before_state = encounter.state();
        let step = encounter
            .step(EncounterInput {
                action: EncounterAction::Recall,
                ..idle()
            })
            .expect("typed recall rejection");
        assert_eq!(encounter.state(), before_state);
        assert_eq!(step.events.len(), 1);
        let EncounterEvent::RecallRejected { reason, .. } = step.events[0] else {
            panic!("recall rejection");
        };
        assert_eq!(reason.code(), "recall_unavailable_combat_laboratory");
    }

    #[test]
    fn boss_completion_summary_and_run_again_preserve_only_best_time() {
        let mut encounter = encounter_at_active_wave_three();
        defeat_all(&mut encounter);
        open_and_close_reward(&mut encounter);
        let EncounterState::BossIntroduction { activates_at } = encounter.state() else {
            panic!("boss intro");
        };
        step_until(&mut encounter, activates_at.0);
        let completion = defeat_all(&mut encounter);
        let EncounterEvent::CompletionSummaryOpened {
            reward_id,
            clear_ticks,
            best_clear_ticks,
            ..
        } = completion.events.last().expect("completion event")
        else {
            panic!("summary event");
        };
        assert_eq!(*reward_id, BOSS_REWARD_ID);
        assert_eq!(clear_ticks, best_clear_ticks);
        let best = *best_clear_ticks;
        let restarted = encounter
            .step(EncounterInput {
                action: EncounterAction::RunAgain,
                ..idle()
            })
            .expect("run again");
        assert_eq!(encounter.state(), EncounterState::AwaitingFirstActivity);
        assert_eq!(encounter.run_ordinal(), 2);
        assert_eq!(encounter.seed(), FIRST_PLAYABLE_DEFAULT_SEED);
        assert_eq!(encounter.best_clear_ticks(), Some(best));
        assert!(encounter.active_instances().is_empty());
        assert!(matches!(
            restarted.events.last(),
            Some(EncounterEvent::RunRestarted {
                reason: RestartReason::RunAgain,
                run_ordinal: 2,
                ..
            })
        ));
    }

    #[test]
    fn death_freezes_then_explicit_restart_uses_new_qualified_ids() {
        let mut encounter = BellLaboratoryEncounter::new_default_seed();
        encounter
            .step(EncounterInput {
                player_moved: true,
                ..idle()
            })
            .expect("activity");
        step_until(&mut encounter, 73);
        assert_eq!(encounter.active_instances()[0].run_ordinal, 1);
        let death = encounter
            .step(EncounterInput {
                action: EncounterAction::PlayerDied,
                ..idle()
            })
            .expect("death freeze");
        assert_eq!(encounter.state(), EncounterState::DeathFrozen);
        assert!(encounter.active_instances().is_empty());
        assert_eq!(encounter.run_ordinal(), 1);
        assert!(matches!(
            death.events.as_slice(),
            [EncounterEvent::PlayerDeathAccepted {
                run_ordinal: 1,
                cleared_instances,
                ..
            }] if cleared_instances.len() == 4
        ));
        let frozen = encounter.clone();
        assert_eq!(
            encounter.step(EncounterInput {
                action: EncounterAction::PlayerDied,
                ..idle()
            }),
            Err(EncounterError::DeathAlreadyAccepted)
        );
        assert_eq!(encounter, frozen);
        encounter
            .step(EncounterInput {
                action: EncounterAction::RunAgain,
                ..idle()
            })
            .expect("explicit restart");
        assert_eq!(encounter.state(), EncounterState::AwaitingFirstActivity);
        assert_eq!(encounter.run_ordinal(), 2);
        encounter
            .step(EncounterInput {
                player_fired: true,
                ..idle()
            })
            .expect("second activity");
        let EncounterState::FirstWaveDelay { starts_at } = encounter.state() else {
            panic!("second delay");
        };
        step_until(&mut encounter, starts_at.0 + SPAWN_TELEGRAPH_TICKS);
        assert_eq!(encounter.active_instances()[0].run_ordinal, 2);
    }

    #[test]
    fn invalid_inputs_are_transactional_and_cannot_skip_flow() {
        let mut encounter = BellLaboratoryEncounter::new_default_seed();
        let before = encounter.clone();
        let duplicate = SpawnInstanceId {
            run_ordinal: 1,
            spawn_ordinal: 1,
        };
        assert_eq!(
            encounter.step(EncounterInput {
                defeated: vec![duplicate, duplicate],
                ..idle()
            }),
            Err(EncounterError::DefeatIdsNotStrictlySorted)
        );
        assert_eq!(encounter, before);
        assert_eq!(
            encounter.step(EncounterInput {
                action: EncounterAction::CloseRewardPanel,
                ..idle()
            }),
            Err(EncounterError::RewardPanelNotOpen)
        );
        assert_eq!(encounter, before);
    }

    #[test]
    fn fixed_script_replays_to_identical_events_and_state() {
        fn replay() -> (BellLaboratoryEncounter, Vec<EncounterEvent>) {
            let mut encounter = BellLaboratoryEncounter::new(99);
            let mut events = Vec::new();
            events.extend(
                encounter
                    .step(EncounterInput {
                        player_fired: true,
                        ..idle()
                    })
                    .expect("activity")
                    .events,
            );
            while encounter.tick().0 < 73 {
                events.extend(encounter.step(idle()).expect("idle").events);
            }
            events.extend(defeat_all(&mut encounter).events);
            (encounter, events)
        }
        let first = replay();
        let second = replay();
        assert_eq!(first, second);
    }

    fn encounter_at_active_wave_three() -> BellLaboratoryEncounter {
        let mut encounter = BellLaboratoryEncounter::new_default_seed();
        encounter
            .step(EncounterInput {
                player_moved: true,
                ..idle()
            })
            .expect("activity");
        step_until(&mut encounter, 73);
        defeat_all(&mut encounter);
        open_and_close_reward(&mut encounter);
        let EncounterState::SpawnTelegraph {
            activates_at: wave2_activation,
            ..
        } = encounter.state()
        else {
            panic!("wave2 telegraph");
        };
        step_until(&mut encounter, wave2_activation.0);
        defeat_all(&mut encounter);
        open_and_close_reward(&mut encounter);
        let EncounterState::SpawnTelegraph {
            activates_at: wave3_activation,
            ..
        } = encounter.state()
        else {
            panic!("wave3 telegraph");
        };
        step_until(&mut encounter, wave3_activation.0);
        encounter
    }
}
