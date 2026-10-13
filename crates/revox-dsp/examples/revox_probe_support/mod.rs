use std::{
    error::Error,
    path::{Path, PathBuf},
};

use heron_revox_dsp::PitchFrame;
use serde::Serialize;

pub const SAMPLE_RATE: u32 = 48_000;

pub struct Options {
    pub input: Option<PathBuf>,
    pub output: PathBuf,
    pub fcpe: PathBuf,
    pub revox: PathBuf,
    pub seconds: f64,
    pub semitones: f32,
    pub seed: u64,
    pub chunk_ms: Option<f64>,
}

impl Options {
    pub fn parse() -> Result<Self, Box<dyn Error>> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut options = Self {
            input: None,
            output: root.join("target/revox/probe"),
            fcpe: root.join("target/revox/fcpe.onnx"),
            revox: root.join("target/revox/models/revox.onnx"),
            seconds: 2.0,
            semitones: 0.0,
            seed: 42,
            chunk_ms: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(argument) = args.next() {
            if argument == "--help" || argument == "-h" {
                println!(
                    "revox_probe [--input mono48k.wav] [--semitones N] [--seconds N] [--seed N] [--chunk-ms N] [--fcpe PATH] [--revox PATH] [--output-dir PATH]"
                );
                std::process::exit(0);
            }
            let value = args
                .next()
                .ok_or_else(|| format!("missing value for {argument}"))?;
            match argument.as_str() {
                "--input" => options.input = Some(value.into()),
                "--output-dir" => options.output = value.into(),
                "--fcpe" => options.fcpe = value.into(),
                "--revox" => options.revox = value.into(),
                "--seconds" => options.seconds = value.parse()?,
                "--semitones" => options.semitones = value.parse()?,
                "--seed" => options.seed = value.parse()?,
                "--chunk-ms" => options.chunk_ms = Some(value.parse()?),
                _ => return Err(format!("invalid argument {argument} {value}; use --help").into()),
            }
        }
        if !options.seconds.is_finite() || !(0.1..=60.0).contains(&options.seconds) {
            return Err("--seconds must be between 0.1 and 60".into());
        }
        if !options.semitones.is_finite() {
            return Err("--semitones must be finite".into());
        }
        if options
            .chunk_ms
            .is_some_and(|ms| !ms.is_finite() || !(20.0..=10_000.0).contains(&ms))
        {
            return Err("--chunk-ms must be between 20 and 10000".into());
        }
        Ok(options)
    }
}

pub fn harmonic_voice(seconds: f64) -> Vec<f32> {
    let count = (seconds * f64::from(SAMPLE_RATE)).round() as usize;
    let normalization = (1..=8).map(|partial| 1.0 / f64::from(partial)).sum::<f64>();
    (0..count)
        .map(|index| {
            let phase = std::f64::consts::TAU * 220.0 * index as f64 / f64::from(SAMPLE_RATE);
            let value = (1..=8)
                .map(|partial| (phase * f64::from(partial)).sin() / f64::from(partial))
                .sum::<f64>();
            let edge = index.min(count - 1 - index) as f64 / (0.02 * f64::from(SAMPLE_RATE));
            let fade = 0.5 - 0.5 * (std::f64::consts::PI * edge.min(1.0)).cos();
            (value * 0.35 * fade / normalization) as f32
        })
        .collect()
}

pub fn read_wav(path: &Path) -> Result<Vec<f32>, Box<dyn Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != SAMPLE_RATE {
        return Err("input WAV must be mono at 48000 Hz".into());
    }
    let audio = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 2.0_f32.powi(i32::from(spec.bits_per_sample) - 1);
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / scale))
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    if audio.is_empty() || audio.iter().any(|sample| !sample.is_finite()) {
        return Err("input WAV must contain finite samples".into());
    }
    Ok(audio)
}

pub fn write_wav(path: &Path, audio: &[f32]) -> Result<(), hound::Error> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for &sample in audio {
        writer.write_sample(sample)?;
    }
    writer.finalize()
}

#[derive(Serialize)]
pub struct AudioStats {
    samples: usize,
    rms: f64,
    peak: f32,
    all_finite: bool,
}

pub fn audio_stats(audio: &[f32]) -> AudioStats {
    AudioStats {
        samples: audio.len(),
        rms: (audio
            .iter()
            .map(|&sample| f64::from(sample).powi(2))
            .sum::<f64>()
            / audio.len() as f64)
            .sqrt(),
        peak: audio
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs())),
        all_finite: audio.iter().all(|sample| sample.is_finite()),
    }
}

#[derive(Serialize)]
pub struct PitchStats {
    frames: usize,
    voiced_frames: usize,
    valid_frames: usize,
    pub median_hz: Option<f64>,
    median_confidence: Option<f64>,
}

pub fn pitch_stats(pitch: &[PitchFrame]) -> PitchStats {
    let mut hz: Vec<_> = pitch
        .iter()
        .filter(|frame| frame.voiced && frame.pitch_valid)
        .map(|frame| f64::from(frame.f0_hz))
        .collect();
    let mut confidence: Vec<_> = pitch
        .iter()
        .map(|frame| f64::from(frame.confidence))
        .collect();
    hz.sort_by(f64::total_cmp);
    confidence.sort_by(f64::total_cmp);
    PitchStats {
        frames: pitch.len(),
        voiced_frames: pitch.iter().filter(|frame| frame.voiced).count(),
        valid_frames: pitch.iter().filter(|frame| frame.pitch_valid).count(),
        median_hz: hz.get(hz.len() / 2).copied(),
        median_confidence: confidence.get(confidence.len() / 2).copied(),
    }
}
