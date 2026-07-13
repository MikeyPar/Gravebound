use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;

use anyhow::{Result, anyhow};
use bevy::prelude::*;
use sim_core::{
    BellProctorDefinition, BellProctorEncounterSimulation, BellProctorEncounterStep, CombatStep,
    DamageBand, DamageEvent, DamageType, EnemyActorKind, EnemyEvent, EnemyHealthActor,
    EnemyHealthSimulation, EnemyHealthSnapshot, EnemyHealthStep, EnemyHurtbox, EnemyLab,
    EnemyLabActorIds, EnemyLabActorPositions, EnemyLabDefinitions, EnemyLabPlayer, EnemyLabStep,
    EntityId, EntityIdAllocator, GrayscaleSignature, HostileEvent, HostileReadabilityProfile,
    LocalDamageObservation, NormalRewardDropEvent, NormalWaveDefinitions, NormalWaveHandoff,
    NormalWaveInstanceSnapshot, NormalWaveSimulation, NormalWaveSpawn, NormalWaveStep, OriginCue,
    PatternContext, PatternDefinition, PatternFairnessFixture, PatternKind, PlayerCombatState,
    RedTonicSimulation, ShapeCue, SimulationVector, Tick, WarningAudioPriority,
    compile_hostile_readability_manifest, normal_wave_projectile_allocator,
};

use crate::{
    FixedSimulationSet, FrameSet, LoadedArena,
    arena_view::simulation_point_to_render,
    oath_feedback::{OathAudioCue, OathAudioCueKind},
    player::PlayerSimulation,
};

const PILGRIM_POSITION: (i32, i32) = (10_000, 12_000);
const REED_POSITION: (i32, i32) = (6_000, 12_000);
const SENTRY_POSITION: (i32, i32) = (12_000, 12_000);
const ENEMY_Z: f32 = 5.5;
const HOSTILE_PROJECTILE_Z: f32 = 7.0;
const HOSTILE_TELEGRAPH_Z: f32 = 7.5;

#[derive(Debug, Clone, Resource)]
pub(crate) struct EnemyLabRuntime {
    lab: EnemyLab,
    health: EnemyHealthSimulation,
    normal_mode: bool,
    normal_definitions: NormalWaveDefinitions,
    normal_arena: sim_core::ArenaGeometry,
    normal_player: Option<EnemyLabPlayer>,
    normal_projectile_ids: Option<EntityIdAllocator>,
    normal_wave: Option<NormalWaveSimulation>,
    boss_definition: BellProctorDefinition,
    run_ordinal: u32,
    boss_instance: Option<sim_core::SpawnInstanceId>,
    boss: Option<BellProctorEncounterSimulation>,
    pending_combat_step: Option<CombatStep>,
    pending_normal_steps: Vec<NormalWaveStep>,
    pending_boss_steps: Vec<BellProctorEncounterStep>,
    damage_policy: sim_core::HostileDamagePolicy,
    patterns: [PatternDebugEntry; 3],
    readability: ReadabilityDebug,
    boss_readability: ReadabilityDebug,
    pending_steps: Vec<EnemyLabStep>,
    pending_health_steps: Vec<EnemyHealthStep>,
    pending_drops: Vec<NormalRewardDropEvent>,
    pending_damage_telemetry: Vec<LocalDamageObservation>,
    pending_death: Option<LocalDamageObservation>,
    active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnemyRunCleanup {
    pub enemies: u32,
    pub hostile_projectiles: u32,
    pub hostile_hazards: u32,
    pub friendly_projectiles: u32,
}

impl EnemyLabRuntime {
    pub(crate) fn new_for_run(
        definitions: EnemyLabDefinitions,
        arena: sim_core::ArenaGeometry,
        player: EnemyLabPlayer,
        boss_definition: BellProctorDefinition,
        run_ordinal: u32,
        normal_mode: bool,
    ) -> Result<Self> {
        let base = u64::from(
            run_ordinal
                .checked_sub(1)
                .ok_or_else(|| anyhow!("LocalLab run ordinal must be nonzero"))?,
        )
        .checked_mul(100_000)
        .ok_or_else(|| anyhow!("LocalLab run-qualified entity ID overflow"))?;
        let qualified = |local: u64| {
            base.checked_add(local)
                .and_then(EntityId::new)
                .ok_or_else(|| anyhow!("LocalLab run-qualified entity ID overflow"))
        };
        let ids = EnemyLabActorIds {
            drowned_pilgrim: qualified(10_001)?,
            bell_reed: qualified(10_002)?,
            chain_sentry: qualified(10_003)?,
        };
        if player.target.entity_id != qualified(10_004)? {
            return Err(anyhow!("LocalLab player ID is not qualified to this run"));
        }
        let positions = EnemyLabActorPositions {
            drowned_pilgrim_milli_tiles: PILGRIM_POSITION,
            bell_reed_milli_tiles: REED_POSITION,
            chain_sentry_milli_tiles: SENTRY_POSITION,
        };
        let projectile_ids = EntityIdAllocator::starting_at(
            NonZeroU64::new(
                base.checked_add(20_000)
                    .ok_or_else(|| anyhow!("hostile projectile ID floor overflow"))?,
            )
            .expect("qualified hostile projectile ID floor is nonzero"),
        );
        let (patterns, readability) = compile_pattern_debug(&definitions)?;
        let boss_readability = compile_boss_readability(&boss_definition)?;
        let normal_definitions = NormalWaveDefinitions {
            drowned_pilgrim: definitions.drowned_pilgrim.clone(),
            bell_reed: definitions.bell_reed.clone(),
            chain_sentry: definitions.chain_sentry.clone(),
        };
        let normal_arena = arena.clone();
        let normal_player = normal_mode.then(|| player.clone());
        let normal_projectile_ids = normal_mode
            .then(|| normal_wave_projectile_allocator(run_ordinal))
            .transpose()?;
        let health = EnemyHealthSimulation::new(vec![
            EnemyHealthActor::drowned_pilgrim(
                ids.drowned_pilgrim,
                &definitions.drowned_pilgrim,
                milli_position(PILGRIM_POSITION),
            ),
            EnemyHealthActor::bell_reed(
                ids.bell_reed,
                &definitions.bell_reed,
                milli_position(REED_POSITION),
            ),
            EnemyHealthActor::chain_sentry(
                ids.chain_sentry,
                &definitions.chain_sentry,
                milli_position(SENTRY_POSITION),
            ),
        ])?;
        Ok(Self {
            lab: EnemyLab::new(definitions, arena, ids, positions, player, projectile_ids)?,
            health,
            normal_mode,
            normal_definitions,
            normal_arena,
            normal_player,
            normal_projectile_ids,
            normal_wave: None,
            boss_definition,
            run_ordinal,
            boss_instance: None,
            boss: None,
            pending_combat_step: None,
            pending_normal_steps: Vec::new(),
            pending_boss_steps: Vec::new(),
            damage_policy: sim_core::HostileDamagePolicy::Standard,
            patterns,
            readability,
            boss_readability,
            pending_steps: Vec::new(),
            pending_health_steps: Vec::new(),
            pending_drops: Vec::new(),
            pending_damage_telemetry: Vec::new(),
            pending_death: None,
            active: true,
        })
    }

    pub(crate) fn combat(&self) -> &PlayerCombatState {
        if let Some(boss) = &self.boss {
            &boss.player().combat
        } else if let Some(wave) = &self.normal_wave {
            &wave.player().combat
        } else if let Some(player) = &self.normal_player {
            &player.combat
        } else {
            &self.lab.player().combat
        }
    }

    pub(crate) fn combat_mut(&mut self) -> &mut PlayerCombatState {
        if let Some(boss) = &mut self.boss {
            &mut boss.player_mut().combat
        } else if let Some(wave) = &mut self.normal_wave {
            &mut wave.player_mut().combat
        } else if let Some(player) = &mut self.normal_player {
            &mut player.combat
        } else {
            &mut self.lab.player_mut().combat
        }
    }

    pub(crate) fn consumables(&self) -> &RedTonicSimulation {
        if let Some(boss) = &self.boss {
            &boss.player().consumables
        } else if let Some(wave) = &self.normal_wave {
            &wave.player().consumables
        } else if let Some(player) = &self.normal_player {
            &player.consumables
        } else {
            &self.lab.player().consumables
        }
    }

