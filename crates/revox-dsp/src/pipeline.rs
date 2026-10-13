//! The allocating analysis/resynthesis path shared by DSP probes and plug-in workers.

use std::path::Path;

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
/// The sample rate is fixed at 48 kHz mono. Calls are independent windows: both
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
        Self::from_sessions(fcpe, fcpe_backend, revox, revox_backend)
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
        Self::from_sessions(fcpe, fcpe_backend, revox, revox_backend)
    }

    fn from_sessions(
        fcpe: Session,
        fcpe_backend: BackendReport,
        revox: Session,
        revox_backend: BackendReport,
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
        // Validate and analyze before resampling so non-finite PCM is rejected.
        let mel = self.revox_features.analyze(audio48k)?;
        let audio16k = self.resampler.process(audio48k);
        let fcpe_mel = self.fcpe_features.analyze(&audio16k)?;
        let input = TensorRef::from_array_view((
            [1, fcpe_mel.frames, MEL_BINS],
            fcpe_mel.values.as_slice(),
        ))?;
        let outputs = self.fcpe.run(ort::inputs!["mel" => input])?;
        let probabilities = output_tensor(
            &outputs,
            "FCPE",
            "probabilities",
            &[1, fcpe_mel.frames as i64, PITCH_BINS as i64],
        )?;
        let raw_pitch = pitch::decode(probabilities, &audio16k)?;
        let pitch = pitch::align_to_revox(&raw_pitch, mel.frames);
        Ok(Analysis {
            mel,
            pitch,
            sample_count: audio48k.len(),
        })
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
        if !semitones.is_finite() || !(-13.0..=13.0).contains(&semitones) {
            return Err(DspError::PitchShift);
        }
        let frames = analysis.frames();
        let ratio = (semitones / 12.0).exp2();
        let f0: Vec<_> = analysis
            .pitch
            .iter()
            .map(|frame| {
                if frame.voiced && frame.pitch_valid {
                    frame.f0_hz * ratio
                } else {
                    0.0
                }
            })
            .collect();
        let voiced: Vec<_> = analysis.pitch.iter().map(|frame| frame.voiced).collect();
        let valid: Vec<_> = analysis
            .pitch
            .iter()
            .map(|frame| frame.pitch_valid)
            .collect();
        let noise = seeded_noise(seed, frames * HOP_SIZE);
        let outputs = self.revox.run(ort::inputs![
            "mel" => TensorRef::from_array_view(([1, MEL_BINS, frames], analysis.mel.values.as_slice()))?,
            "f0_hz" => TensorRef::from_array_view(([1, frames], f0.as_slice()))?,
            "voiced" => TensorRef::from_array_view(([1, frames], voiced.as_slice()))?,
            "pitch_valid" => TensorRef::from_array_view(([1, frames], valid.as_slice()))?,
            "noise" => TensorRef::from_array_view(([1, frames * HOP_SIZE], noise.as_slice()))?,
        ])?;
        let shape = [1, SPECTRUM_BINS as i64, frames as i64];
        let real = output_tensor(&outputs, "Revox", "real", &shape)?;
        let imag = output_tensor(&outputs, "Revox", "imag", &shape)?;
        Ok(self
            .revox_features
            .synthesize(real, imag, frames, analysis.sample_count)?)
    }
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
