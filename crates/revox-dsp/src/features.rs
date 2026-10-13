//! Revox Vocoder 1.5 magnitude-Mel analysis and centered complex iSTFT.
//!
//! These allocating operations belong on the inference worker, never an audio callback.

use std::{fmt, sync::Arc};

use rustfft::{Fft, FftPlanner, num_complex::Complex32};

/// Revox's required mono sample rate.
pub const SAMPLE_RATE: usize = 48_000;
/// Revox's periodic Hann window and FFT length.
pub const FFT_SIZE: usize = 2048;
/// Samples between Revox frame centers.
pub const HOP_SIZE: usize = 480;
/// Number of one-sided complex spectral bins.
pub const SPECTRUM_BINS: usize = FFT_SIZE / 2 + 1;
/// Number of Slaney Mel bands expected by Revox.
pub const MEL_BINS: usize = 128;
pub(crate) const LOG_FLOOR: f32 = 1e-5;

/// Invalid audio, model output, or requested synthesis length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeatureError {
    /// At least one input sample is required.
    EmptyInput,
    /// Input PCM contains a non-finite value.
    NonFiniteAudio { index: usize },
    /// Both complex arrays must have `1025 * frames` entries and nonzero frames.
    SpectrumShape {
        frames: usize,
        real: usize,
        imag: usize,
    },
    /// Requested samples must match the supplied number of 480-sample frames.
    SampleCount { frames: usize, samples: usize },
    /// A complex model output contains a non-finite value.
    NonFiniteSpectrum { index: usize },
    /// Finite input exceeded the numeric range of the DSP calculation.
    NumericOverflow,
}

impl fmt::Display for FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => f.write_str("audio input is empty"),
            Self::NonFiniteAudio { index } => write!(f, "non-finite audio sample at {index}"),
            Self::SpectrumShape { frames, real, imag } => {
                write!(
                    f,
                    "invalid spectrum shape: {frames} frames, {real} real and {imag} imaginary values"
                )
            }
            Self::SampleCount { frames, samples } => {
                write!(f, "sample count {samples} does not match {frames} frames")
            }
            Self::NonFiniteSpectrum { index } => write!(f, "non-finite spectrum value at {index}"),
            Self::NumericOverflow => f.write_str("feature calculation exceeded its numeric range"),
        }
    }
}

impl std::error::Error for FeatureError {}

/// Mel-major model input, indexed as `values[mel * frames + frame]`.
#[derive(Clone, Debug)]
pub struct MelFeatures {
    pub frames: usize,
    pub values: Vec<f32>,
}

/// Fixed Revox 1.5 feature convention and reusable FFT plans.
pub struct RevoxFeatures {
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    filters: Vec<MelFilter>,
}

impl fmt::Debug for RevoxFeatures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RevoxFeatures").finish_non_exhaustive()
    }
}

impl Default for RevoxFeatures {
    fn default() -> Self {
        Self::new()
    }
}

