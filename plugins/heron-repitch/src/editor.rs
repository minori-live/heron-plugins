//! Repitch's pitch controls, composed from the shared Heron EQ visual language.

mod dial;
mod lifecycle;
mod numeric;
#[cfg(test)]
mod tests;

use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use heron_plugin_ui::{chrome, palette, space, type_size};
use truce::prelude::{Editor, IntoEditor, Params};
use truce_iced::{
    IcedEditor, IcedPlugin, Message, ParamCache, PluginContext,
    iced::{
        Alignment, Border, Element, Event, Length, Subscription, Task, Theme, event, keyboard,
        widget::{Column, Row, Space, button, container, text, text_input},
    },
};

use crate::RepitchParams;

pub const EDITOR_SIZE: (u32, u32) = (520, 320);

/// Per-instance native editor recall/closure revision, excluded from saved state.
#[derive(Default)]
pub struct EditorState {
    revision: AtomicU64,
}

impl EditorState {
    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    fn invalidate(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Semitones,
    Cents,
}

impl Field {
    fn id(self) -> u32 {
        match self {
            Self::Semitones => 0,
            Self::Cents => 1,
        }
    }
    fn maximum(self) -> f64 {
        match self {
            Self::Semitones => 12.0,
            Self::Cents => 100.0,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Semitones => "Semitones",
            Self::Cents => "Cents",
        }
    }
    fn unit(self) -> &'static str {
        match self {
            Self::Semitones => "st",
            Self::Cents => "ct",
        }
    }
    fn normalized(self, value: f64) -> f64 {
        (self.quantize(value) + self.maximum()) / (2.0 * self.maximum())
    }
    fn plain(self, normalized: f64) -> f64 {
        self.quantize((normalized * 2.0 - 1.0) * self.maximum())
    }
    fn quantize(self, value: f64) -> f64 {
        value.round().clamp(-self.maximum(), self.maximum())
    }
    fn input_id(self) -> truce_iced::iced::widget::Id {
        truce_iced::iced::widget::Id::new(match self {
            Self::Semitones => "repitch-semitones-input",
            Self::Cents => "repitch-cents-input",
        })
    }
}

#[derive(Clone, Debug)]
pub enum Edit {
    Begin(Field),
    Change(Field, f64),
    End,
    Cancel,
    Step(Field, f64),
    Reset(Field),
    Numeric(Field),
    Draft(String),
    Submit,
}

#[derive(Clone, Debug)]
pub enum RepitchMessage {
    Edit(u64, Edit),
    ToggleFormant,
    SetQuality(i32),
    Key(keyboard::Event),
    Unfocused,
}

#[derive(Clone, Copy)]
struct Gesture {
    field: Field,
    initial: f64,
}

pub struct RepitchUi {
    params: Arc<RepitchParams>,
    revision: u64,
    active: Option<Gesture>,
    focused: Option<Field>,
    numeric: Option<Field>,
    numeric_generation: u64,
    draft: String,
    invalid: bool,
    last_status: Cell<(u32, u64, u64)>,
}

pub fn create(params: Arc<RepitchParams>) -> Box<dyn Editor> {
    let inner =
        IcedEditor::<RepitchParams, RepitchUi>::new(params.clone(), EDITOR_SIZE).into_editor();
    Box::new(lifecycle::RepitchEditor::new(params, inner))
}

impl RepitchUi {
    fn current(&self, field: Field) -> f64 {
        self.params.get_plain(field.id()).unwrap_or(0.0)
    }

    fn synchronize(&mut self) {
        let revision = self.params.editor_state.revision();
        if revision != self.revision {
            self.revision = revision;
            self.active = None;
            self.numeric = None;
            self.draft.clear();
            self.invalid = false;
        }
    }

    fn end(&mut self, context: &PluginContext<RepitchParams>, cancel: bool) {
        if let Some(gesture) = self.active.take() {
            if cancel {
                context.set_param(
                    gesture.field.id(),
                    gesture.field.normalized(gesture.initial),
                );
            }
            context.end_edit(gesture.field.id());
        }
    }

    fn single(&mut self, context: &PluginContext<RepitchParams>, field: Field, value: f64) {
        self.end(context, false);
        context.begin_edit(field.id());
        context.set_param(field.id(), field.normalized(value));
        context.end_edit(field.id());
        self.focused = Some(field);
    }

