//! Fixed-capacity audio exchange. Only this worker loads models and runs inference.

use crate::{
    ProcessingState, ProcessingStatus, models,
    stream::{Geometry, MODEL_WINDOW, Renderer, Stream},
};
use crossbeam_queue::ArrayQueue;
use heron_revox_dsp::DspError;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(crate) const PACKET_FRAMES: usize = 128;
const QUEUE_PACKETS: usize = 2048;

pub(crate) struct Packet {
    pub start: u64,
    pub len: usize,
    pub channels: usize,
    pub shift: f32,
    pub samples: [[f32; 2]; PACKET_FRAMES],
}

impl Packet {
    pub fn new(start: u64, channels: usize, shift: f32) -> Self {
        Self {
            start,
            len: 0,
            channels,
            shift,
            samples: [[0.0; 2]; PACKET_FRAMES],
        }
    }
}

pub(crate) struct Lane {
    pub input: ArrayQueue<Packet>,
    pub output: ArrayQueue<Packet>,
    /// Missing output before this boundary must not hold up an offline render.
    /// Already queued output remains usable, including earlier timestamps.
    pub output_floor: AtomicU64,
    pub stop: AtomicBool,
}

impl Lane {
    pub fn new() -> Self {
        Self::with_capacity(QUEUE_PACKETS)
    }

    fn with_capacity(queue_packets: usize) -> Self {
        Self {
            input: ArrayQueue::new(queue_packets),
            output: ArrayQueue::new(queue_packets),
            output_floor: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        }
    }
}

pub(crate) struct Worker {
    pub lane: Arc<Lane>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(
        sample_rate: f64,
        geometry: Geometry,
        status: Arc<ProcessingStatus>,
    ) -> Option<Self> {
        Self::spawn(
            sample_rate,
            geometry,
            status,
            Arc::new(Lane::new()),
            models::load,
        )
    }

    fn spawn<R: Renderer + 'static>(
        sample_rate: f64,
        geometry: Geometry,
        status: Arc<ProcessingStatus>,
        lane: Arc<Lane>,
        load: impl FnMut(usize) -> Result<R, DspError> + Send + 'static,
    ) -> Option<Self> {
        let shared = lane.clone();
        let worker_status = status.clone();
        let thread = thread::Builder::new()
            .name("heron-revox".into())
            .spawn(move || {
                run(&shared, &worker_status, sample_rate, geometry, load);
            });
        match thread {
            Ok(thread) => Some(Self {
                lane,
                thread: Some(thread),
            }),
            Err(_) => {
                status.set(ProcessingState::Failed);
                None
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    #[cfg(test)]
    pub fn with_factory<R: Renderer + 'static>(
        sample_rate: f64,
        geometry: Geometry,
        status: Arc<ProcessingStatus>,
        queue_packets: usize,
        load: impl FnMut(usize) -> Result<R, DspError> + Send + 'static,
    ) -> Self {
        Self::spawn(
            sample_rate,
            geometry,
            status,
            Arc::new(Lane::with_capacity(queue_packets)),
            load,
        )
        .unwrap()
    }
}

fn run<R: Renderer>(
    lane: &Lane,
    status: &ProcessingStatus,
    sample_rate: f64,
    geometry: Geometry,
    mut load: impl FnMut(usize) -> Result<R, DspError>,
) {
    let mut received_end = 0;
    let mut channels = 0;
    let mut stream: Option<Stream<R>> = None;
    while !lane.stop.load(Ordering::Acquire) {
        let Some(packet) = lane.input.pop() else {
            thread::sleep(Duration::from_millis(1));
            continue;
        };
        if packet.start != received_end || packet.channels != channels {
            // Publish the gap before collecting enough context to render the
            // next region. Otherwise an offline callback can wait for old audio
            // while the worker needs more input from that same callback.
            lane.output_floor.fetch_max(packet.start, Ordering::Release);
        }
        received_end = packet.start + packet.len as u64;
        if packet.channels != channels {
            if !(1..=2).contains(&packet.channels) {
                status.set(ProcessingState::Failed);
                return;
            }
            status.set(ProcessingState::Loading);
            // AudioConfig has no channel count. The first packet chooses batch
            // one or two; a later layout change retires the old sessions here.
            drop(stream.take());
            let prepared = prepare(&mut load, packet.channels, sample_rate, geometry);
            if lane.stop.load(Ordering::Acquire) {
                return;
            }
            let Ok(prepared) = prepared else {
                status.set(ProcessingState::Failed);
                return;
            };
            stream = Some(prepared);
            channels = packet.channels;
            status.set(ProcessingState::Ready);
        }
        let Some(stream) = stream.as_mut() else {
            status.set(ProcessingState::Failed);
            return;
        };
        stream.push(&packet);
        let rendered = match stream.render_ready() {
            Ok(Some(rendered)) => rendered,
            Ok(None) => continue,
            Err(_) => {
                status.set(ProcessingState::Failed);
                return;
            }
        };
        for (index, chunk) in rendered.samples.chunks(PACKET_FRAMES).enumerate() {
            let mut packet = Packet::new(
                rendered.start + (index * PACKET_FRAMES) as u64,
                channels,
                0.0,
            );
            packet.len = chunk.len();
            packet.samples[..chunk.len()].copy_from_slice(chunk);
            while let Err(returned) = lane.output.push(packet) {
                if lane.stop.load(Ordering::Acquire) {
                    return;
                }
                packet = returned;
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

fn prepare<R: Renderer>(
    load: &mut impl FnMut(usize) -> Result<R, DspError>,
    channels: usize,
    sample_rate: f64,
    geometry: Geometry,
) -> Result<Stream<R>, DspError> {
    let mut model = load(channels)?;
    // Compile only this layout's fixed graph on the worker, before live input.
    model.render_channels(&vec![vec![0.0; MODEL_WINDOW]; channels], 0.0)?;
    Ok(Stream::new(model, sample_rate, geometry))
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Truce creates/resets/destroys DspState outside realtime processing.
        self.lane.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
