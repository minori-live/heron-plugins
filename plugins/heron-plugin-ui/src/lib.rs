//! Shared visual language for Heron's built-in iced plug-ins.

pub mod chrome;
mod meters;
pub mod rotary;
pub mod style;
mod widgets;

pub use meters::{LevelMeter, level_meter, level_meter_values};
pub use style::{palette, theme};
pub use widgets::{ParameterKnob, parameter_knob};

pub use heron_iced_ui::{self, Appearance, SemanticPalette, space, type_size};

/// Logical editor size for Heron Sine.
pub const SINE_EDITOR_SIZE: (u32, u32) = (520, 300);
/// Logical editor size for Heron Gain.
pub const GAIN_EDITOR_SIZE: (u32, u32) = (380, 260);
/// Logical editor size for Heron Metronome.
pub const METRONOME_EDITOR_SIZE: (u32, u32) = (600, 300);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_palette_retains_the_eq_neutral_surface_and_warm_accent() {
        use truce_iced::iced::Color;
        assert_eq!(palette().canvas, Color::from_rgb8(36, 36, 38));
        assert_eq!(palette().action, Color::from_rgb8(238, 226, 175));
        assert_ne!(palette().canvas, palette().surface);
    }
}
