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
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(crate) const PACKET_FRAMES: usize = 128;
const QUEUE_PACKETS: usize = 2048;

pub(crate) struct Packet {
    pub generation: u64,
    pub start: u64,
    pub len: usize,
    pub channels: usize,
    pub shift: f32,
    pub samples: [[f32; 2]; PACKET_FRAMES],
}

impl Packet {
    pub fn new(start: u64, channels: usize, shift: f32) -> Self {
        Self {
            generation: 0,
            start,
            len: 0,
            channels,
            shift,
            samples: [[0.0; 2]; PACKET_FRAMES],
        }
    }
}

pub(crate) struct Lane {
    pub generation: AtomicU64,
    pub input: ArrayQueue<Packet>,
    pub output: ArrayQueue<Packet>,
    pub stop: AtomicBool,
}

impl Lane {
    pub fn new() -> Self {
        Self::with_capacity(QUEUE_PACKETS)
    }

    fn with_capacity(queue_packets: usize) -> Self {
        Self {
            generation: AtomicU64::new(0),
            input: ArrayQueue::new(queue_packets),
            output: ArrayQueue::new(queue_packets),
            stop: AtomicBool::new(false),
        }
    }
}

pub(crate) struct Worker {
    pub lane: Arc<Lane>,
    prepare: SyncSender<Preparation>,
    status: Arc<ProcessingStatus>,
    thread: Option<JoinHandle<()>>,
}

struct Preparation {
    generation: u64,
    channels: usize,
    complete: SyncSender<bool>,
}

impl Worker {
    pub fn start(
        sample_rate: f64,
        geometry: Geometry,
        status: Arc<ProcessingStatus>,
        initial_channels: Option<usize>,
    ) -> Option<Self> {
        let generation = status.generation();
        let worker = Self::spawn(
            sample_rate,
            geometry,
            status,
            Arc::new(Lane::new()),
            models::load,
        )?;
        if let Some(channels) = initial_channels {
            worker.prepare_layout(channels, generation);
        }
        Some(worker)
    }

