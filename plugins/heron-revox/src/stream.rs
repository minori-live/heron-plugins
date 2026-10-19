//! Context windows and overlap stitching, entirely owned by the background worker.

use crate::{resampling::Resampler, worker::Packet};
use heron_revox_dsp::{
    DspError, PLUGIN_CONTEXT_SAMPLES, PLUGIN_HOP_SAMPLES, PLUGIN_OVERLAP_SAMPLES,
    PLUGIN_WINDOW_SAMPLES, RevoxDsp,
};
use std::collections::VecDeque;

pub(crate) const MODEL_RATE: f64 = 48_000.0;
pub(crate) const MODEL_WINDOW: usize = PLUGIN_WINDOW_SAMPLES;

#[derive(Clone, Copy)]
pub(crate) struct Geometry {
    pub context: usize,
    pub hop: usize,
    pub overlap: usize,
    pub latency: usize,
}

impl Geometry {
    pub fn new(sample_rate: f64) -> Option<Self> {
        if ![44_100.0, 48_000.0, 88_200.0, 96_000.0, 176_400.0, 192_000.0].contains(&sample_rate) {
            return None;
        }
        let samples = |model_samples: usize| {
            (model_samples as f64 * sample_rate / MODEL_RATE).round() as usize
        };
        let context = samples(PLUGIN_CONTEXT_SAMPLES);
        let hop = samples(PLUGIN_HOP_SAMPLES);
        let overlap = samples(PLUGIN_OVERLAP_SAMPLES);
        Some(Self {
            context,
            hop,
            overlap,
            // A hop needs this future input before rendering. One complete
            // packet covers every hop's input-queue alignment at every rate.
            // Inference time does not advance the host's sample timeline.
            latency: context + hop + overlap + crate::worker::PACKET_FRAMES,
        })
    }

    pub fn window(self) -> usize {
        self.context * 2 + self.hop + self.overlap
    }
}

pub(crate) trait Renderer {
    fn reset_stream(&mut self) {}

    fn render_channels(
        &mut self,
        input: &[Vec<f32>],
        shift: f32,
    ) -> Result<Vec<Vec<f32>>, DspError>;
}

impl Renderer for RevoxDsp {
    fn reset_stream(&mut self) {
        RevoxDsp::reset_stream(self);
    }

    fn render_channels(
        &mut self,
        input: &[Vec<f32>],
        shift: f32,
    ) -> Result<Vec<Vec<f32>>, DspError> {
        self.render_stream_channels(input, shift, 42)
    }
}

pub(crate) struct Rendered {
    pub start: u64,
    pub samples: Vec<[f32; 2]>,
}

pub(crate) struct Stream<R> {
    renderer: R,
    geometry: Geometry,
    to_model: Resampler,
    from_model: Resampler,
    history: VecDeque<[f32; 2]>,
    history_start: u64,
    received_end: u64,
    next_start: u64,
    channels: usize,
    controls: VecDeque<(u64, f32)>,
    last_shift: f32,
    tail: Option<Vec<[f32; 2]>>,
}

impl<R: Renderer> Stream<R> {
    pub fn new(mut renderer: R, sample_rate: f64, geometry: Geometry) -> Self {
        // Warmup uses the same renderer but does not belong to the audio timeline.
        renderer.reset_stream();
        Self {
            renderer,
            geometry,
            to_model: Resampler::new(sample_rate, MODEL_RATE),
            from_model: Resampler::new(MODEL_RATE, sample_rate),
            history: VecDeque::with_capacity(geometry.window() + crate::worker::PACKET_FRAMES),
            history_start: 0,
            received_end: 0,
            next_start: 0,
            channels: 0,
            controls: VecDeque::new(),
            last_shift: 0.0,
            tail: None,
        }
    }

