use super::EqMessage;
use truce_iced::{Message, iced::Element};

/// Actual reported stereo peak levels, on the suite's -60..0 dB scale.
pub fn stereo(peaks: [f32; 2]) -> Element<'static, Message<EqMessage>> {
    heron_plugin_ui::level_meter_values(&peaks)
        .size(15.0, 30.0)
        .into()
}
