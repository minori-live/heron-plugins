//! The allocating analysis/resynthesis path shared by DSP probes and plug-in workers.

use std::{borrow::Cow, path::Path};

use ort::{session::Session, value::TensorRef};
use serde::Serialize;

use crate::{
    fcpe_features::FcpeFeatures,
    features::{FeatureError, HOP_SIZE, MEL_BINS, MelFeatures, RevoxFeatures, SPECTRUM_BINS},
    noise::seeded_noise,
    pitch::{self, PITCH_BINS, PitchError, PitchFrame},
    resample::PitchResampler,
    runtime::{BackendReport, OrtRuntime, RuntimeError},
};

/// Fixed 2.12 second context window used by the mono/stereo plug-in graphs.
pub const PLUGIN_WINDOW_SAMPLES: usize = 101_760;
const PLUGIN_REVOX_FRAMES: usize = PLUGIN_WINDOW_SAMPLES / HOP_SIZE;
const PLUGIN_FCPE_FRAMES: usize = PLUGIN_WINDOW_SAMPLES / 3 / crate::fcpe_features::HOP_SIZE + 1;

#[derive(Debug, thiserror::Error)]
pub enum DspError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Features(#[from] FeatureError),
    #[error(transparent)]
    Pitch(#[from] PitchError),
    #[error("model inference failed")]
    Inference(#[from] ort::Error),
    #[error("{model} model does not match the expected input/output contract")]
    ModelContract { model: &'static str },
    #[error("{model} output {output} has an unexpected tensor shape")]
    OutputShape {
        model: &'static str,
        output: &'static str,
    },
    #[error("pitch shift must be finite and within -13 to +13 semitones")]
    PitchShift,
    #[error("model sessions support one or two channels")]
    UnsupportedChannels,
    #[error("audio channels or window lengths do not match the prepared model shape")]
    InputShape,
}

/// Input-derived features on one common frame grid; immutable across pitch shifts.
#[derive(Debug)]
pub struct Analysis {
    mel: MelFeatures,
    pitch: Vec<PitchFrame>,
    sample_count: usize,
}

impl Analysis {
    pub fn pitch(&self) -> &[PitchFrame] {
        &self.pitch
    }
    pub fn frames(&self) -> usize {
        self.mel.frames
    }
    pub fn sample_count(&self) -> usize {
        self.sample_count
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelBackends {
    pub fcpe: BackendReport,
    pub revox: BackendReport,
}

/// One pair of model sessions, owned by one non-real-time caller.
///
/// The sample rate is fixed at 48 kHz. Each batch item is one audio channel. Calls are independent windows: both
/// models are noncausal, FCPE normalization depends on the entire window, and
/// Revox's source phase resets per inference. This is not a seamless streaming
/// processor and must not be called from a host's audio callback.
pub struct RevoxDsp {
    fcpe: Session,
    revox: Session,
    fcpe_features: FcpeFeatures,
    revox_features: RevoxFeatures,
    resampler: PitchResampler,
    backends: ModelBackends,
    channels: usize,
    fixed_window: bool,
}

impl RevoxDsp {
    /// Loads the pinned mel-to-probabilities FCPE export and Revox 1.5 graph.
    ///
    /// # Errors
    /// Fails on unavailable models/runtime or a mismatched model interface.
    pub fn load(
        runtime: &OrtRuntime,
        fcpe_path: impl AsRef<Path>,
        revox_path: impl AsRef<Path>,
    ) -> Result<Self, DspError> {
        let (fcpe, fcpe_backend) = runtime.create_session(fcpe_path)?;
        let (revox, revox_backend) = runtime.create_session(revox_path)?;
        Self::from_sessions(fcpe, fcpe_backend, revox, revox_backend, 1, false)
    }

    /// Loads embedded FCPE and Revox models without reading model files.
    ///
    /// Sessions retain their environment and parsed model data, so the supplied
    /// runtime handle and bytes need not outlive this processor.
    ///
    /// # Errors
    /// Fails on unavailable runtime, invalid models, or a mismatched interface.
    pub fn from_memory(
        runtime: &OrtRuntime,
        fcpe_model: &[u8],
        revox_model: &[u8],
    ) -> Result<Self, DspError> {
        let (fcpe, fcpe_backend) = runtime.create_session_from_memory(fcpe_model)?;
        let (revox, revox_backend) = runtime.create_session_from_memory(revox_model)?;
        Self::from_sessions(fcpe, fcpe_backend, revox, revox_backend, 1, false)
    }

    /// Loads the plug-in graphs with a concrete mono or stereo input shape.
    ///
    /// Time is fixed in the exported graphs; ORT specializes their shared `batch`
    /// symbol before compiling execution providers. Only this channel layout's
    /// sessions are constructed, using one embedded copy of each model's weights.
    /// Loading and warmup belong on the worker, outside the audio callback.
    ///
    /// # Errors
    /// Rejects unsupported layouts, unavailable runtime, and graphs whose concrete
    /// input shapes do not match the plug-in's prepared window and channel count.
    pub fn from_memory_fixed(
        runtime: &OrtRuntime,
        fcpe_model: &[u8],
        revox_model: &[u8],
        channels: usize,
    ) -> Result<Self, DspError> {
        if !(1..=2).contains(&channels) {
            return Err(DspError::UnsupportedChannels);
        }
        let (fcpe, fcpe_backend) =
            runtime.create_session_from_memory_with_batch(fcpe_model, channels)?;
        let (revox, revox_backend) =
            runtime.create_session_from_memory_with_batch(revox_model, channels)?;
        verify_fixed_inputs(&fcpe, &revox, channels)?;
        Self::from_sessions(fcpe, fcpe_backend, revox, revox_backend, channels, true)
    }

    fn from_sessions(
        fcpe: Session,
        fcpe_backend: BackendReport,
        revox: Session,
        revox_backend: BackendReport,
        channels: usize,
        fixed_window: bool,
    ) -> Result<Self, DspError> {
        verify_names(&fcpe, "FCPE", &["mel"], &["probabilities"])?;
        verify_names(
            &revox,
            "Revox",
            &["mel", "f0_hz", "voiced", "pitch_valid", "noise"],
            &["real", "imag"],
        )?;
        Ok(Self {
            fcpe,
            revox,
            fcpe_features: FcpeFeatures::new(),
            revox_features: RevoxFeatures::new(),
            resampler: PitchResampler::new(),
            channels,
            fixed_window,
            backends: ModelBackends {
                fcpe: fcpe_backend,
                revox: revox_backend,
            },
        })
    }

    /// Registered provider policy for each graph, not a GPU utilization measurement.
    pub fn backends(&self) -> &ModelBackends {
        &self.backends
    }

    /// Extracts Revox Mel and FCPE pitch, aligned by physical frame center.
    ///
    /// # Errors
    /// Rejects invalid PCM, malformed model outputs, and inference failures.
    pub fn analyze(&mut self, audio48k: &[f32]) -> Result<Analysis, DspError> {
        self.analyze_channels(&[audio48k])?
            .pop()
            .ok_or(DspError::InputShape)
    }

    /// Analyzes channels from the same time window in a single FCPE invocation.
    /// Channel order is preserved; normalization and pitch decoding stay per item.
    ///
    /// # Errors
    /// Rejects the wrong channel count, unequal/wrong fixed window lengths,
    /// invalid PCM, malformed model outputs, and inference failures.
    pub fn analyze_channels(&mut self, audio48k: &[&[f32]]) -> Result<Vec<Analysis>, DspError> {
        validate_batch_shape(
            self.channels,
            self.fixed_window,
            audio48k.iter().map(|audio| audio.len()),
        )?;
        let mut mel = Vec::with_capacity(self.channels);
        let mut audio16k = Vec::with_capacity(self.channels);
        let mut fcpe_mel = Vec::with_capacity(self.channels);
        for audio in audio48k {
            // Validate PCM before the resampler and before model inference.
            mel.push(self.revox_features.analyze(audio)?);
            let resampled = self.resampler.process(audio);
            fcpe_mel.push(self.fcpe_features.analyze(&resampled)?);
            audio16k.push(resampled);
        }
        let frames = fcpe_mel[0].frames;
        let values = concatenate(fcpe_mel.iter().map(|mel| mel.values.as_slice()));
        let input =
            TensorRef::from_array_view(([self.channels, frames, MEL_BINS], values.as_ref()))?;
        let outputs = self.fcpe.run(ort::inputs!["mel" => input])?;
        let probabilities = output_tensor(
            &outputs,
            "FCPE",
            "probabilities",
            &[self.channels as i64, frames as i64, PITCH_BINS as i64],
        )?;
        mel.into_iter()
            .zip(&audio16k)
            .zip(probabilities.chunks_exact(frames * PITCH_BINS))
            .map(|((mel, audio), probabilities)| {
                let raw_pitch = pitch::decode(probabilities, audio)?;
                let pitch = pitch::align_to_revox(&raw_pitch, mel.frames);
                Ok(Analysis {
                    mel,
                    pitch,
                    sample_count: audio48k[0].len(),
                })
            })
            .collect()
    }

    /// Resynthesizes immutable Mel conditions at the requested relative pitch.
    ///
    /// Zero semitones still performs vocoder synthesis. The source-noise seed
    /// makes comparisons repeatable. No clipping or loudness normalization occurs.
    /// The accepted range is ±13 semitones, including the plug-in's coarse ±12
    /// semitones and fine ±100 cents controls.
    ///
    /// # Errors
    /// Rejects invalid shifts, inference failures and malformed/non-finite spectra.
    pub fn render(
        &mut self,
        analysis: &Analysis,
        semitones: f32,
        seed: u64,
    ) -> Result<Vec<f32>, DspError> {
        self.render_batch(std::slice::from_ref(analysis), semitones, seed)?
            .pop()
            .ok_or(DspError::InputShape)
    }

    /// Resynthesizes one prepared analysis per channel in one Revox invocation.
    /// The same per-channel noise seed preserves independent mono rendering.
    ///
    /// # Errors
    /// Rejects mismatched batch/window shapes, invalid pitch shifts, model failures
    /// and malformed/non-finite spectra.
    pub fn render_batch(
        &mut self,
        analysis: &[Analysis],
        semitones: f32,
        seed: u64,
    ) -> Result<Vec<Vec<f32>>, DspError> {
        if !semitones.is_finite() || !(-13.0..=13.0).contains(&semitones) {
            return Err(DspError::PitchShift);
        }
        validate_batch_shape(
            self.channels,
            self.fixed_window,
            analysis.iter().map(Analysis::sample_count),
        )?;
        let frames = analysis[0].frames();
        let ratio = (semitones / 12.0).exp2();
        let mel = concatenate(
            analysis
                .iter()
                .map(|analysis| analysis.mel.values.as_slice()),
        );
        let pitch = analysis.iter().flat_map(|analysis| &analysis.pitch);
        let f0: Vec<_> = pitch
            .clone()
            .map(|frame| {
                if frame.voiced && frame.pitch_valid {
                    frame.f0_hz * ratio
                } else {
                    0.0
                }
            })
            .collect();
        let voiced: Vec<_> = pitch.clone().map(|frame| frame.voiced).collect();
        let valid: Vec<_> = pitch.map(|frame| frame.pitch_valid).collect();
        let one_noise = seeded_noise(seed, frames * HOP_SIZE);
        let noise = concatenate((0..self.channels).map(|_| one_noise.as_slice()));
        let outputs = self.revox.run(ort::inputs![
            "mel" => TensorRef::from_array_view(([self.channels, MEL_BINS, frames], mel.as_ref()))?,
            "f0_hz" => TensorRef::from_array_view(([self.channels, frames], f0.as_slice()))?,
            "voiced" => TensorRef::from_array_view(([self.channels, frames], voiced.as_slice()))?,
            "pitch_valid" => TensorRef::from_array_view(([self.channels, frames], valid.as_slice()))?,
            "noise" => TensorRef::from_array_view(([self.channels, frames * HOP_SIZE], noise.as_ref()))?,
        ])?;
        let shape = [self.channels as i64, SPECTRUM_BINS as i64, frames as i64];
        let real = output_tensor(&outputs, "Revox", "real", &shape)?;
        let imag = output_tensor(&outputs, "Revox", "imag", &shape)?;
        let stride = SPECTRUM_BINS * frames;
        real.chunks_exact(stride)
            .zip(imag.chunks_exact(stride))
            .zip(analysis)
            .map(|((real, imag), analysis)| {
                Ok(self
                    .revox_features
                    .synthesize(real, imag, frames, analysis.sample_count)?)
            })
            .collect()
    }

    /// Runs the complete fixed-window channel batch on the inference worker.
    ///
    /// # Errors
    /// Propagates input validation, model inference and synthesis errors.
    pub fn render_channels(
        &mut self,
        audio48k: &[Vec<f32>],
        semitones: f32,
        seed: u64,
    ) -> Result<Vec<Vec<f32>>, DspError> {
        let inputs: Vec<_> = audio48k.iter().map(Vec::as_slice).collect();
        let analysis = self.analyze_channels(&inputs)?;
        self.render_batch(&analysis, semitones, seed)
    }
}

// Batch=1 borrows the existing per-channel buffer; stereo alone needs packing.
fn concatenate<'a>(mut channels: impl ExactSizeIterator<Item = &'a [f32]>) -> Cow<'a, [f32]> {
    if channels.len() == 1 {
        Cow::Borrowed(channels.next().unwrap_or_default())
    } else {
        Cow::Owned(channels.flatten().copied().collect())
    }
}

fn validate_batch_shape(
    channels: usize,
    fixed_window: bool,
    mut samples: impl ExactSizeIterator<Item = usize>,
) -> Result<(), DspError> {
    if samples.len() != channels || channels == 0 {
        return Err(DspError::InputShape);
    }
    let first = samples.next().ok_or(DspError::InputShape)?;
    if first == 0 && !fixed_window {
        return Err(FeatureError::EmptyInput.into());
    }
    if first == 0
        || fixed_window && first != PLUGIN_WINDOW_SAMPLES
        || samples.any(|count| count != first)
    {
        return Err(DspError::InputShape);
    }
    Ok(())
}

fn verify_fixed_inputs(fcpe: &Session, revox: &Session, channels: usize) -> Result<(), DspError> {
    let batch = channels as i64;
    let frames = PLUGIN_REVOX_FRAMES as i64;
    for (session, model, name, expected) in [
        (
            fcpe,
            "FCPE",
            "mel",
            vec![batch, PLUGIN_FCPE_FRAMES as i64, MEL_BINS as i64],
        ),
        (revox, "Revox", "mel", vec![batch, MEL_BINS as i64, frames]),
        (revox, "Revox", "f0_hz", vec![batch, frames]),
        (revox, "Revox", "voiced", vec![batch, frames]),
        (revox, "Revox", "pitch_valid", vec![batch, frames]),
        (
            revox,
            "Revox",
            "noise",
            vec![batch, PLUGIN_WINDOW_SAMPLES as i64],
        ),
    ] {
        let shape = session
            .inputs()
            .iter()
            .find(|input| input.name() == name)
            .and_then(|input| input.dtype().tensor_shape());
        if shape.is_none_or(|shape| &**shape != expected.as_slice()) {
            return Err(DspError::ModelContract { model });
        }
    }
    Ok(())
}

fn verify_names(
    session: &Session,
    model: &'static str,
    inputs: &[&str],
    outputs: &[&str],
) -> Result<(), DspError> {
    if session.inputs().len() != inputs.len()
        || session.outputs().len() != outputs.len()
        || inputs
            .iter()
            .any(|name| !session.inputs().iter().any(|input| input.name() == *name))
        || outputs.iter().any(|name| {
            !session
                .outputs()
                .iter()
                .any(|output| output.name() == *name)
        })
    {
        return Err(DspError::ModelContract { model });
    }
    Ok(())
}

fn output_tensor<'a>(
    outputs: &'a ort::session::SessionOutputs<'_>,
    model: &'static str,
    output: &'static str,
    expected: &[i64],
) -> Result<&'a [f32], DspError> {
    let value = outputs
        .get(output)
        .ok_or(DspError::ModelContract { model })?;
    let (shape, data) = value.try_extract_tensor::<f32>()?;
    if &**shape != expected {
        return Err(DspError::OutputShape { model, output });
    }
    Ok(data)
}