    pub(crate) fn consumables_mut(&mut self) -> &mut RedTonicSimulation {
        if let Some(boss) = &mut self.boss {
            &mut boss.player_mut().consumables
        } else if let Some(wave) = &mut self.normal_wave {
            &mut wave.player_mut().consumables
        } else if let Some(player) = &mut self.normal_player {
            &mut player.consumables
        } else {
            &mut self.lab.player_mut().consumables
        }
    }

    pub(crate) fn target_armor(&self) -> u32 {
        if let Some(boss) = &self.boss {
            boss.player().target.armor
        } else if let Some(wave) = &self.normal_wave {
            wave.player().target.armor
        } else if let Some(player) = &self.normal_player {
            player.target.armor
        } else {
            self.lab.player().target.armor
        }
    }

    pub(crate) fn hostile_projectile_count(&self) -> usize {
        if let Some(boss) = &self.boss {
            return boss.hostile_projectiles().len();
        }
        self.normal_wave.as_ref().map_or_else(
            || self.lab.hostile_projectiles().len(),
            |wave| wave.hostile_projectiles().len(),
        )
    }

    pub(crate) fn hostile_projectiles(&self) -> &[sim_core::HostileProjectile] {
        if let Some(boss) = &self.boss {
            return boss.hostile_projectiles();
        }
        self.normal_wave.as_ref().map_or_else(
            || self.lab.hostile_projectiles(),
            NormalWaveSimulation::hostile_projectiles,
        )
    }

    pub(crate) fn hostile_hazard_count(&self) -> usize {
        if let Some(boss) = &self.boss {
            return boss.active_lane_geometries().len();
        }
        self.normal_wave.as_ref().map_or_else(
            || usize::from(self.lab.active_lane().is_some()),
            |wave| wave.active_lanes().len(),
        )
    }

    pub(crate) fn active_lane_geometries(&self) -> Vec<sim_core::LaneGeometry> {
        if let Some(boss) = &self.boss {
            return boss.active_lane_geometries();
        }
        self.normal_wave.as_ref().map_or_else(
            || {
                self.lab
                    .active_lane()
                    .map(|lane| vec![lane.geometry])
                    .unwrap_or_default()
            },
            |wave| {
                wave.active_lanes()
                    .into_iter()
                    .map(|(_, lane)| lane.geometry)
                    .collect()
            },
        )
    }

    pub(crate) fn showcase_ready(&self) -> bool {
        self.lab.readiness().is_ready()
    }

    pub(crate) const fn normal_mode(&self) -> bool {
        self.normal_mode
    }

    pub(crate) fn set_debug_invulnerable(&mut self, enabled: bool) {
        let policy = if enabled {
            sim_core::HostileDamagePolicy::DebugInvulnerable
        } else {
            sim_core::HostileDamagePolicy::Standard
        };
        self.damage_policy = policy;
        self.lab.set_damage_policy(policy);
        if let Some(wave) = &mut self.normal_wave {
            wave.set_damage_policy(policy);
        }
        if let Some(boss) = &mut self.boss {
            boss.set_damage_policy(policy);
        }
    }

    pub(crate) fn player_can_act(&self) -> bool {
        self.active && self.consumables().vitals().current_health() > 0
    }

    pub(crate) fn active_lane_is_clear(&self) -> bool {
        self.lab.active_lane().is_none()
    }

    pub(crate) const fn is_active(&self) -> bool {
        self.active
    }

    pub(crate) fn take_pending_death(&mut self) -> Option<LocalDamageObservation> {
        self.pending_death.take()
    }

    pub(crate) const fn pending_death(&self) -> Option<&LocalDamageObservation> {
        self.pending_death.as_ref()
    }

    pub(crate) fn drain_damage_telemetry(&mut self) -> Vec<LocalDamageObservation> {
        std::mem::take(&mut self.pending_damage_telemetry)
    }

    pub(crate) fn clear_for_local_death(&mut self) -> EnemyRunCleanup {
        if self.normal_mode {
            let living = self.boss.as_ref().map_or_else(
                || {
                    self.normal_wave.as_ref().map_or(0, |wave| {
                        wave.snapshots()
                            .iter()
                            .filter(|snapshot| snapshot.health.alive)
                            .count()
                    })
                },
                |boss| {
                    usize::from(!matches!(
                        boss.snapshot().state,
                        sim_core::BellProctorStateKind::Defeated
                    ))
                },
            );
            let enemies = u32::try_from(living).unwrap_or(u32::MAX);
            let hostile_projectiles =
                u32::try_from(self.hostile_projectile_count()).unwrap_or(u32::MAX);
            let hostile_hazards = u32::try_from(self.hostile_hazard_count()).unwrap_or(u32::MAX);
            let friendly_projectiles =
                u32::try_from(self.combat_mut().clear_projectiles_for_local_death().len())
                    .unwrap_or(u32::MAX);
            self.normal_wave = None;
            self.boss = None;
            self.boss_instance = None;
            self.normal_player = None;
            self.normal_projectile_ids = None;
            self.pending_combat_step = None;
            self.pending_normal_steps.clear();
            self.pending_boss_steps.clear();
            self.pending_steps.clear();
            self.pending_health_steps.clear();
            self.pending_drops.clear();
            self.active = false;
            return EnemyRunCleanup {
                enemies,
                hostile_projectiles,
                hostile_hazards,
                friendly_projectiles,
            };
        }
        let enemies = u32::try_from(
            self.health_snapshots()
                .iter()
                .filter(|snapshot| snapshot.alive)
                .count(),
        )
        .unwrap_or(u32::MAX);
        let cleared = self.lab.clear_hostiles();
        let hostile_projectiles = u32::try_from(cleared.projectiles.len()).unwrap_or(u32::MAX);
        let hostile_hazards = u32::from(cleared.lane.is_some());
        let friendly_projectiles = u32::try_from(
            self.lab
                .player_mut()
                .combat
                .clear_projectiles_for_local_death()
                .len(),
        )
        .unwrap_or(u32::MAX);
        self.pending_steps.clear();
        self.pending_health_steps.clear();
        self.pending_drops.clear();
        self.active = false;
        EnemyRunCleanup {
            enemies,
            hostile_projectiles,
            hostile_hazards,
            friendly_projectiles,
        }
    }

    pub(crate) fn alive_hurtboxes(&self) -> Result<Vec<EnemyHurtbox>> {
        if !self.active {
            return Ok(Vec::new());
        }
        if self.normal_mode {
            if let Some(boss) = &self.boss {
                return Ok(boss.hurtbox()?.into_iter().collect());
            }
            return self
                .normal_wave
                .as_ref()
                .map_or_else(|| Ok(Vec::new()), |wave| Ok(wave.alive_hurtboxes()?));
        }
        Ok(self.health.alive_hurtboxes()?)
    }

    pub(crate) fn apply_friendly_combat(&mut self, step: &CombatStep) -> Result<()> {
        if self.normal_mode {
            if self.pending_combat_step.replace(step.clone()).is_some() {
                return Err(anyhow!(
                    "normal encounter received two combat steps before resolution"
                ));
            }
            return Ok(());
        }
        let health_step = self.health.apply_combat_step(step)?;
        let drops = self.health.collect_due_drops(step.tick)?;
        self.pending_health_steps.push(health_step);
        self.pending_drops.extend(drops);
        Ok(())
    }

    pub(crate) fn health_snapshots(&self) -> Vec<EnemyHealthSnapshot> {
        if self.normal_mode {
            return self.normal_wave.as_ref().map_or_else(Vec::new, |wave| {
                wave.snapshots()
                    .into_iter()
                    .map(|snapshot| snapshot.health)
                    .collect()
            });
        }
        self.health.snapshots()
    }

