//! Deterministic unit-variance source noise for reproducible model comparisons.

/// Generates standard normal noise from a seed using SplitMix64 and Box–Muller.
///
/// Supply these values directly to Revox: the ONNX graph applies its own 0.003
/// source-noise gain. This allocates and belongs on the inference worker.
pub fn seeded_noise(seed: u64, samples: usize) -> Vec<f32> {
    let mut state = seed;
    let mut output = Vec::with_capacity(samples);
    while output.len() < samples {
        let radius = (-2.0 * uniform_open(&mut state).ln()).sqrt();
        let angle = std::f64::consts::TAU * uniform_open(&mut state);
        let (sin, cos) = angle.sin_cos();
        output.push((radius * cos) as f32);
        if output.len() < samples {
            output.push((radius * sin) as f32);
        }
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
}
