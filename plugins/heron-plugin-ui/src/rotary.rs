//! Shared rotary artwork; parameter and gesture ownership stays with the caller.

use truce_iced::iced::{
    Color, Point, Radians,
    widget::canvas::{Frame, LineCap, Path, Stroke, path::Arc},
};

use crate::palette;

const START: f32 = std::f32::consts::PI * 0.75;
const END: f32 = std::f32::consts::PI * 2.25;

/// Draw the EQ rotary at the frame's center, scaled from its 72 by 62 unit canvas.
/// `center_normalized` anchors a bipolar arc at the parameter's neutral value.
pub fn draw(
    frame: &mut Frame,
    normalized: f32,
    center_normalized: Option<f32>,
    enabled: bool,
    hovered: bool,
) {
    let colors = palette();
    let size = frame.size();
    let scale = (size.width / 72.0).min(size.height / 62.0);
    let center = Point::new(size.width * 0.5, size.height * 0.5);
    let arc = |radius: f32, from: f32, to: f32| {
        Path::new(|path| {
            path.arc(Arc {
                center,
                radius: radius * scale,
                start_angle: Radians(from),
                end_angle: Radians(to),
            });
        })
    };
    let stroke = |color: Color, width| {
        Stroke::default()
            .with_color(color)
            .with_width(width * scale)
            .with_line_cap(LineCap::Round)
    };
    frame.fill(
        &Path::circle(Point::new(center.x, center.y + 2.0 * scale), 23.0 * scale),
        Color::from_rgba(0.0, 0.0, 0.0, 0.3),
    );
    frame.fill(&Path::circle(center, 23.0 * scale), colors.control);
    frame.fill(&Path::circle(center, 19.0 * scale), colors.surface);
    frame.stroke(&arc(23.0, START, END), stroke(colors.border, 2.5));
    let angle = START + normalized.clamp(0.0, 1.0) * (END - START);
    let zero = START + center_normalized.unwrap_or(0.0).clamp(0.0, 1.0) * (END - START);
    frame.stroke(
        &arc(23.0, zero.min(angle), zero.max(angle)),
        stroke(
            if enabled {
                colors.action
            } else {
                colors.text_muted
            },
            2.5,
        ),
    );
    frame.stroke(
        &Path::line(
            center,
            Point::new(
                center.x + angle.cos() * 16.0 * scale,
                center.y + angle.sin() * 16.0 * scale,
            ),
        ),
        stroke(
            if enabled {
                colors.text
            } else {
                colors.text_muted
            },
            2.0,
        ),
    );
    if hovered {
        frame.stroke(&arc(25.0, START, END), stroke(colors.focus, 1.0));
    }
}
