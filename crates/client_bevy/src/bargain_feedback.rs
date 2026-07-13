//! Accessible, presentation-only feedback for active Core Bargain mechanics.

use std::{
    io::Cursor,
    sync::{Arc, mpsc},
    thread,
};

use bevy::{log::warn, prelude::Resource};
use rodio::{Decoder, OutputStream, Sink};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BargainAudioCueKind {
    BellRepeat,
    BeltLocked,
}

#[derive(Debug, Resource)]
pub(crate) struct BargainAudioCue(mpsc::Sender<BargainAudioCueKind>);

impl BargainAudioCue {
    pub(crate) fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        if thread::Builder::new()
            .name("gravebound-bargain-audio".to_owned())
            .spawn(move || bargain_audio_worker(receiver))
            .is_err()
        {
            warn!(
                feature_id = "GB-M03-05E",
                "Bargain audio worker could not start"
            );
        }
        Self(sender)
    }

    pub(crate) fn play(&self, cue: BargainAudioCueKind) -> bool {
        self.0.send(cue).is_ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BellRepeatVisualPlan {
    pub(crate) label: &'static str,
    pub(crate) body_length: f32,
    pub(crate) notch_length: f32,
    pub(crate) animated_pulse: bool,
}

#[must_use]
pub(crate) const fn bell_repeat_visual_plan(reduced_motion: bool) -> BellRepeatVisualPlan {
    BellRepeatVisualPlan {
        label: "Bell echo bolt",
        body_length: 0.28,
        notch_length: 0.13,
        animated_pulse: !reduced_motion,
    }
}

fn bargain_audio_worker(receiver: mpsc::Receiver<BargainAudioCueKind>) {
    let waves = [
        Arc::<[u8]>::from(build_bargain_cue_wav(BargainAudioCueKind::BellRepeat)),
        Arc::<[u8]>::from(build_bargain_cue_wav(BargainAudioCueKind::BeltLocked)),
    ];
    let Ok((_stream, stream_handle)) = OutputStream::try_default() else {
        for _ in receiver {}
        return;
    };
    for cue in receiver {
        let Ok(decoder) = Decoder::new(Cursor::new(Arc::clone(&waves[cue_index(cue)]))) else {
            continue;
        };
        let Ok(sink) = Sink::try_new(&stream_handle) else {
            continue;
        };
        sink.set_volume(0.34);
        sink.append(decoder);
        sink.detach();
    }
}

const fn cue_index(cue: BargainAudioCueKind) -> usize {
    match cue {
        BargainAudioCueKind::BellRepeat => 0,
        BargainAudioCueKind::BeltLocked => 1,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // Bounded PCM synthesis clamps before deliberate f32-to-i16 conversion.
fn build_bargain_cue_wav(cue: BargainAudioCueKind) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 22_050;
    const BITS_PER_SAMPLE: u16 = 16;
    let (sample_count, start_hz, end_hz) = match cue {
        BargainAudioCueKind::BellRepeat => (3_528_u32, 880.0_f32, 520.0_f32),
        BargainAudioCueKind::BeltLocked => (2_205, 190.0, 150.0),
    };
    let data_bytes = sample_count * u32::from(BITS_PER_SAMPLE / 8);
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
    for index in 0..sample_count {
        let progress = index as f32 / sample_count as f32;
        let time = index as f32 / SAMPLE_RATE as f32;
        let frequency = start_hz + (end_hz - start_hz) * progress;
        let envelope = (1.0 - progress).powi(3);
        let fundamental = (std::f32::consts::TAU * frequency * time).sin();
        let overtone = (std::f32::consts::TAU * frequency * 2.0 * time).sin();
        let sample = ((fundamental * 0.82 + overtone * 0.18) * envelope * 8_000.0)
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
    fn reduced_motion_preserves_bell_name_shape_and_audio_identity() {
        let standard = bell_repeat_visual_plan(false);
        let reduced = bell_repeat_visual_plan(true);
        assert_eq!(standard.label, reduced.label);
        assert!((standard.body_length - reduced.body_length).abs() < f32::EPSILON);
        assert!((standard.notch_length - reduced.notch_length).abs() < f32::EPSILON);
        assert!(standard.animated_pulse);
        assert!(!reduced.animated_pulse);
        let bell = build_bargain_cue_wav(BargainAudioCueKind::BellRepeat);
        let locked = build_bargain_cue_wav(BargainAudioCueKind::BeltLocked);
        assert_eq!(&bell[..4], b"RIFF");
        assert_eq!(&locked[..4], b"RIFF");
        assert_ne!(bell, locked);
    }
}
