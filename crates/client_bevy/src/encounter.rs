use std::collections::{BTreeMap, VecDeque};

use anyhow::{Result, anyhow};
use bevy::prelude::*;
use sim_core::{
    BELL_REED_ID, CHAIN_SENTRY_ID, DROWNED_PILGRIM_ID, EncounterAction, EncounterEvent,
    EncounterInput, EncounterSpawnSpec, EncounterStage, EncounterState, EquipmentItem, FieldPickup,
    FieldPickupId, InventoryStack, ItemContentId, ItemInstanceId, NormalWaveEnemyKind,
    NormalWavePhase, NormalWaveSpawn, RawDamageIntent, RawDamageIntentSource, RewardChoice,
    RewardOutcome, SimulationVector, SpawnInstanceId, SpawnLocation,
};

use crate::{
    FixedSimulationSet, FrameSet, LoadedArena,
    arena_view::simulation_point_to_render,
    combat::{CombatInputSampler, EvidenceScenario},
    death::{LocalDeathRuntime, LocalRunFactory},
    enemies::{EnemyLabRuntime, HostileProjectilePresentation, NormalRewardPresentation},
    item_showcase::ItemShowcaseCatalog,
    player::{LatestMovementAction, PlayerSimulation},
};

const TELEGRAPH_Z: f32 = 4.8;
const ENEMY_Z: f32 = 5.5;

#[derive(Debug, Default, Resource)]
pub(crate) struct EncounterClientState {
    pending_spawns: BTreeMap<SpawnInstanceId, EncounterSpawnSpec>,
    reward_open: Option<EncounterStage>,
    reward_offers: VecDeque<FieldPickup>,
    left_rewards: Vec<FieldPickup>,
    latest_message: String,
    pause_open: bool,
    completion_clear_ticks: Option<u64>,
    completion_best_ticks: Option<u64>,
}

impl EncounterClientState {
    pub(crate) const fn completion_clear_ticks(&self) -> Option<u64> {
        self.completion_clear_ticks
    }
}

#[derive(Debug, Component)]
struct EncounterStatus;

#[derive(Debug, Component)]
struct SpawnTelegraphPresentation(SpawnInstanceId);

#[derive(Debug, Component)]
struct NormalEnemyPresentation {
    instance_id: SpawnInstanceId,
    entity_id: sim_core::EntityId,
}

#[derive(Debug, Component)]
struct BellProctorPresentation {
    instance_id: SpawnInstanceId,
}

