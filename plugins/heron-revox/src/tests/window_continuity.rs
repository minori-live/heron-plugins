//! Short-time measurements retain each 160 ms join; a global F0 median would
//! hide phase resets and cancellation caused by independently synthesized windows.

use super::*;
use std::{
    error::Error,
    f64::consts::{PI, TAU},
    fs::{File, create_dir_all},
    io::{BufWriter, Write},
    path::Path,
    time::Duration,
};
use truce_test::{InputSource, driver};

const RATE: usize = 48_000;
const PHASE_RADIUS: usize = 768; // 32 ms Hann projection, about seven periods.
const PHASE_STEP: usize = 48; // 1 ms derivative, resolves the 40 ms crossfade.

#[derive(Clone, Copy)]
struct Measurement {
    sample: usize,
    cents: f64,
    magnitude: f64,
    join: bool,
}

#[derive(Clone, Copy)]
struct Distribution {
    median_abs: f64,
    p95_abs: f64,
    max_abs: f64,
}

fn tone(frequency: f64, samples: usize) -> Vec<f32> {
    (0..samples)
        .map(|sample| {
            let phase = TAU * frequency * sample as f64 / RATE as f64;
            (0.15 * phase.sin() + 0.07 * (2.0 * phase).sin() + 0.04 * (3.0 * phase).sin()) as f32
        })
        .collect()
}

fn carrier(audio: &[f32], center: usize, frequency: f64) -> (f64, f64) {
    let mut real = 0.0;
    let mut imaginary = 0.0;
    let mut weight_sum = 0.0;
    for (offset, sample) in audio[center - PHASE_RADIUS..center + PHASE_RADIUS]
        .iter()
        .enumerate()
    {
        let weight = 0.5 - 0.5 * (TAU * offset as f64 / (2 * PHASE_RADIUS) as f64).cos();
        let phase = TAU * frequency * (center - PHASE_RADIUS + offset) as f64 / RATE as f64;
        let value = f64::from(*sample) * weight;
        real += value * phase.cos();
        imaginary -= value * phase.sin();
        weight_sum += weight;
    }
    (
        imaginary.atan2(real),
        2.0 * real.hypot(imaginary) / weight_sum,
    )
}

fn phase_measurements(
    audio: &[f32],
    frequency: f64,
    start: usize,
    hop: usize,
    overlap: usize,
) -> Vec<Measurement> {
    let mut measurements = Vec::new();
    let (mut previous, _) = carrier(audio, start - PHASE_STEP, frequency);
    for sample in (start..audio.len() - PHASE_RADIUS).step_by(PHASE_STEP) {
        let (phase, magnitude) = carrier(audio, sample, frequency);
        let delta = (phase - previous + PI).rem_euclid(TAU) - PI;
        previous = phase;
        let hz = frequency + delta * RATE as f64 / (TAU * PHASE_STEP as f64);
        let position = sample % hop;
        // A projection centered just outside the crossfade still contains it.
        let join = position < overlap + PHASE_RADIUS || position > hop - PHASE_RADIUS;
        measurements.push(Measurement {
            sample,
            cents: 1200.0 * (hz / frequency).log2(),
            magnitude,
            join,
        });
    }
    measurements
}

fn distribution(measurements: &[Measurement], join: bool) -> Distribution {
    let mut values: Vec<_> = measurements
        .iter()
        .filter(|measurement| measurement.join == join)
        .map(|measurement| measurement.cents.abs())
        .collect();
    assert!(!values.is_empty());
    assert!(values.iter().all(|value| value.is_finite()));
    values.sort_by(f64::total_cmp);
    Distribution {
        median_abs: values[values.len() / 2],
        p95_abs: values[(values.len() - 1) * 95 / 100],
        max_abs: values[values.len() - 1],
    }
}

