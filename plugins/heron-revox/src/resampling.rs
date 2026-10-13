//! Window-local band-limited rate conversion; used only by the inference worker.

use std::ops::Range;

use fearless_simd::{Level, Simd, dispatch};
use fearless_simd_macros::simd;
use heron_revox_dsp::fir;

const TAPS: usize = 64;
const PHASES: usize = 1024;
const CENTER: isize = TAPS as isize / 2 - 1;

pub(crate) struct Resampler {
    ratio: f64,
    coefficients: Vec<[f32; TAPS]>,
    level: Level,
}

impl Resampler {
    pub fn new(input_rate: f64, output_rate: f64) -> Self {
        let ratio = input_rate / output_rate;
        let cutoff = (output_rate / input_rate).min(1.0) * 0.94;
        let coefficients = if input_rate == output_rate {
            Vec::new()
        } else {
            (0..PHASES)
                .map(|phase| {
                    let mut row = [0.0; TAPS];
                    let fraction = phase as f64 / PHASES as f64;
                    for (tap, weight) in row.iter_mut().enumerate() {
                        let distance = tap as f64 - CENTER as f64 - fraction;
                        let sinc = if distance.abs() < 1e-10 {
                            cutoff
                        } else {
                            (std::f64::consts::PI * cutoff * distance).sin()
                                / (std::f64::consts::PI * distance)
                        };
                        let window = 0.42
                            + 0.5 * (std::f64::consts::PI * distance / 32.0).cos()
                            + 0.08 * (std::f64::consts::TAU * distance / 32.0).cos();
                        *weight = (sinc * window) as f32;
                    }
                    let sum: f32 = row.iter().sum();
                    for weight in &mut row {
                        *weight /= sum;
                    }
                    row
                })
                .collect()
        };
        Self {
            ratio,
            coefficients,
            level: Level::new(),
        }
    }

    pub fn convert(&self, input: &[f32], length: usize) -> Vec<f32> {
        self.convert_range(input, 0..length)
    }

    /// Converts absolute output positions while retaining the complete input
    /// window. Cropping never restarts the polyphase clock or moves its edges.
    pub fn convert_range(&self, input: &[f32], range: Range<usize>) -> Vec<f32> {
        if self.coefficients.is_empty() {
            let available = input.get(range.start..).unwrap_or_default();
            let copied = range.len().min(available.len());
            let mut output = available[..copied].to_vec();
            output.resize(range.len(), 0.0);
            return output;
        }
        let mut output = vec![0.0; range.len()];
        dispatch!(self.level, simd => convert(simd, input, self.ratio, &self.coefficients, range.start, &mut output));
        output
    }
}

