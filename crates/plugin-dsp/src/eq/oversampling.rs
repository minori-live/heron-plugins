//! Short stereo FIR with contiguous newest-to-oldest histories.

use fearless_simd::prelude::*;
use std::f64::consts::PI;

pub(super) const OVERSAMPLE_TAPS: usize = 33;

pub(super) fn oversampling_kernel() -> [f64; OVERSAMPLE_TAPS] {
    let mut kernel = std::array::from_fn(|index| {
        let position = index as f64 - 16.0;
        let sinc = if position == 0.0 {
            0.5
        } else {
            (PI * position / 2.0).sin() / (PI * position)
        };
        // Blackman window: stopband rejection is part of the actual response.
        let phase = 2.0 * PI * index as f64 / 32.0;
        sinc * (0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos())
    });
    let sum: f64 = kernel.iter().sum();
    for coefficient in &mut kernel {
        *coefficient /= sum;
    }
    kernel
}

pub(super) struct OversamplingFir {
    kernel: [f64; OVERSAMPLE_TAPS],
    // Each sample is written twice, removing wraparound from the dot product.
    // Storage stays fixed, and no history is copied between audio frames.
    history: [[f64; OVERSAMPLE_TAPS * 2]; 2],
    cursor: usize,
}

impl OversamplingFir {
    pub(super) fn new() -> Self {
        Self {
            kernel: oversampling_kernel(),
            history: [[0.0; OVERSAMPLE_TAPS * 2]; 2],
            cursor: 0,
        }
    }

    #[inline(always)]
    pub(super) fn process<S: Simd>(&mut self, simd: S, input: (f64, f64)) -> (f64, f64) {
        self.cursor = if self.cursor == 0 {
            OVERSAMPLE_TAPS - 1
        } else {
            self.cursor - 1
        };
        for (history, sample) in self.history.iter_mut().zip([input.0, input.1]) {
            history[self.cursor] = sample;
            history[self.cursor + OVERSAMPLE_TAPS] = sample;
        }
        let left = &self.history[0][self.cursor..self.cursor + OVERSAMPLE_TAPS];
        let right = &self.history[1][self.cursor..self.cursor + OVERSAMPLE_TAPS];
        let width = S::f64s::LEN;
        let end = OVERSAMPLE_TAPS / width * width;
        let mut left_sum = S::f64s::splat(simd, 0.0);
        let mut right_sum = S::f64s::splat(simd, 0.0);
        for index in (0..end).step_by(width) {
            let coefficient = S::f64s::from_slice(simd, &self.kernel[index..index + width]);
            left_sum += coefficient * S::f64s::from_slice(simd, &left[index..index + width]);
            right_sum += coefficient * S::f64s::from_slice(simd, &right[index..index + width]);
        }
        let mut output = (left_sum.reduce_sum(), right_sum.reduce_sum());
        for index in end..OVERSAMPLE_TAPS {
            output.0 += self.kernel[index] * left[index];
            output.1 += self.kernel[index] * right[index];
        }
        output
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mirrored_fir_matches_scalar_impulses_and_long_history_on_each_backend() {
        use super::*;
        use fearless_simd::{Fallback, Level, dispatch};

        for level in [Level::Fallback(Fallback::new()), Level::new()] {
            let mut fir = OversamplingFir::new();
            let kernel = oversampling_kernel();
            let mut history = [[0.0; OVERSAMPLE_TAPS]; 2];
            let mut cursor = 0;
            for index in 0..4096 {
                let input = match index {
                    0 => (1.0, 0.0),
                    34 => (0.0, -0.75),
                    100..3000 => (
                        (index as f64 * 0.071).sin() * 0.3,
                        (index as f64 * 0.117).cos() * 0.2,
                    ),
                    _ => (0.0, 0.0),
                };
                history[0][cursor] = input.0;
                history[1][cursor] = input.1;
                let expected: [f64; 2] = std::array::from_fn(|channel| {
                    (0..OVERSAMPLE_TAPS)
                        .map(|tap| {
                            kernel[tap]
                                * history[channel]
                                    [(cursor + OVERSAMPLE_TAPS - tap) % OVERSAMPLE_TAPS]
                        })
                        .sum()
                });
                cursor = (cursor + 1) % OVERSAMPLE_TAPS;
                let actual = dispatch!(level, simd => fir.process(simd, input));
                for (actual, expected) in [actual.0, actual.1].into_iter().zip(expected) {
                    assert!(
                        (actual - expected).abs() <= 2.0e-15,
                        "frame {index}, {level:?}: {actual} != {expected}"
                    );
                }
            }
        }
    }
}
