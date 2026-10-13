//! Build-verified models embedded in the plug-in binary.

use heron_revox_dsp::{DspError, RevoxDsp, runtime::OrtRuntime};

// Source revisions, checksums and licensing are retained in the model preparation
// scripts and release notices. These bytes are shared by all plug-in instances;
// each inference worker owns separate model sessions.
static FCPE: &[u8] = include_bytes!(env!("HERON_REVOX_FCPE_MODEL"));
static REVOX: &[u8] = include_bytes!(env!("HERON_REVOX_VOCODER_MODEL"));

/// Creates one worker's sessions from embedded bytes, outside the audio callback.
pub(crate) fn load(channels: usize) -> Result<RevoxDsp, DspError> {
    let runtime = OrtRuntime::new()?;
    RevoxDsp::from_memory_fixed(&runtime, FCPE, REVOX, channels)
}
