//! Host-rate callback adapter with fixed latency and timestamped worker output.

use crate::{
    ProcessingState, RevoxParams,
    stream::Geometry,
    worker::{Lane, PACKET_FRAMES, Packet, Worker},
};
use std::{
    sync::{Arc, atomic::Ordering},
    thread,
    time::Duration,
};
use truce::prelude::AudioBuffer;

pub struct RevoxDspState {
    worker: Option<Worker>,
    lane: Option<Arc<Lane>>,
    position: u64,
    channels: usize,
    region_start: u64,
    input: Packet,
    output: Option<Packet>,
    dry: Vec<[f32; 2]>,
    latency: usize,
    mix: f32,
    mix_step: f32,
    last_difference: [f32; 2],
}

impl Default for RevoxDspState {
    fn default() -> Self {
        Self {
            worker: None,
            lane: None,
            position: 0,
            channels: 0,
            region_start: 0,
            input: Packet::new(0, 0, 0.0),
            output: None,
            dry: Vec::new(),
            latency: 0,
            mix: 0.0,
            mix_step: 1.0,
            last_difference: [0.0; 2],
        }
    }
}

impl RevoxDspState {
    pub(crate) fn reset(&mut self, params: &RevoxParams, sample_rate: f64) {
        self.worker = None;
        self.lane = None;
        self.position = 0;
        self.channels = 0;
        self.region_start = 0;
        self.input = Packet::new(0, 0, 0.0);
        self.output = None;
        self.mix = 0.0;
        self.last_difference = [0.0; 2];
        let Some(geometry) = Geometry::new(sample_rate) else {
            self.latency = 0;
            self.dry.clear();
            params.status.configure(sample_rate, 0);
            params.status.set(ProcessingState::UnsupportedRate);
            return;
        };
        self.latency = geometry.latency;
        self.dry = vec![[0.0; 2]; self.latency];
        self.mix_step = 1.0 / (sample_rate as f32 * 0.005);
        params.status.configure(sample_rate, self.latency as u32);
        self.worker = Worker::start(sample_rate, geometry, params.status.clone());
        self.lane = self.worker.as_ref().map(|worker| worker.lane.clone());
    }

    pub fn latency_samples(&self) -> u32 {
        self.latency as u32
    }

    pub(crate) fn process(
        &mut self,
        params: &RevoxParams,
        buffer: &mut AudioBuffer,
        offline: bool,
    ) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_running())
        {
            params.status.set(ProcessingState::Failed);
        }
        let channels = buffer
            .num_input_channels()
            .min(buffer.num_output_channels())
            .min(2);
        for index in 0..buffer.num_samples() {
            let mut input = [0.0; 2];
            for (channel, sample) in input.iter_mut().enumerate().take(channels) {
                let value = buffer.input(channel)[index];
                *sample = if value.is_finite() { value } else { 0.0 };
            }
            let output = self.sample(params, input, channels, offline);
            for (channel, sample) in output.iter().enumerate().take(channels) {
                buffer.output(channel)[index] = *sample;
            }
        }
    }

    fn sample(
        &mut self,
        params: &RevoxParams,
        input: [f32; 2],
        channels: usize,
        offline: bool,
    ) -> [f32; 2] {
        if self.latency == 0 {
            return input;
        }
        if self.channels != channels {
            self.channels = channels;
            self.region_start = self.position;
            self.input = Packet::new(self.position, channels, 0.0);
            self.output = None;
            self.mix = 0.0;
            self.last_difference = [0.0; 2];
            if let Some(lane) = &self.lane {
                lane.output_floor
                    .fetch_max(self.position, Ordering::Release);
            }
        }
        let cursor = self.position as usize % self.latency;
        let previous = std::mem::replace(&mut self.dry[cursor], input);
        // A layout change starts a fresh delayed region without clearing the
        // entire delay buffer or rebuilding model sessions in the callback.
        let delayed = self.position - self.region_start >= self.latency as u64;
        let dry = if delayed { previous } else { [0.0; 2] };
        if !delayed {
            // A short layout region may end before its audio becomes audible.
            // Retire its queued output during preroll too, keeping the new
            // region's first packet available for the delayed playback below.
            let _ = self.take_wet(self.region_start);
        }
        if self.input.len == 0 {
            self.input.start = self.position;
            self.input.channels = channels;
            self.input.shift = params.pitch_shift();
        }
        self.input.samples[self.input.len] = input;
        self.input.len += 1;
        if self.input.len == PACKET_FRAMES {
            let mut packet = std::mem::replace(&mut self.input, Packet::new(0, channels, 0.0));
            while let Some(lane) = &self.lane {
                let Err(returned) = lane.input.push(packet) else {
                    break;
                };
                if !offline || !self.can_wait(params) {
                    lane.output_floor
                        .fetch_max(returned.start + returned.len as u64, Ordering::Release);
                    params.status.available(false);
                    break;
                }
                packet = returned;
                // The worker may still be publishing a retired region. Free
                // those output slots while waiting for its input consumption,
                // or two full queues could leave both sides waiting forever.
                let position = if delayed {
                    self.position - self.latency as u64
                } else {
                    self.region_start
                };
                let _ = self.take_wet(position);
                // Only a host-declared offline bounce can wait. Realtime and
                // buffered callbacks drop full packets without any wait.
                thread::sleep(Duration::from_millis(1));
            }
        }
        let wet = if delayed {
            let position = self.position - self.latency as u64;
            let mut wet = self.take_wet(position);
            while wet.is_none() && offline && self.can_wait(params) {
                if self
                    .output
                    .as_ref()
                    .is_some_and(|packet| packet.start > position)
                    || self
                        .lane
                        .as_ref()
                        .is_some_and(|lane| position < lane.output_floor.load(Ordering::Acquire))
                {
                    break;
                }
                thread::sleep(Duration::from_millis(1));
                wet = self.take_wet(position);
            }
            params.status.available(wet.is_some());
            wet
        } else {
            None
        };
        self.position += 1;
        if let Some(wet) = wet {
            self.mix = (self.mix + self.mix_step).min(1.0);
            self.last_difference = [wet[0] - dry[0], wet[1] - dry[1]];
        } else {
            self.mix = (self.mix - self.mix_step).max(0.0);
        }
        [
            dry[0] + self.mix * self.last_difference[0],
            dry[1] + self.mix * self.last_difference[1],
        ]
    }

    fn can_wait(&self, params: &RevoxParams) -> bool {
        self.worker.as_ref().is_some_and(Worker::is_running)
            && !matches!(
                params.status.state(),
                ProcessingState::Failed | ProcessingState::UnsupportedRate
            )
    }

    fn take_wet(&mut self, position: u64) -> Option<[f32; 2]> {
        let lane = self.lane.as_ref()?;
        loop {
            if self.output.is_none() {
                self.output = lane.output.pop();
            }
            if self.output.as_ref().is_some_and(|packet| {
                packet.start + packet.len as u64 <= position
                    || packet.start < self.region_start
                    || packet.channels != self.channels
            }) {
                self.output = None;
                continue;
            }
            let packet = self.output.as_ref()?;
            if packet.start > position {
                return None;
            }
            if position < packet.start + packet.len as u64 {
                return Some(packet.samples[(position - packet.start) as usize]);
            }
            self.output = None;
        }
    }
}

#[cfg(test)]
mod tests;
