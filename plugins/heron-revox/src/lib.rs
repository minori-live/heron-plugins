//! Heron Revox: FCPE-guided vocoder resynthesis with coarse and fine pitch controls.

pub mod editor;
mod models;
mod params;
mod processor;
mod resampling;
mod status;
mod stream;
mod worker;

pub use params::{RevoxParams, RevoxParamsParamId};
pub use processor::RevoxDspState;
pub use status::{ProcessingState, ProcessingStatus};

use std::sync::Arc;
use truce::prelude::*;

pub struct HeronRevox;

impl PluginLogic for HeronRevox {
    type Params = RevoxParams;
    type DspState = RevoxDspState;

    fn bus_layouts() -> Vec<BusLayout> {
        vec![BusLayout::stereo(), BusLayout::mono()]
    }

    fn reset(state: &mut RevoxDspState, params: &RevoxParams, config: &AudioConfig) {
        state.reset(params, config.sample_rate);
    }

    fn latency(state: &RevoxDspState) -> u32 {
        state.latency_samples()
    }

    fn tail(state: &RevoxDspState) -> u32 {
        state.latency_samples()
    }

    fn process(
        state: &mut RevoxDspState,
        params: &RevoxParams,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        _context: &mut ProcessContext,
    ) -> ProcessStatus {
        state.process(params, buffer);
        ProcessStatus::Normal
    }

    fn editor(params: Arc<RevoxParams>) -> Box<dyn Editor> {
        editor::create(params)
    }
}

truce::plugin! { logic: HeronRevox, params: RevoxParams }
truce::enable_rt_paranoid!();

#[cfg(test)]
mod tests;