fn join_amplitude_ratio(measurements: &[Measurement]) -> f64 {
    let mut steady: Vec<_> = measurements
        .iter()
        .filter(|measurement| !measurement.join)
        .map(|measurement| measurement.magnitude)
        .collect();
    steady.sort_by(f64::total_cmp);
    let minimum_join = measurements
        .iter()
        .filter(|measurement| measurement.join)
        .map(|measurement| measurement.magnitude)
        .min_by(f64::total_cmp)
        .unwrap();
    minimum_join / steady[steady.len() / 2]
}

fn period_hz(audio: &[f32], center: usize, frequency: f64) -> f64 {
    // Autocorrelation estimates a waveform period without assuming its phase.
    // It cannot replace the phase derivative: a reset may preserve local period
    // while corrupting the accumulated phase and crossfade.
    let radius = 480; // 20 ms, over four periods of these fixtures.
    let range = (RATE as f64 / (frequency * 1.2)).floor() as usize
        ..=(RATE as f64 / (frequency * 0.8)).ceil() as usize;
    let correlation = |lag: usize| {
        let mut xy = 0.0;
        let mut xx = 0.0;
        let mut yy = 0.0;
        for offset in 0..2 * radius {
            let a = f64::from(audio[center - radius + offset]);
            let b = f64::from(audio[center - radius + offset + lag]);
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        xy / (xx * yy).sqrt()
    };
    let lag = range
        .max_by(|a, b| correlation(*a).total_cmp(&correlation(*b)))
        .unwrap();
    let before = correlation(lag - 1);
    let middle = correlation(lag);
    let after = correlation(lag + 1);
    let fractional = 0.5 * (before - after) / (before - 2.0 * middle + after);
    RATE as f64 / (lag as f64 + fractional)
}

fn write_wav(path: &Path, audio: &[f32]) -> std::io::Result<()> {
    let mut file = BufWriter::new(File::create(path)?);
    let bytes = std::mem::size_of_val(audio) as u32;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&3_u16.to_le_bytes())?; // IEEE float.
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&(RATE as u32).to_le_bytes())?;
    file.write_all(&(RATE as u32 * 4).to_le_bytes())?;
    file.write_all(&4_u16.to_le_bytes())?;
    file.write_all(&32_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&bytes.to_le_bytes())?;
    for sample in audio {
        file.write_all(&sample.to_le_bytes())?;
    }
    file.flush()
}

fn write_report(
    path: &Path,
    frequency: f64,
    latency: usize,
    geometry: stream::Geometry,
    output: &[f32],
    measurements: &[Measurement],
) -> std::io::Result<()> {
    let mut file = BufWriter::new(File::create(path)?);
    let join = distribution(measurements, true);
    let steady = distribution(measurements, false);
    writeln!(
        file,
        "{{\"input_hz\":{frequency},\"sample_rate\":{RATE},\"shift_semitones\":0,\"latency_samples\":{latency},\"hop_samples\":{},\"overlap_samples\":{},\"phase_window_samples\":{},\"phase_step_samples\":{PHASE_STEP},",
        geometry.hop,
        geometry.overlap,
        PHASE_RADIUS * 2
    )?;
    writeln!(
        file,
        "\"join_abs_cents\":{{\"median\":{},\"p95\":{},\"max\":{}}},\"steady_abs_cents\":{{\"median\":{},\"p95\":{},\"max\":{}}},",
        join.median_abs,
        join.p95_abs,
        join.max_abs,
        steady.median_abs,
        steady.p95_abs,
        steady.max_abs
    )?;
    writeln!(
        file,
        "\"join_min_amplitude_to_steady_median\":{},",
        join_amplitude_ratio(measurements)
    )?;
    writeln!(file, "\"measurements\":[")?;
    for (index, measurement) in measurements.iter().enumerate() {
        // Independent period checks at 10 ms spacing; null at other samples.
        let period = if index.is_multiple_of(10) {
            period_hz(output, measurement.sample, frequency).to_string()
        } else {
            "null".to_owned()
        };
        writeln!(
            file,
            "{}{{\"source_sample\":{},\"join\":{},\"phase_cents\":{},\"fundamental_amplitude\":{},\"period_hz\":{period}}}",
            if index == 0 { "" } else { "," },
            measurement.sample,
            measurement.join,
            measurement.cents,
            measurement.magnitude
        )?;
    }
    writeln!(file, "]}}")?;
    file.flush()
}