impl RevoxFeatures {
    /// Prepares FFT plans, the periodic Hann window, and Slaney area-normalized filters.
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        Self {
            forward: planner.plan_fft_forward(FFT_SIZE),
            inverse: planner.plan_fft_inverse(FFT_SIZE),
            window: periodic_hann(FFT_SIZE),
            filters: slaney_filters(SAMPLE_RATE, FFT_SIZE, MEL_BINS),
        }
    }

    /// Analyzes 48 kHz mono PCM without loudness normalization or clipping.
    ///
    /// Frames are centered at `k * 480`, with constant-zero padding, and truncated
    /// to `ceil(samples / 480)`. Mel uses magnitude, not power, followed by natural log.
    ///
    /// # Errors
    /// Rejects empty/non-finite audio and non-finite intermediate calculations.
    pub fn analyze(&self, audio: &[f32]) -> Result<MelFeatures, FeatureError> {
        validate_audio(audio)?;
        let frames = audio.len().div_ceil(HOP_SIZE);
        let mut values = vec![0.0; frames * MEL_BINS];
        let mut buffer = vec![Complex32::default(); FFT_SIZE];
        let mut scratch = vec![Complex32::default(); self.forward.get_inplace_scratch_len()];
        let mut magnitude = vec![0.0; SPECTRUM_BINS];
        for frame in 0..frames {
            let center = frame * HOP_SIZE;
            for (index, (sample, window)) in buffer.iter_mut().zip(&self.window).enumerate() {
                let padded_index = center + index;
                let value = padded_index
                    .checked_sub(FFT_SIZE / 2)
                    .and_then(|source| audio.get(source))
                    .copied()
                    .unwrap_or(0.0);
                *sample = Complex32::new(value * window, 0.0);
            }
            self.forward.process_with_scratch(&mut buffer, &mut scratch);
            for (value, bin) in magnitude.iter_mut().zip(&buffer) {
                *value = bin.norm();
                if !value.is_finite() {
                    return Err(FeatureError::NumericOverflow);
                }
            }
            for (mel, filter) in self.filters.iter().enumerate() {
                let value = filter.apply(&magnitude).max(LOG_FLOOR).ln();
                if !value.is_finite() {
                    return Err(FeatureError::NumericOverflow);
                }
                values[mel * frames + frame] = value;
            }
        }
        Ok(MelFeatures { frames, values })
    }

    /// Synthesizes model output arrays laid out as `[1025, frames]`.
    ///
    /// Applies inverse FFT normalization, periodic Hann weighted overlap-add and
    /// the centered 1024-sample crop. The final partial hop is trimmed exactly.
    /// DC and Nyquist imaginary components are ignored, as required by real iSTFT.
    ///
    /// # Errors
    /// Rejects malformed/non-finite spectra, a length inconsistent with `frames`,
    /// and numeric overflow. Audio is not amplitude-clipped.
    pub fn synthesize(
        &self,
        real: &[f32],
        imag: &[f32],
        frames: usize,
        sample_count: usize,
    ) -> Result<Vec<f32>, FeatureError> {
        if frames == 0
            || frames.checked_mul(SPECTRUM_BINS) != Some(real.len())
            || real.len() != imag.len()
        {
            return Err(FeatureError::SpectrumShape {
                frames,
                real: real.len(),
                imag: imag.len(),
            });
        }
        if sample_count == 0 || sample_count.div_ceil(HOP_SIZE) != frames {
            return Err(FeatureError::SampleCount {
                frames,
                samples: sample_count,
            });
        }
        if let Some(index) = real
            .iter()
            .zip(imag)
            .position(|(re, im)| !re.is_finite() || !im.is_finite())
        {
            return Err(FeatureError::NonFiniteSpectrum { index });
        }
        let padded_count = (frames - 1) * HOP_SIZE + FFT_SIZE;
        let mut audio = vec![0.0_f32; padded_count];
        let mut weights = vec![0.0_f32; padded_count];
        let mut buffer = vec![Complex32::default(); FFT_SIZE];
        let mut scratch = vec![Complex32::default(); self.inverse.get_inplace_scratch_len()];
        for frame in 0..frames {
            for (bin, value) in buffer.iter_mut().enumerate().take(SPECTRUM_BINS) {
                let index = bin * frames + frame;
                *value = Complex32::new(
                    real[index],
                    if bin == 0 || bin == FFT_SIZE / 2 {
                        0.0
                    } else {
                        imag[index]
                    },
                );
            }
            for bin in 1..FFT_SIZE / 2 {
                buffer[FFT_SIZE - bin] = buffer[bin].conj();
            }
            self.inverse.process_with_scratch(&mut buffer, &mut scratch);
            let offset = frame * HOP_SIZE;
            for (index, (sample, window)) in buffer.iter().zip(&self.window).enumerate() {
                audio[offset + index] += sample.re * window / FFT_SIZE as f32;
                weights[offset + index] += window * window;
            }
        }
        let start = FFT_SIZE / 2;
        let mut output = Vec::with_capacity(sample_count);
        for (&sample, &weight) in audio[start..start + sample_count]
            .iter()
            .zip(&weights[start..start + sample_count])
        {
            let value = sample / weight;
            if !value.is_finite() {
                return Err(FeatureError::NumericOverflow);
            }
            output.push(value);
        }
        Ok(output)
    }
}

pub(crate) fn validate_audio(audio: &[f32]) -> Result<(), FeatureError> {
    if audio.is_empty() {
        return Err(FeatureError::EmptyInput);
    }
    if let Some(index) = audio.iter().position(|sample| !sample.is_finite()) {
        return Err(FeatureError::NonFiniteAudio { index });
    }
    Ok(())
}

