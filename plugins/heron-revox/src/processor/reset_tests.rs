use super::*;
use crate::{ProcessingStatus, stream::Renderer};
use heron_revox_dsp::DspError;
use std::{
    sync::{
        Mutex,
        atomic::AtomicUsize,
        mpsc::{self, Receiver, SyncSender},
    },
    time::Instant,
};
use truce::prelude::Params;

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Preparation {
    layouts: Mutex<Vec<usize>>,
    warmups: AtomicUsize,
}

struct InFlight {
    entered: SyncSender<()>,
    release: Receiver<()>,
    fail: bool,
    warmup: bool,
}

impl InFlight {
    fn wait(self) -> Result<(), DspError> {
        let _ = self.entered.send(());
        self.release
            .recv_timeout(TIMEOUT)
            .map_err(|_| DspError::InputShape)?;
        if self.fail {
            Err(DspError::InputShape)
        } else {
            Ok(())
        }
    }
}

struct HistoryGain {
    preparation: Arc<Preparation>,
    warmup: bool,
    history: bool,
    in_flight: Option<InFlight>,
}

impl Renderer for HistoryGain {
    fn reset_stream(&mut self) {
        self.history = false;
    }

    fn render_channels(
        &mut self,
        input: &[Vec<f32>],
        shift: f32,
    ) -> Result<Vec<Vec<f32>>, DspError> {
        let warmup = self.warmup;
        if warmup {
            self.warmup = false;
            self.preparation.warmups.fetch_add(1, Ordering::Relaxed);
        }
        if self
            .in_flight
            .as_ref()
            .is_some_and(|in_flight| in_flight.warmup == warmup)
        {
            self.in_flight.take().unwrap().wait()?;
        }
        let gain = 2.0 + shift + if self.history { 0.5 } else { 0.0 };
        self.history = true;
        Ok(input
            .iter()
            .map(|channel| channel.iter().map(|sample| sample * gain).collect())
            .collect())
    }
}

fn prepared(
    params: &RevoxParams,
    preparation: Arc<Preparation>,
    in_flight: Option<InFlight>,
) -> RevoxDspState {
    prepared_with_capacity(params, preparation, in_flight, 2048)
}

fn prepared_with_capacity(
    params: &RevoxParams,
    preparation: Arc<Preparation>,
    in_flight: Option<InFlight>,
    queue_packets: usize,
) -> RevoxDspState {
    let mut state = RevoxDspState::default();
    // Initialize the real host configuration, then replace its idle worker at
    // the model boundary. No input has been queued, so it cannot load a model.
    state.reset(params, 48_000.0, None);
    let geometry = Geometry::new(48_000.0).unwrap();
    let mut in_flight = in_flight;
    let worker = Worker::with_factory(
        48_000.0,
        geometry,
        params.status.clone(),
        queue_packets,
        move |channels| {
            preparation.layouts.lock().unwrap().push(channels);
            Ok(HistoryGain {
                preparation: preparation.clone(),
                warmup: true,
                history: false,
                in_flight: in_flight.take(),
            })
        },
    );
    state.lane = Some(worker.lane.clone());
    state.worker = Some(worker);
    state
}

fn source(index: usize, base: [f32; 2], channels: usize) -> [f32; 2] {
    std::array::from_fn(|channel| {
        if channel < channels {
            base[channel] + index as f32 / 1_000_000.0
        } else {
            0.0
        }
    })
}

fn region(
    mut state: RevoxDspState,
    params: Arc<RevoxParams>,
    base: [f32; 2],
    channels: usize,
) -> (RevoxDspState, Vec<[f32; 2]>) {
    let lane = state.lane.as_ref().unwrap().clone();
    let (send, receive) = mpsc::sync_channel(1);
    let callback = thread::spawn(move || {
        let output = (0..state.latency + PACKET_FRAMES)
            .map(|index| state.sample(&params, source(index, base, channels), channels))
            .collect();
        let _ = send.send((state, output));
    });
    let completed = receive.recv_timeout(TIMEOUT);
    if completed.is_err() {
        lane.stop.store(true, Ordering::Release);
    }
    callback.join().unwrap();
    completed.expect("new audio region did not produce its delayed wet output")
}

fn assert_fresh_region(
    output: &[[f32; 2]],
    latency: usize,
    base: [f32; 2],
    channels: usize,
    shift: f32,
) {
    assert!(output[..latency].iter().all(|frame| *frame == [0.0; 2]));
    for (index, actual) in output[latency..].iter().enumerate() {
        let expected = source(index, base, channels).map(|sample| sample * (2.0 + shift));
        for channel in 0..2 {
            assert!(
                (actual[channel] - expected[channel]).abs() < 3e-7,
                "sample {index}, channel {channel}: {} != {}",
                actual[channel],
                expected[channel]
            );
        }
    }
}

