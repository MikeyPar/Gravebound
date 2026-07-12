use std::{env, time::Duration};

use bevy::prelude::*;

use crate::{FixedSimulationSet, FrameSet, combat::EvidenceScenario, enemies::EnemyLabRuntime};

const DEVELOPER_TOOLS_ENV: &str = "GRAVEBOUND_DEVELOPER_TOOLS";
const TIME_SCALE_KEY: KeyCode = KeyCode::F4;
const INVULNERABILITY_KEY: KeyCode = KeyCode::F5;
const SINGLE_STEP_KEY: KeyCode = KeyCode::F6;
const NORMAL_SPEED_KEY: KeyCode = KeyCode::F8;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DebugTimeScale {
    Paused,
    Quarter,
    Half,
    #[default]
    One,
    Double,
}

impl DebugTimeScale {
    const fn next(self) -> Self {
        match self {
            Self::One => Self::Half,
            Self::Half => Self::Quarter,
            Self::Quarter => Self::Paused,
            Self::Paused => Self::Double,
            Self::Double => Self::One,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Paused => "PAUSED",
            Self::Quarter => "0.25x",
            Self::Half => "0.5x",
            Self::One => "1x",
            Self::Double => "2x",
        }
    }

    const fn relative_speed(self) -> f32 {
        match self {
            Self::Paused => 0.0,
            Self::Quarter => 0.25,
            Self::Half => 0.5,
            Self::One => 1.0,
            Self::Double => 2.0,
        }
    }
}

#[derive(Debug, Resource)]
pub(crate) struct DeveloperToolsState {
    enabled: bool,
    time_scale: DebugTimeScale,
    invulnerable: bool,
    single_steps: u64,
    evidence_frames: u8,
}

impl DeveloperToolsState {
    fn new(scenario: EvidenceScenario) -> Self {
        let environment_enabled = matches!(
            env::var(DEVELOPER_TOOLS_ENV).as_deref(),
            Ok("1" | "true" | "TRUE")
        );
        let evidence = scenario == EvidenceScenario::DebugToolsShowcase;
        Self {
            enabled: environment_enabled || evidence,
            time_scale: if evidence {
                DebugTimeScale::Half
            } else {
                DebugTimeScale::One
            },
            invulnerable: evidence,
            single_steps: 0,
            evidence_frames: 0,
        }
    }

    pub(crate) const fn gate_metrics_eligible(&self) -> bool {
        !self.enabled
            || (matches!(self.time_scale, DebugTimeScale::One)
                && !self.invulnerable
                && self.single_steps == 0)
    }

    pub(crate) const fn evidence_ready(&self) -> bool {
        self.enabled
            && matches!(self.time_scale, DebugTimeScale::Half)
            && self.invulnerable
            && self.evidence_frames >= 12
    }

    fn request_single_step(&mut self) -> bool {
        if !self.enabled || self.time_scale != DebugTimeScale::Paused {
            return false;
        }
        self.single_steps = self.single_steps.saturating_add(1);
        true
    }
}

#[derive(Debug, Component)]
struct DeveloperToolsOverlay;

pub(crate) fn configure(app: &mut App, scenario: EvidenceScenario) {
    app.insert_resource(DeveloperToolsState::new(scenario))
        .add_systems(Startup, spawn_overlay)
        .add_systems(
            FixedUpdate,
            apply_debug_damage_policy.in_set(FixedSimulationSet::Developer),
        )
        .add_systems(
            Update,
            (sample_controls, update_overlay)
                .chain()
                .in_set(FrameSet::Presentation),
        );
}

#[allow(clippy::needless_pass_by_value)]
fn sample_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    fixed_time: Res<Time<Fixed>>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut tools: ResMut<DeveloperToolsState>,
) {
    if !tools.enabled {
        virtual_time.set_relative_speed(1.0);
        return;
    }
    if keyboard.just_pressed(TIME_SCALE_KEY) {
        tools.time_scale = tools.time_scale.next();
    }
    if keyboard.just_pressed(NORMAL_SPEED_KEY) {
        tools.time_scale = DebugTimeScale::One;
    }
    if keyboard.just_pressed(INVULNERABILITY_KEY) {
        tools.invulnerable = !tools.invulnerable;
    }
    virtual_time.set_relative_speed(tools.time_scale.relative_speed());
    if keyboard.just_pressed(SINGLE_STEP_KEY) && tools.request_single_step() {
        let one_tick = fixed_time.timestep();
        debug_assert!(one_tick > Duration::ZERO);
        virtual_time.advance_by(one_tick);
    }
    tools.evidence_frames = tools.evidence_frames.saturating_add(1);
}

