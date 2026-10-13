//! Block gain and peak metering, sharing one parameter envelope across channels.

use fearless_simd::{Level, dispatch, prelude::*};
use fearless_simd_macros::simd;
use truce::prelude::*;

use crate::GainParams;

const CHUNK: usize = 256;

/// Fixed scratch also handles hosts whose maximum block size changes on reset.
pub struct GainDspState {
    level: Level,
    envelope: [f32; CHUNK],
}

impl Default for GainDspState {
    fn default() -> Self {
        Self {
            // Resolve the x86 detection cache before entering the audio callback.
            level: Level::new(),
            envelope: [0.0; CHUNK],
        }
    }
}

impl GainDspState {
    pub(crate) fn process(&mut self, params: &GainParams, buffer: &mut AudioBuffer) -> [f32; 2] {
        dispatch!(self.level, simd => self.process_simd(simd, params, buffer))
    }

    #[simd]
    fn process_simd<S: Simd>(
        &mut self,
        simd: S,
        params: &GainParams,
        buffer: &mut AudioBuffer,
    ) -> [f32; 2] {
        let mut peaks = [0.0; 2];
        for start in (0..buffer.num_samples()).step_by(CHUNK) {
            let len = CHUNK.min(buffer.num_samples() - start);
            let mut chunk = buffer.slice(start, len);
            if params.gain.is_smoothing() {
                params.gain.read_into(&mut self.envelope[..len]);
                for value in &mut self.envelope[..len] {
                    *value = linear_gain(*value);
                }
                self.process_chunk::<S, true>(simd, &mut chunk, 0.0, &mut peaks);
            } else {
                // is_smoothing uses a tolerance. One read completes Linear's
                // exact target snap before using the constant-gain fast path.
                let gain = linear_gain(params.gain.read());
                self.process_chunk::<S, false>(simd, &mut chunk, gain, &mut peaks);
            }
        }
        peaks
    }

    #[inline(always)]
    fn process_chunk<S: Simd, const RAMP: bool>(
        &mut self,
        simd: S,
        buffer: &mut AudioBuffer,
        gain: f32,
        peaks: &mut [f32; 2],
    ) {
        let len = buffer.num_samples();
        for (channel, peak) in peaks
            .iter_mut()
            .enumerate()
            .take(buffer.num_output_channels())
        {
            let input_channel = channel.min(buffer.num_input_channels() - 1);
            // Gain retains supports_in_place=false: Truce provides disjoint
            // inputs even when a host aliases buffers. Both mono->stereo
            // outputs therefore read the same original mono input.
            let (input, output) = buffer.io_pair(input_channel, channel);
            let block_peak =
                apply_gain::<S, RAMP>(simd, input, output, &self.envelope[..len], gain);
            *peak = if peak.is_nan() || block_peak.is_nan() {
                f32::NAN
            } else {
                peak.max(block_peak)
            };
        }
    }
}

fn linear_gain(db: f32) -> f32 {
    if db <= -90.0 {
        0.0
    } else if db.abs() <= f32::EPSILON {
        1.0
    } else {
        db_to_linear(db)
    }
}

#[inline(always)]
fn apply_gain<S: Simd, const RAMP: bool>(
    simd: S,
    input: &[f32],
    output: &mut [f32],
    envelope: &[f32],
    gain: f32,
) -> f32 {
    let width = S::f32s::LEN;
    let vector_end = output.len() / width * width;
    let mut peak = S::f32s::splat(simd, 0.0);
    let mut nan_lanes = peak.is_nan();
    for start in (0..vector_end).step_by(width) {
        let out = &mut output[start..start + width];
        let values = S::f32s::from_slice(simd, &input[start..start + width]);
        let gains = if RAMP {
            S::f32s::from_slice(simd, &envelope[start..start + width])
        } else {
            S::f32s::splat(simd, gain)
        };
        let values = values * gains;
        values.store_slice(out);
        nan_lanes |= values.is_nan();
        peak = peak.max(values.abs());
    }
    // NaN is a separate signal: vector max's backend-specific NaN choice
    // cannot erase a poisoned audio lane from the published peak meter.
    let mut has_nan = nan_lanes.any_true();
    let mut peak = peak.reduce_max();
    for (index, out) in output.iter_mut().enumerate().skip(vector_end) {
        *out = input[index] * if RAMP { envelope[index] } else { gain };
        has_nan |= out.is_nan();
        peak = peak.max(out.abs());
    }
    if has_nan { f32::NAN } else { peak }
}

#[cfg(test)]
mod tests;
