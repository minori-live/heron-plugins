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
}

#[cfg(test)]
mod tests {
    use super::*;

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