    pub(crate) fn debug_enemy_states(&self) -> Vec<sim_core::DebugEnemyState> {
        if self.normal_mode {
            if let Some(boss) = &self.boss {
                let snapshot = boss.snapshot();
                let position = boss.position();
                return vec![sim_core::DebugEnemyState {
                    entity_id: snapshot.entity_id,
                    x_bits: position.x.to_bits(),
                    y_bits: position.y.to_bits(),
                    health: snapshot.current_health,
                    alive: !matches!(snapshot.state, sim_core::BellProctorStateKind::Defeated),
                }];
            }
            return self
                .normal_snapshots()
                .into_iter()
                .map(|snapshot| {
                    let position = milli_position(snapshot.position_milli_tiles);
                    sim_core::DebugEnemyState {
                        entity_id: snapshot.entity_id,
                        x_bits: position.x.to_bits(),
                        y_bits: position.y.to_bits(),
                        health: snapshot.health.current_health,
                        alive: snapshot.health.alive,
                    }
                })
                .collect();
        }
        let actors = self.lab.actors();
        let positions = [
            (
                actors.drowned_pilgrim().entity_id(),
                actors.drowned_pilgrim().position(),
            ),
            (
                actors.bell_reed().entity_id(),
                actors.bell_reed().position(),
            ),
            (
                actors.chain_sentry().entity_id(),
                actors.chain_sentry().position(),
            ),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
        self.health_snapshots()
            .into_iter()
            .map(|snapshot| {
                let position = positions[&snapshot.actor_id];
                sim_core::DebugEnemyState {
                    entity_id: snapshot.actor_id,
                    x_bits: position.x.to_bits(),
                    y_bits: position.y.to_bits(),
                    health: snapshot.current_health,
                    alive: snapshot.alive,
                }
            })
            .collect()
    }

    pub(crate) fn normal_snapshots(&self) -> Vec<NormalWaveInstanceSnapshot> {
        self.normal_wave
            .as_ref()
            .map_or_else(Vec::new, NormalWaveSimulation::snapshots)
    }

    pub(crate) fn normal_phase(&self) -> Option<sim_core::NormalWavePhase> {
        self.normal_wave.as_ref().map(NormalWaveSimulation::phase)
    }

    pub(crate) fn take_pending_combat_step(&mut self) -> Option<CombatStep> {
        self.pending_combat_step.take()
    }

    pub(crate) fn drain_normal_steps(&mut self) -> Vec<NormalWaveStep> {
        std::mem::take(&mut self.pending_normal_steps)
    }

    pub(crate) fn drain_boss_steps(&mut self) -> Vec<BellProctorEncounterStep> {
        std::mem::take(&mut self.pending_boss_steps)
    }

    pub(crate) fn start_normal_wave(
        &mut self,
        spawns: Vec<NormalWaveSpawn>,
        starts_at: Tick,
    ) -> Result<()> {
        if !self.normal_mode
            || self.normal_wave.as_ref().is_some_and(|wave| {
                !matches!(wave.phase(), sim_core::NormalWavePhase::Cleared { .. })
            })
        {
            return Err(anyhow!(
                "normal wave start is unavailable in the current runtime phase"
            ));
        }
        let (player, projectile_ids) = if let Some(wave) = self.normal_wave.take() {
            let NormalWaveHandoff {
                player,
                hostile_projectile_ids,
            } = wave.into_handoff()?;
            (player, hostile_projectile_ids)
        } else {
            (
                self.normal_player
                    .take()
                    .ok_or_else(|| anyhow!("normal encounter player owner is missing"))?,
                self.normal_projectile_ids
                    .take()
                    .ok_or_else(|| anyhow!("normal encounter projectile allocator is missing"))?,
            )
        };
        let mut wave = NormalWaveSimulation::new(
            self.normal_definitions.clone(),
            self.normal_arena.clone(),
            spawns,
            player,
            projectile_ids,
            starts_at,
        )?;
        wave.set_damage_policy(self.damage_policy);
        self.normal_wave = Some(wave);
        Ok(())
    }

    pub(crate) fn step_normal_wave(
        &mut self,
        combat: &CombatStep,
        player_position: SimulationVector,
    ) -> Result<Vec<sim_core::SpawnInstanceId>> {
        let wave = self
            .normal_wave
            .as_mut()
            .ok_or_else(|| anyhow!("normal wave is not active"))?;
        wave.player_mut().target.position = player_position;
        let step = wave.step(combat)?;
        let damage = normal_damage_observations(wave, &step);
        if self.pending_death.is_none() {
            self.pending_death = damage.iter().find(|entry| entry.damage.lethal).cloned();
        }
        self.pending_damage_telemetry.extend(damage);
        let defeated = step
            .defeats
            .iter()
            .map(|defeat| defeat.instance_id)
            .collect();
        self.pending_normal_steps.push(step);
        Ok(defeated)
    }

    pub(crate) fn start_boss(
        &mut self,
        instance: sim_core::SpawnInstanceId,
        starts_at: Tick,
    ) -> Result<()> {
        if !self.normal_mode || self.boss.is_some() {
            return Err(anyhow!(
                "Bell Proctor start is unavailable in the current runtime phase"
            ));
        }
        let wave = self
            .normal_wave
            .take()
            .ok_or_else(|| anyhow!("Wave 3 handoff owner is missing"))?;
        let handoff = wave.into_handoff()?;
        let mut boss = BellProctorEncounterSimulation::new(
            self.boss_definition.clone(),
            self.normal_arena.clone(),
            handoff,
            self.run_ordinal,
            starts_at,
        )?;
        boss.set_damage_policy(self.damage_policy);
        self.boss_instance = Some(instance);
        self.boss = Some(boss);
        Ok(())
    }

    pub(crate) fn step_boss(
        &mut self,
        combat: &CombatStep,
        player_position: SimulationVector,
    ) -> Result<Vec<sim_core::SpawnInstanceId>> {
        let boss = self
            .boss
            .as_mut()
            .ok_or_else(|| anyhow!("Bell Proctor is not active"))?;
        boss.update_player_position(player_position)?;
        let step = boss.step(combat)?;
        let damage = boss_damage_observations(boss, &step);
        if self.pending_death.is_none() {
            self.pending_death = damage.iter().find(|entry| entry.damage.lethal).cloned();
        }
        self.pending_damage_telemetry.extend(damage);
        let defeated = if step.defeat.is_some() {
            vec![
                self.boss_instance
                    .ok_or_else(|| anyhow!("Bell Proctor instance mapping is missing"))?,
            ]
        } else {
            Vec::new()
        };
        self.pending_boss_steps.push(step);
        Ok(defeated)
    }

    pub(crate) fn boss_snapshot(&self) -> Option<sim_core::BellProctorEncounterSnapshot> {
        self.boss
            .as_ref()
            .map(BellProctorEncounterSimulation::snapshot)
    }
}

fn boss_damage_observations(
    boss: &BellProctorEncounterSimulation,
    step: &BellProctorEncounterStep,
) -> Vec<LocalDamageObservation> {
    let mut observations = Vec::new();
    for contact in &step.lane_contacts {
        observations.push(LocalDamageObservation {
            tick: step.tick,
            pattern_id: sim_core::BELL_PROCTOR_CROSS_ID.to_owned(),
            source_position: boss.position(),
            damage: contact.damage.damage.clone(),
        });
    }
    observations.extend(step.hostile_step.events.iter().filter_map(|event| {
        if let HostileEvent::Contact {
            pattern_id,
            damage: Some(damage),
            ..
        } = event
        {
            Some(LocalDamageObservation {
                tick: step.tick,
                pattern_id: (*pattern_id).to_owned(),
                source_position: boss.position(),
                damage: damage.clone(),
            })
        } else {
            None
        }
    }));
    observations
}

fn normal_damage_observations(
    wave: &NormalWaveSimulation,
    step: &NormalWaveStep,
) -> Vec<LocalDamageObservation> {
    let source_position = |source| {
        wave.snapshots()
            .into_iter()
            .find(|snapshot| snapshot.entity_id == source)
            .map(|snapshot| milli_position(snapshot.position_milli_tiles))
            .expect("normal hostile source remains registered in its wave")
    };
    let mut observations = Vec::new();
    for event in &step.lane_events {
        if let sim_core::NormalWaveLaneEvent::Contact {
            source_entity_id,
            pattern_id,
            damage,
            ..
        } = event
        {
            observations.push(LocalDamageObservation {
                tick: step.tick,
                pattern_id: (*pattern_id).to_owned(),
                source_position: source_position(*source_entity_id),
                damage: damage.damage.clone(),
            });
        }
    }
    for event in &step.hostile_step.events {
        if let HostileEvent::Contact {
            source_entity_id,
            pattern_id,
            damage: Some(damage),
            ..
        } = event
        {
            observations.push(LocalDamageObservation {
                tick: step.tick,
                pattern_id: (*pattern_id).to_owned(),
                source_position: source_position(*source_entity_id),
                damage: damage.clone(),
            });
        }
    }
    observations
}

#[cfg(test)]
fn id(value: u64) -> EntityId {
    EntityId::new(value).expect("LocalLab IDs are nonzero")
}

#[allow(clippy::cast_precision_loss)]
fn milli_position((x, y): (i32, i32)) -> SimulationVector {
    SimulationVector::new(x as f32 / 1_000.0, y as f32 / 1_000.0)
}

#[derive(Debug, Component)]
pub(crate) struct EnemyPresentation {
    kind: EnemyActorKind,
    key: EntityActorKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EntityActorKey(EntityId);

#[derive(Debug, Component)]
pub(crate) struct HostileProjectilePresentation(EntityId);

#[derive(Debug, Component)]
struct EnemyDiagnostics;

#[derive(Debug, Component)]
struct PatternDiagnostics;

#[derive(Debug, Component)]
struct DamageDiagnostics;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PatternDebugEntry {
    label: &'static str,
    first_warning_ticks: u32,
    repeated_warning_ticks: u32,
    threat: u32,
    cap: u32,
    counterplay: &'static str,
    grayscale: &'static str,
    audio_priority: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadabilityDebug {
    total_threat: u32,
    total_active_instances: u32,
    encounter_cap: u32,
    standard_audio_cues: usize,
    major_audio_cues: usize,
}

#[derive(Debug, Component)]
pub(crate) struct NormalRewardPresentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DamagePresentation {
    amount: u32,
    health_before: u32,
    health_after: u32,
    damage_type: DamageType,
    band: Option<DamageBand>,
    source: EntityId,
    lethal: bool,
}

#[derive(Debug, Default, Resource)]
pub(crate) struct EnemyPresentationState {
    fan_fires: u64,
    ring_fires: u64,
    lane_activations: u64,
    player_hits: u64,
    projectile_grace_ignored: u64,
    friendly_hits: u64,
    enemy_deaths: u64,
    reward_drops: u64,
    approach_moves: u64,
    last_lane_geometry: Option<sim_core::LaneGeometry>,
    last_attack: &'static str,
    last_damage: Option<DamagePresentation>,
    attack_telegraphs: [u64; 3],
}

impl EnemyPresentationState {
    pub(crate) const fn death_showcase_ready(&self) -> bool {
        self.enemy_deaths > 0 && self.reward_drops > 0
    }