#[test]
fn same_rate_reset_reuses_models_but_starts_fresh_audio_controls_and_source_history() {
    let params = Arc::new(RevoxParams::default());
    let preparation = Arc::new(Preparation::default());
    let state = prepared(&params, preparation.clone(), None);
    let original_lane = state.lane.as_ref().unwrap().clone();
    let (mut state, output) = region(state, params.clone(), [0.125, -0.25], 2);
    let latency = state.latency;
    assert_fresh_region(&output, latency, [0.125, -0.25], 2, 0.0);
    let old_generation = params.status.generation();

    // An old publisher can be waiting for output capacity when the host resets.
    // The new preroll must retire that full queue before waiting for new wet.
    loop {
        let mut retired = Packet::new(0, 2, 0.0);
        retired.generation = old_generation;
        retired.len = PACKET_FRAMES;
        retired.samples.fill([0.95, -0.85]);
        if original_lane.output.push(retired).is_err() {
            break;
        }
    }
    assert!(original_lane.output.is_full());

    params.set_plain(0, 2.0);
    params.set_plain(1, 50.0);
    state.reset(&params, 48_000.0, None);
    assert!(Arc::ptr_eq(state.lane.as_ref().unwrap(), &original_lane));
    assert_ne!(params.status.generation(), old_generation);
    assert_eq!(state.latency, latency);
    let (mut state, output) = region(state, params.clone(), [-0.375, 0.0625], 2);
    assert_fresh_region(&output, latency, [-0.375, 0.0625], 2, 2.5);
    assert_eq!(*preparation.layouts.lock().unwrap(), [2]);
    assert_eq!(preparation.warmups.load(Ordering::Relaxed), 1);
    assert_eq!(params.status.state(), ProcessingState::Ready);

    // A real channel-layout change still prepares the matching model batch.
    params.set_plain(0, -1.0);
    params.set_plain(1, 0.0);
    let layout_generation = params.status.generation();
    let (new_state, output) = region(state, params.clone(), [0.3, 0.0], 1);
    state = new_state;
    assert_fresh_region(&output, latency, [0.3, 0.0], 1, -1.0);
    assert_ne!(params.status.generation(), layout_generation);
    assert_eq!(*preparation.layouts.lock().unwrap(), [2, 1]);
    assert_eq!(preparation.warmups.load(Ordering::Relaxed), 2);

    // A rate change replaces the worker and publishes that rate's latency.
    state.reset(&params, 44_100.0, None);
    assert!(!Arc::ptr_eq(state.lane.as_ref().unwrap(), &original_lane));
    assert!(original_lane.stop.load(Ordering::Acquire));
    assert_eq!(params.status.sample_rate(), 44_100.0);
    assert_eq!(state.latency, Geometry::new(44_100.0).unwrap().latency);
    assert_eq!(state.sample(&params, [0.9, -0.8], 2), [0.0; 2]);
}

