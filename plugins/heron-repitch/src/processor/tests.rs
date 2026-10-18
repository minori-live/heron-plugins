use super::*;
use crate::{Plugin, RepitchParams};
use rustfft::{FftPlanner, num_complex::Complex};
use std::sync::Arc;
use std::time::Duration;
use truce::core::PluginRuntime;
use truce::prelude::*;
use truce_test::{InputSource, assert_no_audio_alloc, assertions, driver};

fn peak_hz(samples: &[f32], sample_rate: f64) -> f64 {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(samples.len());
    let mut buffer: Vec<Complex<f64>> = samples
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            let w = 0.5
                - 0.5 * (std::f64::consts::TAU * index as f64 / (samples.len() - 1) as f64).cos();
            Complex::new(f64::from(*sample) * w, 0.0)
        })
        .collect();
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

fn render_shifted_sine(semitones: f64) -> (Vec<f32>, u32, f64) {
    let sample_rate = 48_000.0;
    let input_hz = 440.0;
    let frames = 48_000;
    let signal: Vec<f32> = (0..frames)
        .map(|sample| {
            let phase = std::f64::consts::TAU * input_hz * f64::from(sample) / sample_rate;
            (0.2 * phase.sin()) as f32
        })
        .collect();
    let result = driver!(Plugin)
        .sample_rate(sample_rate)
        .channels(1)
        .block_size(256)
        .duration(Duration::from_secs(1))
        .input(InputSource::Buffer(vec![signal]))
        .setup(move |plugin, _| {
            let whole = semitones.trunc();
            let cents = ((semitones - whole) * 100.0).round();
            plugin.params().set_plain(0u32, whole);
            plugin.params().set_plain(1u32, cents);
            plugin.params().set_plain(2u32, 1.0);
            plugin.params().set_plain(3u32, 1.0);
            plugin.params().set_plain(4u32, 100.0);
        })
        .run();
    let latency = result.plugin.latency();
    (result.output[0].clone(), latency, sample_rate)
}

#[test]
fn plus_twelve_semitones_moves_sine_fft_peak_up_an_octave() {
    let (output, latency, sample_rate) = render_shifted_sine(12.0);
    assert!(latency > 0);
    let start = latency as usize + 4_096;
    let analysis = &output[start..start + 8_192];
    let measured = peak_hz(analysis, sample_rate);
    let expected = 880.0;
    let cents = 1200.0 * (measured / expected).log2();
    assert!(
        cents.abs() < 40.0,
        "expected {expected} Hz, measured {measured} Hz ({cents} cents)"
    );
}

#[test]
fn minus_twelve_semitones_moves_sine_fft_peak_down_an_octave() {
    let (output, latency, sample_rate) = render_shifted_sine(-12.0);
    assert!(latency > 0);
    let start = latency as usize + 4_096;
    let analysis = &output[start..start + 8_192];
    let measured = peak_hz(analysis, sample_rate);
    let expected = 220.0;
    let cents = 1200.0 * (measured / expected).log2();
    assert!(
        cents.abs() < 40.0,
        "expected {expected} Hz, measured {measured} Hz ({cents} cents)"
    );
}

#[test]
fn parameter_state_round_trips_through_collect_and_restore() {
    let params = RepitchParams::default();
    params.set_plain(0u32, 5.0);
    params.set_plain(1u32, -40.0);
    params.set_plain(2u32, 0.0);
    params.set_plain(3u32, 0.0);
    params.set_plain(4u32, 35.0);
    let (ids, values) = params.collect_values();
    let stored: Vec<_> = ids.into_iter().zip(values).collect();
    let restored = RepitchParams::default();
    restored.restore_values(&stored);
    assert_eq!(restored.semitones.value(), 5);
    assert_eq!(restored.cents.value(), -40);
    assert!(!restored.formant.value());
    assert_eq!(restored.quality.value(), 0);
    assert!((restored.mix.value() - 35.0).abs() < 1.0e-6);
    assert!(!Arc::ptr_eq(&params.editor_state, &restored.editor_state));
}

