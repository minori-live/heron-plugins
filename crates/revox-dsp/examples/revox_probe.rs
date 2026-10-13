//! Reproducible, non-real-time FCPE → Revox experiment.
//!
//! `cargo run --release -p heron-revox-dsp --example revox_probe -- --chunk-ms 320`
//! Add `--input vocal.wav` for mono 48 kHz audio, `--semitones 3` for transposition,
//! or `--chunk-ms 320` to benchmark independent windows. Window timing does not
//! establish seamless streaming or audio-callback safety.

mod revox_probe_support;

use std::{error::Error, fs, time::Instant};

use heron_revox_dsp::{RevoxDsp, runtime::OrtRuntime};
use revox_probe_support::{
    Options, SAMPLE_RATE, audio_stats, harmonic_voice, pitch_stats, read_wav, write_wav,
};
use serde_json::{Value, json};

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::parse()?;
    let audio = if let Some(path) = &options.input {
        read_wav(path)?
    } else {
        harmonic_voice(options.seconds)
    };
    fs::create_dir_all(&options.output)?;
    write_wav(&options.output.join("input.wav"), &audio)?;
    let runtime = OrtRuntime::new()?;
    eprintln!("Loading model sessions...");
    let start = Instant::now();
    let mut dsp = RevoxDsp::load(&runtime, &options.fcpe, &options.revox)?;
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    eprintln!("Analyzing and rendering...");
    let start = Instant::now();
    let analysis = dsp.analyze(&audio)?;
    let analysis_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let output = dsp.render(&analysis, options.semitones, options.seed)?;
    let render_ms = start.elapsed().as_secs_f64() * 1000.0;
    write_wav(&options.output.join("output.wav"), &output)?;
    let start = Instant::now();
    let output_analysis = dsp.analyze(&output)?;
    let output_analysis_ms = start.elapsed().as_secs_f64() * 1000.0;
    let input_pitch = pitch_stats(analysis.pitch());
    let output_pitch = pitch_stats(output_analysis.pitch());
    let drift_cents = input_pitch
        .median_hz
        .zip(output_pitch.median_hz)
        .map(|(input, output)| {
            1200.0 * (output / (input * 2.0_f64.powf(f64::from(options.semitones) / 12.0))).log2()
        });
    let chunk_benchmark = options
        .chunk_ms
        .map(|milliseconds| benchmark_windows(&mut dsp, &audio, &output, milliseconds, &options))
        .transpose()?;
    let seconds = audio.len() as f64 / f64::from(SAMPLE_RATE);
    let report = json!({
        "sample_rate": SAMPLE_RATE,
        "input_source": options.input,
        "generated_signal": options.input.is_none().then_some("220 Hz harmonic series, eight partials, 0.35 peak scale, 20 ms cosine fade"),
        "semitones": options.semitones,
        "seed": options.seed,
        "fcpe_model": options.fcpe,
        "revox_model": options.revox,
        "backends": dsp.backends(),
        "provider_note": "A registered provider is not proof of graph operator placement or GPU use.",
        "load_ms": load_ms,
        "analysis_ms": analysis_ms,
        "render_ms": render_ms,
        "output_reanalysis_ms": output_analysis_ms,
        "processing_real_time_factor": (analysis_ms + render_ms) / (seconds * 1000.0),
        "frames": analysis.frames(),
        "input": audio_stats(&audio),
        "output": audio_stats(&output),
        "input_pitch": input_pitch,
        "output_pitch": output_pitch,
        "median_pitch_drift_cents_from_requested_shift": drift_cents,
        "independent_window_benchmark": chunk_benchmark,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    fs::write(
        options.output.join("report.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    Ok(())
}

fn benchmark_windows(
    dsp: &mut RevoxDsp,
    audio: &[f32],
    whole_output: &[f32],
    milliseconds: f64,
    options: &Options,
) -> Result<Value, Box<dyn Error>> {
    let samples = (milliseconds * f64::from(SAMPLE_RATE) / 1000.0).round() as usize;
    let mut times = Vec::new();
    let mut processed_samples = 0;
    let mut concatenated: Vec<f32> = Vec::with_capacity(audio.len());
    let mut maximum_seam_jump = 0.0_f32;
    for (index, window) in audio.chunks(samples).enumerate() {
        let start = Instant::now();
        let analysis = dsp.analyze(window)?;
        let rendered = dsp.render(
            &analysis,
            options.semitones,
            options.seed.wrapping_add(index as u64),
        )?;
        if rendered.len() != window.len() || rendered.iter().any(|sample| !sample.is_finite()) {
            return Err("independent-window inference returned invalid audio".into());
        }
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        if let (Some(previous), Some(next)) = (concatenated.last(), rendered.first()) {
            maximum_seam_jump = maximum_seam_jump.max((next - previous).abs());
        }
        concatenated.extend_from_slice(&rendered);
        processed_samples += window.len();
    }
    let output_path = options.output.join("windows.wav");
    write_wav(&output_path, &concatenated)?;
    let difference_rms = (concatenated
        .iter()
        .zip(whole_output)
        .map(|(windowed, whole)| (f64::from(*windowed) - f64::from(*whole)).powi(2))
        .sum::<f64>()
        / concatenated.len() as f64)
        .sqrt();
    let whole_rms = (whole_output
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / whole_output.len() as f64)
        .sqrt();
    times.sort_by(f64::total_cmp);
    let total_ms: f64 = times.iter().sum();
    let duration_ms = processed_samples as f64 / f64::from(SAMPLE_RATE) * 1000.0;
    let percentile = |fraction: f64| times[((times.len() - 1) as f64 * fraction).ceil() as usize];
    Ok(json!({
        "note": "Independent windows reset model context and source phase and regenerate seeded noise. Difference RMS and seam jumps describe these outputs, not perceptual quality or end-to-end latency.",
        "window_ms": milliseconds,
        "windows": times.len(),
        "output_wav": output_path,
        "difference_rms_from_whole": difference_rms,
        "difference_rms_relative_to_whole": (whole_rms > 0.0).then_some(difference_rms / whole_rms),
        "maximum_seam_adjacent_sample_jump": maximum_seam_jump,
        "median_processing_ms": percentile(0.5),
        "p95_processing_ms": percentile(0.95),
        "maximum_processing_ms": times.last(),
        "real_time_factor": total_ms / duration_ms,
    }))
}
