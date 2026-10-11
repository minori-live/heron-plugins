//! Gain's compact editor, composed from the shared EQ visual language.

use std::sync::Arc;

use heron_plugin_ui::{chrome, level_meter, parameter_knob, space, type_size};
use truce_iced::iced::widget::{Column, Row, Space, container, text};
use truce_iced::iced::{Alignment, Element, Length};
use truce_iced::{IcedPlugin, IntoElement, Message, ParamCache};

use crate::{GainParams, P};

pub struct GainUi;

#[derive(Clone, Debug)]
pub enum GainMessage {}

impl IcedPlugin<GainParams> for GainUi {
    type Message = GainMessage;

    fn new(_params: Arc<GainParams>) -> Self {
        Self
    }

    fn view<'a>(
        &'a self,
        params: &'a ParamCache<GainParams>,
    ) -> Element<'a, Message<Self::Message>> {
        let palette = heron_plugin_ui::palette();
        let meter = Column::new()
            .push(
                text("Output")
                    .size(type_size::CAPTION)
                    .color(palette.text_muted),
            )
            .push(
                level_meter(&[P::MeterLeft, P::MeterRight], params)
                    .size(14.0, 80.0)
                    .el(),
            )
            .push(
                text("L  R")
                    .size(type_size::CAPTION)
                    .color(palette.text_muted),
            )
            .spacing(space::XS)
            .align_x(Alignment::Center);
        let controls = Row::new()
            // Balance the output meter so the main gain control stays centered.
            .push(Space::new().width(48.0))
            .push(
                parameter_knob(P::Gain, params)
                    .label("Gain")
                    .centered(90.0 / 114.0)
                    .size(104.0)
                    .el(),
            )
            .push(container(meter).width(48.0).align_x(Alignment::Center))
            .spacing(space::XL)
            .align_y(Alignment::Center);
        let body = container(controls)
            .center_x(Length::Fill)
            .center_y(Length::Fill);
        let footer = chrome::footer(
            Row::new()
                .push(
                    text(format!("L  {}", output_level(params.meter(P::MeterLeft))))
                        .size(type_size::CONTROL)
                        .color(palette.text_muted),
                )
                .push(Space::new().width(Length::Fill))
                .push(
                    text(format!("R  {}", output_level(params.meter(P::MeterRight))))
                        .size(type_size::CONTROL)
                        .color(palette.text_muted),
                )
                .align_y(Alignment::Center)
                .into(),
        );
        chrome::editor(
            chrome::header("Gain", Space::new().into()).into(),
            body.into(),
            footer.into(),
        )
    }

    fn title(&self) -> String {
        String::from("Heron Gain")
    }

    fn theme(&self) -> truce_iced::iced::Theme {
        heron_plugin_ui::theme()
    }
}

fn output_level(peak: f32) -> String {
    if peak > 0.0 {
        format!("{:.1} dBFS", 20.0 * peak.log10())
    } else {
        String::from("−∞ dBFS")
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
        let params = Arc::new(GainParams::default());
        let directory = std::path::Path::new("../../out/plugin-native");
        std::fs::create_dir_all(directory).expect("native evidence directory");
        for scale in [1, 2] {
            let size = heron_plugin_ui::GAIN_EDITOR_SIZE;
            let mut editor = IcedEditor::<GainParams, GainUi>::new(params.clone(), size);
            editor.set_scale_factor(f64::from(scale));
            let erased: Arc<dyn Params> = params.clone();
            let (pixels, width, height) = editor.screenshot(erased).expect("native editor render");
            assert_eq!((width, height), (size.0 * scale, size.1 * scale));
            truce::core::screenshot::save_png(
                &directory.join(format!("gain-{scale}x.png")),
                &pixels,
                width,
                height,
            );
        }
    }
}
