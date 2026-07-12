use std::collections::VecDeque;

use bevy::prelude::*;
use sim_core::{EncounterStage, EncounterState, LocalDebugStateInput, LocalDebugStateSnapshot};

use crate::{
    FrameSet, LoadedArena, combat::EvidenceScenario, death::LocalDeathRuntime,
    enemies::EnemyLabRuntime, player::PlayerSimulation,
};

const DEBUG_TOGGLE: KeyCode = KeyCode::F3;
const FRAME_SAMPLE_CAPACITY: usize = 600;

#[derive(Debug, Resource)]
pub(crate) struct DebugOverlayState {
    visible: bool,
    frame_ms: VecDeque<f32>,
    snapshot: Option<LocalDebugStateSnapshot>,
    entity_count: u32,
}

impl Default for DebugOverlayState {
    fn default() -> Self {
        Self {
            visible: false,
            frame_ms: VecDeque::with_capacity(FRAME_SAMPLE_CAPACITY),
            snapshot: None,
            entity_count: 0,
        }
    }
}

impl DebugOverlayState {
    pub(crate) const fn visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn evidence_ready(&self) -> bool {
        self.visible && self.frame_ms.len() >= 60 && self.snapshot.is_some()
    }
}

#[derive(Debug, Component)]
struct DebugOverlay;

