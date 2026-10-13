//! FCPE pitch analysis and Revox resynthesis for non-real-time inference workers.
//!
//! Model loading and inference allocate and may block. These APIs must not run
//! inside a host audio callback. The DSP experiment does not yet expose a plug-in.

pub mod fcpe_features;
pub mod features;
pub mod noise;
mod pipeline;
mod pitch;
mod resample;
pub mod runtime;

pub use pipeline::{Analysis, DspError, ModelBackends, RevoxDsp};
pub use pitch::PitchFrame;
