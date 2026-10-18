//! FCPE pitch analysis and Revox resynthesis for non-real-time inference workers.
//!
//! Model loading and inference allocate and may block. These APIs must not run
//! inside a host audio callback. Heron Revox owns the streaming plug-in adapter.

pub mod fcpe_features;
pub mod features;
pub mod fir;
pub mod noise;
mod pipeline;
mod pitch;
mod resample;
pub mod runtime;
mod streaming;

pub use pipeline::{
    Analysis, DspError, ModelBackends, PLUGIN_CONTEXT_SAMPLES, PLUGIN_HOP_SAMPLES,
    PLUGIN_OVERLAP_SAMPLES, PLUGIN_WINDOW_SAMPLES, RevoxDsp,
};
pub use pitch::PitchFrame;
