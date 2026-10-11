//! Compact shared framing for native plug-in editors.

use truce_iced::iced::{
    Alignment, Element, Length,
    widget::{Column, Container, Row, Space, container, text},
};

use crate::style;

pub const HEADER_HEIGHT: f32 = 44.0;
pub const FOOTER_HEIGHT: f32 = 40.0;

/// Shared title bar with a plug-in-owned action at the trailing edge.
pub fn header<'a, M: 'a>(product: &'a str, trailing: Element<'a, M>) -> Container<'a, M> {
    let colors = style::palette();
    let row = Row::new()
        .push(text("Heron").size(21.0).color(colors.text))
        .push(text(product).size(21.0).color(colors.action))
        .push(Space::new().width(Length::Fill))
        .push(trailing)
        .spacing(2)
        .align_y(Alignment::Center);
    container(row)
        .padding([0, 14])
        .height(Length::Fixed(HEADER_HEIGHT))
        .width(Length::Fill)
        .center_y(Length::Fixed(HEADER_HEIGHT))
        .style(style::bar)
}

/// Shared status bar; the editor owns the status and action content.
pub fn footer<'a, M: 'a>(content: Element<'a, M>) -> Container<'a, M> {
    container(content)
        .padding([0, 10])
        .height(Length::Fixed(FOOTER_HEIGHT))
        .width(Length::Fill)
        .center_y(Length::Fixed(FOOTER_HEIGHT))
        .style(style::bar)
}

/// Compose the two bars around the editor's primary surface.
pub fn editor<'a, M: 'a>(
    header: Element<'a, M>,
    body: Element<'a, M>,
    footer: Element<'a, M>,
) -> Element<'a, M> {
    container(
        Column::new()
            .push(header)
            .push(body)
            .push(footer)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(style::palette().canvas.into()),
        text_color: Some(style::palette().text),
        ..Default::default()
    })
    .into()
}
