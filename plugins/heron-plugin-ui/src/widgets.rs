use std::{fmt::Debug, marker::PhantomData};

use truce::{core::Float, params::Params};
use truce_iced::{
    Message, ParamCache, ParamMessage,
    iced::{
        Alignment, Element, Font, Length, Rectangle, Renderer, Theme, mouse,
        widget::{
            Canvas, Column,
            canvas::{self, Event, Frame, Geometry},
            text,
        },
    },
};

use crate::{palette, rotary, type_size};

const DRAG_SENSITIVITY: f32 = 200.0;

/// Create a parameter knob whose drawing is sourced from Heron semantic tokens.
pub fn parameter_knob<M: Clone + Debug + 'static>(
    id: impl Into<u32>,
    params: &ParamCache<impl Params>,
) -> ParameterKnob<'_, M> {
    ParameterKnob::new(id.into(), params)
}

pub struct ParameterKnob<'a, M> {
    id: u32,
    value: f64,
    center_normalized: Option<f64>,
    display: String,
    label: Option<&'a str>,
    size: f32,
    font: Font,
    marker: PhantomData<M>,
}

impl<'a, M: Clone + Debug + 'static> ParameterKnob<'a, M> {
    fn new(id: u32, params: &'a ParamCache<impl Params>) -> Self {
        Self {
            id,
            value: params.get(id),
            center_normalized: None,
            display: params.label(id).to_owned(),
            label: None,
            size: 72.0,
            font: params.font(),
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    #[must_use]
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub fn font(mut self, font: Font) -> Self {
        self.font = font;
        self
    }

    /// Anchor the rotary arc at a neutral value in the parameter's normalized range.
    #[must_use]
    pub fn centered(mut self, normalized: f64) -> Self {
        self.center_normalized = Some(normalized.clamp(0.0, 1.0));
        self
    }

    #[must_use]
    pub fn into_element(self) -> Element<'a, Message<M>> {
        let colors = palette();
        let mut content = Column::new()
            .align_x(Alignment::Center)
            .spacing(1)
            .width(Length::Fixed(self.size));
        if let Some(label) = self.label {
            content = content.push(
                text(label)
                    .size(type_size::CAPTION)
                    .font(self.font)
                    .color(colors.text_muted),
            );
        }
        content
            .push(
                Canvas::new(KnobProgram {
                    id: self.id,
                    value: f32::from_f64(self.value),
                    center_normalized: self.center_normalized.map(f32::from_f64),
                })
                .width(Length::Fixed(self.size))
                .height(Length::Fixed(self.size * 62.0 / 72.0)),
            )
            .push(
                text(self.display)
                    .size(type_size::CONTROL)
                    .font(self.font)
                    .color(colors.text),
            )
            .into()
    }
}

impl<'a, M: Clone + Debug + 'static> From<ParameterKnob<'a, M>> for Element<'a, Message<M>> {
    fn from(knob: ParameterKnob<'a, M>) -> Self {
        knob.into_element()
    }
}

struct KnobProgram {
    id: u32,
    value: f32,
    center_normalized: Option<f32>,
}

#[derive(Default)]
struct KnobState {
    dragging: bool,
    start_value: f32,
    start_y: f32,
}

impl<M: Clone + Debug + 'static> canvas::Program<Message<M>> for KnobProgram {
    type State = KnobState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        rotary::draw(
            &mut frame,
            self.value,
            self.center_normalized,
            true,
            state.dragging || cursor.position_in(bounds).is_some(),
        );
        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message<M>>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.position_in(bounds).is_some() =>
            {
                state.dragging = true;
                state.start_value = self.value;
                state.start_y = cursor.position_in(bounds)?.y;
                Some(
                    canvas::Action::publish(Message::Param(ParamMessage::BeginEdit(self.id)))
                        .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                let position = cursor.position()?;
                let delta = (state.start_y - (position.y - bounds.y)) / DRAG_SENSITIVITY;
                let value = (state.start_value + delta).clamp(0.0, 1.0);
                Some(
                    canvas::Action::publish(Message::Param(ParamMessage::SetNormalized(
                        self.id,
                        f64::from(value),
                    )))
                    .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                Some(
                    canvas::Action::publish(Message::Param(ParamMessage::EndEdit(self.id)))
                        .and_capture(),
                )
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if cursor.position_in(bounds).is_some() {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use truce_iced::iced::{Point, Size, event::Status};

    fn knob() -> KnobProgram {
        KnobProgram {
            id: 7,
            value: 0.5,
            center_normalized: None,
        }
    }

    fn update(
        knob: &KnobProgram,
        state: &mut KnobState,
        event: Event,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message<()>>> {
        <KnobProgram as canvas::Program<Message<()>>>::update(
            knob,
            state,
            &event,
            Rectangle::new(Point::new(10.0, 20.0), Size::new(64.0, 88.0)),
            cursor,
        )
    }

    #[test]
    fn knob_drag_publishes_a_balanced_parameter_gesture() {
        let knob = knob();
        let mut state = KnobState::default();

        let begin = update(
            &knob,
            &mut state,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            mouse::Cursor::Available(Point::new(30.0, 60.0)),
        )
        .expect("press inside the knob should begin editing");
        let (message, _, status) = begin.into_inner();
        assert!(matches!(
            message,
            Some(Message::Param(ParamMessage::BeginEdit(7)))
        ));
        assert_eq!(status, Status::Captured);
        assert!(state.dragging);

        let drag = update(
            &knob,
            &mut state,
            Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(30.0, -200.0),
            }),
            mouse::Cursor::Available(Point::new(30.0, -200.0)),
        )
        .expect("drag should publish a normalized value");
        let (message, _, _) = drag.into_inner();
        assert!(matches!(
            message,
            Some(Message::Param(ParamMessage::SetNormalized(7, value))) if value == 1.0
        ));

        let end = update(
            &knob,
            &mut state,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            mouse::Cursor::Unavailable,
        )
        .expect("release should end editing even outside the knob");
        assert!(matches!(
            end.into_inner().0,
            Some(Message::Param(ParamMessage::EndEdit(7)))
        ));
        assert!(!state.dragging);
    }

    #[test]
    fn knob_ignores_unrelated_events_and_reports_cursor_state() {
        let knob = knob();
        let mut state = KnobState::default();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(64.0, 88.0));

        assert!(
            update(
                &knob,
                &mut state,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
                mouse::Cursor::Available(Point::new(20.0, 30.0)),
            )
            .is_none()
        );
        assert_eq!(
            <KnobProgram as canvas::Program<Message<()>>>::mouse_interaction(
                &knob,
                &state,
                bounds,
                mouse::Cursor::Available(Point::new(20.0, 30.0)),
            ),
            mouse::Interaction::Grab
        );
        assert_eq!(
            <KnobProgram as canvas::Program<Message<()>>>::mouse_interaction(
                &knob,
                &state,
                bounds,
                mouse::Cursor::Unavailable,
            ),
            mouse::Interaction::default()
        );

        state.dragging = true;
        assert_eq!(
            <KnobProgram as canvas::Program<Message<()>>>::mouse_interaction(
                &knob,
                &state,
                bounds,
                mouse::Cursor::Unavailable,
            ),
            mouse::Interaction::Grabbing
        );
    }
}
