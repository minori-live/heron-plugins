//! Heron's built-in stereo gain VST3.

use std::sync::Arc;

use truce::prelude::*;
use truce_iced::IcedEditor;

mod editor;
mod processor;
pub use editor::{GainMessage, GainUi};
pub use processor::GainDspState;

use GainParamsParamId as P;

#[derive(Params)]
pub struct GainParams {
    #[param(
        name = "Gain",
        range = "linear(-90, 24)",
        unit = "dB",
        default = 0,
        smooth = "linear(10)"
    )]
    pub gain: FloatParam,
    #[meter]
    pub meter_left: MeterSlot,
    #[meter]
    pub meter_right: MeterSlot,
}

/// Gain processor with bounded scratch for a shared stereo gain envelope.
pub struct HeronGain;

impl PluginLogic for HeronGain {
    type Params = GainParams;
    type DspState = GainDspState;

    fn reset(state: &mut GainDspState, _params: &GainParams, _config: &AudioConfig) {
        *state = GainDspState::default();
    }

    fn bus_layouts() -> Vec<BusLayout> {
        vec![
            BusLayout::stereo(),
            BusLayout::mono(),
            BusLayout::new()
                .with_input("Main", ChannelConfig::Mono)
                .with_output("Main", ChannelConfig::Stereo),
        ]
    }

    fn process(
        state: &mut Self::DspState,
        params: &Self::Params,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        context: &mut ProcessContext,
    ) -> ProcessStatus {
        let peaks = state.process(params, buffer);
        if buffer.num_output_channels() > 0 {
            context.set_meter(P::MeterLeft, peaks[0]);
        }
        if buffer.num_output_channels() > 1 {
            context.set_meter(P::MeterRight, peaks[1]);
        }
        ProcessStatus::Normal
    }

    fn editor(params: Arc<GainParams>) -> Box<dyn Editor> {
        IcedEditor::<GainParams, GainUi>::new(params, heron_plugin_ui::GAIN_EDITOR_SIZE)
            .with_meter_ids(vec![P::MeterLeft, P::MeterRight])
            .into_editor()
    }
}

truce::plugin! {
    logic: HeronGain,
    params: GainParams,
}

truce::enable_rt_paranoid!();

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use truce_test::{InputSource, assert_no_audio_alloc, assertions, driver};

    #[test]
    fn exposes_mono_mono_to_stereo_and_stereo_layouts() {
        let layouts = <HeronGain as PluginLogic>::bus_layouts()
            .into_iter()
            .map(|layout| {
                (
                    layout.total_input_channels(),
                    layout.total_output_channels(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(layouts, vec![(2, 2), (1, 1), (1, 2)]);
    }

    #[test]
    fn zero_db_is_exact_stereo_passthrough() {
        let result = driver!(Plugin)
            .duration(Duration::from_millis(10))
            .input(InputSource::Constant(0.25))
            .run();
        for channel in &result.output {
            assert!(
                channel
                    .iter()
                    .all(|sample| sample.to_bits() == 0.25f32.to_bits())
            );
        }
    }

    #[test]
    fn gain_process_is_allocation_free_and_finite() {
        let result = assert_no_audio_alloc(|| {
            driver!(Plugin)
                .duration(Duration::from_millis(40))
                .input(InputSource::Constant(0.5))
                .script(|script| {
                    script.set_param(P::Gain, 6.0);
                    script.wait_ms(15);
                    script.set_param(P::Gain, -90.0);
                })
                .run()
        });
        assertions::assert_no_nans(&result);
    }
}
