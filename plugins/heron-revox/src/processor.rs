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
    latency: usize,
    sample_rate: f64,
    generation: u64,
    prepared_channels: Option<usize>,
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
            latency: 0,
            sample_rate: 0.0,
            generation: 0,
            prepared_channels: None,
        }
    }
}

impl RevoxDspState {
    pub(crate) fn reset(
        &mut self,
        params: &RevoxParams,
        sample_rate: f64,
        initial_channels: Option<usize>,
    ) {
        let reuse = self.sample_rate == sample_rate
            && params.status.state() != ProcessingState::Failed
            && self.worker.as_ref().is_some_and(Worker::is_running)
            && self
                .lane
                .as_ref()
                .is_some_and(|lane| !lane.stop.load(Ordering::Acquire));
        let already_prepared =
            reuse && initial_channels.is_some() && initial_channels == self.prepared_channels;
        if !reuse {
            self.worker = None;
            self.lane = None;
            self.prepared_channels = None;
        }
        self.position = 0;
        self.channels = 0;
        self.region_start = 0;
        self.input = Packet::new(0, 0, 0.0);
        self.output = None;
        self.sample_rate = sample_rate;
        let Some(geometry) = Geometry::new(sample_rate) else {
            self.latency = 0;
            self.generation = params.status.configure(sample_rate, 0).0;
            params.status.set(ProcessingState::UnsupportedRate);
            return;
        };
        self.latency = geometry.latency;
        let (generation, previous_state) =
            params.status.configure(sample_rate, self.latency as u32);
        self.generation = generation;
        if reuse {
            if let Some(lane) = &self.lane {
                lane.generation.store(self.generation, Ordering::Release);
            }
            // The atomic generation transition must observe Ready. An old
            // inference failure may otherwise retire its sessions after the
            // earlier reuse check but before this reset advances generation.
            if already_prepared && previous_state == ProcessingState::Ready {
                params.status.set(ProcessingState::Ready);
            } else if let (Some(worker), Some(channels)) = (&self.worker, initial_channels) {
                worker.prepare_layout(channels, self.generation);
                self.prepared_channels =
                    (params.status.state() == ProcessingState::Ready).then_some(channels);
            }
        } else {
            self.worker = Worker::start(
                sample_rate,
                geometry,
                params.status.clone(),
                initial_channels,
            );
            self.lane = self.worker.as_ref().map(|worker| worker.lane.clone());
            self.prepared_channels =
                initial_channels.filter(|_| params.status.state() == ProcessingState::Ready);
        }
    }

    pub fn latency_samples(&self) -> u32 {
        self.latency as u32
    }

    pub(crate) fn process(&mut self, params: &RevoxParams, buffer: &mut AudioBuffer) {
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
            let output = self.sample(params, input, channels);
            for (channel, sample) in output.iter().enumerate().take(channels) {
                buffer.output(channel)[index] = *sample;
            }
        }
    }

    fn sample(&mut self, params: &RevoxParams, input: [f32; 2], channels: usize) -> [f32; 2] {
        if self.latency == 0 || !self.can_process(params) {
            return [0.0; 2];
        }
        if self.channels != channels {
            if self.prepared_channels != Some(channels) {
                self.prepared_channels = None;
            }
            if self.channels != 0 {
                self.generation = params.status.begin_region();
                if let Some(lane) = &self.lane {
                    lane.generation.store(self.generation, Ordering::Release);
                }
            }
            self.channels = channels;
            self.region_start = self.position;
            self.input = Packet::new(self.position, channels, 0.0);
            self.output = None;
        }
        // A layout change starts a fresh delayed region without clearing the
        // old worker queues or rebuilding model sessions in the callback.
        let delayed = self.position - self.region_start >= self.latency as u64;
        if !delayed {
            // A short layout region may end before its audio becomes audible.
            // Retire its queued output during preroll too, keeping the new
            // region's first packet available for the delayed playback below.
            let _ = self.take_wet(self.region_start);
        }
        if self.input.len == 0 {
            self.input.generation = self.generation;
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
                if !self.can_process(params) {
                    return [0.0; 2];
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
                // Backpressure preserves every input packet in all host modes.
                // The DAW owns missed deadlines; they never change our audio.
                thread::sleep(Duration::from_millis(1));
            }
        }
        let wet = if delayed {
            let position = self.position - self.latency as u64;
            let mut wet = self.take_wet(position);
            while wet.is_none() && self.can_process(params) {
                thread::sleep(Duration::from_millis(1));
                wet = self.take_wet(position);
            }
            wet
        } else {
            None
        };
        self.position += 1;
        wet.unwrap_or([0.0; 2])
    }

    fn can_process(&self, params: &RevoxParams) -> bool {
        if matches!(
            params.status.state(),
            ProcessingState::Failed | ProcessingState::UnsupportedRate
        ) {
            return false;
        }
        if self.worker.as_ref().is_some_and(Worker::is_running)
            && self
                .lane
                .as_ref()
                .is_some_and(|lane| !lane.stop.load(Ordering::Acquire))
        {
            return true;
        }
        let _ = params
            .status
            .set_for_generation(self.generation, ProcessingState::Failed);
        false
    }

    fn take_wet(&mut self, position: u64) -> Option<[f32; 2]> {
        let lane = self.lane.as_ref()?;
        loop {
            if self.output.is_none() {
                self.output = lane.output.pop();
            }
            if self.output.as_ref().is_some_and(|packet| {
                packet.generation != self.generation
                    || packet.start + packet.len as u64 <= position
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

#[cfg(test)]
mod reset_tests;

#[cfg(test)]
mod latency_tests;

#[cfg(test)]
mod preparation_tests;
