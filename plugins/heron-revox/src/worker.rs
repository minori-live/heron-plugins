//! Fixed-capacity audio exchange. Only this worker loads models and runs inference.

use crate::{
    ProcessingState, ProcessingStatus, models,
    stream::{Geometry, MODEL_WINDOW, Renderer, Stream},
};
use crossbeam_queue::ArrayQueue;
use heron_revox_dsp::DspError;
use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
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
    // Diagnostic branch only: the callback publishes bounded counters; only the
    // worker reads clocks or writes diagnostics. These never affect scheduling.
    pub diagnostics: bool,
    pub callback_position: AtomicU64,
    pub dropped_input_packets: AtomicU64,
    pub expired_output_packets: AtomicU64,
    pub expired_output_frames: AtomicU64,
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
            diagnostics: std::env::var_os("HERON_REVOX_DIAGNOSTICS").as_deref()
                == Some(std::ffi::OsStr::new("1")),
            callback_position: AtomicU64::new(0),
            dropped_input_packets: AtomicU64::new(0),
            expired_output_packets: AtomicU64::new(0),
            expired_output_frames: AtomicU64::new(0),
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
    let mut live_seq = 0_u64;
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
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    if lane.diagnostics {
                        let _ = writeln!(
                            std::io::stderr(),
                            "REVOX_WORKER event=prepare_error channels={} error={error:?}",
                            packet.channels
                        );
                    }
                    status.set(ProcessingState::Failed);
                    return;
                }
            };
            stream = Some(prepared);
            channels = packet.channels;
            live_seq = 0;
            status.set(ProcessingState::Ready);
        }
        let Some(stream) = stream.as_mut() else {
            status.set(ProcessingState::Failed);
            return;
        };
        stream.push(&packet);
        let started = lane.diagnostics.then(Instant::now);
        let rendered = match stream.render_ready() {
            Ok(Some(rendered)) => rendered,
            Ok(None) => continue,
            Err(error) => {
                if lane.diagnostics {
                    let _ = writeln!(
                        std::io::stderr(),
                        "REVOX_WORKER event=render_error live_seq={} channels={channels} error={error:?}",
                        live_seq + 1
                    );
                }
                status.set(ProcessingState::Failed);
                return;
            }
        };
        live_seq += 1;
        let render_ms = started.map(|start| start.elapsed().as_secs_f64() * 1000.0);
        let completion_position = lane.callback_position.load(Ordering::Relaxed);
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
        if let Some(started) = started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let deadline = rendered.start + geometry.latency as u64;
            let ahead_ms = (deadline as f64 - completion_position as f64) * 1000.0 / sample_rate;
            let _ = writeln!(
                std::io::stderr(),
                "REVOX_WORKER event=window live_seq={live_seq} channels={channels} output_start={} output_frames={} render_ms={:.3} publish_ms={:.3} completion_position={completion_position} start_deadline_ahead_ms={ahead_ms:.3} expired_frames_at_completion={} input_queue_packets={} output_queue_packets={} dropped_input_packets={} expired_output_packets={} expired_output_frames={}",
                rendered.start,
                rendered.samples.len(),
                render_ms.unwrap_or_default(),
                elapsed_ms - render_ms.unwrap_or_default(),
                completion_position
                    .saturating_sub(deadline)
                    .min(rendered.samples.len() as u64),
                lane.input.len(),
                lane.output.len(),
                lane.dropped_input_packets.load(Ordering::Relaxed),
                lane.expired_output_packets.load(Ordering::Relaxed),
                lane.expired_output_frames.load(Ordering::Relaxed),
            );
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
