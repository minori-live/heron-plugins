//! Accepted pitch history and excitation state for fixed, overlapping windows.

use crate::{
    features::HOP_SIZE,
    noise::seeded_noise_at,
    pipeline::{
        PLUGIN_CONTEXT_SAMPLES, PLUGIN_HOP_SAMPLES, PLUGIN_OVERLAP_SAMPLES, PLUGIN_WINDOW_SAMPLES,
    },
    pitch::PitchFrame,
};

const CONTEXT_FRAMES: usize = PLUGIN_CONTEXT_SAMPLES / HOP_SIZE;
const HOP_FRAMES: usize = PLUGIN_HOP_SAMPLES / HOP_SIZE;
// Include the right-hand frame interpolated by the final overlap sample.
const ACCEPTED_FRAMES: usize = CONTEXT_FRAMES + HOP_FRAMES + PLUGIN_OVERLAP_SAMPLES / HOP_SIZE + 1;

pub(crate) struct StreamState {
    channels: Vec<ChannelState>,
    window_start: i64,
}

struct ChannelState {
    pitch: Vec<PitchFrame>,
    boundary_phase: f32,
}

pub(crate) struct PreparedSource {
    pub phase_cycles: Vec<f32>,
    channels: Vec<ChannelState>,
}

impl Default for StreamState {
    fn default() -> Self {
        Self {
            channels: Vec::new(),
            window_start: -(PLUGIN_CONTEXT_SAMPLES as i64),
        }
    }
}

impl StreamState {
    pub fn noise(&self, seed: u64) -> Vec<f32> {
        seeded_noise_at(seed, self.window_start, PLUGIN_WINDOW_SAMPLES)
    }

    /// Prepare without changing accepted state. Inference/synthesis failure can
    /// retry the same window with the same phase, noise and historical pitch.
    pub fn prepare<'a>(
        &self,
        pitch: impl ExactSizeIterator<Item = &'a mut [PitchFrame]>,
        ratio: f32,
    ) -> PreparedSource {
        let mut phase_cycles = Vec::with_capacity(pitch.len());
        let mut channels = Vec::with_capacity(pitch.len());
        for (channel, pitch) in pitch.enumerate() {
            let previous = self.channels.get(channel);
            if let Some(previous) = previous {
                pitch[..previous.pitch.len()].copy_from_slice(&previous.pitch);
            }
            let phase_at_crop = local_phase(pitch, ratio, PLUGIN_CONTEXT_SAMPLES);
            let offset = previous.map_or(0.0, |previous| {
                (previous.boundary_phase - phase_at_crop).rem_euclid(1.0)
            });
            let phase_at_next_crop =
                local_phase(pitch, ratio, PLUGIN_CONTEXT_SAMPLES + PLUGIN_HOP_SAMPLES);
            phase_cycles.push(offset);
            channels.push(ChannelState {
                pitch: pitch[HOP_FRAMES..ACCEPTED_FRAMES].to_vec(),
                boundary_phase: (phase_at_next_crop + offset).rem_euclid(1.0),
            });
        }
        PreparedSource {
            phase_cycles,
            channels,
        }
    }

    pub fn commit(&mut self, prepared: PreparedSource) {
        self.channels = prepared.channels;
        self.window_start += PLUGIN_HOP_SAMPLES as i64;
    }
}

/// Matches the pinned graph's masked linear interpolation and nearest-frame UV
/// rule. No interpolation crosses a voiced/invalid boundary.
fn sample_frequency(pitch: &[PitchFrame], ratio: f32, sample: usize) -> f32 {
    let left = pitch[sample / HOP_SIZE];
    let right = pitch[(sample / HOP_SIZE + 1).min(pitch.len() - 1)];
    let frequency = |frame: PitchFrame| {
        if frame.voiced && frame.pitch_valid && frame.f0_hz > 0.0 {
            frame.f0_hz * ratio
        } else {
            0.0
        }
    };
    let left_hz = frequency(left);
    let right_hz = frequency(right);
    let remainder = sample % HOP_SIZE;
    if left_hz > 0.0 && right_hz > 0.0 {
        let fraction = remainder as f32 / HOP_SIZE as f32;
        left_hz * (1.0 - fraction) + right_hz * fraction
    } else if remainder * 2 <= HOP_SIZE {
        left_hz
    } else {
        right_hz
    }
}