    fn spawn<R: Renderer + 'static>(
        sample_rate: f64,
        geometry: Geometry,
        status: Arc<ProcessingStatus>,
        lane: Arc<Lane>,
        load: impl FnMut(usize) -> Result<R, DspError> + Send + 'static,
    ) -> Option<Self> {
        lane.generation
            .store(status.generation(), Ordering::Release);
        let shared = lane.clone();
        let worker_status = status.clone();
        let (prepare, requests) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("heron-revox".into())
            .spawn(move || {
                run(
                    &shared,
                    &worker_status,
                    sample_rate,
                    geometry,
                    requests,
                    load,
                );
            });
        match thread {
            Ok(thread) => Some(Self {
                lane,
                prepare,
                status,
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

    /// Called only during host preparation, before audio processing resumes.
    /// The new generation retires any old publisher before this request waits.
    pub fn prepare_layout(&self, channels: usize, generation: u64) {
        self.lane.generation.store(generation, Ordering::Release);
        let (complete, result) = mpsc::sync_channel(1);
        let request = Preparation {
            generation,
            channels,
            complete,
        };
        let ready = self.prepare.send(request).is_ok() && result.recv().is_ok_and(|ready| ready);
        if !ready {
            let _ = self
                .status
                .set_for_generation(generation, ProcessingState::Failed);
        }
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

    #[cfg(test)]
    pub fn with_prepared_factory<R: Renderer + 'static>(
        sample_rate: f64,
        geometry: Geometry,
        status: Arc<ProcessingStatus>,
        queue_packets: usize,
        channels: usize,
        load: impl FnMut(usize) -> Result<R, DspError> + Send + 'static,
    ) -> Self {
        let generation = status.generation();
        let worker = Self::with_factory(sample_rate, geometry, status, queue_packets, load);
        worker.prepare_layout(channels, generation);
        worker
    }
}

fn run<R: Renderer>(
    lane: &Lane,
    status: &ProcessingStatus,
    sample_rate: f64,
    geometry: Geometry,
    requests: Receiver<Preparation>,
    mut load: impl FnMut(usize) -> Result<R, DspError>,
) {
    let mut channels = 0;
    let mut generation = 0;
    let mut failed_generation = None;
    let mut stream: Option<Stream<R>> = None;
    while !lane.stop.load(Ordering::Acquire) {
        if let Ok(request) = requests.try_recv() {
            if request.generation != lane.generation.load(Ordering::Acquire) {
                let _ = request.complete.send(false);
                continue;
            }
            generation = request.generation;
            failed_generation = None;
            if stream.is_none() || channels != request.channels {
                let _ = status.set_for_generation(generation, ProcessingState::Loading);
                drop(stream.take());
                stream = prepare(&mut load, request.channels, sample_rate, geometry).ok();
                channels = request.channels;
            } else if let Some(stream) = stream.as_mut() {
                stream.reset();
            }
            let ready = stream.is_some();
            let state = if ready {
                ProcessingState::Ready
            } else {
                failed_generation = Some(generation);
                ProcessingState::Failed
            };
            let _ = status.set_for_generation(generation, state);
            let _ = request.complete.send(ready);
            continue;
        }
        let Some(packet) = lane.input.pop() else {
            thread::sleep(Duration::from_millis(1));
            continue;
        };
        if packet.generation != lane.generation.load(Ordering::Acquire)
            || failed_generation == Some(packet.generation)
        {
            continue;
        }
        if packet.generation != generation {
            if let Some(stream) = stream.as_mut() {
                stream.reset();
            }
            generation = packet.generation;
            failed_generation = None;
        }
        if stream.is_none() || packet.channels != channels {
            if !(1..=2).contains(&packet.channels) {
                if status.set_for_generation(generation, ProcessingState::Failed) {
                    failed_generation = Some(generation);
                }
                continue;
            }
            let _ = status.set_for_generation(generation, ProcessingState::Loading);
            // Adapters without a prepared layout choose batch one or two from
            // the first packet. A later layout change retires old sessions here.
            drop(stream.take());
            let prepared = prepare(&mut load, packet.channels, sample_rate, geometry);
            if lane.stop.load(Ordering::Acquire) {
                return;
            }
            let Ok(prepared) = prepared else {
                if status.set_for_generation(generation, ProcessingState::Failed) {
                    failed_generation = Some(generation);
                }
                continue;
            };
            stream = Some(prepared);
            channels = packet.channels;
        }
        if packet.generation != lane.generation.load(Ordering::Acquire) {
            continue;
        }
        let _ = status.set_for_generation(generation, ProcessingState::Ready);
        let Some(active) = stream.as_mut() else {
            continue;
        };
        active.push(&packet);
        let rendered = match active.render_ready() {
            Ok(Some(rendered)) => rendered,
            Ok(None) => continue,
            Err(_) => {
                if status.set_for_generation(generation, ProcessingState::Failed) {
                    failed_generation = Some(generation);
                    // A genuine failure retires these sessions. Keep the worker
                    // alive so a concurrent host reset can enqueue a new region
                    // rather than racing this thread's termination.
                    stream = None;
                    channels = 0;
                }
                continue;
            }
        };
        'publish: for (index, chunk) in rendered.samples.chunks(PACKET_FRAMES).enumerate() {
            if generation != lane.generation.load(Ordering::Acquire) {
                break;
            }
            let mut packet = Packet::new(
                rendered.start + (index * PACKET_FRAMES) as u64,
                channels,
                0.0,
            );
            packet.generation = generation;
            packet.len = chunk.len();
            packet.samples[..chunk.len()].copy_from_slice(chunk);
            while let Err(returned) = lane.output.push(packet) {
                if lane.stop.load(Ordering::Acquire) {
                    return;
                }
                if generation != lane.generation.load(Ordering::Acquire) {
                    break 'publish;
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