pub(crate) fn configure(app: &mut App) {
    app.init_resource::<EncounterClientState>()
        .add_systems(Startup, spawn_encounter_status)
        .add_systems(
            Update,
            sync_modal_combat_gate.in_set(FrameSet::CameraFollow),
        )
        .add_systems(
            FixedUpdate,
            (advance_normal_encounter, handle_victory_restart)
                .chain()
                .in_set(FixedSimulationSet::Encounter),
        )
        .add_systems(
            Update,
            (sync_normal_enemy_presentation, update_encounter_status)
                .chain()
                .in_set(FrameSet::Presentation),
        );
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)] // Fixed-tick orchestration remains linear so event ordering is directly reviewable.
fn advance_normal_encounter(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    arena: Res<LoadedArena>,
    movement: Res<LatestMovementAction>,
    combat_input: Res<CombatInputSampler>,
    scenario: Res<EvidenceScenario>,
    player: Res<PlayerSimulation>,
    catalog: Res<ItemShowcaseCatalog>,
    mut death: ResMut<LocalDeathRuntime>,
    mut runtime: ResMut<EnemyLabRuntime>,
    mut state: ResMut<EncounterClientState>,
    telegraphs: Query<(Entity, &SpawnTelegraphPresentation)>,
    mut enemies: Query<
        (Entity, &NormalEnemyPresentation, &mut Visibility),
        Without<BellProctorPresentation>,
    >,
    mut bosses: Query<
        (Entity, &BellProctorPresentation, &mut Visibility),
        Without<NormalEnemyPresentation>,
    >,
) {
    if !runtime.normal_mode() || !runtime.player_can_act() {
        return;
    }
    let Some(mut combat_step) = runtime.take_pending_combat_step() else {
        return;
    };

    if matches!(
        *scenario,
        EvidenceScenario::BossShowcase | EvidenceScenario::BossCompletionShowcase
    ) {
        if matches!(runtime.normal_phase(), Some(NormalWavePhase::Active)) {
            let targets = runtime
                .alive_hurtboxes()
                .expect("scripted evidence hurtboxes")
                .into_iter()
                .map(sim_core::EnemyHurtbox::id)
                .collect();
            combat_step = scripted_damage_step(combat_step.tick, targets, 1_000);
        } else if *scenario == EvidenceScenario::BossCompletionShowcase
            && let Some(snapshot) = runtime.boss_snapshot()
            && snapshot.local_tick.0 >= 60
            && !matches!(snapshot.state, sim_core::BellProctorStateKind::Defeated)
        {
            combat_step = scripted_damage_step(combat_step.tick, vec![snapshot.entity_id], 4_000);
        }
    }

    let mut defeated = if runtime
        .boss_snapshot()
        .is_some_and(|snapshot| !matches!(snapshot.state, sim_core::BellProctorStateKind::Defeated))
    {
        runtime
            .step_boss(&combat_step, player.state().position())
            .expect("validated Bell Proctor must advance transactionally")
    } else if runtime.normal_phase().is_some() {
        runtime
            .step_normal_wave(&combat_step, player.state().position())
            .expect("validated normal wave must advance transactionally")
    } else {
        Vec::new()
    };
    defeated.sort_unstable();

    let scripted_reward = matches!(
        *scenario,
        EvidenceScenario::BossShowcase | EvidenceScenario::BossCompletionShowcase
    ) && state.reward_open.is_some_and(|stage| {
        stage != EncounterStage::Boss || *scenario == EvidenceScenario::BossCompletionShowcase
    }) && !state.reward_offers.is_empty();
    let reward_resolved = if scripted_reward {
        let offers = std::mem::take(&mut state.reward_offers);
        state.left_rewards.extend(offers);
        true
    } else {
        process_reward_input(&keyboard, &mut death, &mut state, combat_step.tick)
    };
    if reward_resolved && state.reward_open == Some(EncounterStage::Boss) {
        state.reward_open = None;
        state.latest_message = completion_summary_message(&state, &runtime);
    }
    if matches!(death.encounter().state(), EncounterState::ClearedArena)
        && keyboard.just_pressed(KeyCode::Escape)
    {
        state.pause_open = !state.pause_open;
        state.latest_message = if state.pause_open {
            "PAUSED | CLEARED ARENA | [R] RUN AGAIN | [ESC] RESUME".to_owned()
        } else {
            "CLEARED ARENA | [R] RUN AGAIN | [ESC] PAUSE".to_owned()
        };
    }
    let action = modal_encounter_action(
        death.encounter().state(),
        state.reward_offers.is_empty(),
        keyboard.just_pressed(KeyCode::Escape),
        reward_resolved,
    );
    let scripted_debug_activity = matches!(
        *scenario,
        EvidenceScenario::DebugOverlayShowcase
            | EvidenceScenario::DebugToolsShowcase
            | EvidenceScenario::BossShowcase
            | EvidenceScenario::BossCompletionShowcase
    ) && matches!(
        death.encounter().state(),
        EncounterState::AwaitingFirstActivity
    );
    let player_moved = movement.0.normalized_vector().length_squared() > 0.0;
    let step = death
        .advance_encounter(EncounterInput {
            player_moved,
            player_fired: combat_input.player_fired() || scripted_debug_activity,
            defeated,
            action,
        })
        .expect("client encounter inputs are derived from authoritative runtime state");

    for event in step.events {
        match event {
            EncounterEvent::FirstActivityObserved { wave_starts_at, .. } => {
                state.latest_message = format!("BELL STIRS | WAVE 1 AT {}T", wave_starts_at.0);
            }
            EncounterEvent::SpawnTelegraphStarted {
                stage,
                activates_at,
                spawns,
                ..
            } => {
                state.pending_spawns = spawns
                    .iter()
                    .map(|spawn| (spawn.instance_id, *spawn))
                    .collect();
                let normal_spawns = spawns
                    .iter()
                    .map(|spawn| compile_normal_spawn(*spawn, &arena.0))
                    .collect::<Result<Vec<_>>>()
                    .expect("validated authored normal spawn");
                runtime
                    .start_normal_wave(normal_spawns, combat_step.tick)
                    .expect("cleared wave hands off persistent player state");
                runtime
                    .step_normal_wave(&combat_step, player.state().position())
                    .expect("new telegraph consumes its authored first global tick");
                spawn_telegraphs(&mut commands, &arena.0, &spawns);
                spawn_normal_enemies(&mut commands, &arena.0, &runtime, false);
                state.latest_message = format!(
                    "{} TELEGRAPH | {} HOSTILES | ACTIVE {}T",
                    stage_label(stage),
                    spawns.len(),
                    activates_at.0
                );
            }
            EncounterEvent::HostilesActivated {
                stage, instances, ..
            } => {
                if stage == EncounterStage::Boss {
                    let [instance] = instances.as_slice() else {
                        panic!("Bell Proctor activation must contain exactly one instance")
                    };
                    runtime
                        .start_boss(*instance, combat_step.tick)
                        .expect("Wave 3 handoff starts the real Bell Proctor");
                    runtime
                        .step_boss(&combat_step, player.state().position())
                        .expect("Bell Proctor consumes its first active global tick");
                    spawn_bell_proctor(&mut commands, &arena.0, *instance);
                } else {
                    assert_eq!(runtime.normal_phase(), Some(NormalWavePhase::Active));
                }
                for (entity, telegraph) in &telegraphs {
                    if instances.binary_search(&telegraph.0).is_ok() {
                        commands.entity(entity).despawn();
                    }
                }
                for (_, enemy, mut visibility) in &mut enemies {
                    if instances.binary_search(&enemy.instance_id).is_ok() {
                        *visibility = Visibility::Inherited;
                    }
                }
                for (_, boss, mut visibility) in &mut bosses {
                    if instances.binary_search(&boss.instance_id).is_ok() {
                        *visibility = Visibility::Inherited;
                    }
                }
                state.latest_message =
                    format!("{} ACTIVE | {} REMAIN", stage_label(stage), instances.len());
            }
            EncounterEvent::HostileDefeatAccepted {
                instance,
                remaining_hostiles,
                ..
            } => {
                for (entity, enemy, _) in &enemies {
                    if enemy.instance_id == instance {
                        commands.entity(entity).despawn();
                    }
                }
                for (entity, boss, _) in &bosses {
                    if boss.instance_id == instance {
                        commands.entity(entity).despawn();
                    }
                }
                state.latest_message = format!("HOSTILE DOWN | {remaining_hostiles} REMAIN");
            }
            EncounterEvent::HostileProjectilesCleared {
                completed_stage, ..
            } => {
                assert_eq!(runtime.hostile_projectile_count(), 0);
                assert_eq!(runtime.hostile_hazard_count(), 0);
                state.latest_message =
                    format!("{} CLEAR | REWARD IN 1.5S", stage_label(completed_stage));
            }
            EncounterEvent::RewardPanelOpened {
                completed_stage,
                reward_id,
                ..
            } => {
                state.reward_open = Some(completed_stage);
                state.reward_offers = compile_reward_offers(
                    &catalog,
                    reward_id,
                    death.encounter().seed(),
                    death.encounter().run_ordinal(),
                    completed_stage,
                    player.state().position(),
                    combat_step.tick,
                )
                .expect("validated reward table compiles immutable offers");
                state.latest_message = if completed_stage == EncounterStage::Wave3 {
                    "WAVE 3 REWARD | RESOLVE ITEMS | BELL PROCTOR NEXT".to_owned()
                } else {
                    format!(
                        "{} REWARD | [1] TAKE [2] EQUIP [4] LEAVE",
                        stage_label(completed_stage)
                    )
                };
            }
            EncounterEvent::RewardPanelClosed {
                completed_stage, ..
            } => {
                state.reward_open = None;
                state.reward_offers.clear();
                state.latest_message = format!("{} REWARD CLOSED", stage_label(completed_stage));
            }
            EncounterEvent::BossIntroductionStarted {
                activates_at,
                spawn,
                ..
            } => {
                state.pending_spawns = BTreeMap::from([(spawn.instance_id, spawn)]);
                spawn_telegraphs(&mut commands, &arena.0, &[spawn]);
                state.latest_message = format!(
                    "BELL PROCTOR INTRODUCTION | ACTIVE {}T | NO HURTBOX / NO ATTACKS",
                    activates_at.0
                );
            }
            EncounterEvent::CompletionSummaryOpened {
                reward_id,
                clear_ticks,
                best_clear_ticks,
                ..
            } => {
                state.completion_clear_ticks = Some(clear_ticks);
                state.completion_best_ticks = Some(best_clear_ticks);
                state.reward_open = Some(EncounterStage::Boss);
                state.reward_offers = compile_reward_offers(
                    &catalog,
                    reward_id,
                    death.encounter().seed(),
                    death.encounter().run_ordinal(),
                    EncounterStage::Boss,
                    player.state().position(),
                    combat_step.tick,
                )
                .expect("validated boss reward compiles immutable offers");
                state.latest_message = format!(
                    "BELL PROCTOR DEFEATED | CLEAR {}S | BEST {}S | DAMAGE TAKEN {} | TONICS {} | LETHAL NONE | REWARD: [1] TAKE [2] EQUIP [4] LEAVE",
                    format_ticks_tenths(clear_ticks),
                    format_ticks_tenths(best_clear_ticks),
                    runtime.consumables().cumulative_damage_taken(),
                    runtime.consumables().accepted_tonic_uses(),
                );
            }
            EncounterEvent::CompletionSummaryClosed { .. } => {
                state.reward_open = None;
                state.reward_offers.clear();
                "CLEARED ARENA | [R] RUN AGAIN".clone_into(&mut state.latest_message);
            }
            EncounterEvent::PlayerDeathAccepted { .. }
            | EncounterEvent::RunRestarted { .. }
            | EncounterEvent::RecallRejected { .. }
            | EncounterEvent::RewardDelayStarted { .. } => {}
        }
    }
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn handle_victory_restart(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    factory: Res<LocalRunFactory>,
    mut death: ResMut<LocalDeathRuntime>,
    mut player: ResMut<PlayerSimulation>,
    mut runtime: ResMut<EnemyLabRuntime>,
    mut state: ResMut<EncounterClientState>,
    visuals: Query<
        Entity,
        Or<(
            With<SpawnTelegraphPresentation>,
            With<NormalEnemyPresentation>,
            With<BellProctorPresentation>,
            With<HostileProjectilePresentation>,
            With<NormalRewardPresentation>,
        )>,
    >,
) {
    if !keyboard.just_pressed(KeyCode::KeyR)
        || !state.reward_offers.is_empty()
        || !matches!(
            death.encounter().state(),
            EncounterState::CompletionSummary | EncounterState::ClearedArena
        )
    {
        return;
    }
    let commit = death
        .restart_after_victory()
        .expect("victory Run Again is valid only from a cleared state");
    runtime.clear_for_local_death();
    for entity in &visuals {
        commands.entity(entity).despawn();
    }
    let (fresh_player, fresh_runtime) = factory
        .build_fresh_run(commit.new_run_ordinal)
        .expect("victory restart reconstructs the exact default run");
    *player = fresh_player;
    *runtime = fresh_runtime;
    *state = EncounterClientState {
        latest_message: format!(
            "RUN {} FRESH | CONTROL READY | RESTART {}T",
            commit.new_run_ordinal, commit.restart_elapsed_ticks
        ),
        ..EncounterClientState::default()
    };
}

fn format_ticks_tenths(ticks: u64) -> String {
    let tenths = ticks.saturating_mul(10) / 30;
    format!("{}.{:01}", tenths / 10, tenths % 10)
}

fn completion_summary_message(state: &EncounterClientState, runtime: &EnemyLabRuntime) -> String {
    format!(
        "BELL PROCTOR DEFEATED | CLEAR {}S | BEST {}S | DAMAGE TAKEN {} | TONICS {} | LETHAL NONE | [R] RUN AGAIN (PRIMARY) | [ESC] STAY IN CLEARED ARENA",
        format_ticks_tenths(state.completion_clear_ticks.unwrap_or_default()),
        format_ticks_tenths(state.completion_best_ticks.unwrap_or_default()),
        runtime.consumables().cumulative_damage_taken(),
        runtime.consumables().accepted_tonic_uses(),
    )
}

const fn modal_encounter_action(
    encounter_state: EncounterState,
    reward_offers_empty: bool,
    escape_pressed: bool,
    reward_resolved: bool,
) -> EncounterAction {
    if matches!(encounter_state, EncounterState::CompletionSummary)
        && reward_offers_empty
        && escape_pressed
    {
        EncounterAction::CloseCompletionSummary
    } else if reward_resolved && !matches!(encounter_state, EncounterState::CompletionSummary) {
        EncounterAction::CloseRewardPanel
    } else {
        EncounterAction::None
    }
}

fn scripted_damage_step(
    tick: sim_core::Tick,
    mut targets: Vec<sim_core::EntityId>,
    raw: u32,
) -> sim_core::CombatStep {
    targets.sort_unstable();
    let mut step = sim_core::CombatStep {
        tick,
        ..sim_core::CombatStep::default()
    };
    for (index, target) in targets.into_iter().enumerate() {
        let index = u64::try_from(index).expect("evidence target count is bounded");
        let projectile_id = sim_core::EntityId::new(
            900_000_u64
                .checked_add(tick.0.saturating_mul(32))
                .and_then(|value| value.checked_add(index + 1))
                .expect("evidence projectile identity fits u64"),
        )
        .expect("evidence projectile ID is nonzero");
        step.collisions.push(sim_core::ProjectileCollision {
            tick,
            projectile_id,
            source: sim_core::FriendlyProjectileSource::Primary,
            target: sim_core::CollisionTarget::Enemy(target),
            final_position: SimulationVector::default(),
            distance_travelled_tiles: 0.0,
            contact_ordinal: 0,
            empowered_by_slipstep: false,
            focused_by_stillness: false,
            projectile_continues: false,
        });
        step.raw_damage_intents.push(RawDamageIntent {
            tick,
            projectile_id,
            source: RawDamageIntentSource::Primary,
            target,
            base_raw_damage: raw,
            multiplier_basis_points: 10_000,
            resolved_raw_damage: raw,
            contact_ordinal: 0,
        });
    }
    step
}

#[allow(clippy::needless_pass_by_value)]
fn sync_modal_combat_gate(
    state: Res<EncounterClientState>,
    death: Res<LocalDeathRuntime>,
    mut gate: ResMut<crate::combat::CombatInputGate>,
) {
    gate.blocked = state.reward_open.is_some()
        || state.pause_open
        || matches!(
            death.encounter().state(),
            EncounterState::CompletionSummary | EncounterState::ClearedArena
        );
}

fn process_reward_input(
    keyboard: &ButtonInput<KeyCode>,
    death: &mut LocalDeathRuntime,
    state: &mut EncounterClientState,
    now: sim_core::Tick,
) -> bool {
    if state.reward_open.is_none() || state.reward_offers.is_empty() {
        return false;
    }
    let choice = if keyboard.just_pressed(KeyCode::Digit1) {
        Some(RewardChoice::Take)
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        Some(RewardChoice::Equip)
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        Some(RewardChoice::LeaveReward)
    } else {
        None
    };
    let Some(choice) = choice else {
        return false;
    };
    let mut offer = state
        .reward_offers
        .pop_front()
        .expect("nonempty reward queue was checked");
    match death.apply_reward_choice(&mut offer, choice, now) {
        Ok(RewardOutcome::Collected { dropped, .. }) => {
            if let Some(dropped) = dropped {
                state.left_rewards.push(dropped);
            }
            state.latest_message = format!(
                "REWARD ACCEPTED | {} OFFER{} REMAIN",
                state.reward_offers.len(),
                if state.reward_offers.len() == 1 {
                    ""
                } else {
                    "S"
                }
            );
        }
        Ok(RewardOutcome::LeftReward { .. }) => {
            state.left_rewards.push(offer);
            state.latest_message = format!(
                "REWARD LEFT IN ARENA | {} OFFER{} REMAIN",
                state.reward_offers.len(),
                if state.reward_offers.len() == 1 {
                    ""
                } else {
                    "S"
                }
            );
        }
        Ok(RewardOutcome::CapacityBlocked { .. }) => {
            state.reward_offers.push_front(offer);
            "CAPACITY BLOCKED | REWARD PRESERVED".clone_into(&mut state.latest_message);
            return false;
        }
        Err(error) => {
            state.reward_offers.push_front(offer);
            state.latest_message = format!("REWARD ACTION REJECTED | {error}");
            return false;
        }
    }
    state.reward_offers.is_empty()
}

#[allow(clippy::too_many_arguments)]
fn compile_reward_offers(
    catalog: &ItemShowcaseCatalog,
    reward_id: &str,
    seed: u64,
    run_ordinal: u32,
    stage: EncounterStage,
    position: SimulationVector,
    now: sim_core::Tick,
) -> Result<VecDeque<FieldPickup>> {
    let stage_ordinal = match stage {
        EncounterStage::Wave1 => 1_u64,
        EncounterStage::Wave2 => 2,
        EncounterStage::Wave3 => 3,
        EncounterStage::Boss => 4,
    };
    let resolution_id = u64::from(run_ordinal)
        .checked_mul(10)
        .and_then(|value| value.checked_add(stage_ordinal))
        .ok_or_else(|| anyhow!("reward resolution ID overflow"))?;
    let grants = catalog.resolve_reward(reward_id, seed, resolution_id)?;
    let run_base = u64::from(
        run_ordinal
            .checked_sub(1)
            .ok_or_else(|| anyhow!("run ordinal must be nonzero"))?,
    )
    .checked_mul(100_000)
    .ok_or_else(|| anyhow!("reward identity run base overflow"))?;
    grants
        .into_iter()
        .enumerate()
        .map(|(index, grant)| {
            let index = u64::try_from(index).map_err(|_| anyhow!("too many reward grants"))?;
            let id = run_base
                .checked_add(60_000)
                .and_then(|value| value.checked_add(stage_ordinal * 100))
                .and_then(|value| value.checked_add(index + 1))
                .ok_or_else(|| anyhow!("reward identity overflow"))?;
            let item_id = ItemInstanceId::new(id)?;
            let stack = if grant.item_id == "consumable.red_tonic" {
                InventoryStack::red_tonic(
                    item_id,
                    u8::try_from(grant.quantity)
                        .map_err(|_| anyhow!("reward Tonic quantity exceeds u8"))?,
                )?
            } else {
                let slot = catalog
                    .equipment_slot(&grant.item_id)
                    .ok_or_else(|| anyhow!("unknown prototype reward item {}", grant.item_id))?;
                InventoryStack::Equipment(EquipmentItem::new(
                    item_id,
                    ItemContentId::new(&grant.item_id)?,
                    slot,
                ))
            };
            Ok(FieldPickup::new(
                FieldPickupId::new(id)?,
                stack,
                position,
                now,
            )?)
        })
        .collect()
}

fn compile_normal_spawn(
    spawn: EncounterSpawnSpec,
    arena: &sim_core::ArenaGeometry,
) -> Result<NormalWaveSpawn> {
    let kind = match spawn.content_id {
        DROWNED_PILGRIM_ID => NormalWaveEnemyKind::DrownedPilgrim,
        BELL_REED_ID => NormalWaveEnemyKind::BellReed,
        CHAIN_SENTRY_ID => NormalWaveEnemyKind::ChainSentry,
        other => return Err(anyhow!("{other} is not a normal-wave enemy")),
    };
    let position_milli_tiles = match spawn.location {
        SpawnLocation::PointMilliTiles { x, y } => (x, y),
        SpawnLocation::Anchor(id) => {
            let point = arena
                .anchors
                .iter()
                .find(|anchor| anchor.id == id)
                .ok_or_else(|| anyhow!("missing authored arena anchor {id}"))?
                .point;
            (point.x_milli_tiles, point.y_milli_tiles)
        }
    };
    Ok(NormalWaveSpawn {
        instance_id: spawn.instance_id,
        kind,
        position_milli_tiles,
    })
}

fn spawn_telegraphs(
    commands: &mut Commands,
    arena: &sim_core::ArenaGeometry,
    spawns: &[EncounterSpawnSpec],
) {
    for spawn in spawns {
        let position_milli_tiles = match spawn.location {
            SpawnLocation::PointMilliTiles { x, y } => (x, y),
            SpawnLocation::Anchor(id) => {
                let point = arena
                    .anchors
                    .iter()
                    .find(|anchor| anchor.id == id)
                    .expect("validated authored arena anchor")
                    .point;
                (point.x_milli_tiles, point.y_milli_tiles)
            }
        };
        let position = simulation_point_to_render(milli_position(position_milli_tiles), arena);
        commands
            .spawn((
                Name::new(format!("Spawn telegraph {:?}", spawn.instance_id)),
                SpawnTelegraphPresentation(spawn.instance_id),
                Sprite::from_color(Color::srgba_u8(229, 96, 76, 105), Vec2::splat(1.1)),
                Transform::from_xyz(position.x, position.y, TELEGRAPH_Z)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            ))
            .with_child((
                Sprite::from_color(Color::srgba_u8(8, 12, 16, 215), Vec2::splat(0.72)),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ));
    }
}

fn spawn_bell_proctor(
    commands: &mut Commands,
    arena: &sim_core::ArenaGeometry,
    instance_id: SpawnInstanceId,
) {
    let position = simulation_point_to_render(
        milli_position((
            arena.boss_spawn.x_milli_tiles,
            arena.boss_spawn.y_milli_tiles,
        )),
        arena,
    );
    commands
        .spawn((
            Name::new("Bell Proctor"),
            BellProctorPresentation { instance_id },
            Sprite::from_color(Color::srgb_u8(188, 73, 63), Vec2::splat(1.3)),
            Transform::from_xyz(position.x, position.y, ENEMY_Z),
        ))
        .with_children(|children| {
            children.spawn((
                Sprite::from_color(Color::srgb_u8(240, 211, 128), Vec2::new(0.22, 1.6)),
                Transform::from_xyz(0.0, 0.0, 0.1)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            ));
            children.spawn((
                Sprite::from_color(Color::srgb_u8(240, 211, 128), Vec2::new(0.22, 1.6)),
                Transform::from_xyz(0.0, 0.0, 0.1)
                    .with_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_4)),
            ));
        });
}