#[test]
fn short_time_phase_measurement_distinguishes_continuity_from_a_reset() {
    let frequency = 221.875; // 35.5 cycles in each 160 ms hop.
    let geometry = stream::Geometry::new(RATE as f64).unwrap();
    let signal = tone(frequency, RATE);
    let continuous =
        phase_measurements(&signal, frequency, RATE / 4, geometry.hop, geometry.overlap);
    assert!(distribution(&continuous, true).max_abs < 2.0);
    assert!(distribution(&continuous, false).max_abs < 2.0);

    let reset: Vec<_> = (0..RATE)
        .map(|sample| signal[sample % geometry.hop])
        .collect();
    let discontinuous =
        phase_measurements(&reset, frequency, RATE / 4, geometry.hop, geometry.overlap);
    assert!(distribution(&discontinuous, true).max_abs > 100.0);
    assert!(distribution(&discontinuous, false).max_abs < 2.0);
}

#[test]
#[ignore = "runs embedded models through the plugin and writes short-time window evidence"]
fn embedded_models_keep_pitch_continuous_at_every_window_join() -> Result<(), Box<dyn Error>> {
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/revox/window-continuity");
    create_dir_all(&directory)?;
    let geometry = stream::Geometry::new(RATE as f64).unwrap();
    for frequency in [220.0, 221.875] {
        let signal = tone(frequency, RATE * 6);
        write_wav(&directory.join(format!("input-{frequency}.wav")), &signal)?;
        let result = driver!(Plugin)
            .sample_rate(RATE as f64)
            .channels(1)
            .block_size(257)
            .process_mode(ProcessMode::Offline)
            .duration(Duration::from_secs(6))
            .input(InputSource::Buffer(vec![signal]))
            .run();
        assert_eq!(
            result.plugin.params().status.state(),
            ProcessingState::Ready
        );
        let latency = result.plugin.latency() as usize;
        assert_eq!(latency, geometry.latency);
        assert!(
            result.output[0][..latency]
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert!(result.output[0].iter().all(|sample| sample.is_finite()));
        let output = &result.output[0][latency..];
        write_wav(&directory.join(format!("output-{frequency}.wav")), output)?;
        let measurements = phase_measurements(
            output,
            frequency,
            geometry.hop * 8,
            geometry.hop,
            geometry.overlap,
        );
        write_report(
            &directory.join(format!("report-{frequency}.json")),
            frequency,
            latency,
            geometry,
            output,
            &measurements,
        )?;
        let join = distribution(&measurements, true);
        let steady = distribution(&measurements, false);
        println!(
            "{frequency} Hz: join |cents| median={:.3}, p95={:.3}, max={:.3}; steady median={:.3}, p95={:.3}, max={:.3}",
            join.median_abs,
            join.p95_abs,
            join.max_abs,
            steady.median_abs,
            steady.p95_abs,
            steady.max_abs
        );
        // The calibrated harmonic fixture contributes less than 2 cents of
        // projection error. Five cents allows errors on both sides plus f32
        // synthesis variation, without allowing joins to add a pitch excursion.
        assert!(
            join.max_abs <= steady.max_abs + 5.0,
            "{frequency} Hz window joins introduced phase excursions: join maximum {:.3} cents, steady maximum {:.3} cents",
            join.max_abs,
            steady.max_abs
        );
        // On a steady tone, stitching must not cancel its fundamental by more
        // than half amplitude (6 dB) relative to the unstitched steady regions.
        // This catches the near-antiphase collapse without constraining timbre.
        let amplitude_ratio = join_amplitude_ratio(&measurements);
        assert!(
            amplitude_ratio >= 0.5,
            "{frequency} Hz window joins cancelled the fundamental: minimum join amplitude / steady median = {amplitude_ratio:.3}"
        );
    }
    Ok(())
}
