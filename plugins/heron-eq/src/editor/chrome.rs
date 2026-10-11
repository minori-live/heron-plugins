//! The two small bars surrounding the frequency display.
use std::time::Duration;

use super::{
    EqMessage, EqUi, Panel, choices,
    icons::{self, Icon},
    meters,
    model::Model,
    style,
};
use truce_iced::{
    Message,
    iced::{
        Alignment, Element, Length,
        widget::{Button, Row, Space, button, text, tooltip},
    },
};

#[cfg(test)]
pub const TOP_HEIGHT: f32 = heron_plugin_ui::chrome::HEADER_HEIGHT;
pub const BOTTOM_HEIGHT: f32 = heron_plugin_ui::chrome::FOOTER_HEIGHT;

pub fn entry<'a>(
    label: impl Into<String>,
    message: EqMessage,
    active: bool,
) -> Button<'a, Message<EqMessage>> {
    button(text(label.into()).size(12.0))
        .padding([5, 9])
        .on_press(Message::Plugin(message))
        .style(style::chrome_button(active))
}

pub fn top<'a>(ui: &EqUi) -> Element<'a, Message<EqMessage>> {
    heron_plugin_ui::chrome::header(
        "EQ",
        icons::button(
            Icon::More,
            "Display & tools",
            EqMessage::Menu(Panel::Tools),
            ui.panel_for_view() == Some(Panel::Tools),
        ),
    )
    .id("heron-eq-topbar")
    .into()
}

pub fn bottom<'a>(
    ui: &EqUi,
    model: &Model,
    peaks: [f32; 2],
    pending: bool,
    error: bool,
) -> Element<'a, Message<EqMessage>> {
    let analyzer = format!(
        "Analyzer · Pre {} · Post {}",
        if ui.pre { "on" } else { "off" },
        if ui.post { "on" } else { "off" }
    );
    let bar = Row::new()
        .push(icons::button(
            Icon::Power,
            if model.config.bypass {
                "Enable EQ"
            } else {
                "Bypass EQ"
            },
            EqMessage::Bypass,
            !model.config.bypass,
        ))
        .push(readout(
            Icon::Phase,
            choices::Mode(model.config.processing_mode).to_string(),
            "Processing mode".to_owned(),
            EqMessage::Menu(Panel::Processing),
            ui.panel_for_view() == Some(Panel::Processing),
        ))
        .push(Space::new().width(Length::Fill))
        .push(icons::button(
            Icon::Spectrum,
            analyzer,
            EqMessage::Analyzer,
            ui.panel_for_view() == Some(Panel::Analyzer),
        ))
        .push(Space::new().width(Length::Fill))
        .push(
            text(if error {
                "!"
            } else if pending {
                "Preparing…"
            } else {
                ""
            })
            .size(11.0)
            .color(style::palette().warning),
        )
        .push(readout(
            Icon::Volume,
            format!(
                "{:.0}%  {:+.1} dB",
                model.extras.gain_scale * 100.0,
                model.config.output_gain_db
            ),
            "Output and gain scale".to_owned(),
            EqMessage::Menu(Panel::Output),
            ui.panel_for_view() == Some(Panel::Output),
        ))
        .push(meters::stereo(peaks))
        .spacing(5)
        .align_y(Alignment::Center);
    heron_plugin_ui::chrome::footer(bar.into())
        .id("heron-eq-footer")
        .into()
}

/// Current algorithm choices and numeric values remain visible beside their icon.
fn readout<'a>(
    icon: Icon,
    label: String,
    hint: String,
    message: EqMessage,
    active: bool,
) -> Element<'a, Message<EqMessage>> {
    let colors = style::palette();
    let content = Row::new()
        .push(icons::glyph(
            icon,
            16.0,
            if active {
                colors.action
            } else {
                colors.text_muted
            },
        ))
        .push(text(label).size(12))
        .spacing(6)
        .align_y(Alignment::Center);
    let action = button(content)
        .padding([5, 7])
        .on_press(Message::Plugin(message))
        .style(style::chrome_button(active));
    tooltip(
        action,
        text(hint).size(11).color(colors.text),
        tooltip::Position::Top,
    )
    .delay(Duration::from_millis(325))
    .gap(4)
    .padding(6)
    .snap_within_viewport(true)
    .style(style::panel)
    .into()
}
