// Pre-SIMD reference from commit 7d204b4; kept only for comparative benchmarks.
//! Center-aligned, antialiased 48 kHz to 16 kHz conversion for FCPE.
//! Input is zero-extended at both ends without renormalizing the edge kernels.

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

    /// Isolates contiguous layout from explicit SIMD, preserving scalar order.
    pub fn process_contiguous(&self, input: &[f32]) -> Vec<f32> {
        let mut output = vec![0.0; input.len().div_ceil(3)];
        for (frame, sample) in output.iter_mut().enumerate() {
            let center = frame * 3;
            *sample = if center >= RADIUS && center + RADIUS < input.len() {
                input[center - RADIUS..=center + RADIUS]
                    .iter()
                    .zip(&self.kernel)
                    .map(|(sample, weight)| sample * weight)
                    .sum()
            } else {
                self.kernel
                    .iter()
                    .enumerate()
                    .filter_map(|(tap, &weight)| {
                        let index = (center + tap).checked_sub(RADIUS)?;
                        Some(input.get(index).copied().unwrap_or(0.0) * weight)
                    })
                    .sum()
            };
        }
        output
    }
}
