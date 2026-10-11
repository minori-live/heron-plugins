//! Metronome's persistent click controls and MIDI trigger guidance.

use std::sync::Arc;

use heron_plugin_ui::{chrome, parameter_knob, space, type_size};
use truce_iced::iced::widget::{Row, Space, container, text};
use truce_iced::iced::{Alignment, Element, Length};
use truce_iced::{IcedPlugin, IntoElement, Message, ParamCache};

use crate::{ACCENT_NOTE, MetronomeParams, P};

pub struct MetronomeUi;

#[derive(Clone, Debug)]
pub enum MetronomeMessage {}

impl IcedPlugin<MetronomeParams> for MetronomeUi {
    type Message = MetronomeMessage;

    fn new(_params: Arc<MetronomeParams>) -> Self {
        Self
    }

    fn view<'a>(
        &'a self,
        params: &'a ParamCache<MetronomeParams>,
    ) -> Element<'a, Message<Self::Message>> {
        let palette = heron_plugin_ui::palette();
        let controls = Row::new()
            .push(
                parameter_knob(P::Output, params)
                    .label("Output")
                    .size(80.0)
                    .el(),
            )
            .push(
                parameter_knob(P::AccentTone, params)
                    .label("Accent tone")
                    .size(80.0)
                    .el(),
            )
            .push(
                parameter_knob(P::BeatTone, params)
                    .label("Beat tone")
                    .size(80.0)
                    .el(),
            )
            .push(
                parameter_knob(P::Decay, params)
                    .label("Decay")
                    .size(80.0)
                    .el(),
            )
            .spacing(space::XL * 2.0)
            .align_y(Alignment::Center);
        let body = container(controls)
            .center_x(Length::Fill)
            .center_y(Length::Fill);
        let footer = chrome::footer(
            Row::new()
                .push(
                    text("MIDI trigger")
                        .size(type_size::CONTROL)
                        .color(palette.text_muted),
                )
                .push(Space::new().width(Length::Fill))
                .push(
                    text(format!("Accent  {ACCENT_NOTE}    Beat  72"))
                        .size(type_size::CONTROL)
                        .color(palette.text_muted),
                )
                .align_y(Alignment::Center)
                .into(),
        );
        chrome::editor(
            chrome::header("Metronome", Space::new().into()).into(),
            body.into(),
            footer.into(),
        )
    }

    fn title(&self) -> String {
        String::from("Heron Metronome")
    }

    fn theme(&self) -> truce_iced::iced::Theme {
        heron_plugin_ui::theme()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use truce::prelude::{Editor, Params};
    use truce_iced::IcedEditor;

    #[test]
    #[ignore = "manual native 1× and 2× editor render evidence"]
    fn render_editor_evidence() {
        let params = Arc::new(MetronomeParams::default());
        let directory = std::path::Path::new("../../out/plugin-native");
        std::fs::create_dir_all(directory).expect("native evidence directory");
        for scale in [1, 2] {
            let size = heron_plugin_ui::METRONOME_EDITOR_SIZE;
            let mut editor = IcedEditor::<MetronomeParams, MetronomeUi>::new(params.clone(), size);
            editor.set_scale_factor(f64::from(scale));
            let erased: Arc<dyn Params> = params.clone();
            let (pixels, width, height) = editor.screenshot(erased).expect("native editor render");
            assert_eq!((width, height), (size.0 * scale, size.1 * scale));
            truce::core::screenshot::save_png(
                &directory.join(format!("metronome-{scale}x.png")),
                &pixels,
                width,
                height,
            );
        }
    }
}