pub(crate) fn configure(app: &mut App) {
    app.init_resource::<DebugOverlayState>()
        .add_systems(Startup, spawn_overlay)
        .add_systems(
            Update,
            (sample_debug_state, update_overlay)
                .chain()
                .in_set(FrameSet::Presentation),
        );
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn sample_debug_state(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    player: Res<PlayerSimulation>,
    runtime: Res<EnemyLabRuntime>,
    death: Res<LocalDeathRuntime>,
    entities: Query<Entity>,
    mut debug: ResMut<DebugOverlayState>,
) {
    if keyboard.just_pressed(DEBUG_TOGGLE) {
        debug.visible = !debug.visible;
    }
    let milliseconds = time.delta_secs() * 1_000.0;
    if milliseconds.is_finite() && milliseconds > 0.0 {
        if debug.frame_ms.len() == FRAME_SAMPLE_CAPACITY {
            debug.frame_ms.pop_front();
        }
        debug.frame_ms.push_back(milliseconds);
    }
    debug.entity_count = u32::try_from(entities.iter().count()).unwrap_or(u32::MAX);
    let encounter = death.encounter();
    let position = player.state().position();
    let vitals = runtime.consumables().vitals();
    let enemies = runtime.debug_enemy_states();
    let boss = runtime
        .boss_snapshot()
        .map(|boss| sim_core::DebugBossState {
            entity_id: boss.entity_id,
            local_tick: boss.local_tick,
            state_code: boss_state_code(boss.state),
            current_health: boss.current_health,
            maximum_health: boss.maximum_health,
            active_projectiles: u32::try_from(boss.active_projectiles).unwrap_or(u32::MAX),
            active_lanes: u32::try_from(boss.active_lanes).unwrap_or(u32::MAX),
        });
    debug.snapshot = Some(
        LocalDebugStateSnapshot::compile(&LocalDebugStateInput {
            run_ordinal: encounter.run_ordinal(),
            seed: encounter.seed(),
            encounter_tick: encounter.tick(),
            encounter_state_code: encounter_state_code(encounter.state()),
            combat_tick: runtime.combat().tick(),
            player_x_bits: position.x.to_bits(),
            player_y_bits: position.y.to_bits(),
            health: vitals.current_health(),
            maximum_health: vitals.maximum_health(),
            enemies,
            boss,
            friendly_projectiles: u32::try_from(runtime.combat().projectiles().len())
                .unwrap_or(u32::MAX),
            hostile_projectiles: u32::try_from(runtime.hostile_projectile_count())
                .unwrap_or(u32::MAX),
            hostile_hazards: u32::try_from(runtime.hostile_hazard_count()).unwrap_or(u32::MAX),
        })
        .expect("validated LocalLab state must compile a debug snapshot"),
    );
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_overlay(
    mut commands: Commands,
    scenario: Res<EvidenceScenario>,
    mut debug: ResMut<DebugOverlayState>,
) {
    debug.visible = matches!(
        *scenario,
        EvidenceScenario::DebugOverlayShowcase
            | EvidenceScenario::BossShowcase
            | EvidenceScenario::BossCompletionShowcase
    );
    commands.spawn((
        Name::new("Authoritative debug state overlay"),
        DebugOverlay,
        Text::new("DEBUG STATE"),
        TextFont::from_font_size(12.0),
        TextColor(Color::srgb_u8(207, 239, 222)),
        Node {
            position_type: PositionType::Absolute,
            right: px(14),
            bottom: px(14),
            width: px(500),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(7, 14, 14, 238)),
        BorderColor::all(Color::srgba_u8(72, 191, 166, 225)),
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn update_overlay(
    debug: Res<DebugOverlayState>,
    scenario: Res<EvidenceScenario>,
    arena: Res<LoadedArena>,
    runtime: Res<EnemyLabRuntime>,
    death: Res<LocalDeathRuntime>,
    mut overlay: Single<(&mut Text, &mut Visibility), With<DebugOverlay>>,
) {
    *overlay.1 = if debug.visible {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    let Some(snapshot) = &debug.snapshot else {
        return;
    };
    let (p95, p99) = frame_percentiles(&debug.frame_ms);
    let fps = debug
        .frame_ms
        .back()
        .filter(|milliseconds| **milliseconds > 0.0)
        .map_or(0.0, |milliseconds| 1_000.0 / milliseconds);
    let encounter = death.encounter();
    let boss_snapshot = runtime.boss_snapshot();
    let threat = if boss_snapshot.is_some() { 41 } else { 33 };
    let boss_line = boss_snapshot.map_or_else(
        || "BOSS --".to_owned(),
        |boss| {
            format!(
                "BOSS HP {}/{} | {} | LOCAL {}T | HAZARDS P{}/L{}",
                boss.current_health,
                boss.maximum_health,
                boss_state_label(boss.state),
                boss.local_tick.0,
                boss.active_projectiles,
                boss.active_lanes,
            )
        },
    );
    overlay.0.0 = format!(
        "DEBUG ONLY [F3] | {} | SEED {:08X} | HASH {}\nSCRIPT {} @{}T | LAB ACTIVE {} | COMBAT {}T | 30 HZ FIXED\n{}\nANCHORS {} | HITBOXES P1/E{} | PATTERNS FAN/RING/LANE | THREAT {}\nENTITIES {} | ENEMIES {} | PROJECTILES {} | HOSTILE HAZARDS {}\nFPS {:>5.1} | FRAME P95 {:.2}MS P99 {:.2}MS | SAMPLES {}\nTOGGLE PRESENTATION-ONLY | GAMEPLAY HASH EXCLUDES FRAME/UI STATE",
        if matches!(
            *scenario,
            EvidenceScenario::DebugOverlayShowcase
                | EvidenceScenario::BossShowcase
                | EvidenceScenario::BossCompletionShowcase
        ) {
            "EVIDENCE"
        } else {
            "LOCAL LAB"
        },
        encounter.seed(),
        &snapshot.state_hash_blake3[..12],
        encounter_state_label(encounter.state()),
        encounter.tick().0,
        runtime.is_active(),
        runtime.combat().tick().0,
        boss_line,
        arena.0.anchors.len(),
        snapshot.enemy_count,
        threat,
        debug.entity_count,
        snapshot.enemy_count,
        snapshot.projectile_count,
        runtime.hostile_hazard_count(),
        fps,
        p95,
        p99,
        debug.frame_ms.len(),
    );
}

const fn boss_state_label(state: sim_core::BellProctorStateKind) -> &'static str {
    match state {
        sim_core::BellProctorStateKind::Active(sim_core::BellProctorPhase::Phase1) => "PHASE 1",
        sim_core::BellProctorStateKind::Active(sim_core::BellProctorPhase::Phase2) => "PHASE 2",
        sim_core::BellProctorStateKind::Active(sim_core::BellProctorPhase::Phase3) => "PHASE 3",
        sim_core::BellProctorStateKind::Break { .. } => "PHASE BREAK +20%",
        sim_core::BellProctorStateKind::Defeated => "DEFEATED",
    }
}

const fn boss_state_code(state: sim_core::BellProctorStateKind) -> u8 {
    match state {
        sim_core::BellProctorStateKind::Active(sim_core::BellProctorPhase::Phase1) => 1,
        sim_core::BellProctorStateKind::Break {
            entering: sim_core::BellProctorPhase::Phase2,
        } => 2,
        sim_core::BellProctorStateKind::Active(sim_core::BellProctorPhase::Phase2) => 3,
        sim_core::BellProctorStateKind::Break {
            entering: sim_core::BellProctorPhase::Phase3,
        } => 4,
        sim_core::BellProctorStateKind::Active(sim_core::BellProctorPhase::Phase3) => 5,
        sim_core::BellProctorStateKind::Defeated => 6,
        sim_core::BellProctorStateKind::Break {
            entering: sim_core::BellProctorPhase::Phase1,
        } => 7,
    }
}

fn frame_percentiles(samples: &VecDeque<f32>) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let mut sorted = samples.iter().copied().collect::<Vec<_>>();
    sorted.sort_by(f32::total_cmp);
    let percentile = |numerator: usize| {
        let index = (sorted.len() * numerator).div_ceil(100).saturating_sub(1);
        sorted[index.min(sorted.len() - 1)]
    };
    (percentile(95), percentile(99))
}

const fn encounter_state_code(state: EncounterState) -> u8 {
    match state {
        EncounterState::AwaitingFirstActivity => 0,
        EncounterState::FirstWaveDelay { .. } => 1,
        EncounterState::SpawnTelegraph { .. } => 2,
        EncounterState::Active { .. } => 3,
        EncounterState::RewardDelay { .. } => 4,
        EncounterState::RewardOpen { .. } => 5,
        EncounterState::BossIntroduction { .. } => 6,
        EncounterState::DeathFrozen => 7,
        EncounterState::CompletionSummary => 8,
        EncounterState::ClearedArena => 9,
    }
}

fn encounter_state_label(state: EncounterState) -> String {
    match state {
        EncounterState::AwaitingFirstActivity => "AWAITING_FIRST_ACTIVITY".to_owned(),
        EncounterState::FirstWaveDelay { .. } => "FIRST_WAVE_DELAY".to_owned(),
        EncounterState::SpawnTelegraph { stage, .. } => format!("{}_TELEGRAPH", stage_label(stage)),
        EncounterState::Active { stage, .. } => format!("{}_ACTIVE", stage_label(stage)),
        EncounterState::RewardDelay {
            completed_stage, ..
        } => {
            format!("{}_REWARD_DELAY", stage_label(completed_stage))
        }
        EncounterState::RewardOpen { completed_stage } => {
            format!("{}_REWARD_OPEN", stage_label(completed_stage))
        }
        EncounterState::BossIntroduction { .. } => "BOSS_INTRODUCTION".to_owned(),
        EncounterState::DeathFrozen => "DEATH_FROZEN".to_owned(),
        EncounterState::CompletionSummary => "COMPLETION_SUMMARY".to_owned(),
        EncounterState::ClearedArena => "CLEARED_ARENA".to_owned(),
    }
}

const fn stage_label(stage: EncounterStage) -> &'static str {
    match stage {
        EncounterStage::Wave1 => "WAVE1",
        EncounterStage::Wave2 => "WAVE2",
        EncounterStage::Wave3 => "WAVE3",
        EncounterStage::Boss => "BOSS",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_changes_only_presentation_state() {
        let snapshot = LocalDebugStateSnapshot {
            state_hash_blake3: "a".repeat(64),
            enemy_count: 3,
            projectile_count: 4,
        };
        let mut state = DebugOverlayState {
            snapshot: Some(snapshot.clone()),
            ..DebugOverlayState::default()
        };
        state.visible = !state.visible;
        assert_eq!(state.snapshot, Some(snapshot));
    }

    #[test]
    fn percentile_selection_is_stable_and_uses_nearest_rank() {
        let samples = (1_u16..=100).map(f32::from).collect::<VecDeque<_>>();
        assert_eq!(frame_percentiles(&samples), (95.0, 99.0));
        assert_eq!(frame_percentiles(&VecDeque::new()), (0.0, 0.0));
    }
}
