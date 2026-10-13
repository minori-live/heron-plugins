// Pre-SIMD reference from commit 7d204b4; kept only for comparative benchmarks.
//! Window-local band-limited rate conversion; used only by the inference worker.

const TAPS: usize = 64;
const PHASES: usize = 1024;
const CENTER: isize = TAPS as isize / 2 - 1;

pub(crate) struct Resampler {
    ratio: f64,
    coefficients: Vec<[f32; TAPS]>,
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
        }
    }

    pub fn convert(&self, input: &[f32], length: usize) -> Vec<f32> {
        if self.coefficients.is_empty() {
            let mut result = input.to_vec();
            result.resize(length, 0.0);
            return result;
        }
        (0..length)
            .map(|index| {
                let position = index as f64 * self.ratio;
                let center = position.floor() as isize;
                let phase = ((position.fract() * PHASES as f64) as usize).min(PHASES - 1);
                self.coefficients[phase]
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

    /// Isolates contiguous layout from explicit SIMD, preserving scalar order.
    pub fn convert_contiguous(&self, input: &[f32], length: usize) -> Vec<f32> {
        let mut output = vec![0.0; length];
        if self.coefficients.is_empty() {
            let copied = input.len().min(length);
            output[..copied].copy_from_slice(&input[..copied]);
            return output;
        }
        for (index, sample) in output.iter_mut().enumerate() {
            let position = index as f64 * self.ratio;
            let center = position.floor() as isize;
            let phase = ((position.fract() * PHASES as f64) as usize).min(PHASES - 1);
            let source = center - CENTER;
            *sample = if source >= 0 && source as usize + TAPS <= input.len() {
                input[source as usize..source as usize + TAPS]
                    .iter()
                    .zip(&self.coefficients[phase])
                    .map(|(sample, weight)| sample * weight)
                    .sum()
            } else {
                self.coefficients[phase]
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
        output
    }
}