    pub fn push(&mut self, packet: &Packet) {
        if packet.start != self.received_end
            || (self.channels != 0 && self.channels != packet.channels)
        {
            // A layout change discards a partial input packet and starts a new
            // region. Never bridge that boundary with old waveform or controls.
            self.renderer.reset_stream();
            self.history.clear();
            self.controls.clear();
            self.history_start = packet.start;
            self.next_start = packet.start;
            self.tail = None;
        }
        self.channels = packet.channels;
        self.history.extend(&packet.samples[..packet.len]);
        self.controls.push_back((packet.start, packet.shift));
        self.received_end = packet.start + packet.len as u64;
    }

    /// Reset the host timeline without recreating this layout's model sessions.
    pub fn reset(&mut self) {
        self.renderer.reset_stream();
        self.history.clear();
        self.controls.clear();
        self.history_start = 0;
        self.received_end = 0;
        self.next_start = 0;
        self.channels = 0;
        self.last_shift = 0.0;
        self.tail = None;
    }

    pub fn render_ready(&mut self) -> Result<Option<Rendered>, DspError> {
        let geometry = self.geometry;
        let required_end =
            self.next_start + (geometry.hop + geometry.overlap + geometry.context) as u64;
        if self.received_end < required_end {
            return Ok(None);
        }
        while self
            .controls
            .front()
            .is_some_and(|(start, _)| *start <= self.next_start)
        {
            if let Some((_, shift)) = self.controls.pop_front() {
                self.last_shift = shift;
            }
        }
        let mut output = vec![[0.0; 2]; geometry.hop];
        let mut tail = vec![[0.0; 2]; geometry.overlap];
        let mut model_inputs = Vec::with_capacity(self.channels);
        for channel in 0..self.channels {
            let input: Vec<_> = (0..geometry.window())
                .map(|index| {
                    let position = self.next_start as i64 + index as i64 - geometry.context as i64;
                    if position < self.history_start as i64 {
                        0.0
                    } else {
                        self.history
                            .get((position as u64 - self.history_start) as usize)
                            .map_or(0.0, |frame| frame[channel])
                    }
                })
                .collect();
            model_inputs.push(self.to_model.convert(&input, MODEL_WINDOW));
        }
        let model_outputs = self
            .renderer
            .render_channels(&model_inputs, self.last_shift)?;
        if model_outputs.len() != self.channels {
            return Err(DspError::OutputShape {
                model: "Revox",
                output: "waveform batch",
            });
        }
        for (channel, model_output) in model_outputs.iter().enumerate() {
            if model_output.len() != MODEL_WINDOW
                || model_output.iter().any(|sample| !sample.is_finite())
            {
                return Err(DspError::OutputShape {
                    model: "Revox",
                    output: "waveform",
                });
            }
            let rendered = self.from_model.convert_range(
                model_output,
                geometry.context..geometry.context + geometry.hop + geometry.overlap,
            );
            for (index, frame) in output.iter_mut().enumerate() {
                let value = rendered[index];
                frame[channel] = if index < geometry.overlap {
                    if let Some(previous) = &self.tail {
                        let phase = (index + 1) as f32 / (geometry.overlap + 1) as f32;
                        let wet = 0.5 - 0.5 * (std::f32::consts::PI * phase).cos();
                        previous[index][channel] * (1.0 - wet) + value * wet
                    } else {
                        value
                    }
                } else {
                    value
                };
            }
            for (index, frame) in tail.iter_mut().enumerate() {
                frame[channel] = rendered[geometry.hop + index];
            }
        }
        let start = self.next_start;
        self.next_start += geometry.hop as u64;
        self.tail = Some(tail);
        let keep_from = self.next_start.saturating_sub(geometry.context as u64);
        let remove = keep_from.saturating_sub(self.history_start) as usize;
        self.history.drain(..remove.min(self.history.len()));
        self.history_start = self.history_start.max(keep_from);
        Ok(Some(Rendered {
            start,
            samples: output,
        }))
    }
}

#[cfg(test)]
mod tests;