#[simd]
fn convert<S: Simd>(
    simd: S,
    input: &[f32],
    ratio: f64,
    coefficients: &[[f32; TAPS]],
    start: usize,
    output: &mut [f32],
) {
    for (offset, sample) in output.iter_mut().enumerate() {
        let index = start + offset;
        let position = index as f64 * ratio;
        let center = position.floor() as isize;
        let phase = ((position.fract() * PHASES as f64) as usize).min(PHASES - 1);
        let source = center - CENTER;
        *sample = if source >= 0 && source as usize + TAPS <= input.len() {
            fir::dot(
                simd,
                &input[source as usize..source as usize + TAPS],
                &coefficients[phase],
            )
        } else {
            coefficients[phase]
                .iter()
                .enumerate()
                .filter_map(|(tap, weight)| {
                    let source = center + tap as isize - CENTER;
                    usize::try_from(source)
                        .ok()
                        .and_then(|source| input.get(source))
                        .map(|sample| sample * weight)
                })
                .sum()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original_convert(resampler: &Resampler, input: &[f32], length: usize) -> Vec<f32> {
        if resampler.coefficients.is_empty() {
            let mut result = input.to_vec();
            result.resize(length, 0.0);
            return result;
        }
        (0..length)
            .map(|index| {
                let position = index as f64 * resampler.ratio;
                let center = position.floor() as isize;
                let phase = ((position.fract() * PHASES as f64) as usize).min(PHASES - 1);
                resampler.coefficients[phase]
                    .iter()
                    .enumerate()
                    .filter_map(|(tap, weight)| {
                        let source = center + tap as isize - CENTER;
                        usize::try_from(source)
                            .ok()
                            .and_then(|source| input.get(source))
                            .map(|sample| sample * weight)
                    })
                    .sum()
            })
            .collect()
    }

    #[test]
    fn cropped_conversion_preserves_absolute_phase_zero_edges_and_partial_vectors() {
        let input: Vec<_> = (0..257)
            .map(|i| 0.2 + (i as f32 * 0.13).sin() * 0.3)
            .collect();
        for host_rate in [44_100.0, 48_000.0, 88_200.0, 96_000.0, 176_400.0, 192_000.0] {
            for (from, to) in [(host_rate, 48_000.0), (48_000.0, host_rate)] {
                let mut resampler = Resampler::new(from, to);
                let expected = original_convert(&resampler, &input, 1_117);
                for level in [Level::fallback(), Level::new()] {
                    resampler.level = level;
                    let full = resampler.convert(&input, expected.len());
                    for (actual, expected) in full.iter().zip(&expected) {
                        // Same 1e-6 amplitude budget as the FCPE FIR edge policy.
                        assert!((actual - expected).abs() < 1e-6, "{from}->{to}, {level:?}");
                    }
                    for range in [0..0, 0..1, 0..67, 23..211, 193..347, 1_097..1_117] {
                        let cropped = resampler.convert_range(&input, range.clone());
                        // Cropping changes only which output points we compute;
                        // each point must be bit-identical to full SIMD + slice.
                        assert_eq!(cropped, full[range], "{from}->{to}, {level:?}");
                    }
                    assert_eq!(resampler.convert_range(&[], 3..20), vec![0.0; 17]);
                }
            }
        }
    }

    #[test]
    fn streaming_window_crop_matches_full_conversion_at_every_host_rate() {
        let input: Vec<_> = (0..101_760)
            .map(|i| (i as f32 * 0.017).sin() * 0.3)
            .collect();
        for rate in [44_100.0, 48_000.0, 88_200.0, 96_000.0, 176_400.0, 192_000.0] {
            let resampler = Resampler::new(48_000.0, rate);
            let full = resampler.convert(&input, (2.12 * rate) as usize);
            let start = (0.96 * rate) as usize;
            let end = start + (0.20 * rate) as usize;
            assert_eq!(
                resampler.convert_range(&input, start..end),
                full[start..end]
            );
        }
    }

    #[test]
    fn host_conversion_preserves_voice_band_and_rejects_out_of_band_aliases() {
        let resampler = Resampler::new(96_000.0, 48_000.0);
        let rms = |hz: f64| {
            let signal: Vec<_> = (0..9_600)
                .map(|i| (std::f64::consts::TAU * hz * i as f64 / 96_000.0).sin() as f32)
                .collect();
            let output = resampler.convert(&signal, 4_800);
            let interior = &output[64..output.len() - 64];
            (interior.iter().map(|v| v * v).sum::<f32>() / interior.len() as f32).sqrt()
        };
        assert!((rms(440.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
        assert!(rms(32_000.0) < 0.001);
        let signal: Vec<f32> = (0..4_410).map(|i| (i as f32 * 0.01).sin()).collect();
        let up = Resampler::new(44_100.0, 48_000.0).convert(&signal, 4_800);
        let roundtrip = Resampler::new(48_000.0, 44_100.0).convert(&up, signal.len());
        assert_eq!(roundtrip.len(), signal.len());
        let error = signal[64..4_300]
            .iter()
            .zip(&roundtrip[64..4_300])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            error < 0.001,
            "rate conversion changed timing or voice-band level: {error}"
        );
    }
}
