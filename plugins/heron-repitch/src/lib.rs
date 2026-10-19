//! Heron Repitch: realtime Rubber Band pitch shifting with Revox-like controls.

pub mod editor;
mod params;
mod processor;
mod status;

pub use params::{RepitchParams, RepitchParamsParamId};
pub use processor::RepitchDspState;
pub use status::ProcessingStatus;

use std::sync::Arc;
use truce::prelude::*;

pub struct HeronRepitch;

impl PluginLogic for HeronRepitch {
    type Params = RepitchParams;
    type DspState = RepitchDspState;

    fn bus_layouts() -> Vec<BusLayout> {
        vec![BusLayout::stereo(), BusLayout::mono()]
    }

    fn reset(state: &mut RepitchDspState, params: &RepitchParams, config: &AudioConfig) {
        state.reset(params, config.sample_rate, config.max_block_size);
    }

    fn latency(state: &RepitchDspState) -> u32 {
        state.latency_samples()
    }

    fn tail(state: &RepitchDspState) -> u32 {
        state.latency_samples()
    }

    fn process(
        state: &mut RepitchDspState,
        params: &RepitchParams,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        _context: &mut ProcessContext,
    ) -> ProcessStatus {
        state.process(params, buffer);
        ProcessStatus::Normal
    }

    fn editor(params: Arc<RepitchParams>) -> Box<dyn Editor> {
        editor::create(params)
    }
}

truce::plugin! { logic: HeronRepitch, params: RepitchParams }
truce::enable_rt_paranoid!();

#[cfg(test)]
mod tests;