    fn edit(&mut self, edit: Edit, context: &PluginContext<RepitchParams>) {
        match edit {
            Edit::Begin(field) => {
                self.end(context, false);
                self.numeric = None;
                self.invalid = false;
                self.focused = Some(field);
                self.active = Some(Gesture {
                    field,
                    initial: self.current(field),
                });
                context.begin_edit(field.id());
            }
            Edit::Change(field, value)
                if value.is_finite()
                    && self.active.is_some_and(|gesture| gesture.field == field) =>
            {
                context.set_param(field.id(), field.normalized(value));
            }
            Edit::End => self.end(context, false),
            Edit::Cancel => {
                self.end(context, true);
                self.numeric = None;
                self.invalid = false;
                self.draft.clear();
            }
            Edit::Step(field, delta) => self.single(context, field, self.current(field) + delta),
            Edit::Reset(field) => self.single(context, field, 0.0),
            Edit::Numeric(field) => {
                self.end(context, false);
                self.focused = Some(field);
                self.numeric = Some(field);
                self.numeric_generation = self.numeric_generation.wrapping_add(1);
                self.draft = format!("{:.0}", self.current(field));
                self.invalid = false;
            }
            Edit::Draft(value) if self.numeric.is_some() => {
                self.draft = value;
                self.invalid = false;
            }
            Edit::Submit => {
                if let Some(field) = self.numeric {
                    if let Some(value) = parse_value(field, &self.draft) {
                        self.single(context, field, value);
                        self.numeric = None;
                        self.invalid = false;
                    } else {
                        self.invalid = true;
                    }
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, event: keyboard::Event, context: &PluginContext<RepitchParams>) {
        let keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            modifiers,
            ..
        } = event
        else {
            return;
        };
        use keyboard::key::Named;
        match key {
            Named::Escape => self.edit(Edit::Cancel, context),
            Named::Tab => {
                self.edit(Edit::Cancel, context);
                self.focused = Some(match self.focused {
                    Some(Field::Semitones) => Field::Cents,
                    Some(Field::Cents) => Field::Semitones,
                    None if modifiers.shift() => Field::Cents,
                    None => Field::Semitones,
                });
            }
            Named::Enter if self.numeric.is_none() => {
                if let Some(field) = self.focused {
                    self.edit(Edit::Numeric(field), context);
                }
            }
            Named::Home if self.numeric.is_none() => {
                if let Some(field) = self.focused {
                    self.edit(Edit::Reset(field), context);
                }
            }
            Named::ArrowUp | Named::ArrowDown if self.numeric.is_none() => {
                if let Some(field) = self.focused {
                    let step = if modifiers.shift() {
                        match field {
                            Field::Semitones => 3.0,
                            Field::Cents => 10.0,
                        }
                    } else {
                        1.0
                    };
                    self.edit(
                        Edit::Step(field, if key == Named::ArrowUp { step } else { -step }),
                        context,
                    );
                }
            }
            _ => {}
        }
    }

    fn control<'a>(
        &'a self,
        field: Field,
        params: &'a ParamCache<RepitchParams>,
    ) -> Element<'a, Message<RepitchMessage>> {
        let colors = palette();
        let value = params.get_plain(field.id());
        let revision = self.params.editor_state.revision();
        let numeric = self.numeric.filter(|_| self.revision == revision);
        let dial = dial::view(field, value, revision, self.focused == Some(field));
        let readout: Element<'a, Message<RepitchMessage>> = if numeric == Some(field) {
            numeric::focus_on_open(
                text_input("", &self.draft)
                    .id(field.input_id())
                    .style(input_style(self.invalid))
                    .size(type_size::CONTROL)
                    .padding([4, 8])
                    .on_input(move |value| {
                        Message::Plugin(RepitchMessage::Edit(revision, Edit::Draft(value)))
                    })
                    .on_submit(Message::Plugin(RepitchMessage::Edit(
                        revision,
                        Edit::Submit,
                    ))),
                self.numeric_generation,
            )
        } else {
            button(
                text(format!("{value:+.0} {}", field.unit()))
                    .size(type_size::CONTROL)
                    .color(colors.text),
            )
            .padding([4, 8])
            .style(heron_plugin_ui::style::chrome_button(false))
            .on_press(Message::Plugin(RepitchMessage::Edit(
                revision,
                Edit::Numeric(field),
            )))
            .into()
        };
        Column::new()
            .push(
                text(field.label())
                    .size(type_size::CONTROL)
                    .color(colors.text_muted),
            )
            .push(dial)
            .push(container(readout).width(96).center_x(96))
            .spacing(space::XS)
            .align_x(Alignment::Center)
            .width(Length::Fixed(132.0))
            .into()
    }
}

fn parse_value(field: Field, draft: &str) -> Option<f64> {
    let value = draft.trim().parse::<f64>().ok()?;
    (value.is_finite() && value.fract() == 0.0 && value.abs() <= field.maximum()).then_some(value)
}

fn input_style(invalid: bool) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let colors = palette();
        text_input::Style {
            background: colors.control.into(),
            border: Border {
                color: if invalid {
                    colors.warning
                } else if matches!(status, text_input::Status::Focused { .. }) {
                    colors.focus
                } else {
                    colors.border_strong
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            icon: colors.text_muted,
            placeholder: colors.text_subtle,
            value: colors.text,
            selection: colors.selection,
        }
    }
}

impl IcedPlugin<RepitchParams> for RepitchUi {
    type Message = RepitchMessage;
    fn new(params: Arc<RepitchParams>) -> Self {
        Self {
            revision: params.editor_state.revision(),
            params,
            active: None,
            focused: None,
            numeric: None,
            numeric_generation: 0,
            draft: String::new(),
            invalid: false,
            last_status: Cell::new((0, 0, 0)),
        }
    }