#[allow(clippy::needless_pass_by_value)]
fn apply_debug_damage_policy(
    tools: Res<DeveloperToolsState>,
    mut runtime: ResMut<EnemyLabRuntime>,
) {
    runtime.set_debug_invulnerable(tools.enabled && tools.invulnerable);
}

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Name::new("Developer time and invulnerability tools"),
        DeveloperToolsOverlay,
        Text::new("DEVELOPER TOOLS DISABLED"),
        TextFont::from_font_size(12.0),
        TextColor(Color::srgb_u8(246, 223, 177)),
        Node {
            position_type: PositionType::Absolute,
            left: px(14),
            bottom: px(176),
            width: px(430),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(18, 12, 8, 236)),
        BorderColor::all(Color::srgba_u8(221, 162, 71, 220)),
        Visibility::Hidden,
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn update_overlay(
    tools: Res<DeveloperToolsState>,
    mut overlay: Single<(&mut Text, &mut Visibility), With<DeveloperToolsOverlay>>,
) {
    *overlay.1 = if tools.enabled {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    overlay.0.0 = format!(
        "DEBUG ONLY | DEVELOPER TOOLS | GATE METRICS {}\n[F4] SCALE {}  [F6] STEP  [F8] 1x  |  [F5] INVULN {}\nWHOLE FIXED TICKS ONLY | STEPS {} | COLLISION/DAMAGE EVENTS PRESERVED",
        if tools.gate_metrics_eligible() {
            "ELIGIBLE"
        } else {
            "EXCLUDED"
        },
        tools.time_scale.label(),
        if tools.invulnerable { "ON" } else { "OFF" },
        tools.single_steps,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_cycle_is_exact_and_default_is_release_safe() {
        assert_eq!(DebugTimeScale::default(), DebugTimeScale::One);
        let mut scale = DebugTimeScale::One;
        let mut observed = Vec::new();
        for _ in 0..5 {
            scale = scale.next();
            observed.push(scale);
        }
        assert_eq!(
            observed,
            vec![
                DebugTimeScale::Half,
                DebugTimeScale::Quarter,
                DebugTimeScale::Paused,
                DebugTimeScale::Double,
                DebugTimeScale::One,
            ]
        );
        assert_eq!(
            DebugTimeScale::Paused.relative_speed().to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            DebugTimeScale::Quarter.relative_speed().to_bits(),
            0.25_f32.to_bits()
        );
        assert_eq!(
            DebugTimeScale::Half.relative_speed().to_bits(),
            0.5_f32.to_bits()
        );
        assert_eq!(
            DebugTimeScale::One.relative_speed().to_bits(),
            1.0_f32.to_bits()
        );
        assert_eq!(
            DebugTimeScale::Double.relative_speed().to_bits(),
            2.0_f32.to_bits()
        );
    }

    #[test]
    fn single_step_and_gate_exclusion_are_fail_closed() {
        let mut disabled = DeveloperToolsState {
            enabled: false,
            time_scale: DebugTimeScale::Paused,
            invulnerable: false,
            single_steps: 0,
            evidence_frames: 0,
        };
        assert!(!disabled.request_single_step());
        assert!(disabled.gate_metrics_eligible());

        disabled.enabled = true;
        assert!(disabled.request_single_step());
        assert_eq!(disabled.single_steps, 1);
        assert!(!disabled.gate_metrics_eligible());
        disabled.time_scale = DebugTimeScale::One;
        disabled.single_steps = 0;
        assert!(disabled.gate_metrics_eligible());
        disabled.invulnerable = true;
        assert!(!disabled.gate_metrics_eligible());
    }
}