pub(crate) fn periodic_hann(size: usize) -> Vec<f32> {
    (0..size)
        .map(|index| {
            (0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / size as f64).cos()) as f32
        })
        .collect()
}

pub(crate) struct MelFilter {
    start: usize,
    weights: Vec<f32>,
}

impl MelFilter {
    pub(crate) fn apply(&self, magnitude: &[f32]) -> f32 {
        self.weights
            .iter()
            .zip(&magnitude[self.start..])
            .map(|(weight, magnitude)| weight * magnitude)
            .sum()
    }
}

// Slaney's linear scale below 1 kHz, logarithmic above, with unit-area triangles.
// Calculated in f64 before rounding weights to the float32 model convention.
pub(crate) fn slaney_filters(
    sample_rate: usize,
    fft_size: usize,
    mel_bins: usize,
) -> Vec<MelFilter> {
    let linear_step = 200.0 / 3.0;
    let log_step = 6.4_f64.ln() / 27.0;
    let nyquist = sample_rate as f64 / 2.0;
    let max_mel = 15.0 + (nyquist / 1000.0).ln() / log_step;
    let edges: Vec<f64> = (0..mel_bins + 2)
        .map(|index| {
            let mel = max_mel * index as f64 / (mel_bins + 1) as f64;
            if mel < 15.0 {
                mel * linear_step
            } else {
                1000.0 * ((mel - 15.0) * log_step).exp()
            }
        })
        .collect();
    (0..mel_bins)
        .map(|band| {
            let left = edges[band];
            let center = edges[band + 1];
            let right = edges[band + 2];
            let normalization = 2.0 / (right - left);
            let frequency_step = sample_rate as f64 / fft_size as f64;
            let start = (left / frequency_step).ceil() as usize;
            let end = ((right / frequency_step).floor() as usize).min(fft_size / 2);
            let weights = (start..=end)
                .map(|bin| {
                    let frequency = bin as f64 * frequency_step;
                    let rising = (frequency - left) / (center - left);
                    let falling = (right - frequency) / (right - center);
                    (rising.min(falling).max(0.0) * normalization) as f32
                })
                .collect();
            MelFilter { start, weights }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnitude_mel_tracks_amplitude_and_centered_zero_padding() {
        let dsp = RevoxFeatures::new();
        let mut impulse = vec![0.0; 1201];
        impulse[0] = 1.0;
        let full = dsp.analyze(&impulse).unwrap();
        impulse[0] = 0.25;
        let quiet = dsp.analyze(&impulse).unwrap();
        assert_eq!(full.frames, 3);
        for band in [0, 12, 64, 127] {
            let base = full.values[band * 3];
            // A quarter-amplitude input changes magnitude Mel by ln(1/4), not ln(1/16).
            assert!((quiet.values[band * 3] - base - 0.25_f32.ln()).abs() < 2e-6);
            for frame in 1..3 {
                let window = 0.5
                    + 0.5
                        * (std::f32::consts::TAU * (frame * HOP_SIZE) as f32 / FFT_SIZE as f32)
                            .cos();
                let expected = (base.exp() * window).max(LOG_FLOOR).ln();
                assert!((full.values[band * 3 + frame] - expected).abs() < 3e-6);
            }
        }
    }

    #[test]
    fn silence_uses_natural_log_floor_and_ceiling_frame_count() {
        let features = RevoxFeatures::new().analyze(&vec![0.0; 481]).unwrap();
        assert_eq!(features.frames, 2);
        assert_eq!(features.values.len(), 2 * MEL_BINS);
        assert!(features.values.iter().all(|value| *value == LOG_FLOOR.ln()));
    }

    #[test]
    fn revox_magnitude_mel_matches_independent_torch_librosa_reference() {
        // Reference: Revox 1.5 manifest, torch 2.9.1 stft(center=True,
        // pad_mode="constant", periodic Hann) and librosa 0.11.0 Slaney filters.
        // Signal is calculated in f64 then cast to f32 before analysis.
        let input: Vec<f32> = (0..4097)
            .map(|index| {
                let phase = std::f64::consts::TAU * index as f64 / SAMPLE_RATE as f64;
                (0.3 * (220.0 * phase).sin() + 0.1 * (660.0 * phase).cos()) as f32
            })
            .collect();
        let features = RevoxFeatures::new().analyze(&input).unwrap();
        assert_eq!(features.frames, 9);
        let expected = [
            (0, 0, -0.84316295),
            (0, 12, -1.7473601),
            (0, 31, -2.8219028),
            (0, 64, -4.474461),
            (0, 100, -5.6594396),
            (0, 127, -6.1927733),
            (1, 0, -1.4547864),
            (1, 12, -2.291477),
            (1, 31, -3.4195857),
            (1, 64, -5.076946),
            (1, 100, -6.26066),
            (1, 127, -6.79387),
            (4, 0, -5.298835),
            (4, 12, -5.6273427),
            (4, 31, -9.314407),
            (4, 64, -11.512925),
            (4, 100, -11.512925),
            (4, 127, -11.512925),
            (8, 0, -2.8706467),
            (8, 12, -1.3388159),
            (8, 31, -2.1973808),
            (8, 64, -3.421726),
            (8, 100, -4.5475736),
            (8, 127, -5.076087),
        ];
        for (frame, mel, expected) in expected {
            let actual = features.values[mel * features.frames + frame];
            // 5e-4 log magnitude is <0.005 dB, including weak-bin float32 FFT noise.
            assert!(
                (actual - expected).abs() < 5e-4,
                "frame {frame}, mel {mel}: {actual} != {expected}"
            );
        }
    }

    #[test]
    fn synthesis_preserves_edge_impulses_gain_and_exact_final_length() {
        let samples: usize = 1201;
        let frames = samples.div_ceil(HOP_SIZE);
        let impulses = [(0, 0.7_f64), (711, -0.35), (samples - 1, 0.9)];
        let mut real = vec![0.0; SPECTRUM_BINS * frames];
        let mut imag = real.clone();
        // Independent closed-form DFT of windowed impulses: amplitude * exp(-iωn).
        for frame in 0..frames {
            for (position, amplitude) in impulses {
                let index =
                    position as isize + (FFT_SIZE / 2) as isize - (frame * HOP_SIZE) as isize;
                if !(0..FFT_SIZE as isize).contains(&index) {
                    continue;
                }
                let window =
                    0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / FFT_SIZE as f64).cos();
                for bin in 0..SPECTRUM_BINS {
                    let phase =
                        -std::f64::consts::TAU * (bin as f64 * index as f64) / FFT_SIZE as f64;
                    real[bin * frames + frame] += (amplitude * window * phase.cos()) as f32;
                    imag[bin * frames + frame] += (amplitude * window * phase.sin()) as f32;
                }
            }
        }
        let result = RevoxFeatures::new()
            .synthesize(&real, &imag, frames, samples)
            .unwrap();
        assert_eq!(result.len(), samples);
        for (index, value) in result.iter().enumerate() {
            let expected = impulses
                .iter()
                .find(|(position, _)| *position == index)
                .map_or(0.0, |(_, amplitude)| *amplitude as f32);
            assert!(
                (value - expected).abs() < 3e-7,
                "sample {index}: {value} != {expected}"
            );
        }
    }

    #[test]
    fn non_finite_audio_and_malformed_model_output_are_typed_errors() {
        let dsp = RevoxFeatures::new();
        assert_eq!(dsp.analyze(&[]).unwrap_err(), FeatureError::EmptyInput);
        assert_eq!(
            dsp.analyze(&[0.0, f32::NAN]).unwrap_err(),
            FeatureError::NonFiniteAudio { index: 1 }
        );
        assert!(matches!(
            dsp.synthesize(&[0.0], &[0.0], 1, 480),
            Err(FeatureError::SpectrumShape { .. })
        ));
        let mut real = vec![0.0; SPECTRUM_BINS];
        let imag = real.clone();
        assert!(matches!(
            dsp.synthesize(&real, &imag, 1, 481),
            Err(FeatureError::SampleCount { .. })
        ));
        real[10] = f32::INFINITY;
        assert_eq!(
            dsp.synthesize(&real, &imag, 1, 480).unwrap_err(),
            FeatureError::NonFiniteSpectrum { index: 10 }
        );
    }
}
