//! EQ-only signal colors; editor chrome and controls use the suite foundations.

pub use heron_plugin_ui::style::{active_button, chrome_button, panel};
pub use heron_plugin_ui::{palette, theme};
use truce_iced::iced::Color;

pub fn spectrum_pre() -> Color {
    Color::from_rgb8(127, 132, 140)
}

pub fn spectrum_post() -> Color {
    Color::from_rgb8(180, 204, 216)
}

pub fn band_color(index: usize) -> Color {
    let colors = palette();
    [
        Color::from_rgb8(111, 205, 150),
        colors.midi,
        colors.warning,
        colors.focus,
        colors.success,
        colors.danger,
    ][index % 6]
}
