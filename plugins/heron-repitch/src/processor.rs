//! Realtime Rubber Band LiveShifter adapter with dual quality engines.

use heron_rubberband::{LiveOptions, LivePitchShifter, Quality, pitch_scale_from_semitones};
use truce::prelude::AudioBuffer;

use crate::RepitchParams;

const MAX_HOST_BLOCK: usize = 8192;

struct EngineSet {
    mono: LivePitchShifter,
    stereo: LivePitchShifter,
}

#[derive(Default)]
pub struct RepitchDspState {
    faster: Option<EngineSet>,
    finer: Option<EngineSet>,
    latency: u32,
    scratch_in: Vec<f32>,
    scratch_out: Vec<f32>,
}

impl RepitchDspState {
    pub(crate) fn reset(&mut self, params: &RepitchParams, sample_rate: f64, max_block: usize) {
        self.faster = None;
        self.finer = None;
        self.latency = 0;
        let max_block = max_block.clamp(1, MAX_HOST_BLOCK);
        self.scratch_in = vec![0.0; max_block * 2];
        self.scratch_out = vec![0.0; max_block * 2];
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return;
        }
        let rate = sample_rate.round().clamp(1.0, u32::MAX as f64) as u32;
        let scale = pitch_scale_from_semitones(f64::from(params.pitch_shift()));
        let formant = params.formant.value();

        let mut latency = 0_u32;
        for quality in [Quality::Faster, Quality::Finer] {
            let options = LiveOptions {
                formant_preserved: formant,
                quality,
                channels_together: true,
            };
            let (Ok(mono), Ok(stereo)) = (
                LivePitchShifter::prepare(rate, 1, max_block, options, scale),
                LivePitchShifter::prepare(rate, 2, max_block, options, scale),
            ) else {
                self.faster = None;
                self.finer = None;
                self.latency = 0;
                return;
            };
            latency = latency
                .max(mono.latency_samples())
                .max(stereo.latency_samples());
            let set = EngineSet { mono, stereo };
            match quality {
                Quality::Faster => self.faster = Some(set),
                Quality::Finer => self.finer = Some(set),
            }
        }
        self.latency = latency;
        params.status.configure(sample_rate, latency);
    }

    pub fn latency_samples(&self) -> u32 {
        self.latency
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

        let Some(set) = (match params.quality() {
            Quality::Faster => self.faster.as_mut(),
            Quality::Finer => self.finer.as_mut(),
        }) else {
            for channel in 0..buffer.num_output_channels() {
                let input_channel = channel.min(buffer.num_input_channels().saturating_sub(1));
                let (input, output) = buffer.io_pair(input_channel, channel);
                output.copy_from_slice(input);
            }
            return;
        };

        let shifter = if channels == 1 {
            &mut set.mono
        } else {
            &mut set.stereo
        };
        shifter.set_pitch_scale(pitch_scale_from_semitones(f64::from(params.pitch_shift())));
        shifter.set_formant_preserved(params.formant.value());

        for frame in 0..frames {
            for channel in 0..channels {
                let sample = buffer.input(channel)[frame];
                self.scratch_in[frame * channels + channel] =
                    if sample.is_finite() { sample } else { 0.0 };
            }
        }
        shifter.process_interleaved(
            &self.scratch_in[..frames * channels],
            &mut self.scratch_out[..frames * channels],
            frames,
            channels,
        );
        for frame in 0..frames {
            for channel in 0..channels {
                buffer.output(channel)[frame] = self.scratch_out[frame * channels + channel];
            }
        }
        for channel in channels..buffer.num_output_channels() {
            buffer.output(channel).fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests;
