//! Deterministic unit-variance source noise for reproducible model comparisons.

/// Generates standard normal noise from a seed using SplitMix64 and Box–Muller.
///
/// Supply these values directly to Revox: the ONNX graph applies its own 0.003
/// source-noise gain. This allocates and belongs on the inference worker.
pub fn seeded_noise(seed: u64, samples: usize) -> Vec<f32> {
    seeded_noise_at(seed, 0, samples)
}

/// Generates the same deterministic source at a signed absolute sample position.
///
/// Negative positions cover the first window's padded left context. Box–Muller
/// pairs are indexed before choosing their odd/even member, so overlapping
/// windows receive identical noise even when either begins on an odd sample.
pub fn seeded_noise_at(seed: u64, start: i64, samples: usize) -> Vec<f32> {
    const STEP: u64 = 0x9e3779b97f4a7c15;
    let pair = start.div_euclid(2) as u64;
    let mut state = seed.wrapping_add(STEP.wrapping_mul(pair.wrapping_mul(2)));
    let mut output = Vec::with_capacity(samples);
    let mut skip_first = start.rem_euclid(2) == 1;
    while output.len() < samples {
        let radius = (-2.0 * uniform_open(&mut state).ln()).sqrt();
        let angle = std::f64::consts::TAU * uniform_open(&mut state);
        let (sin, cos) = angle.sin_cos();
        if !skip_first {
            output.push((radius * cos) as f32);
        }
        if output.len() < samples {
            output.push((radius * sin) as f32);
        }
        skip_first = false;
    }
    output
}

fn uniform_open(state: &mut u64) -> f64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^= value >> 31;
    ((value >> 12) as f64 + 0.5) / (1_u64 << 52) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_noise_is_reproducible_unit_variance_and_length_exact() {
        let noise = seeded_noise(42, 48_001);
        assert_eq!(noise.len(), 48_001);
        // Recorded from the original sequential SplitMix64/Box–Muller source.
        assert_eq!(
            noise[..4],
            [0.414_719_76, 0.652_681_23, -0.891_886_23, 1.326_833_6]
        );
        assert_eq!(noise, seeded_noise(42, 48_001));
        assert_ne!(noise[..16], seeded_noise(43, 16));
        let mean = noise.iter().map(|value| *value as f64).sum::<f64>() / noise.len() as f64;
        let variance = noise
            .iter()
            .map(|value| (*value as f64 - mean).powi(2))
            .sum::<f64>()
            / noise.len() as f64;
        assert!(mean.abs() < 0.015, "mean {mean}");
        assert!((variance - 1.0).abs() < 0.025, "variance {variance}");
    }

    #[test]
    fn absolute_noise_preserves_the_original_seed_sequence_and_signed_overlap() {
        let original = seeded_noise(42, 101_760);
        for start in [0, 1, 17, 7_680] {
            assert_eq!(
                seeded_noise_at(42, start, 1_001),
                original[start as usize..start as usize + 1_001]
            );
        }
        let earlier = seeded_noise_at(42, -46_081, 101_760);
        let later = seeded_noise_at(42, -38_400, 101_760);
        assert_eq!(earlier[7_681..], later[..101_760 - 7_681]);
        assert_eq!(seeded_noise_at(42, -1, 1_001)[1..], original[..1_000]);
        assert!(seeded_noise_at(42, -1, 0).is_empty());
    }
}