    pub(crate) const fn lethal_showcase_ready(&self) -> bool {
        matches!(self.last_damage, Some(damage) if damage.lethal)
    }

    pub(crate) const fn grace_showcase_ready(&self) -> bool {
        self.last_damage.is_some() && self.projectile_grace_ignored > 0
    }

    pub(crate) fn readability_showcase_ready(&self) -> bool {
        self.fan_fires >= 3
            && self.ring_fires >= 3
            && self.attack_telegraphs.iter().all(|count| *count >= 2)
    }
}

pub(crate) fn configure(app: &mut App) {
    app.insert_resource(EnemyPresentationState::default())
        .add_systems(Startup, spawn_enemy_presentation)
        .add_systems(
            FixedUpdate,
            simulate_enemy_lab.in_set(FixedSimulationSet::Hostile),
        )
        .add_systems(
            Update,
            (
                present_enemy_steps,
                sync_enemy_actors,
                sync_hostile_projectiles,
                draw_active_lane,
                update_enemy_diagnostics,
                update_pattern_diagnostics,
                update_damage_diagnostics,
            )
                .chain()
                .in_set(FrameSet::Presentation),
        );
}

#[allow(clippy::needless_pass_by_value)]
fn simulate_enemy_lab(
    player: Res<PlayerSimulation>,
    scenario: Res<crate::combat::EvidenceScenario>,
    mut runtime: ResMut<EnemyLabRuntime>,
) {
    if !runtime.active || runtime.normal_mode {
        return;
    }
    if !matches!(
        *scenario,
        crate::combat::EvidenceScenario::None
            | crate::combat::EvidenceScenario::EnemyShowcase
            | crate::combat::EvidenceScenario::EnemyDeathShowcase
            | crate::combat::EvidenceScenario::DamageLethalShowcase
            | crate::combat::EvidenceScenario::DamageGraceShowcase
            | crate::combat::EvidenceScenario::DeathRestartShowcase
            | crate::combat::EvidenceScenario::DeathRecapShowcase
            | crate::combat::EvidenceScenario::DebugOverlayShowcase
    ) {
        return;
    }
    runtime
        .lab
        .update_target_position(player.state().position())
        .expect("validated player position remains legal in Bell Laboratory");
    let step = runtime
        .lab
        .step()
        .expect("validated enemy laboratory state remains legal");
    let actor_positions = {
        let actors = runtime.lab.actors();
        [
            (
                actors.drowned_pilgrim().entity_id(),
                actors.drowned_pilgrim().position(),
            ),
            (
                actors.bell_reed().entity_id(),
                actors.bell_reed().position(),
            ),
            (
                actors.chain_sentry().entity_id(),
                actors.chain_sentry().position(),
            ),
        ]
    };
    for (actor_id, position) in actor_positions {
        runtime
            .health
            .update_actor_position(actor_id, position)
            .expect("coordinator actor remains a registered finite health target");
    }
    let damage = damage_observations(&runtime.lab, &step);
    if runtime.pending_death.is_none() {
        runtime.pending_death = damage.iter().find(|entry| entry.damage.lethal).cloned();
    }
    runtime.pending_damage_telemetry.extend(damage);
    runtime.pending_steps.push(step);
}

fn damage_observations(lab: &EnemyLab, step: &EnemyLabStep) -> Vec<LocalDamageObservation> {
    let mut observations = Vec::new();
    for event in &step.lane_events {
        if let sim_core::EnemyLaneEvent::Contact {
            source_entity_id,
            pattern_id,
            damage,
            ..
        } = event
        {
            observations.push(LocalDamageObservation {
                tick: step.tick,
                pattern_id: (*pattern_id).to_owned(),
                source_position: enemy_source_position(lab, *source_entity_id),
                damage: damage.damage.clone(),
            });
        }
    }
    for event in &step.hostile_step.events {
        if let HostileEvent::Contact {
            source_entity_id,
            pattern_id,
            damage: Some(damage),
            ..
        } = event
        {
            observations.push(LocalDamageObservation {
                tick: step.tick,
                pattern_id: (*pattern_id).to_owned(),
                source_position: enemy_source_position(lab, *source_entity_id),
                damage: damage.clone(),
            });
        }
    }
    observations
}

fn enemy_source_position(lab: &EnemyLab, source: EntityId) -> SimulationVector {
    let actors = lab.actors();
    [
        actors.drowned_pilgrim(),
        actors.bell_reed(),
        actors.chain_sentry(),
    ]
    .into_iter()
    .find(|actor| actor.entity_id() == source)
    .expect("hostile damage source is one of the three registered actors")
    .position()
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)] // Startup keeps the three related diagnostic surfaces co-located for evidence review.
fn spawn_enemy_presentation(
    mut commands: Commands,
    runtime: Res<EnemyLabRuntime>,
    arena: Res<LoadedArena>,
) {
    if !runtime.normal_mode {
        for (actor, label, color, size, rotation) in [
            (
                runtime.lab.actors().drowned_pilgrim(),
                "Drowned Pilgrim",
                Color::srgb_u8(102, 143, 157),
                Vec2::new(0.58, 0.78),
                std::f32::consts::FRAC_PI_4,
            ),
            (
                runtime.lab.actors().bell_reed(),
                "Bell Reed",
                Color::srgb_u8(157, 111, 179),
                Vec2::splat(0.78),
                std::f32::consts::FRAC_PI_4,
            ),
            (
                runtime.lab.actors().chain_sentry(),
                "Chain Sentry",
                Color::srgb_u8(183, 134, 65),
                Vec2::splat(1.0),
                0.0,
            ),
        ] {
            let position = simulation_point_to_render(actor.position(), &arena.0);
            commands
                .spawn((
                    Name::new(label),
                    EnemyPresentation {
                        kind: actor.kind(),
                        key: EntityActorKey(actor.entity_id()),
                    },
                    Sprite::from_color(color, size),
                    Transform::from_xyz(position.x, position.y, ENEMY_Z)
                        .with_rotation(Quat::from_rotation_z(rotation)),
                ))
                .with_children(|parent| {
                    parent.spawn((
                        Sprite::from_color(Color::srgb_u8(229, 224, 197), Vec2::splat(0.24)),
                        Transform::from_xyz(0.0, 0.0, 0.1),
                    ));
                });
        }
    }
    commands.spawn((
        Name::new("Enemy laboratory diagnostics"),
        EnemyDiagnostics,
        if runtime.normal_mode {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        },
        Text::new("ENEMY LAB INITIALIZING"),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(232, 221, 190)),
        Node {
            position_type: PositionType::Absolute,
            right: px(14),
            top: px(252),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(7)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 220)),
        BorderColor::all(Color::srgba_u8(174, 82, 91, 190)),
    ));
    commands.spawn((
        Name::new("Pattern fairness diagnostics"),
        PatternDiagnostics,
        if runtime.normal_mode {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        },
        Text::new("PATTERN VALIDATION INITIALIZING"),
        TextFont::from_font_size(12.0),
        TextColor(Color::srgb_u8(207, 226, 218)),
        Node {
            position_type: PositionType::Absolute,
            right: px(14),
            top: px(346),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(7)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 220)),
        BorderColor::all(Color::srgba_u8(74, 158, 144, 190)),
    ));
    commands.spawn((
        Name::new("Damage readability diagnostics"),
        DamageDiagnostics,
        if runtime.normal_mode {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        },
        Text::new("DAMAGE | WAITING FOR HOSTILE CONTACT"),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(244, 224, 205)),
        Node {
            position_type: PositionType::Absolute,
            left: px(14),
            bottom: px(176),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(7)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(16, 9, 10, 226)),
        BorderColor::all(Color::srgba_u8(224, 97, 72, 210)),
    ));
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)] // One ordered presentation drain covers semantic and authored-wave event streams.
fn present_enemy_steps(
    mut commands: Commands,
    arena: Res<LoadedArena>,
    mut runtime: ResMut<EnemyLabRuntime>,
    mut presentation: ResMut<EnemyPresentationState>,
    oath_audio: Res<OathAudioCue>,
) {
    for step in runtime.drain_normal_steps() {
        presentation.approach_moves = presentation
            .approach_moves
            .saturating_add(step.actor_movements.len() as u64);
        for timeline in step.timeline_events {
            match timeline.event {
                EnemyEvent::FanFired { .. } => presentation.fan_fires += 1,
                EnemyEvent::RingFired { .. } => presentation.ring_fires += 1,
                EnemyEvent::LanesActivated { .. } => presentation.lane_activations += 1,
                EnemyEvent::StateChanged {
                    state:
                        sim_core::EnemyStateKind::AttackWindup
                        | sim_core::EnemyStateKind::AttackTelegraph,
                    ..
                } => {
                    let index = match timeline.kind {
                        sim_core::NormalWaveEnemyKind::DrownedPilgrim => 0,
                        sim_core::NormalWaveEnemyKind::BellReed => 1,
                        sim_core::NormalWaveEnemyKind::ChainSentry => 2,
                    };
                    presentation.attack_telegraphs[index] =
                        presentation.attack_telegraphs[index].saturating_add(1);
                }
                _ => {}
            }
        }
        presentation.friendly_hits = presentation
            .friendly_hits
            .saturating_add(step.enemy_health_step.damage_events.len() as u64);
        presentation.enemy_deaths = presentation
            .enemy_deaths
            .saturating_add(step.defeats.len() as u64);
        for drop in step.drops {
            presentation.reward_drops = presentation.reward_drops.saturating_add(1);
            let position = simulation_point_to_render(drop.event.position, &arena.0);
            commands.spawn((
                Name::new(format!("Normal reward from {}", drop.event.actor_id)),
                NormalRewardPresentation,
                Sprite::from_color(Color::srgb_u8(235, 202, 92), Vec2::splat(0.34)),
                Transform::from_xyz(position.x, position.y, ENEMY_Z + 0.3)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            ));
        }
    }
    for step in runtime.drain_boss_steps() {
        presentation.friendly_hits = presentation
            .friendly_hits
            .saturating_add(step.friendly_damage.len() as u64);
        presentation.enemy_deaths = presentation
            .enemy_deaths
            .saturating_add(u64::from(step.defeat.is_some()));
        for event in step.scheduler_events {
            match event {
                sim_core::BossEvent::FanTelegraph { .. } => {
                    presentation.attack_telegraphs[0] =
                        presentation.attack_telegraphs[0].saturating_add(1);
                    presentation.last_attack = "PROCTOR FAN WARNING";
                }
                sim_core::BossEvent::RingTelegraph { .. }
                | sim_core::BossEvent::RingPreview { .. } => {
                    presentation.attack_telegraphs[1] =
                        presentation.attack_telegraphs[1].saturating_add(1);
                    presentation.last_attack = "PROCTOR GAP MEMORY";
                }
                sim_core::BossEvent::CrossTelegraph { .. } => {
                    presentation.attack_telegraphs[2] =
                        presentation.attack_telegraphs[2].saturating_add(1);
                    presentation.last_attack = "PROCTOR CROSS WARNING";
                }
                sim_core::BossEvent::FanFired { .. } => presentation.fan_fires += 1,
                sim_core::BossEvent::RingFired { .. } => presentation.ring_fires += 1,
                sim_core::BossEvent::CrossActivated { .. } => {
                    presentation.lane_activations += 1;
                }
                _ => {}
            }
        }
        for immunity in &step.status_immunities {
            if immunity.status == sim_core::BellProctorImmuneStatus::Frostbind {
                presentation.last_attack = "FROSTBIND IMMUNE";
                if !oath_audio.play(OathAudioCueKind::FrostbindImmune) {
                    warn!(
                        feature_id = "GB-M03-05C",
                        "Frostbind immunity cue was unavailable"
                    );
                }
            }
        }
        for event in &step.hostile_step.events {
            if matches!(event, HostileEvent::ProjectileGraceIgnored { .. }) {
                presentation.projectile_grace_ignored =
                    presentation.projectile_grace_ignored.saturating_add(1);
            }
            if let HostileEvent::Contact {
                source_entity_id,
                damage: Some(damage),
                ..
            } = event
            {
                presentation.player_hits = presentation.player_hits.saturating_add(1);
                presentation.last_damage = Some(damage_presentation(*source_entity_id, damage));
            }
        }
        for contact in step.lane_contacts {
            presentation.player_hits = presentation.player_hits.saturating_add(1);
            presentation.last_damage = Some(damage_presentation(
                runtime
                    .boss_snapshot()
                    .map_or(EntityId::new(1).expect("nonzero"), |snapshot| {
                        snapshot.entity_id
                    }),
                &contact.damage.damage,
            ));
        }
    }
    for step in runtime.pending_steps.drain(..) {
        presentation.approach_moves += step.actor_movements.len() as u64;
        for timeline in step.enemy_events {
            if matches!(
                &timeline.event,
                EnemyEvent::StateChanged {
                    state: sim_core::EnemyStateKind::AttackWindup
                        | sim_core::EnemyStateKind::AttackTelegraph,
                    ..
                }
            ) {
                let index = match timeline.source_kind {
                    EnemyActorKind::DrownedPilgrim => 0,
                    EnemyActorKind::BellReed => 1,
                    EnemyActorKind::ChainSentry => 2,
                };
                presentation.attack_telegraphs[index] =
                    presentation.attack_telegraphs[index].saturating_add(1);
            }
            match timeline.event {
                EnemyEvent::FanFired { .. } => {
                    presentation.fan_fires += 1;
                    presentation.last_attack = "PILGRIM FAN";
                }
                EnemyEvent::RingFired { .. } => {
                    presentation.ring_fires += 1;
                    presentation.last_attack = "REED GAP RING";
                }
                EnemyEvent::LanesActivated { .. } => {
                    presentation.lane_activations += 1;
                    presentation.last_attack = "SENTRY LANES";
                }
                _ => {}
            }
        }
        for event in &step.hostile_step.events {
            if matches!(event, HostileEvent::ProjectileGraceIgnored { .. }) {
                presentation.projectile_grace_ignored =
                    presentation.projectile_grace_ignored.saturating_add(1);
            }
            if let HostileEvent::Contact {
                source_entity_id,
                damage: Some(damage),
                health_application: Some(_),
                ..
            } = event
            {
                presentation.player_hits += 1;
                presentation.last_damage = Some(damage_presentation(*source_entity_id, damage));
            }
        }
        for event in &step.lane_events {
            if let sim_core::EnemyLaneEvent::Contact {
                source_entity_id,
                damage,
                ..
            } = event
            {
                presentation.player_hits += 1;
                presentation.last_damage =
                    Some(damage_presentation(*source_entity_id, &damage.damage));
            }
        }
    }
    for step in runtime.pending_health_steps.drain(..) {
        presentation.friendly_hits += step.damage_events.len() as u64;
        presentation.enemy_deaths += step.death_events.len() as u64;
    }
    for drop in runtime.pending_drops.drain(..) {
        presentation.reward_drops += 1;
        let position = simulation_point_to_render(drop.position, &arena.0);
        commands.spawn((
            Name::new(format!("Normal reward from {}", drop.actor_id)),
            NormalRewardPresentation,
            Sprite::from_color(Color::srgb_u8(235, 202, 92), Vec2::splat(0.34)),
            Transform::from_xyz(position.x, position.y, ENEMY_Z + 0.3)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
        ));
    }
}

