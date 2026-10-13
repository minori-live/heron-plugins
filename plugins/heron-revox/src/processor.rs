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
        let cursor = self.position as usize % self.latency;
        let dry = std::mem::replace(&mut self.dry[cursor], input);
        if self.input.len == 0 {
            self.input.start = self.position;
            self.input.channels = channels;
            self.input.shift = params.pitch_shift();
        }
        self.input.samples[self.input.len] = input;
        self.input.len += 1;
        if self.input.len == PACKET_FRAMES {
            let mut packet = std::mem::replace(&mut self.input, Packet::new(0, channels, 0.0));
            if let Some(lane) = &self.lane {
                while let Err(returned) = lane.input.push(packet) {
                    if !offline || !self.can_wait(params) {
                        lane.output_floor
                            .fetch_max(returned.start + returned.len as u64, Ordering::Release);
                        params.status.available(false);
                        break;
                    }
                    packet = returned;
                    // Only a host-declared offline bounce can wait. Realtime and
                    // buffered callbacks drop full packets without any wait.
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
        let wet = if self.position >= self.latency as u64 {
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
            if self
                .output
                .as_ref()
                .is_some_and(|packet| packet.start + packet.len as u64 <= position)
            {
                self.output = None;
            }
            if self.output.is_none() {
                self.output = lane.output.pop();
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