/// The source divides in float32, sums in float64, casts back to float32, then
/// subtracts the first cumulative sample. Preserve that sample-zero convention
/// when anchoring the next window's phase to its emitted crop boundary.
fn local_phase(pitch: &[PitchFrame], ratio: f32, sample: usize) -> f32 {
    let first = sample_frequency(pitch, ratio, 0) / 48_000.0;
    let total: f64 = (0..=sample)
        .map(|sample| f64::from(sample_frequency(pitch, ratio, sample) / 48_000.0))
        .sum();
    total as f32 - first
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voiced(f0_hz: f32) -> PitchFrame {
        PitchFrame {
            f0_hz,
            confidence: 0.9,
            voiced: true,
            pitch_valid: true,
        }
    }

    fn same_frame(left: PitchFrame, right: PitchFrame) {
        assert_eq!(left.f0_hz, right.f0_hz);
        assert_eq!(left.confidence, right.confidence);
        assert_eq!(left.voiced, right.voiced);
        assert_eq!(left.pitch_valid, right.pitch_valid);
    }

    #[test]
    fn accepted_overlap_keeps_pitch_masks_noise_and_source_phase_despite_reanalysis() {
        let mut state = StreamState::default();
        let frames = PLUGIN_WINDOW_SAMPLES / HOP_SIZE;
        let mut first = [vec![voiced(220.0); frames], vec![voiced(330.0); frames]];
        // A UV gap crossing the next seam also exercises phase hold and the
        // nearest-frame mask transition; a fresh analysis revises it below.
        first[0][CONTEXT_FRAMES + HOP_FRAMES + 1] = PitchFrame {
            f0_hz: 0.0,
            confidence: 0.001,
            voiced: false,
            pitch_valid: false,
        };
        let ratio = (3.5_f32 / 12.0).exp2();
        let original_noise = state.noise(42);
        let prepared = state.prepare(first.iter_mut().map(Vec::as_mut_slice), ratio);
        let first_offsets = prepared.phase_cycles.clone();
        state.commit(prepared);
        let mut second = [vec![voiced(440.0); frames], vec![voiced(110.0); frames]];
        let prepared = state.prepare(second.iter_mut().map(Vec::as_mut_slice), ratio);
        for channel in 0..2 {
            for frame in 0..ACCEPTED_FRAMES - HOP_FRAMES {
                same_frame(first[channel][frame + HOP_FRAMES], second[channel][frame]);
            }
            // Fresh future estimates remain usable once they are outside the
            // already accepted span; no guessed pitch smoothing is applied.
            assert_eq!(
                second[channel][ACCEPTED_FRAMES - HOP_FRAMES].f0_hz,
                [440.0, 110.0][channel]
            );
            for offset in [0, 239, 240, 241, 719, PLUGIN_OVERLAP_SAMPLES - 1] {
                let old = local_phase(
                    &first[channel],
                    ratio,
                    PLUGIN_CONTEXT_SAMPLES + PLUGIN_HOP_SAMPLES + offset,
                ) + first_offsets[channel];
                let new = local_phase(&second[channel], ratio, PLUGIN_CONTEXT_SAMPLES + offset)
                    + prepared.phase_cycles[channel];
                let error = (old - new + 0.5).rem_euclid(1.0) - 0.5;
                assert!(
                    error.abs() < 1.0e-4,
                    "channel {channel} phase error {error} at {offset}"
                );
            }
        }
        let noise = state.noise(42);
        assert_eq!(
            original_noise[PLUGIN_HOP_SAMPLES..],
            noise[..PLUGIN_WINDOW_SAMPLES - PLUGIN_HOP_SAMPLES]
        );

        // A new shift uses the accepted unshifted F0 while inheriting the same
        // boundary phase. History must not freeze the previous pitch control.
        let shifted = state.prepare(second.iter_mut().map(Vec::as_mut_slice), 1.0);
        for channel in 0..2 {
            let old = local_phase(
                &first[channel],
                ratio,
                PLUGIN_CONTEXT_SAMPLES + PLUGIN_HOP_SAMPLES,
            ) + first_offsets[channel];
            let new = local_phase(&second[channel], 1.0, PLUGIN_CONTEXT_SAMPLES)
                + shifted.phase_cycles[channel];
            let error = (old - new + 0.5).rem_euclid(1.0) - 0.5;
            assert!(
                error.abs() < 1.0e-4,
                "shift changed boundary phase by {error}"
            );
            assert_eq!(
                sample_frequency(&second[channel], 1.0, PLUGIN_CONTEXT_SAMPLES),
                [220.0, 330.0][channel]
            );
        }
    }

    #[test]
    fn reset_discards_accepted_history_and_uncommitted_preparation_does_not_advance() {
        let mut state = StreamState::default();
        let frames = PLUGIN_WINDOW_SAMPLES / HOP_SIZE;
        let initial_noise = state.noise(42);
        let mut first = [vec![voiced(220.0); frames]];
        let uncommitted = state.prepare(first.iter_mut().map(Vec::as_mut_slice), 1.0);
        assert_eq!(uncommitted.phase_cycles, [0.0]);
        assert_eq!(state.noise(42), initial_noise);
        let prepared = state.prepare(first.iter_mut().map(Vec::as_mut_slice), 1.0);
        state.commit(prepared);
        assert_ne!(state.noise(42), initial_noise);
        state = StreamState::default();
        let mut reset = [vec![voiced(440.0); frames]];
        let prepared = state.prepare(reset.iter_mut().map(Vec::as_mut_slice), 1.0);
        assert_eq!(prepared.phase_cycles, [0.0]);
        assert_eq!(reset[0][CONTEXT_FRAMES].f0_hz, 440.0);
        assert_eq!(state.noise(42), initial_noise);
    }

    #[test]
    fn source_interpolation_holds_phase_through_unvoiced_or_invalid_frames() {
        let mut invalid = voiced(400.0);
        invalid.pitch_valid = false;
        let pitch = [voiced(100.0), voiced(200.0), invalid, voiced(300.0)];
        assert_eq!(sample_frequency(&pitch, 1.0, 240), 150.0);
        assert_eq!(sample_frequency(&pitch, 1.0, 480 + 240), 200.0);
        assert_eq!(sample_frequency(&pitch, 1.0, 480 + 241), 0.0);
        assert_eq!(sample_frequency(&pitch, 1.0, 960 + 240), 0.0);
        assert_eq!(sample_frequency(&pitch, 1.0, 960 + 241), 300.0);
        assert_eq!(local_phase(&pitch, 1.0, 0), 0.0);
        assert_eq!(
            local_phase(&pitch, 1.0, 800),
            local_phase(&pitch, 1.0, 1_000)
        );
    }
}