#[test]
fn process_is_allocation_free_after_reset_and_stays_finite() {
    let sample_rate = 48_000.0;
    let frames = 4_096;
    let signal: Vec<f32> = (0..frames)
        .map(|sample| {
            let phase = std::f64::consts::TAU * 440.0 * f64::from(sample) / sample_rate;
            (0.2 * phase.sin()) as f32
        })
        .collect();
    let result = assert_no_audio_alloc(|| {
        driver!(Plugin)
            .sample_rate(sample_rate)
            .channels(2)
            .block_size(128)
            .duration(Duration::from_millis(80))
            .input(InputSource::Buffer(vec![signal.clone(), signal.clone()]))
            .script(|script| {
                // set_param takes normalized values.
                script.set_param(0u32, (3.0 + 12.0) / 24.0);
                script.wait_ms(20);
                script.set_param(1u32, (-25.0 + 100.0) / 200.0);
                script.wait_ms(20);
                script.set_param(2u32, 0.0);
                script.wait_ms(20);
                script.set_param(4u32, 0.5);
                script.wait_ms(20);
            })
            .run()
    });
    assertions::assert_no_nans(&result);
    assert!(result.plugin.latency() > 0);
}

#[test]
fn reset_reports_active_mode_start_delay_to_the_host() {
    let params = RepitchParams::default();
    assert_eq!(params.quality(), Quality::Finer);
    let mut state = RepitchDspState::default();
    state.reset(&params, 48_000.0, 512);
    let latency = state.latency_samples();
    assert!(latency > 0);
    assert_eq!(params.status.latency_samples(), latency);
    let expected = state
        .finer
        .as_ref()
        .expect("finer engine")
        .mono
        .start_delay() as u32;
    assert_eq!(latency, expected);
}

fn process_mono(state: &mut RepitchDspState, params: &RepitchParams, input: &[f32]) -> Vec<f32> {
    let frames = input.len();
    let mut output = vec![0.0; frames];
    let inputs = [input];
    let mut outputs = [output.as_mut_slice()];
    let mut buffer = AudioBuffer::from_slices_checked(&inputs, &mut outputs, frames);
    state.process(params, &mut buffer);
    output
}

#[test]
fn quality_change_updates_reported_latency_to_active_mode() {
    let params = RepitchParams::default();
    let mut state = RepitchDspState::default();
    state.reset(&params, 48_000.0, 512);
    let finer = state.latency_samples();
    params.set_plain(3u32, 0.0);
    assert_eq!(params.quality(), Quality::Faster);
    let faster_direct = state
        .faster
        .as_ref()
        .expect("faster engine")
        .mono
        .start_delay() as u32;
    assert_ne!(
        finer, faster_direct,
        "Faster and Finer should report different start delays at 48 kHz"
    );

    let input = vec![0.1_f32; 128];
    let _ = process_mono(&mut state, &params, &input);
    assert_eq!(state.latency_samples(), faster_direct);
    assert_eq!(params.status.latency_samples(), faster_direct);
}

#[test]
fn mix_zero_returns_latency_aligned_dry_impulse() {
    let params = RepitchParams::default();
    params.set_plain(4u32, 0.0);
    let mut state = RepitchDspState::default();
    state.reset(&params, 48_000.0, 512);
    let latency = state.latency_samples() as usize;
    assert!(latency > 0);

    let total = latency + 8;
    let mut input = vec![0.0_f32; total];
    input[0] = 1.0;
    let mut output = Vec::with_capacity(total);
    let block = 128;
    let mut offset = 0;
    while offset < total {
        let end = (offset + block).min(total);
        output.extend(process_mono(
            &mut state,
            &params,
            &input[offset..end],
        ));
        offset = end;
    }
    for (index, sample) in output.iter().enumerate() {
        if index == latency {
            assert!(
                (sample - 1.0).abs() < 1.0e-5,
                "dry impulse at {index}: {sample}"
            );
        } else {
            assert!(
                sample.abs() < 1.0e-5,
                "unexpected energy at {index}: {sample}"
            );
        }
    }
}
