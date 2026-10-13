//! Opt-in integration evidence against acquired real FCPE and Revox models.
//!
//! First acquire/export models, then run:
//! `cargo test --release -p heron-revox-dsp --test model_pipeline -- --ignored`

use std::path::Path;

use heron_revox_dsp::{RevoxDsp, runtime::OrtRuntime};

#[test]
#[ignore = "requires the pinned model files in target/revox"]
fn real_models_preserve_timing_pitch_masks_and_pitch_shift()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/revox");
    let runtime = OrtRuntime::new()?;
    let mut dsp = RevoxDsp::load(
        &runtime,
        root.join("fcpe.onnx"),
        root.join("models/revox.onnx"),
    )?;
    // Rich, steady 220 Hz fixture checks pitch wiring independently of the
    // speech-trained vocoder's subjective treatment of synthetic timbre.
    let audio: Vec<f32> = (0..48_013)
        .map(|sample| {
            let phase = std::f64::consts::TAU * 220.0 * f64::from(sample) / 48_000.0;
            (0.15 * phase.sin() + 0.07 * (2.0 * phase).sin() + 0.04 * (3.0 * phase).sin()) as f32
        })
        .collect();
    let analysis = dsp.analyze(&audio)?;
    assert_eq!(analysis.sample_count(), audio.len());
    assert_eq!(analysis.pitch().len(), analysis.frames());
    assert!(
        analysis
            .pitch()
            .iter()
            .all(|frame| frame.f0_hz.is_finite() && frame.confidence.is_finite())
    );
    assert!(
        analysis
            .pitch()
            .iter()
            .all(|frame| !frame.voiced || frame.pitch_valid && frame.f0_hz > 0.0)
    );
    assert!(
        analysis
            .pitch()
            .iter()
            .all(|frame| frame.voiced || frame.f0_hz == 0.0)
    );
    let mut frequencies: Vec<f32> = analysis
        .pitch()
        .iter()
        .filter(|frame| frame.pitch_valid && frame.voiced)
        .map(|frame| frame.f0_hz)
        .collect();
    assert!(
        frequencies.len() > analysis.frames() / 2,
        "the stable voiced fixture needs a majority of valid frames"
    );
    frequencies.sort_by(f32::total_cmp);
    let median = frequencies[frequencies.len() / 2];
    // 50 cents distinguishes the intended pitch from a semitone/ octave wiring
    // error while allowing estimator variation on a synthetic source.
    assert!(
        (1200.0 * (median / 220.0).log2()).abs() < 50.0,
        "FCPE median: {median}"
    );
    let output = dsp.render(&analysis, 0.0, 417)?;
    assert_eq!(output.len(), audio.len());
    assert!(output.iter().all(|sample| sample.is_finite()));
    let shifted = dsp.render(&analysis, 3.0, 417)?;
    assert_eq!(shifted.len(), audio.len());
    assert!(shifted.iter().all(|sample| sample.is_finite()));
    let output_pitch = dsp.analyze(&output)?;
    let shifted_pitch = dsp.analyze(&shifted)?;
    let median_voiced = |analysis: &heron_revox_dsp::Analysis| {
        let mut frequencies: Vec<_> = analysis
            .pitch()
            .iter()
            .filter(|frame| frame.voiced && frame.pitch_valid)
            .map(|frame| frame.f0_hz)
            .collect();
        assert!(frequencies.len() > analysis.frames() / 2);
        frequencies.sort_by(f32::total_cmp);
        frequencies[frequencies.len() / 2]
    };
    let actual_shift_cents =
        1200.0 * (median_voiced(&shifted_pitch) / median_voiced(&output_pitch)).log2();
    assert!(
        (actual_shift_cents - 300.0).abs() < 50.0,
        "requested +300 cents, measured {actual_shift_cents}"
    );
    let silence = dsp.analyze(&vec![0.0; 12_001])?;
    assert!(
        silence
            .pitch()
            .iter()
            .all(|frame| !frame.voiced && frame.pitch_valid)
    );
    let silent_render = dsp.render(&silence, 0.0, 417)?;
    assert_eq!(silent_render.len(), silence.sample_count());
    assert!(silent_render.iter().all(|sample| sample.is_finite()));
    Ok(())
}
