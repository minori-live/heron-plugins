//! Safe Rubber Band wrappers for live pitch shifting and realtime stretching.
//!
//! `LivePitchShifter` wraps `RubberBandLiveShifter` (v4 R3-live) with a fixed
//! host↔engine block adapter. Construction and scratch allocation happen off
//! the audio thread; `process_*` / `set_pitch_scale` / `set_formant_preserved`
//! are intended for single-owner realtime use with no further allocation.
//!
//! `RealtimeStretcher` exposes the classic Stretcher API (including R2/R3 engine
//! selection) for Heron's resampling / time-stretch work. Repitch uses the live
//! shifter for lower reported latency.

mod live;
mod stretcher;

pub use live::{LiveOptions, LivePitchShifter, PrepareError, Quality};
pub use stretcher::{RealtimeStretcher, StretcherOptions};

/// Pitch scale for an equal-tempered semitone shift: `2^(semitones/12)`.
pub fn pitch_scale_from_semitones(semitones: f64) -> f64 {
    2.0_f64.powf(semitones / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustfft::{FftPlanner, num_complex::Complex};

    fn peak_hz(samples: &[f32], sample_rate: f64) -> f64 {
        let mut planner = FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(samples.len());
        let windowed: Vec<Complex<f64>> = samples
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let w = 0.5
                    - 0.5
                        * (std::f64::consts::TAU * index as f64 / (samples.len() - 1) as f64).cos();
                Complex::new(f64::from(*sample) * w, 0.0)
            })
            .collect();
        let mut buffer = windowed;
        fft.process(&mut buffer);
        let bin = buffer
            .iter()
            .take(buffer.len() / 2)
            .enumerate()
            .max_by(|(_, a), (_, b)| a.norm_sqr().total_cmp(&b.norm_sqr()))
            .map(|(index, _)| index)
            .unwrap();
        bin as f64 * sample_rate / samples.len() as f64
    }

    #[test]
    fn live_shifter_reports_nonzero_start_delay() {
        let shifter = LivePitchShifter::prepare(48_000, 1, 512, LiveOptions::default(), 1.0)
            .expect("prepare");
        assert!(shifter.block_size() > 0);
        assert!(shifter.start_delay() > 0);
        assert_eq!(shifter.latency_samples(), shifter.start_delay() as u32);
    }

    #[test]
    fn plus_and_minus_twelve_semitones_move_a_sine_peak() {
        let sample_rate = 48_000.0;
        let channels = 1;
        let host_block = 256;
        let input_hz = 440.0;
        for semitones in [12.0_f64, -12.0] {
            let scale = pitch_scale_from_semitones(semitones);
            let mut shifter = LivePitchShifter::prepare(
                sample_rate as u32,
                channels,
                host_block,
                LiveOptions {
                    formant_preserved: true,
                    quality: Quality::Finer,
                    channels_together: true,
                },
                scale,
            )
            .expect("prepare");
            let latency = shifter.latency_samples() as usize;
            let total = latency + 16_384;
            let mut input = vec![0.0_f32; total];
            for (index, sample) in input.iter_mut().enumerate() {
                let phase = std::f64::consts::TAU * input_hz * index as f64 / sample_rate;
                *sample = (0.25 * phase.sin()) as f32;
            }
            let mut output = vec![0.0_f32; total];
            let mut offset = 0;
            while offset < total {
                let frames = host_block.min(total - offset);
                shifter.process_interleaved(
                    &input[offset..offset + frames],
                    &mut output[offset..offset + frames],
                    frames,
                    channels,
                );
                offset += frames;
            }
            let analysis = &output[latency + 2_048..latency + 2_048 + 8_192];
            let measured = peak_hz(analysis, sample_rate);
            let expected = input_hz * scale;
            let cents = 1200.0 * (measured / expected).log2();
            assert!(
                cents.abs() < 40.0,
                "{semitones:+} st: expected {expected} Hz, measured {measured} Hz ({cents} cents)"
            );
        }
    }

    #[test]
    fn stretcher_engine_finer_reports_version_three() {
        let stretcher = RealtimeStretcher::prepare(
            48_000,
            1,
            StretcherOptions {
                engine_finer: true,
                formant_preserved: false,
                ..StretcherOptions::default()
            },
            1.0,
            1.0,
            512,
        )
        .expect("prepare");
        assert_eq!(stretcher.engine_version(), 3);
        assert!(stretcher.start_delay() > 0);
    }

    #[test]
    fn reported_latency_equals_rubber_band_start_delay_for_each_mode() {
        for rate in [44_100u32, 48_000, 96_000] {
            for quality in [Quality::Faster, Quality::Finer] {
                let shifter = LivePitchShifter::prepare(
                    rate,
                    1,
                    512,
                    LiveOptions {
                        formant_preserved: true,
                        quality,
                        channels_together: true,
                    },
                    1.0,
                )
                .expect("prepare");
                assert_eq!(
                    shifter.latency_samples() as usize,
                    shifter.start_delay(),
                    "rate={rate} {quality:?}"
                );
                assert!(shifter.start_delay() > 0);
            }
        }
    }
}
