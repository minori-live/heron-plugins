//! Realtime Rubber Band LiveShifter adapter with dual quality engines.

use heron_rubberband::{LiveOptions, LivePitchShifter, Quality, pitch_scale_from_semitones};
use truce::prelude::AudioBuffer;

use crate::RepitchParams;

const MAX_HOST_BLOCK: usize = 8192;
/// Combined pitch range ceiling used only to size the dry delay line so Mix
/// stays aligned when the user sweeps Semitones/Cents up to +12.
const MAX_PITCH_SEMITONES: f64 = 12.0;

struct EngineSet {
    mono: LivePitchShifter,
    stereo: LivePitchShifter,
}

#[derive(Default)]
pub struct RepitchDspState {
    faster: Option<EngineSet>,
    finer: Option<EngineSet>,
    /// Host-reported latency for the currently selected Quality (and pitch).
    latency: u32,
    /// Circular delay for Mix dry path; capacity covers max Quality × max pitch.
    dry_delay: Vec<(f32, f32)>,
    dry_write: usize,
    scratch_in: Vec<f32>,
    scratch_out: Vec<f32>,
}

impl RepitchDspState {
    pub(crate) fn reset(&mut self, params: &RepitchParams, sample_rate: f64, max_block: usize) {
        self.faster = None;
        self.finer = None;
        self.latency = 0;
        self.dry_delay.clear();
        self.dry_write = 0;
        let max_block = max_block.clamp(1, MAX_HOST_BLOCK);
        self.scratch_in = vec![0.0; max_block * 2];
        self.scratch_out = vec![0.0; max_block * 2];
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            params.status.configure(sample_rate, 0);
            return;
        }
        let rate = sample_rate.round().clamp(1.0, u32::MAX as f64) as u32;
        let scale = pitch_scale_from_semitones(f64::from(params.pitch_shift()));
        let formant = params.formant.value();

        let mut dry_capacity = 0_usize;
        for quality in [Quality::Faster, Quality::Finer] {
            let options = LiveOptions {
                formant_preserved: formant,
                quality,
                channels_together: true,
            };
            let (Ok(mut mono), Ok(mut stereo)) = (
                LivePitchShifter::prepare(rate, 1, max_block, options, scale),
                LivePitchShifter::prepare(rate, 2, max_block, options, scale),
            ) else {
                self.faster = None;
                self.finer = None;
                self.latency = 0;
                params.status.configure(sample_rate, 0);
                return;
            };
            // Size Mix dry delay from Rubber Band's own start delay at the
            // highest in-range pitch; restore the live pitch afterward.
            let max_scale = pitch_scale_from_semitones(MAX_PITCH_SEMITONES);
            for shifter in [&mut mono, &mut stereo] {
                shifter.set_pitch_scale(max_scale);
                dry_capacity = dry_capacity.max(shifter.start_delay());
                shifter.set_pitch_scale(scale);
            }
            let set = EngineSet { mono, stereo };
            match quality {
                Quality::Faster => self.faster = Some(set),
                Quality::Finer => self.finer = Some(set),
            }
        }
        self.dry_delay = vec![(0.0, 0.0); dry_capacity.max(1)];
        self.dry_write = 0;
        self.latency = self.active_engine_latency(params.quality(), 1);
        params.status.configure(sample_rate, self.latency);
    }

    fn active_engine_latency(&self, quality: Quality, channels: usize) -> u32 {
        let Some(set) = (match quality {
            Quality::Faster => self.faster.as_ref(),
            Quality::Finer => self.finer.as_ref(),
        }) else {
            return 0;
        };
        if channels > 1 {
            set.stereo.latency_samples()
        } else {
            set.mono.latency_samples()
        }
    }

    pub fn latency_samples(&self) -> u32 {
        self.latency
    }

    fn delayed_dry(&mut self, left: f32, right: f32) -> (f32, f32) {
        let delay = self.latency as usize;
        if delay == 0 || self.dry_delay.is_empty() {
            return (left, right);
        }
        let capacity = self.dry_delay.len();
        let delay = delay.min(capacity);
        let read = (self.dry_write + capacity - delay) % capacity;
        let delayed = self.dry_delay[read];
        self.dry_delay[self.dry_write] = (left, right);
        self.dry_write = (self.dry_write + 1) % capacity;
        delayed
    }

    pub(crate) fn process(&mut self, params: &RepitchParams, buffer: &mut AudioBuffer) {
        let channels = buffer
            .num_input_channels()
            .min(buffer.num_output_channels())
            .min(2);
        let frames = buffer.num_samples();
        if channels == 0 || frames == 0 {
            return;
        }
        if frames * channels > self.scratch_in.len() {
            // Host exceeded the prepared max block; fail safe to silence rather
            // than allocate on the audio thread.
            for channel in 0..buffer.num_output_channels() {
                buffer.output(channel).fill(0.0);
            }
            return;
        }

        let quality = params.quality();
        let engines_ready = match quality {
            Quality::Faster => self.faster.is_some(),
            Quality::Finer => self.finer.is_some(),
        };
        if !engines_ready {
            for channel in 0..buffer.num_output_channels() {
                let input_channel = channel.min(buffer.num_input_channels().saturating_sub(1));
                let (input, output) = buffer.io_pair(input_channel, channel);
                output.copy_from_slice(input);
            }
            if self.latency != 0 {
                self.latency = 0;
                params.status.configure(params.status.sample_rate(), 0);
            }
            return;
        }

        let scale = pitch_scale_from_semitones(f64::from(params.pitch_shift()));
        let formant = params.formant.value();

        for frame in 0..frames {
            for channel in 0..channels {
                let sample = buffer.input(channel)[frame];
                self.scratch_in[frame * channels + channel] =
                    if sample.is_finite() { sample } else { 0.0 };
            }
        }

        let latency = {
            let set = match quality {
                Quality::Faster => self.faster.as_mut().expect("checked"),
                Quality::Finer => self.finer.as_mut().expect("checked"),
            };
            for shifter in [&mut set.mono, &mut set.stereo] {
                shifter.set_pitch_scale(scale);
                shifter.set_formant_preserved(formant);
            }
            let shifter = if channels == 1 {
                &mut set.mono
            } else {
                &mut set.stereo
            };
            // Trust Rubber Band's live getStartDelay for the active mode/pitch.
            let latency = shifter.latency_samples();
            shifter.process_interleaved(
                &self.scratch_in[..frames * channels],
                &mut self.scratch_out[..frames * channels],
                frames,
                channels,
            );
            latency
        };
        if latency != self.latency {
            self.latency = latency;
            params
                .status
                .configure(params.status.sample_rate(), latency);
        }

        let wet = params.wet_mix();
        let dry_gain = 1.0 - wet;
        for frame in 0..frames {
            let dry_l = self.scratch_in[frame * channels];
            let dry_r = if channels > 1 {
                self.scratch_in[frame * channels + 1]
            } else {
                dry_l
            };
            let (delayed_l, delayed_r) = self.delayed_dry(dry_l, dry_r);
            let wet_l = self.scratch_out[frame * channels];
            let wet_r = if channels > 1 {
                self.scratch_out[frame * channels + 1]
            } else {
                wet_l
            };
            buffer.output(0)[frame] = delayed_l * dry_gain + wet_l * wet;
            if channels > 1 {
                buffer.output(1)[frame] = delayed_r * dry_gain + wet_r * wet;
            }
        }
        for channel in channels..buffer.num_output_channels() {
            buffer.output(channel).fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests;
