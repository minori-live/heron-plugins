//! Pitch-domain pointer gestures using the suite's shared EQ rotary artwork.

use truce_iced::{
    Message,
    iced::{
        Element, Length, Point, Rectangle, Renderer, Theme, keyboard, mouse,
        widget::{
            Canvas,
            canvas::{self, Event, Frame, Geometry},
        },
    },
};

use super::{Edit, Field, RepitchMessage};

pub(super) fn view(
    field: Field,
    value: f64,
    revision: u64,
    focused: bool,
) -> Element<'static, Message<RepitchMessage>> {
    Canvas::new(Dial {
        field,
        value,
        revision,
        focused,
    })
    .width(Length::Fixed(112.0))
    .height(Length::Fixed(96.0))
    .into()
}

pub(super) struct Dial {
    field: Field,
    value: f64,
    revision: u64,
    focused: bool,
}

#[derive(Default)]
pub(super) struct DialState {
    revision: u64,
    drag: Option<(f64, f32)>,
    modifiers: keyboard::Modifiers,
    last_click: Option<mouse::Click>,
    wheel: f64,
}

impl Dial {
    fn inside(&self, point: Point, bounds: Rectangle) -> bool {
        point.distance(Point::new(bounds.width / 2.0, bounds.height / 2.0)) <= 44.0
    }
}

impl canvas::Program<Message<RepitchMessage>> for Dial {
    type State = DialState;

    fn draw(
        &self,
        state: &DialState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let hovered = self.focused
            || state.drag.is_some()
            || cursor
                .position_in(bounds)
                .is_some_and(|point| self.inside(point, bounds));
        heron_plugin_ui::rotary::draw(
            &mut frame,
            self.field.normalized(self.value) as f32,
            if self.field.bipolar() {
                Some(0.5)
            } else {
                None
            },
            true,
            hovered,
        );
        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut DialState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message<RepitchMessage>>> {
        if state.revision != self.revision {
            state.revision = self.revision;
            state.drag = None;
            state.last_click = None;
            state.wheel = 0.0;
        }
        let publish = |edit| {
            Some(
                canvas::Action::publish(Message::Plugin(RepitchMessage::Edit(self.revision, edit)))
                    .and_capture(),
            )
        };
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                None
            }
            Event::Window(truce_iced::iced::window::Event::Unfocused)
                if state.drag.take().is_some() =>
            {
                publish(Edit::Cancel)
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) if state.drag.take().is_some() => publish(Edit::Cancel),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position_in(bounds)?;
                if !self.inside(position, bounds) {
                    return None;
                }
                if state.modifiers.command() || state.modifiers.control() {
                    return publish(Edit::Reset(self.field));
                }
                let click = mouse::Click::new(position, mouse::Button::Left, state.last_click);
                state.last_click = Some(click);
                if click.kind() == mouse::click::Kind::Double {
                    return publish(Edit::Numeric(self.field));
                }
                state.drag = Some((self.value, cursor.position()?.y));
                publish(Edit::Begin(self.field))
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.drag.is_some() => {
                let (initial, y) = state.drag?;
                let fine = if state.modifiers.shift() { 0.1 } else { 1.0 };
                let delta = f64::from(y - cursor.position()?.y) * fine / 180.0;
                publish(Edit::Change(
                    self.field,
                    self.field.plain(self.field.normalized(initial) + delta),
                ))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.drag.take().is_some() =>
            {
                publish(Edit::End)
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if !self.inside(cursor.position_in(bounds)?, bounds) {
                    return None;
                }
                state.wheel += match delta {
                    mouse::ScrollDelta::Lines { y, .. } => f64::from(*y),
                    mouse::ScrollDelta::Pixels { y, .. } => f64::from(*y) / 40.0,
                };
                let steps = state.wheel.trunc();
                state.wheel -= steps;
                if steps == 0.0 {
                    Some(canvas::Action::request_redraw().and_capture())
                } else {
                    publish(Edit::Step(self.field, steps))
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                Some(canvas::Action::request_redraw())
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        state: &DialState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor
            .position_in(bounds)
            .is_some_and(|point| self.inside(point, bounds))
        {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}