#[allow(clippy::needless_pass_by_value)]
fn sync_enemy_actors(
    runtime: Res<EnemyLabRuntime>,
    arena: Res<LoadedArena>,
    mut visuals: Query<(&mut EnemyPresentation, &mut Transform, &mut Visibility)>,
) {
    if runtime.normal_mode {
        return;
    }
    let actors = runtime.lab.actors();
    let health: BTreeMap<_, _> = runtime
        .health_snapshots()
        .into_iter()
        .map(|snapshot| (EntityActorKey(snapshot.actor_id), snapshot.alive))
        .collect();
    for (mut presentation, mut transform, mut visibility) in &mut visuals {
        let actor = actors.actor(presentation.kind);
        presentation.key = EntityActorKey(actor.entity_id());
        let render = simulation_point_to_render(actor.position(), &arena.0);
        transform.translation.x = render.x;
        transform.translation.y = render.y;
        *visibility = if runtime.active && health.get(&presentation.key).copied().unwrap_or(false) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[allow(clippy::needless_pass_by_value)]
fn sync_hostile_projectiles(
    mut commands: Commands,
    runtime: Res<EnemyLabRuntime>,
    arena: Res<LoadedArena>,
    mut visuals: Query<(Entity, &HostileProjectilePresentation, &mut Transform)>,
) {
    let mut existing = BTreeMap::new();
    for (entity, presentation, _) in &visuals {
        existing.insert(presentation.0, entity);
    }
    for projectile in runtime.hostile_projectiles() {
        let render = simulation_point_to_render(projectile.position(), &arena.0);
        if let Some(entity) = existing.remove(&projectile.id()) {
            if let Ok((_, _, mut transform)) = visuals.get_mut(entity) {
                transform.translation.x = render.x;
                transform.translation.y = render.y;
            }
        } else {
            let color = match projectile.damage_type() {
                sim_core::DamageType::Physical => Color::srgb_u8(235, 102, 82),
                sim_core::DamageType::Veil => Color::srgb_u8(198, 105, 231),
            };
            let radius = projectile.radius_tiles();
            let (label, outer_size, inner_size, rotation, hollow) = match projectile.source_kind() {
                sim_core::HostileProjectileSourceKind::AimedFan => {
                    let direction = projectile.direction().vector();
                    (
                        "tapered fan bolt",
                        Vec2::new(radius * 3.4, radius * 1.8),
                        Vec2::new(radius * 2.7, radius * 1.1),
                        -direction.y.atan2(direction.x),
                        false,
                    )
                }
                sim_core::HostileProjectileSourceKind::GapRing => (
                    "hollow gap-ring bolt",
                    Vec2::splat(radius * 3.2),
                    Vec2::splat(radius * 1.35),
                    std::f32::consts::FRAC_PI_4,
                    true,
                ),
            };
            commands
                .spawn((
                    Name::new(format!("Hostile {label} {}", projectile.id())),
                    HostileProjectilePresentation(projectile.id()),
                    crate::accessibility::HostileOutlineBaseSize(outer_size),
                    Sprite::from_color(Color::srgb_u8(244, 239, 214), outer_size),
                    Transform::from_xyz(render.x, render.y, HOSTILE_PROJECTILE_Z)
                        .with_rotation(Quat::from_rotation_z(rotation)),
                ))
                .with_children(|parent| {
                    if hollow {
                        parent
                            .spawn((
                                Name::new("Gap-ring color body"),
                                Sprite::from_color(color, Vec2::splat(radius * 2.55)),
                                Transform::from_xyz(0.0, 0.0, 0.1),
                            ))
                            .with_child((
                                Name::new("Hollow center"),
                                Sprite::from_color(Color::srgb_u8(16, 12, 20), inner_size),
                                Transform::from_xyz(0.0, 0.0, 0.1),
                            ));
                    } else {
                        parent.spawn((
                            Name::new("Fan bolt body"),
                            Sprite::from_color(color, inner_size),
                            Transform::from_xyz(0.0, 0.0, 0.1),
                        ));
                    }
                });
        }
    }
    for entity in existing.into_values() {
        commands.entity(entity).despawn();
    }
}

#[allow(clippy::needless_pass_by_value)]
fn draw_active_lane(
    mut gizmos: Gizmos,
    runtime: Res<EnemyLabRuntime>,
    arena: Res<LoadedArena>,
    scenario: Res<crate::combat::EvidenceScenario>,
    mut presentation: ResMut<EnemyPresentationState>,
) {
    let mut geometries = runtime.active_lane_geometries();
    if let Some(geometry) = geometries.first().copied() {
        presentation.last_lane_geometry = Some(geometry);
    }
    if geometries.is_empty()
        && *scenario == crate::combat::EvidenceScenario::EnemyShowcase
        && let Some(trace) = presentation.last_lane_geometry
    {
        geometries.push(trace);
    }
    let color = if runtime.hostile_hazard_count() > 0 {
        Color::srgba_u8(246, 96, 78, 205)
    } else {
        Color::srgba_u8(246, 184, 92, 110)
    };
    for geometry in geometries {
        let origin = simulation_point_to_render(geometry.origin, &arena.0);
        for axis in geometry.axes_degrees {
            let radians = f32::from(axis).to_radians();
            let direction = Vec2::new(radians.cos(), radians.sin());
            let normal = Vec2::new(-direction.y, direction.x);
            for side in [-0.5, 0.5] {
                let offset = normal * geometry.width_tiles * side;
                let from = origin - direction * 30.0 + offset;
                let to = origin + direction * 30.0 + offset;
                gizmos.line(
                    Vec3::new(from.x, from.y, HOSTILE_TELEGRAPH_Z),
                    Vec3::new(to.x, to.y, HOSTILE_TELEGRAPH_Z),
                    color,
                );
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn update_enemy_diagnostics(
    runtime: Res<EnemyLabRuntime>,
    presentation: Res<EnemyPresentationState>,
    mut text: Single<&mut Text, With<EnemyDiagnostics>>,
) {
    let readiness = runtime.lab.readiness();
    let health: Vec<_> = runtime
        .health_snapshots()
        .into_iter()
        .map(|snapshot| snapshot.current_health)
        .collect();
    let lane_visual = if runtime.lab.active_lane().is_some() {
        "ACTIVE"
    } else if presentation.last_lane_geometry.is_some() {
        "TRACE"
    } else {
        "NONE"
    };
    text.0 = format!(
        "THREE-ROLE ENEMY LAB\nFAN {} / HIT {}   RING {} / HIT {}   LANES {} / HIT {}   MOVES {}\nENEMY HP {}/{}/{}   FRIENDLY HITS {}   DEATHS {}   DROPS {}\nHOSTILES {}   GRACE IGNORED {}   PLAYER HP {}   LANE VIS {}   LAST {}",
        presentation.fan_fires,
        u8::from(readiness.fan_damaged_player()),
        presentation.ring_fires,
        u8::from(readiness.ring_damaged_player()),
        presentation.lane_activations,
        u8::from(readiness.lane_damaged_player()),
        presentation.approach_moves,
        health.first().copied().unwrap_or(0),
        health.get(1).copied().unwrap_or(0),
        health.get(2).copied().unwrap_or(0),
        presentation.friendly_hits,
        presentation.enemy_deaths,
        presentation.reward_drops,
        runtime.hostile_projectile_count(),
        presentation.projectile_grace_ignored,
        runtime.consumables().vitals().current_health(),
        lane_visual,
        presentation.last_attack,
    );
}

#[allow(clippy::needless_pass_by_value)]
fn update_pattern_diagnostics(
    runtime: Res<EnemyLabRuntime>,
    presentation: Res<EnemyPresentationState>,
    mut text: Single<&mut Text, With<PatternDiagnostics>>,
) {
    let [fan, ring, lane] = &runtime.patterns;
    let readability = if runtime.boss_snapshot().is_some() {
        &runtime.boss_readability
    } else {
        &runtime.readability
    };
    let exposure = if presentation
        .attack_telegraphs
        .iter()
        .all(|count| *count >= 2)
    {
        "REPEAT LEGAL"
    } else {
        "FIRST USE"
    };
    text.0 = format!(
        "READABILITY + FAIRNESS | TICK {}\n{} W{}/{} THR{} C{} {} {} A{}\n{} W{}/{} THR{} C{} {} {} A{}\n{} W{}/{} THR{} C{} {} {} A{}\nSAFE .80 | SPD4.5 | RTT120 | REACH350\nGRAY DART / HOLLOW / BANDS DISTINCT\nLAYER T50>H40>F30>L20>D10\nAUDIO WARN {}@80 MAJOR {}@100\nBUDGET THR{} INST{}/{} | COMPAT CLEAR\nEXPOSURE F{} R{} L{} | {exposure}",
        runtime.lab.tick().0,
        fan.label,
        fan.first_warning_ticks,
        fan.repeated_warning_ticks,
        fan.threat,
        fan.cap,
        fan.counterplay,
        fan.grayscale,
        fan.audio_priority,
        ring.label,
        ring.first_warning_ticks,
        ring.repeated_warning_ticks,
        ring.threat,
        ring.cap,
        ring.counterplay,
        ring.grayscale,
        ring.audio_priority,
        lane.label,
        lane.first_warning_ticks,
        lane.repeated_warning_ticks,
        lane.threat,
        lane.cap,
        lane.counterplay,
        lane.grayscale,
        lane.audio_priority,
        readability.standard_audio_cues,
        readability.major_audio_cues,
        readability.total_threat,
        readability.total_active_instances,
        readability.encounter_cap,
        presentation.attack_telegraphs[0],
        presentation.attack_telegraphs[1],
        presentation.attack_telegraphs[2],
    );
}

#[allow(clippy::needless_pass_by_value)]
fn update_damage_diagnostics(
    runtime: Res<EnemyLabRuntime>,
    presentation: Res<EnemyPresentationState>,
    collision_diagnostics: Res<crate::combat::CollisionDiagnostics>,
    mut text: Single<&mut Text, With<DamageDiagnostics>>,
) {
    let Some(damage) = presentation.last_damage else {
        "DAMAGE | WAITING FOR HOSTILE CONTACT\nHEALTH [----------------]  TYPE --  BAND --\nSOURCE --  DIRECTION --"
            .clone_into(&mut text.0);
        return;
    };
    let vitals = runtime.consumables().vitals();
    let current = vitals.current_health();
    let maximum = vitals.maximum_health();
    let state = if damage.lethal && collision_diagnostics.later_action_rejected() {
        "LETHAL | LATER ACTIONS REJECTED"
    } else {
        "ALIVE | COMBAT INPUT ACTIVE"
    };
    text.0 = format!(
        "CURRENT HEALTH {}/{}  [{}]  |  {state}\nLAST HIT {} > {}  |  DAMAGE {}  |  {} {}\nSOURCE {}  |  SOURCE SIDE EAST  |  TRAVEL WESTBOUND",
        current,
        maximum,
        health_frame(current, maximum),
        damage.health_before,
        damage.health_after,
        damage.amount,
        damage_type_label(damage.damage_type),
        damage_band_label(damage.band),
        source_label(damage.source),
    );
}

fn damage_presentation(source: EntityId, damage: &DamageEvent) -> DamagePresentation {
    DamagePresentation {
        amount: damage.health_damage_applied,
        health_before: damage.health_before,
        health_after: damage.health_after,
        damage_type: damage.damage_type,
        band: damage.resolved_band,
        source,
        lethal: damage.lethal,
    }
}

fn source_label(source: EntityId) -> &'static str {
    match source.get() % 100_000 {
        10_001 => "DROWNED PILGRIM",
        10_002 => "BELL REED",
        10_003 => "CHAIN SENTRY",
        40_001 => "BELL PROCTOR",
        _ => "UNKNOWN HOSTILE",
    }
}

const fn damage_type_label(damage_type: DamageType) -> &'static str {
    match damage_type {
        DamageType::Physical => "PHYSICAL",
        DamageType::Veil => "VEIL",
    }
}

const fn damage_band_label(band: Option<DamageBand>) -> &'static str {
    match band {
        Some(DamageBand::Chip) => "CHIP",
        Some(DamageBand::Pressure) => "PRESSURE",
        Some(DamageBand::Major) => "MAJOR",
        Some(DamageBand::Severe) => "SEVERE",
        Some(DamageBand::Execution) => "EXECUTION",
        None => "ABSORBED",
    }
}

fn health_frame(current: u32, maximum: u32) -> String {
    const SEGMENTS: u32 = 16;
    let filled = if maximum == 0 {
        0
    } else {
        current
            .saturating_mul(SEGMENTS)
            .div_ceil(maximum)
            .min(SEGMENTS)
    };
    let mut frame = String::with_capacity(SEGMENTS as usize);
    frame.extend(std::iter::repeat_n('#', filled as usize));
    frame.extend(std::iter::repeat_n('-', (SEGMENTS - filled) as usize));
    frame
}

fn compile_pattern_debug(
    definitions: &EnemyLabDefinitions,
) -> Result<([PatternDebugEntry; 3], ReadabilityDebug)> {
    let pilgrim_parameters = definitions.drowned_pilgrim.parameters();
    let mut fan = PatternDefinition::from_projectile_attack(
        &pilgrim_parameters.attack,
        PatternKind::Fan {
            projectile_count: 3,
            offsets_degrees: vec![-15, 0, 15],
        },
        PatternContext::Normal,
        300,
        300,
        OriginCue::SourceSilhouette,
        ShapeCue::Fan,
        PatternFairnessFixture::baseline(800, 5_000, 0),
    );
    fan.compatibility_tags = tags(&["fan_projectile"]);
    let fan = fan
        .validate()
        .map_err(|diagnostics| anyhow!("Pilgrim pattern validation failed: {diagnostics:?}"))?;

    let reed_parameters = definitions.bell_reed.parameters();
    let mut ring = PatternDefinition::from_projectile_attack(
        &reed_parameters.attack,
        PatternKind::RingWithGap {
            index_count: 8,
            omitted_count: 2,
            omitted_start_advance: 3,
        },
        PatternContext::Normal,
        450,
        300,
        OriginCue::SourceSilhouette,
        ShapeCue::RingGap,
        PatternFairnessFixture::baseline(800, 4_000, 0),
    );
    ring.compatibility_tags = tags(&["radial_projectile"]);
    let ring = ring
        .validate()
        .map_err(|diagnostics| anyhow!("Reed pattern validation failed: {diagnostics:?}"))?;

    let sentry_parameters = definitions.chain_sentry.parameters();
    let mut lane = PatternDefinition::from_lane_attack(
        &sentry_parameters.attack,
        PatternContext::Normal,
        800,
        650,
        PatternFairnessFixture::baseline(800, 2_000, 800),
    );
    lane.compatibility_tags = tags(&["lane_or_beam"]);
    let lane = lane
        .validate()
        .map_err(|diagnostics| anyhow!("Sentry pattern validation failed: {diagnostics:?}"))?;

    sim_core::validate_pattern_combination(&[fan.clone(), ring.clone(), lane.clone()])
        .map_err(|diagnostics| anyhow!("enemy pattern combination failed: {diagnostics:?}"))?;
    let manifest = compile_hostile_readability_manifest(&[fan.clone(), ring.clone(), lane.clone()])
        .map_err(|diagnostics| anyhow!("enemy readability validation failed: {diagnostics:?}"))?;
    let entries = [
        debug_entry(
            "FAN",
            &fan,
            "STRAFE",
            manifest
                .profile(fan.definition().pattern_id.as_str())
                .expect("compiled fan profile"),
        ),
        debug_entry(
            "RING",
            &ring,
            "FOLLOW GAP",
            manifest
                .profile(ring.definition().pattern_id.as_str())
                .expect("compiled ring profile"),
        ),
        debug_entry(
            "LANE",
            &lane,
            "LEAVE SHAPE",
            manifest
                .profile(lane.definition().pattern_id.as_str())
                .expect("compiled lane profile"),
        ),
    ];
    let readability = ReadabilityDebug {
        total_threat: manifest.total_threat_cost(),
        total_active_instances: manifest.total_maximum_active_instances(),
        encounter_cap: manifest.encounter_projectile_cap(),
        standard_audio_cues: manifest
            .profiles()
            .iter()
            .filter(|profile| profile.audio_priority == WarningAudioPriority::Standard)
            .count(),
        major_audio_cues: manifest
            .profiles()
            .iter()
            .filter(|profile| profile.audio_priority == WarningAudioPriority::MajorOrHigher)
            .count(),
    };
    Ok((entries, readability))
}

fn compile_boss_readability(definition: &BellProctorDefinition) -> Result<ReadabilityDebug> {
    let parameters = definition.parameters();
    let mut fan = PatternDefinition::from_projectile_attack(
        &parameters.fan,
        PatternKind::Fan {
            projectile_count: 5,
            offsets_degrees: parameters.fan_offsets_degrees.to_vec(),
        },
        PatternContext::Boss,
        400,
        400,
        OriginCue::SourceSilhouette,
        ShapeCue::Fan,
        PatternFairnessFixture::baseline(800, 5_000, 0),
    );
    fan.compatibility_tags = tags(&["fan_projectile"]);
    let fan = fan
        .validate()
        .map_err(|diagnostics| anyhow!("Bell fan validation failed: {diagnostics:?}"))?;

    let mut ring = PatternDefinition::from_projectile_attack(
        &parameters.ring,
        PatternKind::RingWithGap {
            index_count: parameters.ring_index_count,
            omitted_count: parameters.ring_omitted_count,
            omitted_start_advance: parameters.ring_gap_advance,
        },
        PatternContext::Boss,
        650,
        650,
        OriginCue::SourceSilhouette,
        ShapeCue::RingGap,
        PatternFairnessFixture::baseline(800, 4_000, 0),
    );
    ring.compatibility_tags = tags(&["radial_projectile"]);
    let ring = ring
        .validate()
        .map_err(|diagnostics| anyhow!("Bell ring validation failed: {diagnostics:?}"))?;

    let mut cross = PatternDefinition::from_lane_attack(
        &parameters.cross,
        PatternContext::Boss,
        900,
        900,
        PatternFairnessFixture::baseline(800, 2_000, 800),
    );
    cross.compatibility_tags = tags(&["lane_or_beam"]);
    let cross = cross
        .validate()
        .map_err(|diagnostics| anyhow!("Bell cross validation failed: {diagnostics:?}"))?;
    sim_core::validate_pattern_combination(&[fan.clone(), ring.clone(), cross.clone()])
        .map_err(|diagnostics| anyhow!("Bell pattern combination failed: {diagnostics:?}"))?;
    let manifest = compile_hostile_readability_manifest(&[fan, ring, cross])
        .map_err(|diagnostics| anyhow!("Bell readability validation failed: {diagnostics:?}"))?;
    if manifest.total_threat_cost() != 41
        || manifest.total_maximum_active_instances() != 36
        || manifest.encounter_projectile_cap() != 500
    {
        return Err(anyhow!(
            "Bell readability aggregate differs from exact threat 41 / instances 36 / cap 500"
        ));
    }
    Ok(ReadabilityDebug {
        total_threat: manifest.total_threat_cost(),
        total_active_instances: manifest.total_maximum_active_instances(),
        encounter_cap: manifest.encounter_projectile_cap(),
        standard_audio_cues: 2,
        major_audio_cues: 1,
    })
}

fn debug_entry(
    label: &'static str,
    pattern: &sim_core::ValidatedPattern,
    counterplay: &'static str,
    profile: &HostileReadabilityProfile,
) -> PatternDebugEntry {
    PatternDebugEntry {
        label,
        first_warning_ticks: pattern.first_warning_ticks(),
        repeated_warning_ticks: pattern.repeated_warning_ticks(),
        threat: pattern.definition().threat_cost,
        cap: pattern.definition().maximum_active_instances,
        counterplay,
        grayscale: grayscale_label(profile.grayscale_signature),
        audio_priority: profile.audio_priority.priority(),
    }
}

const fn grayscale_label(signature: GrayscaleSignature) -> &'static str {
    match signature {
        GrayscaleSignature::TaperedFanBolt => "DART",
        GrayscaleSignature::HollowGapRing => "HOLLOW",
        GrayscaleSignature::BandedLane => "BANDS",
        GrayscaleSignature::TimelineSequence => "TIMELINE",
    }
}

fn tags(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showcase_ids_and_positions_are_stable_and_distinct() {
        let ids = [id(10_001), id(10_002), id(10_003), id(10_004)];
        assert!(ids.windows(2).all(|pair| pair[0] != pair[1]));
        assert_ne!(PILGRIM_POSITION, REED_POSITION);
        assert_ne!(REED_POSITION, SENTRY_POSITION);
    }

    #[test]
    fn each_enemy_kind_has_a_distinct_presentation_role() {
        assert_ne!(
            sim_core::EnemyActorKind::DrownedPilgrim,
            sim_core::EnemyActorKind::BellReed
        );
        assert_ne!(
            sim_core::EnemyActorKind::BellReed,
            sim_core::EnemyActorKind::ChainSentry
        );
    }

    #[test]
    fn damage_labels_never_rely_on_color_alone() {
        assert_eq!(source_label(id(10_001)), "DROWNED PILGRIM");
        assert_eq!(source_label(id(10_002)), "BELL REED");
        assert_eq!(source_label(id(10_003)), "CHAIN SENTRY");
        assert_eq!(damage_type_label(DamageType::Physical), "PHYSICAL");
        assert_eq!(damage_type_label(DamageType::Veil), "VEIL");
        assert_eq!(damage_band_label(Some(DamageBand::Major)), "MAJOR");
        assert_eq!(damage_band_label(None), "ABSORBED");
    }

    #[test]
    fn health_frame_is_bounded_and_uses_sixteen_segments() {
        assert_eq!(health_frame(128, 128), "################");
        assert_eq!(health_frame(64, 128), "########--------");
        assert_eq!(health_frame(1, 128), "#---------------");
        assert_eq!(health_frame(0, 128), "----------------");
        assert_eq!(health_frame(200, 128), "################");
        assert_eq!(health_frame(1, 0), "----------------");
    }

    #[test]
    fn actual_combat_layers_match_the_validated_priority_stack() {
        const {
            assert!(HOSTILE_TELEGRAPH_Z > HOSTILE_PROJECTILE_Z);
            assert!(HOSTILE_PROJECTILE_Z > crate::combat::FRIENDLY_PROJECTILE_Z);
        }
        let loot_z = ENEMY_Z + 0.3;
        assert!(crate::combat::FRIENDLY_PROJECTILE_Z > loot_z);
        assert!(sim_core::canonical_priority_stack_is_valid());
    }
}
