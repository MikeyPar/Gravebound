//! Accessible, presentation-only Oath mechanic audio cues.

use std::{
    io::Cursor,
    sync::{Arc, mpsc},
    thread,
};

use bevy::{log::warn, prelude::Resource};
use rodio::{Decoder, OutputStream, Sink};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OathAudioCueKind {
    TrapArmed,
    TrapTriggered,
    FrostbindImmune,
}

#[derive(Debug, Resource)]
pub(crate) struct OathAudioCue(mpsc::Sender<OathAudioCueKind>);

impl OathAudioCue {
    pub(crate) fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        if thread::Builder::new()
            .name("gravebound-oath-audio".to_owned())
            .spawn(move || oath_audio_worker(receiver))
            .is_err()
        {
            warn!(
                feature_id = "GB-M03-05C",
                "Oath audio worker could not start"
            );
        }
        Self(sender)
    }

    pub(crate) fn play(&self, cue: OathAudioCueKind) -> bool {
        self.0.send(cue).is_ok()
    }
}

fn oath_audio_worker(receiver: mpsc::Receiver<OathAudioCueKind>) {
    let waves = [
        Arc::<[u8]>::from(build_oath_cue_wav(OathAudioCueKind::TrapArmed)),
        Arc::<[u8]>::from(build_oath_cue_wav(OathAudioCueKind::TrapTriggered)),
        Arc::<[u8]>::from(build_oath_cue_wav(OathAudioCueKind::FrostbindImmune)),
    ];
    let Ok((_stream, stream_handle)) = OutputStream::try_default() else {
        for _ in receiver {}
        return;
    };
    for cue in receiver {
        let wav = Arc::clone(&waves[cue_index(cue)]);
        let Ok(decoder) = Decoder::new(Cursor::new(wav)) else {
            continue;
        };
        let Ok(sink) = Sink::try_new(&stream_handle) else {
            continue;
        };
        sink.set_volume(0.36);
        sink.append(decoder);
        sink.detach();
    }
}

const fn cue_index(cue: OathAudioCueKind) -> usize {
    match cue {
        OathAudioCueKind::TrapArmed => 0,
        OathAudioCueKind::TrapTriggered => 1,
        OathAudioCueKind::FrostbindImmune => 2,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // Bounded PCM synthesis clamps before deliberate f32-to-i16 conversion.
fn build_oath_cue_wav(cue: OathAudioCueKind) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 22_050;
    const BITS_PER_SAMPLE: u16 = 16;
    let (sample_count, start_hz, end_hz, pulse_hz) = match cue {
        OathAudioCueKind::TrapArmed => (2_646_u32, 420.0_f32, 620.0_f32, 0.0_f32),
        OathAudioCueKind::TrapTriggered => (3_087, 760.0, 280.0, 0.0),
        OathAudioCueKind::FrostbindImmune => (3_528, 210.0, 210.0, 18.0),
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
        let envelope = (1.0 - progress).powi(2);
        let pulse = if pulse_hz == 0.0 || (time * pulse_hz).fract() < 0.52 {
            1.0
        } else {
            0.18
        };
        let fundamental = (std::f32::consts::TAU * frequency * time).sin();
        let harmonic = (std::f32::consts::TAU * frequency * 1.5 * time).sin();
        let sample = ((fundamental * 0.76 + harmonic * 0.24) * envelope * pulse * 8_200.0)
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
    fn oath_cues_are_bounded_well_formed_and_distinct() {
        let cues = [
            build_oath_cue_wav(OathAudioCueKind::TrapArmed),
            build_oath_cue_wav(OathAudioCueKind::TrapTriggered),
            build_oath_cue_wav(OathAudioCueKind::FrostbindImmune),
        ];
        for cue in &cues {
            assert_eq!(&cue[..4], b"RIFF");
            assert_eq!(&cue[8..12], b"WAVE");
            assert!((5_000..8_000).contains(&cue.len()));
        }
        assert_ne!(cues[0], cues[1]);
        assert_ne!(cues[1], cues[2]);
        assert_ne!(cues[0], cues[2]);
    }
}