    fn update(
        &mut self,
        message: Message<RepitchMessage>,
        _cache: &ParamCache<RepitchParams>,
        context: &PluginContext<RepitchParams>,
    ) -> Task<Message<RepitchMessage>> {
        self.synchronize();
        match message {
            Message::Plugin(RepitchMessage::Edit(revision, edit)) if revision == self.revision => {
                self.edit(edit, context)
            }
            Message::Plugin(RepitchMessage::ToggleFormant) => {
                self.end(context, false);
                let next = !self.params.formant.value();
                context.begin_edit(2u32);
                context.set_param(2u32, if next { 1.0 } else { 0.0 });
                context.end_edit(2u32);
            }
            Message::Plugin(RepitchMessage::SetQuality(value)) => {
                self.end(context, false);
                let plain = f64::from(value.clamp(0, 1));
                context.begin_edit(3u32);
                // Quality is discrete(0,1); normalized equals plain for two steps.
                context.set_param(3u32, plain);
                context.end_edit(3u32);
            }
            Message::Plugin(RepitchMessage::Key(event)) => self.key(event, context),
            Message::Plugin(RepitchMessage::Unfocused) => {
                self.edit(Edit::Cancel, context);
                self.focused = None;
            }
            _ => {}
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message<RepitchMessage>> {
        event::listen_with(|event, status, _| match event {
            Event::Keyboard(
                event @ keyboard::Event::KeyPressed {
                    key:
                        keyboard::Key::Named(keyboard::key::Named::Tab | keyboard::key::Named::Escape),
                    ..
                },
            ) => Some(Message::Plugin(RepitchMessage::Key(event))),
            Event::Keyboard(event) if status == event::Status::Ignored => {
                Some(Message::Plugin(RepitchMessage::Key(event)))
            }
            Event::Window(truce_iced::iced::window::Event::Unfocused) => {
                Some(Message::Plugin(RepitchMessage::Unfocused))
            }
            _ => None,
        })
    }

    fn view<'a>(
        &'a self,
        params: &'a ParamCache<RepitchParams>,
    ) -> Element<'a, Message<RepitchMessage>> {
        let colors = palette();
        self.last_status.set(status_stamp(&self.params));
        let controls = Row::new()
            .push(self.control(Field::Semitones, params))
            .push(self.control(Field::Cents, params))
            .spacing(48)
            .align_y(Alignment::Center);
        let formant_on = params.get_plain(2u32) >= 0.5;
        let quality = params.get_plain(3u32).round() as i32;
        let options = Row::new()
            .push(
                button(
                    text(if formant_on {
                        "Formant On"
                    } else {
                        "Formant Off"
                    })
                    .size(type_size::CONTROL)
                    .color(colors.text),
                )
                .padding([6, 12])
                .style(heron_plugin_ui::style::chrome_button(formant_on))
                .on_press(Message::Plugin(RepitchMessage::ToggleFormant)),
            )
            .push(
                button(text("Faster").size(type_size::CONTROL).color(colors.text))
                    .padding([6, 12])
                    .style(heron_plugin_ui::style::chrome_button(quality == 0))
                    .on_press(Message::Plugin(RepitchMessage::SetQuality(0))),
            )
            .push(
                button(text("Finer").size(type_size::CONTROL).color(colors.text))
                    .padding([6, 12])
                    .style(heron_plugin_ui::style::chrome_button(quality == 1))
                    .on_press(Message::Plugin(RepitchMessage::SetQuality(1))),
            )
            .spacing(space::SM)
            .align_y(Alignment::Center);
        let body = container(
            Column::new()
                .push(controls)
                .push(options)
                .spacing(space::MD)
                .align_x(Alignment::Center),
        )
        .padding([12, 16])
        .center_x(Length::Fill)
        .center_y(Length::Fill);
        let rate = self.params.status.sample_rate();
        let latency = if rate > 0.0 {
            format!(
                "{:.0} ms",
                f64::from(self.params.status.latency_samples()) / rate * 1000.0
            )
        } else {
            String::from("—")
        };
        let footer = Row::new()
            .push(
                text("Rubber Band Live")
                    .size(type_size::CONTROL)
                    .color(colors.text_muted),
            )
            .push(Space::new().width(Length::Fill))
            .push(
                text(format!("Latency {latency}"))
                    .size(type_size::CONTROL)
                    .color(colors.text_muted),
            )
            .align_y(Alignment::Center);
        chrome::editor(
            chrome::header("Repitch", Space::new().into()).into(),
            body.into(),
            chrome::footer(footer.into()).into(),
        )
    }
    fn theme(&self) -> Theme {
        heron_plugin_ui::theme()
    }
    fn title(&self) -> String {
        String::from("Heron Repitch")
    }
    fn needs_redraw(&self) -> bool {
        self.last_status.get() != status_stamp(&self.params)
    }
}

fn status_stamp(params: &RepitchParams) -> (u32, u64, u64) {
    (
        params.status.latency_samples(),
        params.status.sample_rate().to_bits(),
        params.editor_state.revision(),
    )
}
