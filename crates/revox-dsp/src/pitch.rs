//! FCPE local-argmax decoding and alignment to Revox's acoustic frame centers.

use serde::Serialize;

pub(crate) const PITCH_BINS: usize = 360;
const MIN_HZ: f32 = 32.7;
const MAX_HZ: f32 = 1_975.5;
const VOICED_THRESHOLD: f32 = 0.006;

/// Pitch conditions aligned to a Revox frame (10 ms grid starting at time zero).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PitchFrame {
    pub f0_hz: f32,
    pub confidence: f32,
    pub voiced: bool,
    pub pitch_valid: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PitchError {
    #[error("FCPE probabilities must have a nonzero number of 360-bin frames")]
    Shape,
    #[error("FCPE probabilities must be finite and in [0, 1]")]
    InvalidProbability,
}

pub(crate) fn decode(
    probabilities: &[f32],
    audio16k: &[f32],
) -> Result<Vec<PitchFrame>, PitchError> {
    if probabilities.is_empty() || !probabilities.len().is_multiple_of(PITCH_BINS) {
        return Err(PitchError::Shape);
    }
    if probabilities
        .iter()
        .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
    {
        return Err(PitchError::InvalidProbability);
    }
    let min_cents = 1_200.0 * (MIN_HZ / 10.0).log2();
    let cents_step = 1_200.0 * (MAX_HZ / MIN_HZ).log2() / (PITCH_BINS - 1) as f32;
    Ok(probabilities
        .as_chunks::<PITCH_BINS>()
        .0
        .iter()
        .enumerate()
        .map(|(frame, bins)| {
            // Strict > preserves the first argmax, matching the official decoder.
            let peak =
                (1..PITCH_BINS).fold(0, |best, i| if bins[i] > bins[best] { i } else { best });
            let confidence = bins[peak];
            let mut sum = 0.0;
            let mut weighted_cents = 0.0;
            for offset in -4_isize..=4 {
                // The official local decoder repeats clamped edge bins.
                let index = (peak as isize + offset).clamp(0, (PITCH_BINS - 1) as isize) as usize;
                let weight = bins[index];
                sum += weight;
                weighted_cents += weight * (min_cents + index as f32 * cents_step);
            }
            // FCPE confidence alone cannot distinguish reliable UV from failed pitch
            // tracking. Only measured silence (< -60 dBFS RMS over 20 ms) is marked
            // reliable UV; low-confidence nonsilent frames remain pitch-invalid.
            let center = frame * 160 + 80;
            let start = center.saturating_sub(160).min(audio16k.len());
            let end = (center + 160).min(audio16k.len());
            let samples = &audio16k[start..end];
            let silent = samples.is_empty()
                || samples.iter().map(|x| x * x).sum::<f32>() / (samples.len() as f32) < 1.0e-6;
            let voiced = confidence > VOICED_THRESHOLD && !silent;
            PitchFrame {
                f0_hz: if voiced {
                    10.0 * (weighted_cents / sum / 1_200.0).exp2()
                } else {
                    0.0
                },
                confidence,
                voiced,
                pitch_valid: silent || voiced,
            }
        })
        .collect())
}

pub(crate) fn align_to_revox(source: &[PitchFrame], frames: usize) -> Vec<PitchFrame> {
    // FCPE physical STFT centers are 5,15,... ms; Revox centers are 0,10,... ms.
    // Interpolate in log-frequency only within reliably voiced regions. Masks
    // use the nearest center (ties select the later frame), never bridge UV gaps.
    (0..frames)
        .map(|i| {
            let right = source[i.min(source.len() - 1)];
            let left = source[i.saturating_sub(1).min(source.len() - 1)];
            if i > 0 && left.voiced && left.pitch_valid && right.voiced && right.pitch_valid {
                PitchFrame {
                    f0_hz: (left.f0_hz * right.f0_hz).sqrt(),
                    ..right
                }
            } else {
                right
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncertain_pitch_and_silence_have_distinct_validity() {
        let uncertain = vec![0.001; PITCH_BINS];
        let voice = decode(&uncertain, &vec![0.1; 480]).unwrap()[0];
        assert!(!voice.voiced && !voice.pitch_valid);
        let silence = decode(&uncertain, &vec![0.0; 480]).unwrap()[0];
        assert!(!silence.voiced && silence.pitch_valid);
    }

    #[test]
    fn local_decoder_ignores_distant_peak_mass() {
        let mut probabilities = vec![0.0; PITCH_BINS];
        probabilities[180] = 0.8;
        probabilities[300] = 0.7;
        let decoded = decode(&probabilities, &vec![0.1; 480]).unwrap()[0];
        let expected = MIN_HZ * (MAX_HZ / MIN_HZ).powf(180.0 / 359.0);
        assert!((decoded.f0_hz / expected - 1.0).abs() < 1.0e-5);
        assert!(decoded.voiced && decoded.pitch_valid);
    }

    #[test]
    fn aligns_physical_centers_without_stretching_or_voicing_gaps() {
        let voiced = |f0_hz| PitchFrame {
            f0_hz,
            confidence: 0.9,
            voiced: true,
            pitch_valid: true,
        };
        let uv = PitchFrame {
            f0_hz: 0.0,
            confidence: 0.0,
            voiced: false,
            pitch_valid: false,
        };
        let aligned = align_to_revox(&[voiced(100.0), voiced(400.0), uv], 3);
        assert_eq!(aligned[0].f0_hz, 100.0);
        assert_eq!(aligned[1].f0_hz, 200.0);
        assert!(!aligned[2].voiced && !aligned[2].pitch_valid);
    }

    #[test]
    fn rejects_nonfinite_model_output() {
        let mut probabilities = vec![0.0; PITCH_BINS];
        probabilities[10] = f32::NAN;
        assert!(matches!(
            decode(&probabilities, &[0.0; 480]),
            Err(PitchError::InvalidProbability)
        ));
    }
}
