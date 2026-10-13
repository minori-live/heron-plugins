//! The official FCPE Wav2Mel frontend for already-resampled 16 kHz mono PCM.
//!
//! This intentionally differs from Revox: reflected padding, 5 ms frame centers,
//! magnitude epsilon and time-major output follow FCPE's training frontend.

use std::{fmt, sync::Arc};

use rustfft::{Fft, FftPlanner, num_complex::Complex32};

use crate::features::{
    FeatureError, LOG_FLOOR, MelFilter, periodic_hann, slaney_filters, validate_audio,
};

pub const SAMPLE_RATE: usize = 16_000;
pub const FFT_SIZE: usize = 1024;
pub const HOP_SIZE: usize = 160;
pub const MEL_BINS: usize = 128;
const LEFT_PADDING: usize = (FFT_SIZE - HOP_SIZE) / 2;

/// FCPE model input, indexed as `values[frame * 128 + mel]`.
#[derive(Clone, Debug)]
pub struct FcpeMelFeatures {
    pub frames: usize,
    pub values: Vec<f32>,
}

/// Reusable FFT plan and official FCPE 16 kHz Mel convention.
pub struct FcpeFeatures {
    forward: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    filters: Vec<MelFilter>,
}

impl fmt::Debug for FcpeFeatures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FcpeFeatures").finish_non_exhaustive()
    }
}

impl Default for FcpeFeatures {
    fn default() -> Self {
        Self::new()
    }
}

impl FcpeFeatures {
    /// Prepares the periodic Hann FFT and Slaney area-normalized filter bank.
    pub fn new() -> Self {
        Self {
            forward: FftPlanner::new().plan_fft_forward(FFT_SIZE),
            window: periodic_hann(FFT_SIZE),
            filters: slaney_filters(SAMPLE_RATE, FFT_SIZE, MEL_BINS),
        }
    }

    /// Extracts time-major Mel features matching official FCPE Wav2MelModule.
    ///
    /// Pads 432 samples on the left and at least 432 on the right. When reflection
    /// would require more samples than provided, constant-zero padding is used.
    /// The FFT center is 80 samples (5 ms) after each hop. The final spectrum is
    /// repeated to produce `floor(samples / 160) + 1` frames. This allocates.
    ///
    /// # Errors
    /// Rejects empty/non-finite PCM and non-finite feature calculations.
    pub fn analyze(&self, audio: &[f32]) -> Result<FcpeMelFeatures, FeatureError> {
        validate_audio(audio)?;
        let right_padding = LEFT_PADDING.max((FFT_SIZE - LEFT_PADDING).saturating_sub(audio.len()));
        let reflect = right_padding < audio.len();
        let computed_frames =
            (audio.len() + LEFT_PADDING + right_padding - FFT_SIZE) / HOP_SIZE + 1;
        let frames = audio.len() / HOP_SIZE + 1;
        let mut values = Vec::with_capacity(frames * MEL_BINS);
        let mut buffer = vec![Complex32::default(); FFT_SIZE];
        let mut scratch = vec![Complex32::default(); self.forward.get_inplace_scratch_len()];
        let mut magnitude = vec![0.0; FFT_SIZE / 2 + 1];
        for frame in 0..computed_frames {
            for (index, (sample, window)) in buffer.iter_mut().zip(&self.window).enumerate() {
                let position = frame * HOP_SIZE + index;
                let source = if position < LEFT_PADDING {
                    reflect.then_some(LEFT_PADDING - position)
                } else {
                    let source = position - LEFT_PADDING;
                    if source < audio.len() {
                        Some(source)
                    } else if reflect {
                        Some(2 * audio.len() - 2 - source)
                    } else {
                        None
                    }
                };
                *sample = Complex32::new(
                    source
                        .and_then(|source| audio.get(source))
                        .copied()
                        .unwrap_or(0.0)
                        * window,
                    0.0,
                );
            }
            self.forward.process_with_scratch(&mut buffer, &mut scratch);
            for (value, bin) in magnitude.iter_mut().zip(&buffer) {
                *value = (bin.norm_sqr() + 1e-9).sqrt();
                if !value.is_finite() {
                    return Err(FeatureError::NumericOverflow);
                }
            }
            for filter in &self.filters {
                let value = filter.apply(&magnitude).max(LOG_FLOOR).ln();
                if !value.is_finite() {
                    return Err(FeatureError::NumericOverflow);
                }
                values.push(value);
            }
        }
        if computed_frames < frames {
            values.extend_from_within(values.len() - MEL_BINS..);
        }
        Ok(FcpeMelFeatures { frames, values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_fcpe_frame_repeats_and_short_audio_still_has_one_fft() {
        let dsp = FcpeFeatures::new();
        let short = dsp.analyze(&[0.25]).unwrap();
        assert_eq!(short.frames, 1);
        assert_eq!(short.values.len(), MEL_BINS);
        let two = dsp.analyze(&vec![0.25; HOP_SIZE]).unwrap();
        assert_eq!(two.frames, 2);
        assert_eq!(two.values[..MEL_BINS], two.values[MEL_BINS..]);
    }

    #[test]
    fn mel_layout_padding_and_scale_match_official_fcpe_reference() {
        // Actual Wav2MelModule, torchfcpe commit 6a149c1afb1c7e7821b71869dfb31ad50c95b516,
        // torch 2.9.1 / librosa 0.11.0. Generated by scripts/revox/export_fcpe.py.
        // This fixture validates our model-input mapping, not upstream DSP internals.
        let input: Vec<f32> = (0..2049)
            .map(|index| {
                let phase = std::f64::consts::TAU * index as f64 / SAMPLE_RATE as f64;
                (0.3 * (220.0 * phase).sin() + 0.1 * (660.0 * phase).cos()) as f32
            })
            .collect();
        let features = FcpeFeatures::new().analyze(&input).unwrap();
        assert_eq!(features.frames, 13);
        let expected = [
            (0, 0, -0.9737758),
            (0, 12, -0.83238596),
            (0, 31, -3.1553361),
            (0, 64, -4.9361434),
            (0, 100, -6.4908414),
            (0, 127, -7.1542206),
            (1, 0, -1.5138663),
            (1, 12, -1.3955431),
            (1, 31, -3.649_984),
            (1, 64, -5.474972),
            (1, 100, -7.02964),
            (1, 127, -7.6930075),
            (6, 0, -8.238596),
            (6, 12, -5.987416),
            (6, 31, -6.0579844),
            (6, 64, -11.512925),
            (6, 100, -11.512925),
            (6, 127, -11.512925),
            (12, 0, -2.0044222),
            (12, 12, -1.8284796),
            (12, 31, -5.702187),
            (12, 64, -6.250965),
            (12, 100, -7.7646704),
            (12, 127, -8.423541),
        ];
        for (frame, mel, expected) in expected {
            let actual = features.values[frame * MEL_BINS + mel];
            // 5e-4 log magnitude is <0.005 dB; float32 FFTs differ most in weak bins.
            assert!(
                (actual - expected).abs() < 5e-4,
                "frame {frame}, mel {mel}: {actual} != {expected}"
            );
        }
        let short = FcpeFeatures::new().analyze(&[0.25]).unwrap();
        for (mel, expected) in [
            (0, -4.3126364),
            (12, -4.291762),
            (64, -4.214133),
            (127, -4.1967845),
        ] {
            assert!((short.values[mel] - expected).abs() < 2e-6);
        }
    }
}
