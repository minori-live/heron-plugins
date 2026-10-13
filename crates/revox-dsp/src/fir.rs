//! Shared tap-wise FIR arithmetic for the Revox worker's two resamplers.

use fearless_simd::prelude::*;
use fearless_simd_macros::simd;

/// Multiplies corresponding samples and taps, summing their common prefix.
///
/// The caller dispatches once for the whole output window and passes that SIMD
/// token to each dot product. Unaligned slices and a final partial vector are
/// supported. Products are not fused; lane reduction may change f32 rounding.
#[simd]
#[inline(always)]
pub fn dot<S: Simd>(simd: S, samples: &[f32], taps: &[f32]) -> f32 {
    let count = samples.len().min(taps.len());
    let mut samples = samples[..count].chunks_exact(S::f32s::LEN);
    let mut taps = taps[..count].chunks_exact(S::f32s::LEN);
    let mut sum = S::f32s::splat(simd, 0.0);
    for (samples, taps) in (&mut samples).zip(&mut taps) {
        sum += S::f32s::from_slice(simd, samples) * S::f32s::from_slice(simd, taps);
    }
    samples
        .remainder()
        .iter()
        .zip(taps.remainder())
        .fold(sum.reduce_sum(), |sum, (sample, tap)| sum + sample * tap)
}
