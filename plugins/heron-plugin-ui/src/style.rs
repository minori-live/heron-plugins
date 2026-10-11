//! The shared plug-in appearance, derived from Heron EQ's native editor.

use iced_widget::Theme;
use truce_iced::iced::{
    Border, Color,
    widget::{button, container},
};

use crate::{Appearance, SemanticPalette};

/// Fixed neutral dark surfaces and warm accents used by every Heron plug-in.
#[must_use]
pub const fn palette() -> SemanticPalette {
    SemanticPalette {
        canvas: Color::from_rgb8(36, 36, 38),
        canvas_subtle: Color::from_rgb8(40, 40, 43),
        surface: Color::from_rgb8(43, 43, 46),
        surface_raised: Color::from_rgb8(52, 52, 56),
        control: Color::from_rgb8(52, 52, 56),
        control_hover: Color::from_rgb8(69, 69, 73),
        control_pressed: Color::from_rgb8(29, 29, 32),
        text: Color::from_rgb8(229, 228, 223),
        text_muted: Color::from_rgb8(160, 160, 161),
        text_subtle: Color::from_rgb8(114, 114, 117),
        border: Color::from_rgb8(68, 68, 71),
        border_strong: Color::from_rgb8(92, 92, 96),
        action: Color::from_rgb8(238, 226, 175),
        focus: Color::from_rgb8(238, 226, 175),
        selection: Color::from_rgb8(68, 64, 48),
        selection_border: Color::from_rgb8(165, 150, 103),
        audio: Color::from_rgb8(170, 173, 179),
        ..Appearance::Dark.palette()
    }
}

/// Build the fixed plug-in theme from the shared semantic palette.
#[must_use]
pub fn theme() -> Theme {
    let colors = palette();
    Theme::custom(
        "Heron Plugins".to_owned(),
        truce_iced::iced::theme::Palette {
            background: colors.canvas,
            text: colors.text,
            primary: colors.action,
            success: colors.success,
            warning: colors.warning,
            danger: colors.danger,
        },
    )
}

pub fn chrome_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let colors = palette();
        let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hover.then_some(colors.control_hover.into()),
            text_color: if matches!(status, button::Status::Disabled) {
                colors.text_subtle
            } else if active {
                colors.action
            } else {
                colors.text_muted
            },
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub fn bar(_: &Theme) -> container::Style {
    let colors = palette();
    container::Style {
        background: Some(colors.surface.into()),
        text_color: Some(colors.text),
        ..Default::default()
    }
}

pub fn panel(_: &Theme) -> container::Style {
    let colors = palette();
    container::Style {
        background: Some(colors.surface_raised.into()),
        text_color: Some(colors.text),
        border: Border {
            color: colors.border_strong,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: Default::default(),
        ..Default::default()
    }
}

pub fn active_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let colors = palette();
        let disabled = matches!(status, button::Status::Disabled);
        button::Style {
            background: Some(
                if active {
                    colors.selection
                } else {
                    colors.control
                }
                .into(),
            ),
            text_color: if active || !disabled {
                colors.text
            } else {
                colors.text_muted
            },
            border: Border {
                color: if active {
                    colors.selection_border
                } else {
                    colors.border
                },
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    }
}
