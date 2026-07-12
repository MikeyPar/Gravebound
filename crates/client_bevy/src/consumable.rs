use std::{
    io::Cursor,
    sync::{Arc, mpsc},
    thread,
};

use bevy::{
    log::{info, warn},
    prelude::*,
};
use rodio::{Decoder, OutputStream, Sink};
use sim_core::{BeltSlot, ConsumableAction, ConsumableEvent};
use thiserror::Error;

use crate::{FixedSimulationSet, FrameSet, combat::CombatInputGate, enemies::EnemyLabRuntime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub struct ConsumableBindings {
    pub keyboard: KeyCode,
    pub gamepad: GamepadButton,
}

impl Default for ConsumableBindings {
    fn default() -> Self {
        Self {
            keyboard: KeyCode::KeyQ,
            gamepad: GamepadButton::West,
        }
    }
}

#[derive(Debug, Default, Resource)]
pub(crate) struct ConsumableInputSampler {
    latest: ConsumableAction,
    q: SequencedButtonState,
}

#[derive(Debug, Default)]
struct SequencedButtonState {
    was_pressed: bool,
    suppressed_until_release: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
enum ConsumableSequenceError {
    #[error("consumable press sequence exhausted u32")]
    Exhausted,
}

#[derive(Debug, Component)]
struct ConsumableHud;

#[derive(Debug, Resource)]
struct TonicAudioCue(mpsc::Sender<()>);

impl TonicAudioCue {
    fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        let wav = build_tonic_confirmation_wav();
        if thread::Builder::new()
            .name("gravebound-tonic-audio".to_owned())
            .spawn(move || tonic_audio_worker(receiver, wav))
            .is_err()
        {
            warn!(
                feature_id = "GB-M01-11",
                "Red Tonic audio worker could not start"
            );
        }
        Self(sender)
    }

    fn play(&self) -> bool {
        self.0.send(()).is_ok()
    }
}

#[derive(Debug, Resource)]
pub(crate) struct ConsumableDiagnostics {
    accepted_uses: u64,
    healing_ticks: u64,
    last_applied_healing: u32,
    feedback: &'static str,
    confirmation_cues: u64,
}

impl Default for ConsumableDiagnostics {
    fn default() -> Self {
        Self {
            accepted_uses: 0,
            healing_ticks: 0,
            last_applied_healing: 0,
            feedback: "READY",
            confirmation_cues: 0,
        }
    }
}

impl ConsumableDiagnostics {
    pub(crate) const fn showcase_ready(&self) -> bool {
        self.accepted_uses > 0 && self.healing_ticks >= 4
    }
}

pub(crate) fn configure(app: &mut App) {
    app.insert_resource(ConsumableBindings::default())
        .insert_resource(ConsumableInputSampler::default())
        .insert_resource(ConsumableDiagnostics::default())
        .add_systems(Startup, spawn_consumable_hud)
        .add_systems(
            FixedUpdate,
            simulate_consumable.in_set(FixedSimulationSet::Consumable),
        )
        .add_systems(
            Update,
            (
                sample_consumable_input.in_set(FrameSet::InputSample),
                update_consumable_hud.in_set(FrameSet::Presentation),
            ),
        );
}

fn spawn_consumable_hud(mut commands: Commands) {
    commands.insert_resource(TonicAudioCue::start());
    commands.spawn((
        Name::new("Red Tonic HUD"),
        ConsumableHud,
        Text::new("Q  RED TONIC"),
        TextFont::from_font_size(15.0),
        TextColor(Color::srgb_u8(244, 226, 192)),
        Node {
            position_type: PositionType::Absolute,
            left: px(14),
            bottom: px(54),
            min_width: px(238),
            border: UiRect::all(px(1)),
            padding: UiRect::axes(px(10), px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 232)),
        BorderColor::all(Color::srgba_u8(156, 48, 55, 220)),
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn sample_consumable_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    bindings: Res<ConsumableBindings>,
    gate: Res<CombatInputGate>,
    scenario: Res<crate::combat::EvidenceScenario>,
    mut sampler: ResMut<ConsumableInputSampler>,
) {
    if matches!(
        *scenario,
        crate::combat::EvidenceScenario::RedTonicShowcase
            | crate::combat::EvidenceScenario::ItemCatalogShowcase
    ) {
        if sampler.latest.use_q_press_sequence == 0 {
            sampler.latest.use_q_press_sequence = 1;
        }
        return;
    }
    let gamepad_pressed = gamepads
        .iter()
        .any(|gamepad| gamepad.pressed(bindings.gamepad));
    sample_q_button(
        &mut sampler,
        keyboard.pressed(bindings.keyboard) || gamepad_pressed,
        gate.blocked,
    )
    .expect("consumable sequence space must not exhaust during LocalLab");
}

fn sample_q_button(
    sampler: &mut ConsumableInputSampler,
    physically_pressed: bool,
    blocked: bool,
) -> Result<(), ConsumableSequenceError> {
    if blocked {
        sampler.q.suppressed_until_release |= physically_pressed;
        sampler.q.was_pressed = physically_pressed;
        return Ok(());
    }
    if sampler.q.suppressed_until_release {
        if !physically_pressed {
            sampler.q.suppressed_until_release = false;
        }
        sampler.q.was_pressed = physically_pressed;
        return Ok(());
    }
    if physically_pressed && !sampler.q.was_pressed {
        sampler.latest.use_q_press_sequence = sampler
            .latest
            .use_q_press_sequence
            .checked_add(1)
            .ok_or(ConsumableSequenceError::Exhausted)?;
    }
    sampler.q.was_pressed = physically_pressed;
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn simulate_consumable(
    input: Res<ConsumableInputSampler>,
    audio_cue: Res<TonicAudioCue>,
    mut runtime: ResMut<EnemyLabRuntime>,
    mut diagnostics: ResMut<ConsumableDiagnostics>,
) {
    if !runtime.player_can_act() {
        return;
    }
    let step = runtime
        .consumables_mut()
        .step(input.latest)
        .expect("validated LocalLab consumable input must remain legal");
    for event in step.events {
        match event {
            ConsumableEvent::UseAccepted {
                scheduled_healing,
                slot_remaining,
                ..
            } => {
                diagnostics.accepted_uses += 1;
                diagnostics.feedback = "DRINK CONFIRMED";
                diagnostics.confirmation_cues += 1;
                if !audio_cue.play() {
                    warn!(
                        feature_id = "GB-M01-11",
                        "Red Tonic audio cue was unavailable"
                    );
                }
                info!(
                    feature_id = "GB-M01-11",
                    scheduled_healing, slot_remaining, "Red Tonic use accepted"
                );
            }
            ConsumableEvent::HealingTick { applied, .. } => {
                diagnostics.healing_ticks += 1;
                diagnostics.last_applied_healing = applied;
            }
            ConsumableEvent::UseRejected { reason, .. } => {
                diagnostics.feedback = match reason {
                    sim_core::TonicUseRejection::EmptyQSlot => "NO TONIC IN Q SLOT",
                    sim_core::TonicUseRejection::SharedCooldown { .. } => "POTION COOLING DOWN",
                    sim_core::TonicUseRejection::FullHealth => "HEALTH ALREADY FULL",
                    sim_core::TonicUseRejection::NoEffectiveHealing => "TONIC HAS NO EFFECT",
                };
                info!(feature_id = "GB-M01-11", %reason, "Red Tonic use rejected");
            }
            ConsumableEvent::RestoreCompleted { .. }
            | ConsumableEvent::SharedCooldownReady { .. } => {}
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn update_consumable_hud(
    runtime: Res<EnemyLabRuntime>,
    diagnostics: Res<ConsumableDiagnostics>,
    mut hud: Single<&mut Text, With<ConsumableHud>>,
) {
    let vitals = runtime.consumables().vitals();
    let count = runtime
        .consumables()
        .belt()
        .slot(0)
        .unwrap_or(BeltSlot::Empty)
        .tonic_count();
    let cooldown = runtime.consumables().shared_cooldown_remaining_ticks();
    let restore = runtime.consumables().active_restore_remaining_ticks();
    hud.0 = format!(
        "Q  RED TONIC  x{count}\nHEALTH  {}/{}\nRESTORE  {:02}t   COOLDOWN  {:02}t   +{}\n{}  |  CUE {}",
        vitals.current_health(),
        vitals.maximum_health(),
        restore,
        cooldown,
        diagnostics.last_applied_healing,
        diagnostics.feedback,
        diagnostics.confirmation_cues
    );
}

fn tonic_audio_worker(receiver: mpsc::Receiver<()>, wav: Vec<u8>) {
    let wav: Arc<[u8]> = wav.into();
    let Ok((_stream, stream_handle)) = OutputStream::try_default() else {
        // Keep the receiver alive for the application lifetime. Headless machines may have no
        // audio endpoint; gameplay and authoritative confirmation events must continue normally.
        for () in receiver {}
        return;
    };
    for () in receiver {
        let Ok(decoder) = Decoder::new(Cursor::new(Arc::clone(&wav))) else {
            continue;
        };
        let Ok(sink) = Sink::try_new(&stream_handle) else {
            continue;
        };
        sink.set_volume(0.42);
        sink.append(decoder);
        sink.sleep_until_end();
    }
}

/// Builds a deterministic prototype cue in memory so `LocalLab` has audible confirmation without
/// treating an unlicensed temporary binary as a production asset. Its quiet descending partials
/// stay subordinate to hostile warning audio.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // The bounded PCM synthesis range is clamped before its deliberate f32-to-i16 conversion.
fn build_tonic_confirmation_wav() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 22_050;
    const SAMPLE_COUNT: u32 = 3_969; // 180 ms.
    const BITS_PER_SAMPLE: u16 = 16;
    let data_bytes = SAMPLE_COUNT * u32::from(BITS_PER_SAMPLE / 8);
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for index in 0..SAMPLE_COUNT {
        let time = index as f32 / SAMPLE_RATE as f32;
        let progress = index as f32 / SAMPLE_COUNT as f32;
        let envelope = (1.0 - progress).powi(3);
        let carrier = (std::f32::consts::TAU * (520.0 - 90.0 * progress) * time).sin();
        let body = (std::f32::consts::TAU * 260.0 * time).sin();
        let sample = ((carrier * 0.72 + body * 0.28) * envelope * 9_000.0)
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_binding_defaults_to_keyboard_q_and_gamepad_west() {
        let binding = ConsumableBindings::default();
        assert_eq!(binding.keyboard, KeyCode::KeyQ);
        assert_eq!(binding.gamepad, GamepadButton::West);
    }

    #[test]
    fn press_is_sequenced_once_until_release() {
        let mut sampler = ConsumableInputSampler::default();
        sample_q_button(&mut sampler, true, false).expect("press");
        sample_q_button(&mut sampler, true, false).expect("held");
        assert_eq!(sampler.latest.use_q_press_sequence, 1);
        sample_q_button(&mut sampler, false, false).expect("release");
        sample_q_button(&mut sampler, true, false).expect("second press");
        assert_eq!(sampler.latest.use_q_press_sequence, 2);
    }

    #[test]
    fn blocked_press_requires_release_before_rearming() {
        let mut sampler = ConsumableInputSampler::default();
        sample_q_button(&mut sampler, true, true).expect("blocked");
        sample_q_button(&mut sampler, true, false).expect("still suppressed");
        assert_eq!(sampler.latest.use_q_press_sequence, 0);
        sample_q_button(&mut sampler, false, false).expect("release");
        sample_q_button(&mut sampler, true, false).expect("fresh press");
        assert_eq!(sampler.latest.use_q_press_sequence, 1);
    }

    #[test]
    fn sequence_exhaustion_is_typed_and_does_not_wrap() {
        let mut sampler = ConsumableInputSampler::default();
        sampler.latest.use_q_press_sequence = u32::MAX;
        assert_eq!(
            sample_q_button(&mut sampler, true, false),
            Err(ConsumableSequenceError::Exhausted)
        );
        assert_eq!(sampler.latest.use_q_press_sequence, u32::MAX);
    }

    #[test]
    fn tonic_confirmation_is_a_well_formed_bounded_pcm_wave() {
        let wav = build_tonic_confirmation_wav();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(wav[24..28].try_into().expect("rate")),
            22_050
        );
        assert_eq!(
            u16::from_le_bytes(wav[34..36].try_into().expect("bits")),
            16
        );
        assert_eq!(
            usize::try_from(u32::from_le_bytes(
                wav[40..44].try_into().expect("data length")
            ))
            .expect("usize"),
            wav.len() - 44
        );
        assert!(wav.len() < 10_000);
    }
}
