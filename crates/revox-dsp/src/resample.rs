//! Center-aligned, antialiased 48 kHz to 16 kHz conversion for FCPE.
//! Input is zero-extended at both ends without renormalizing the edge kernels.

use fearless_simd::{Level, Simd, dispatch};
use fearless_simd_macros::simd;

use crate::fir;

const RADIUS: usize = 96;

pub(crate) struct PitchResampler {
    kernel: [f32; RADIUS * 2 + 1],
    level: Level,
}

impl PitchResampler {
    pub fn new() -> Self {
        // 7.5 kHz cutoff leaves a transition band before the 8 kHz Nyquist limit.
        let cutoff = 7_500.0_f64 / 48_000.0;
        let mut kernel = [0.0; RADIUS * 2 + 1];
        for (i, value) in kernel.iter_mut().enumerate() {
            let x = i as f64 - RADIUS as f64;
            let sinc = if x == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x)
            };
            let phase = 2.0 * std::f64::consts::PI * i as f64 / (2 * RADIUS) as f64;
            let blackman = 0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos();
            *value = (sinc * blackman) as f32;
        }
        let sum: f32 = kernel.iter().sum();
        for value in &mut kernel {
            *value /= sum;
        }
        Self {
            kernel,
            level: Level::new(),
        }
    }

    pub fn process(&self, input: &[f32]) -> Vec<f32> {
        let mut output = vec![0.0; input.len().div_ceil(3)];
        dispatch!(self.level, simd => process(simd, input, &self.kernel, &mut output));
        output
    }
}

#[simd]
fn process<S: Simd>(simd: S, input: &[f32], kernel: &[f32], output: &mut [f32]) {
    for (frame, sample) in output.iter_mut().enumerate() {
        let center = frame * 3;
        *sample = if center >= RADIUS && center + RADIUS < input.len() {
            fir::dot(simd, &input[center - RADIUS..=center + RADIUS], kernel)
        } else {
            // Only the two window edges need zero extension and tap checks.
            kernel
                .iter()
                .enumerate()
                .filter_map(|(tap, &weight)| {
                    let index = (center + tap).checked_sub(RADIUS)?;
                    Some(input.get(index).copied().unwrap_or(0.0) * weight)
                })
                .sum()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_simd_preserves_scalar_convolution_on_edges_and_unaligned_interiors() {
        for length in [0, 1, 63, 192, 193, 194, 257, 4_097] {
            let input: Vec<_> = (0..length)
                .map(|i| 0.2 + (i as f32 * 0.13).sin() * 0.3)
                .collect();
            let mut resampler = PitchResampler::new();
            // The original per-tap zero-extension calculation is the reference.
            let expected: Vec<f32> = (0..input.len().div_ceil(3))
                .map(|frame| {
                    let center = frame * 3;
                    resampler
                        .kernel
                        .iter()
                        .enumerate()
                        .filter_map(|(tap, weight)| {
                            let index = (center + tap).checked_sub(RADIUS)?;
                            Some(input.get(index).copied().unwrap_or(0.0) * weight)
                        })
                        .sum()
                })
                .collect();
            for level in [Level::fallback(), Level::new()] {
                resampler.level = level;
                let actual = resampler.process(&input);
                assert_eq!(actual.len(), expected.len());
                for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
                    // Preserve the existing zero-extension comparison's 1e-6
                    // absolute amplitude limit (below -120 dBFS).
                    assert!(
                        (actual - expected).abs() < 1e-6,
                        "length {length}, sample {index}, {level:?}: {actual} != {expected}"
                    );
                }
            }
        }
    }

    #[test]
    fn preserves_pitch_band_level_and_rejects_aliases() {
        let resampler = PitchResampler::new();
        let rms = |frequency: f32| {
            let input: Vec<_> = (0..48_000)
                .map(|i| {
                    (std::f64::consts::TAU * frequency as f64 * i as f64 / 48_000.0).sin() as f32
                })
                .collect();
            let output = resampler.process(&input);
            let middle = &output[100..15_900];
            (middle.iter().map(|x| x * x).sum::<f32>() / middle.len() as f32).sqrt()
        };
        assert!((rms(440.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.001);
        assert!(
            rms(12_000.0) < 0.0001,
            "aliased 12 kHz must be below -80 dBFS"
        );
    }

    #[test]
    fn retains_time_origin_and_partial_final_sample() {
        let resampler = PitchResampler::new();
        let mut input = vec![0.0; 1_001];
        input[480] = 1.0;
        let output = resampler.process(&input);
        assert_eq!(output.len(), 334);
        let peak = output
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(peak, 160);
    }

    #[test]
    fn window_edges_match_explicit_zero_extension() {
        let resampler = PitchResampler::new();
        // Keep both endpoints nonzero to expose clamping or edge renormalization.
        let input: Vec<_> = (0..257).map(|i| 0.25 + i as f32 / 512.0).collect();
        let output = resampler.process(&input);

        // Ten milliseconds of silence on each side moves both signal edges
        // inside the buffer while preserving the 3:1 sampling phase.
        let mut padded = vec![0.0; 480];
        padded.extend_from_slice(&input);
        padded.resize(padded.len() + 480, 0.0);
        let padded_output = resampler.process(&padded);
        let expected = &padded_output[160..160 + output.len()];

        for (frame, (&actual, &reference)) in output.iter().zip(expected).enumerate() {
            assert!(
                (actual - reference).abs() < 1e-6,
                "zero-extension mismatch at frame {frame}: {actual} vs {reference}"
            );
        }
    }
}
