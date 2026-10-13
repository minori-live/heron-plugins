//! Center-aligned, antialiased 48 kHz to 16 kHz conversion for FCPE.

const RADIUS: usize = 96;

pub(crate) struct PitchResampler {
    kernel: [f32; RADIUS * 2 + 1],
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
        Self { kernel }
    }

    pub fn process(&self, input: &[f32]) -> Vec<f32> {
        (0..input.len().div_ceil(3))
            .map(|frame| {
                let center = frame * 3;
                self.kernel
                    .iter()
                    .enumerate()
                    .filter_map(|(tap, &weight)| {
                        let index = (center + tap).checked_sub(RADIUS)?;
                        Some(input.get(index).copied().unwrap_or(0.0) * weight)
                    })
                    .sum()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
