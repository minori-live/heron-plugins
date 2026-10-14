//! Plugin-owned packing, batch specialization and per-channel output independence.
//! Models are the same pinned assets prepared for every workspace/CI build.
use std::{error::Error, fs, path::Path};

use heron_revox_dsp::{
    Analysis, DspError, PLUGIN_HOP_SAMPLES, PLUGIN_WINDOW_SAMPLES, RevoxDsp, runtime::OrtRuntime,
};

fn harmonic(hz: f64) -> Vec<f32> {
    harmonic_at(hz, 0)
}

fn harmonic_at(hz: f64, start: usize) -> Vec<f32> {
    (0..PLUGIN_WINDOW_SAMPLES)
        .map(|sample| {
            let phase = std::f64::consts::TAU * hz * (start + sample) as f64 / 48_000.0;
            (0.15 * phase.sin() + 0.07 * (2.0 * phase).sin() + 0.04 * (3.0 * phase).sin()) as f32
        })
        .collect()
}

fn same_pitch(reference: &Analysis, actual: &Analysis) {
    assert_eq!(reference.frames(), actual.frames());
    for (reference, actual) in reference.pitch().iter().zip(actual.pitch()) {
        assert_eq!(reference.voiced, actual.voiced);
        assert_eq!(reference.pitch_valid, actual.pitch_valid);
        if reference.voiced {
            // Batching must preserve pitch substantially below the one-cent UI step.
            let cents = 1200.0 * (actual.f0_hz / reference.f0_hz).log2();
            assert!(cents.abs() < 0.1, "batch pitch changed by {cents} cents");
        }
    }
}

fn same_audio(reference: &[f32], actual: &[f32]) {
    assert_eq!(reference.len(), PLUGIN_WINDOW_SAMPLES);
    assert_eq!(reference.len(), actual.len());
    assert!(actual.iter().all(|sample| sample.is_finite()));
    let mut error = 0.0_f64;
    let mut signal = 0.0_f64;
    for (&reference, &actual) in reference.iter().zip(actual) {
        let difference = f64::from(reference) - f64::from(actual);
        assert!(difference.abs() < 1e-3);
        error += difference * difference;
        signal += f64::from(reference).powi(2);
    }
    // Permit provider floating-point variation, including phase accumulation,
    // while keeping error below -60 dB relative to the independently rendered signal.
    assert!(error <= signal * 1e-6 + 1e-14 * actual.len() as f64);
}

#[test]
fn fixed_channel_batch_matches_independent_mono_and_rejects_wrong_shapes()
-> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/revox");
    let fcpe = fs::read(root.join("fcpe-plugin.onnx"))?;
    let revox = fs::read(root.join("models/revox-plugin.onnx"))?;
    let runtime = OrtRuntime::new()?;
    assert!(matches!(
        RevoxDsp::from_memory_fixed(&runtime, &fcpe, &revox, 0),
        Err(DspError::UnsupportedChannels)
    ));
    assert!(matches!(
        RevoxDsp::from_memory_fixed(&runtime, &fcpe, &revox, 3),
        Err(DspError::UnsupportedChannels)
    ));
    let mut mono = RevoxDsp::from_memory_fixed(&runtime, &fcpe, &revox, 1)?;
    let mut stereo = RevoxDsp::from_memory_fixed(&runtime, &fcpe, &revox, 2)?;
    let left = harmonic(220.0);
    let right = harmonic(330.0);
    assert!(matches!(stereo.analyze(&left), Err(DspError::InputShape)));
    assert!(matches!(
        stereo.analyze_channels(&[&left, &right[..right.len() - 1]]),
        Err(DspError::InputShape)
    ));
    assert!(matches!(
        mono.analyze(&left[..left.len() - 1]),
        Err(DspError::InputShape)
    ));
    assert!(matches!(
        mono.analyze_channels(&[]),
        Err(DspError::InputShape)
    ));

    let left_analysis = mono.analyze(&left)?;
    let right_analysis = mono.analyze(&right)?;
    let left_output = mono.render(&left_analysis, 3.5, 42)?;
    let right_output = mono.render(&right_analysis, 3.5, 42)?;
    let analysis = stereo.analyze_channels(&[&left, &right])?;
    same_pitch(&left_analysis, &analysis[0]);
    same_pitch(&right_analysis, &analysis[1]);
    assert!(matches!(
        stereo.render_batch(&analysis[..1], 0.0, 42),
        Err(DspError::InputShape)
    ));
    assert!(matches!(
        stereo.render_batch(&analysis, f32::NAN, 42),
        Err(DspError::PitchShift)
    ));
    let output = stereo.render_batch(&analysis, 3.5, 42)?;
    assert_eq!(output.len(), 2);
    same_audio(&left_output, &output[0]);
    same_audio(&right_output, &output[1]);

    // Swap batch order to expose transposed/incorrectly sliced channel buffers.
    let swapped = stereo.render_channels(&[right, left.clone()], 3.5, 42)?;
    same_audio(&right_output, &swapped[0]);
    same_audio(&left_output, &swapped[1]);
    // A silent neighbor must not affect the other item's normalization or pitch.
    let silence = vec![0.0; PLUGIN_WINDOW_SAMPLES];
    let silent_analysis = mono.analyze(&silence)?;
    let silent_output = mono.render(&silent_analysis, 3.5, 42)?;
    let one_silent = stereo.render_channels(&[left, silence], 3.5, 42)?;
    same_audio(&left_output, &one_silent[0]);
    same_audio(&silent_output, &one_silent[1]);

    // Non-integral cycles per hop exercise each channel's accepted pitch and
    // phase carry on the second and third windows, beyond stateless batching.
    let mut independent = [Vec::with_capacity(3), Vec::with_capacity(3)];
    for (hz, windows) in [220.0, 330.0].into_iter().zip(&mut independent) {
        mono.reset_stream();
        for window in 0..3 {
            let mut output = mono.render_stream_channels(
                &[harmonic_at(hz, window * PLUGIN_HOP_SAMPLES)],
                3.5,
                42,
            )?;
            windows.push(output.remove(0));
        }
    }
    stereo.reset_stream();
    for (window, (left, right)) in independent[0].iter().zip(&independent[1]).enumerate() {
        let start = window * PLUGIN_HOP_SAMPLES;
        let output = stereo.render_stream_channels(
            &[harmonic_at(220.0, start), harmonic_at(330.0, start)],
            3.5,
            42,
        )?;
        same_audio(left, &output[0]);
        same_audio(right, &output[1]);
    }
    Ok(())
}
