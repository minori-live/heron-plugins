//! The two stable automation slots; the fine control adds to the coarse shift.

use std::sync::Arc;
use truce::prelude::*;

use crate::{ProcessingStatus, editor::EditorState};

#[derive(Params)]
pub struct RevoxParams {
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
    #[skip]
    pub status: Arc<ProcessingStatus>,
    #[skip]
    pub editor_state: Arc<EditorState>,
}

impl RevoxParams {
    pub(crate) fn pitch_shift(&self) -> f32 {
        self.semitones.value() as f32 + self.cents.value() as f32 / 100.0
    }
}
