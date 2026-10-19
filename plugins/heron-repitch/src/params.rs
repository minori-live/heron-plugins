//! Stable automation slots mirrored from Revox, plus formant, quality, and mix.

use std::sync::Arc;
use truce::prelude::*;

use crate::{ProcessingStatus, editor::EditorState};
use heron_rubberband::Quality;

#[derive(Params)]
pub struct RepitchParams {
    #[param(
        id = 0,
        name = "Semitones",
        range = "discrete(-12, 12)",
        unit = "st",
        default = 0
    )]
    pub semitones: IntParam,
    // Truce has no cents unit; the host parameter name and native readout name it.
    #[param(id = 1, name = "Cents", range = "discrete(-100, 100)", default = 0)]
    pub cents: IntParam,
    #[param(id = 2, name = "Formant", default = true)]
    pub formant: BoolParam,
    /// 0 = Faster (short window), 1 = Finer (medium window). LiveShifter is
    /// always R3-live; this selects the window trade-off without leaving that path.
    #[param(id = 3, name = "Quality", range = "discrete(0, 1)", default = 1)]
    pub quality: IntParam,
    /// Dry/wet mix: 0% = latency-aligned dry, 100% = fully wet (default).
    #[param(
        id = 4,
        name = "Mix",
        range = "linear(0, 100)",
        unit = "%",
        default = 100
    )]
    pub mix: FloatParam,
    #[skip]
    pub status: Arc<ProcessingStatus>,
    #[skip]
    pub editor_state: Arc<EditorState>,
}

impl RepitchParams {
    pub(crate) fn pitch_shift(&self) -> f32 {
        self.semitones.value() as f32 + self.cents.value() as f32 / 100.0
    }

    pub(crate) fn quality(&self) -> Quality {
        Quality::from_discrete(self.quality.value() as i32)
    }

    /// Wet fraction in \[0, 1\] from the 0–100% Mix parameter.
    pub(crate) fn wet_mix(&self) -> f32 {
        (self.mix.value() / 100.0).clamp(0.0, 1.0)
    }
}
