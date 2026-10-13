//! Fixed-capacity audio exchange. Only this worker loads models and runs inference.

use crate::{
    ProcessingState, ProcessingStatus, models,
    stream::{Geometry, MODEL_WINDOW, Renderer, Stream},
};
use crossbeam_queue::ArrayQueue;
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
        Self {
            input: ArrayQueue::new(QUEUE_PACKETS),
            output: ArrayQueue::new(QUEUE_PACKETS),
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
        let lane = Arc::new(Lane::new());
        let shared = lane.clone();
        let worker_status = status.clone();
        let thread = thread::Builder::new()
            .name("heron-revox".into())
            .spawn(move || {
                let Ok(mut model) = models::load() else {
                    worker_status.set(ProcessingState::Failed);
                    return;
                };
                // Compile the fixed graph shape before accepting live work. This and
                // all model allocations are confined to the instance's worker.
                if Renderer::render(&mut model, &vec![0.0; MODEL_WINDOW], 0.0).is_err() {
                    worker_status.set(ProcessingState::Failed);
                    return;
                }
                worker_status.set(ProcessingState::Ready);
                run(
                    &shared,
                    &worker_status,
                    Stream::new(model, sample_rate, geometry),
                );
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
    pub fn with_renderer<R: Renderer + Send + 'static>(
        renderer: R,
        sample_rate: f64,
        geometry: Geometry,
        status: Arc<ProcessingStatus>,
    ) -> Self {
        let lane = Arc::new(Lane::new());
        let shared = lane.clone();
        let thread = thread::spawn(move || {
            run(
                &shared,
                &status,
                Stream::new(renderer, sample_rate, geometry),
            );
        });
        Self {
            lane,
            thread: Some(thread),
        }
    }
}

fn run<R: Renderer>(lane: &Lane, status: &ProcessingStatus, mut stream: Stream<R>) {
    let mut received_end = 0;
    while !lane.stop.load(Ordering::Acquire) {
        let Some(packet) = lane.input.pop() else {
            thread::sleep(Duration::from_millis(1));
            continue;
        };
        if packet.start != received_end {
            // Publish the gap before collecting enough context to render the
            // next region. Otherwise an offline callback can wait for old audio
            // while the worker needs more input from that same callback.
            lane.output_floor.fetch_max(packet.start, Ordering::Release);
        }
        received_end = packet.start + packet.len as u64;
        let channels = packet.channels;
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

impl Drop for Worker {
    fn drop(&mut self) {
        // Truce creates/resets/destroys DspState outside realtime processing.
        self.lane.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