#[test]
fn reset_during_old_inference_retires_its_result_or_failure_without_poisoning_new_audio() {
    for fail in [false, true] {
        let params = Arc::new(RevoxParams::default());
        let preparation = Arc::new(Preparation::default());
        let (entered_send, entered_receive) = mpsc::sync_channel(1);
        let (release_send, release_receive) = mpsc::sync_channel(1);
        let mut state = prepared(
            &params,
            preparation.clone(),
            Some(InFlight {
                entered: entered_send,
                release: release_receive,
                fail,
                warmup: false,
            }),
        );
        let lane = state.lane.as_ref().unwrap().clone();
        let geometry = Geometry::new(48_000.0).unwrap();
        let required = geometry.context + geometry.hop + geometry.overlap;
        for index in 0..required {
            assert_eq!(
                state.sample(&params, source(index, [0.4, -0.3], 2), 2),
                [0.0; 2]
            );
        }
        entered_receive
            .recv_timeout(TIMEOUT)
            .expect("old inference never started");
        // Leave an old packet queued as well as the in-flight crop starting at
        // sample zero, which collides with the new generation's sample zero.
        for index in required..required + PACKET_FRAMES {
            assert_eq!(
                state.sample(&params, source(index, [0.4, -0.3], 2), 2),
                [0.0; 2]
            );
        }
        let old_generation = params.status.generation();
        state.reset(&params, 48_000.0, None);
        assert!(Arc::ptr_eq(state.lane.as_ref().unwrap(), &lane));
        assert_ne!(params.status.generation(), old_generation);
        assert_eq!(preparation.warmups.load(Ordering::Relaxed), 1);
        release_send.send(()).unwrap();

        params.set_plain(0, -1.0);
        let (state, output) = region(state, params.clone(), [-0.2, 0.1], 2);
        assert_fresh_region(&output, state.latency, [-0.2, 0.1], 2, -1.0);
        assert_eq!(
            params.status.state(),
            ProcessingState::Ready,
            "old failure={fail}"
        );
        assert_eq!(*preparation.layouts.lock().unwrap(), [2]);
        assert_eq!(preparation.warmups.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn reset_during_model_warmup_keeps_the_prepared_sessions_and_new_region_processable() {
    let params = Arc::new(RevoxParams::default());
    let preparation = Arc::new(Preparation::default());
    let (entered_send, entered_receive) = mpsc::sync_channel(1);
    let (release_send, release_receive) = mpsc::sync_channel(1);
    let mut state = prepared(
        &params,
        preparation.clone(),
        Some(InFlight {
            entered: entered_send,
            release: release_receive,
            fail: false,
            warmup: true,
        }),
    );
    let lane = state.lane.as_ref().unwrap().clone();
    for _ in 0..PACKET_FRAMES {
        assert_eq!(state.sample(&params, [0.4, -0.3], 2), [0.0; 2]);
    }
    entered_receive
        .recv_timeout(TIMEOUT)
        .expect("model warmup never started");
    assert_eq!(params.status.state(), ProcessingState::Loading);
    let old_generation = params.status.generation();
    state.reset(&params, 48_000.0, None);
    assert!(Arc::ptr_eq(state.lane.as_ref().unwrap(), &lane));
    assert_ne!(params.status.generation(), old_generation);
    release_send.send(()).unwrap();

    let (state, output) = region(state, params.clone(), [-0.2, 0.1], 2);
    assert_fresh_region(&output, state.latency, [-0.2, 0.1], 2, 0.0);
    assert_eq!(params.status.state(), ProcessingState::Ready);
    assert_eq!(*preparation.layouts.lock().unwrap(), [2]);
    assert_eq!(preparation.warmups.load(Ordering::Relaxed), 1);
}

#[test]
fn retired_worker_status_updates_cannot_overwrite_new_host_configuration() {
    let status = ProcessingStatus::default();
    let (old_generation, previous) = status.configure(48_000.0, 63_488);
    assert_eq!(previous, ProcessingState::Loading);
    assert!(status.set_for_generation(old_generation, ProcessingState::Ready));
    let (current_generation, previous) = status.configure(44_100.0, 58_340);
    assert_eq!(previous, ProcessingState::Ready);
    assert!(!status.set_for_generation(old_generation, ProcessingState::Failed));
    assert_eq!(status.state(), ProcessingState::Loading);
    assert_eq!(status.sample_rate(), 44_100.0);
    assert_eq!(status.latency_samples(), 58_340);
    assert!(status.set_for_generation(current_generation, ProcessingState::Ready));
    assert!(!status.set_for_generation(old_generation, ProcessingState::Loading));
    assert_eq!(status.state(), ProcessingState::Ready);
    assert!(status.set_for_generation(current_generation, ProcessingState::Failed));
    let (next_generation, previous) = status.configure(44_100.0, 58_340);
    assert_eq!(previous, ProcessingState::Failed);
    assert_ne!(next_generation, current_generation);
    assert!(!status.set_for_generation(current_generation, ProcessingState::Ready));
}

fn wait_until(label: &str, condition: impl Fn() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(start.elapsed() < TIMEOUT, "{label}");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn reset_releases_old_output_publication_without_waiting_for_callback_retirement() {
    let params = RevoxParams::default();
    let preparation = Arc::new(Preparation::default());
    let mut state = prepared_with_capacity(&params, preparation.clone(), None, 1);
    let lane = state.lane.as_ref().unwrap().clone();
    let geometry = Geometry::new(48_000.0).unwrap();
    for _ in 0..geometry.context + geometry.hop + geometry.overlap {
        assert_eq!(state.sample(&params, [0.4, -0.3], 2), [0.0; 2]);
    }
    wait_until("old output publication never filled its queue", || {
        lane.output.is_full()
    });
    assert_eq!(params.status.state(), ProcessingState::Ready);
    let old_generation = params.status.generation();
    state.reset(&params, 48_000.0, None);
    assert!(Arc::ptr_eq(state.lane.as_ref().unwrap(), &lane));
    assert_ne!(params.status.generation(), old_generation);
    assert_eq!(params.status.state(), ProcessingState::Loading);

    let mut new_input = Packet::new(0, 2, 0.0);
    new_input.generation = params.status.generation();
    new_input.len = PACKET_FRAMES;
    new_input.samples.fill([-0.2, 0.1]);
    assert!(lane.input.push(new_input).is_ok());
    // No callback runs and the old output slot remains occupied. The worker
    // must leave the retired publication loop to consume this new input.
    wait_until(
        "old output publication prevented the new region from starting",
        || lane.input.is_empty() && params.status.state() == ProcessingState::Ready,
    );
    assert!(lane.output.is_full());
    assert_eq!(*preparation.layouts.lock().unwrap(), [2]);
    assert_eq!(preparation.warmups.load(Ordering::Relaxed), 1);
}