fn spawn_normal_enemies(
    commands: &mut Commands,
    arena: &sim_core::ArenaGeometry,
    runtime: &EnemyLabRuntime,
    visible: bool,
) {
    for snapshot in runtime
        .normal_snapshots()
        .into_iter()
        .filter(|snapshot| snapshot.health.alive)
    {
        let (label, color, size, rotation) = enemy_visual(snapshot.kind);
        let position =
            simulation_point_to_render(milli_position(snapshot.position_milli_tiles), arena);
        commands
            .spawn((
                Name::new(format!("{label} {:?}", snapshot.instance_id)),
                NormalEnemyPresentation {
                    instance_id: snapshot.instance_id,
                    entity_id: snapshot.entity_id,
                },
                if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                Sprite::from_color(color, size),
                Transform::from_xyz(position.x, position.y, ENEMY_Z)
                    .with_rotation(Quat::from_rotation_z(rotation)),
            ))
            .with_child((
                Sprite::from_color(Color::srgb_u8(229, 224, 197), Vec2::splat(0.24)),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ));
    }
}

#[allow(clippy::needless_pass_by_value)]
fn sync_normal_enemy_presentation(
    runtime: Res<EnemyLabRuntime>,
    arena: Res<LoadedArena>,
    mut visuals: Query<(&NormalEnemyPresentation, &mut Transform, &mut Visibility)>,
) {
    if !runtime.normal_mode() {
        return;
    }
    let snapshots = runtime
        .normal_snapshots()
        .into_iter()
        .map(|snapshot| (snapshot.entity_id, snapshot))
        .collect::<BTreeMap<_, _>>();
    for (visual, mut transform, mut visibility) in &mut visuals {
        let Some(snapshot) = snapshots.get(&visual.entity_id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let position =
            simulation_point_to_render(milli_position(snapshot.position_milli_tiles), &arena.0);
        transform.translation.x = position.x;
        transform.translation.y = position.y;
        *visibility = if snapshot.health.alive
            && matches!(runtime.normal_phase(), Some(NormalWavePhase::Active))
        {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn spawn_encounter_status(mut commands: Commands) {
    commands.spawn((
        Name::new("Encounter status"),
        EncounterStatus,
        Text::new("BELL LABORATORY | MOVE OR FIRE TO BEGIN"),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(236, 225, 197)),
        Node {
            position_type: PositionType::Absolute,
            top: px(14),
            right: px(14),
            width: percent(24),
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            padding: UiRect::axes(px(10), px(7)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 224)),
        BorderColor::all(Color::srgba_u8(178, 142, 75, 205)),
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn update_encounter_status(
    runtime: Res<EnemyLabRuntime>,
    death: Res<LocalDeathRuntime>,
    state: Res<EncounterClientState>,
    mut status: Single<(&mut Text, &mut Visibility), With<EncounterStatus>>,
) {
    if !runtime.normal_mode() {
        *status.1 = Visibility::Hidden;
        return;
    }
    *status.1 = Visibility::Inherited;
    let encounter = death.encounter();
    let summary = match encounter.state() {
        EncounterState::AwaitingFirstActivity => "MOVE OR FIRE TO BEGIN".to_owned(),
        EncounterState::FirstWaveDelay { starts_at } => format!("WAVE 1 BEGINS {}T", starts_at.0),
        EncounterState::SpawnTelegraph {
            stage,
            activates_at,
        } => {
            format!("{} SPAWNING | {}T", stage_label(stage), activates_at.0)
        }
        EncounterState::Active {
            stage,
            remaining_hostiles,
        } => {
            format!("{} | {remaining_hostiles} HOSTILES", stage_label(stage))
        }
        EncounterState::RewardDelay {
            completed_stage,
            opens_at,
        } => {
            format!(
                "{} CLEAR | REWARD {}T",
                stage_label(completed_stage),
                opens_at.0
            )
        }
        EncounterState::RewardOpen { completed_stage } => {
            format!("{} REWARD OPEN", stage_label(completed_stage))
        }
        EncounterState::BossIntroduction { activates_at } => {
            format!("BOSS INTRO | {activates_at}T")
        }
        EncounterState::DeathFrozen => "RUN ENDED".to_owned(),
        EncounterState::CompletionSummary => "LAB CLEARED".to_owned(),
        EncounterState::ClearedArena => "CLEARED ARENA".to_owned(),
    };
    status.0.0 = if state.latest_message.is_empty() {
        format!("BELL LABORATORY | {summary}")
    } else {
        format!("BELL LABORATORY | {summary}\n{}", state.latest_message)
    };
}

fn enemy_visual(kind: NormalWaveEnemyKind) -> (&'static str, Color, Vec2, f32) {
    match kind {
        NormalWaveEnemyKind::DrownedPilgrim => (
            "Drowned Pilgrim",
            Color::srgb_u8(102, 143, 157),
            Vec2::new(0.58, 0.78),
            std::f32::consts::FRAC_PI_4,
        ),
        NormalWaveEnemyKind::BellReed => (
            "Bell Reed",
            Color::srgb_u8(157, 111, 179),
            Vec2::splat(0.78),
            std::f32::consts::FRAC_PI_4,
        ),
        NormalWaveEnemyKind::ChainSentry => (
            "Chain Sentry",
            Color::srgb_u8(183, 134, 65),
            Vec2::splat(1.0),
            0.0,
        ),
    }
}

#[allow(clippy::cast_precision_loss)]
fn milli_position((x, y): (i32, i32)) -> SimulationVector {
    SimulationVector::new(x as f32 / 1_000.0, y as f32 / 1_000.0)
}

const fn stage_label(stage: EncounterStage) -> &'static str {
    match stage {
        EncounterStage::Wave1 => "WAVE 1",
        EncounterStage::Wave2 => "WAVE 2",
        EncounterStage::Wave3 => "WAVE 3",
        EncounterStage::Boss => "BELL PROCTOR",
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use sim_core::{ArenaAnchor, ArenaGeometry, TilePoint};

    #[test]
    fn authored_anchor_and_point_spawns_compile_without_client_identity() {
        let arena = ArenaGeometry {
            id: "arena.test".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars: Vec::new(),
            anchors: vec![ArenaAnchor {
                id: "N1".to_owned(),
                point: TilePoint::new(8_000, 3_000),
            }],
        };
        let anchored = compile_normal_spawn(
            EncounterSpawnSpec {
                instance_id: SpawnInstanceId {
                    run_ordinal: 1,
                    spawn_ordinal: 1,
                },
                content_id: DROWNED_PILGRIM_ID,
                location: SpawnLocation::Anchor("N1"),
                budget_cost: 1,
            },
            &arena,
        )
        .expect("anchor");
        assert_eq!(anchored.position_milli_tiles, (8_000, 3_000));
        assert_eq!(anchored.kind, NormalWaveEnemyKind::DrownedPilgrim);
    }

    #[test]
    fn wave_reward_offer_is_resolved_once_with_stable_owned_identities() {
        let content_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("content");
        let (package, report) =
            sim_content::load_and_validate(&content_root).expect("strict First Playable package");
        let catalog = ItemShowcaseCatalog::new(
            sim_content::first_playable_equipment_catalog(&package).expect("equipment"),
            sim_content::first_playable_reward_catalog(&package).expect("rewards"),
            report.content_version,
        )
        .expect("catalog");
        let resolve = || {
            compile_reward_offers(
                &catalog,
                "reward.prototype.wave_1",
                0xB311_A501,
                1,
                EncounterStage::Wave1,
                SimulationVector::new(4.0, 12.0),
                sim_core::Tick(100),
            )
            .expect("offers")
        };
        let first = resolve();
        let second = resolve();
        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
        assert_ne!(first[0].pickup_id(), first[1].pickup_id());
        assert!(
            first
                .iter()
                .any(|offer| matches!(offer.stack(), InventoryStack::Equipment(_)))
        );
        assert!(
            first
                .iter()
                .any(|offer| matches!(offer.stack(), InventoryStack::RedTonic { quantity: 1, .. }))
        );
    }

    #[test]
    fn resolving_boss_reward_does_not_emit_normal_reward_close_action() {
        assert_eq!(
            modal_encounter_action(EncounterState::CompletionSummary, true, false, true),
            EncounterAction::None
        );
        assert_eq!(
            modal_encounter_action(EncounterState::CompletionSummary, true, true, false),
            EncounterAction::CloseCompletionSummary
        );
        assert_eq!(
            modal_encounter_action(
                EncounterState::RewardOpen {
                    completed_stage: EncounterStage::Wave3,
                },
                true,
                false,
                true,
            ),
            EncounterAction::CloseRewardPanel
        );
    }
}
