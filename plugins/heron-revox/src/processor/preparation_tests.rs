use super::*;
use crate::stream::Renderer;
use heron_revox_dsp::DspError;
use std::sync::{
    Mutex,
    atomic::AtomicUsize,
    mpsc::{self, Receiver, SyncSender},
};
use truce_test::assert_realtime_clean;

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Evidence {
    layouts: Mutex<Vec<usize>>,
    warmups: AtomicUsize,
    live_renders: AtomicUsize,
}

struct WarmupGate {
    entered: SyncSender<()>,
    release: Receiver<()>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Failure {
    Load,
    Warmup,
}

struct Double {
    evidence: Arc<Evidence>,
    owner: thread::ThreadId,
    warmup: bool,
    gate: Option<WarmupGate>,
    fail_warmup: bool,
}

impl Renderer for Double {
    fn render_channels(&mut self, input: &[Vec<f32>], _: f32) -> Result<Vec<Vec<f32>>, DspError> {
        assert_eq!(thread::current().id(), self.owner);
        if self.warmup {
            self.warmup = false;
            self.evidence.warmups.fetch_add(1, Ordering::Relaxed);
            if let Some(gate) = self.gate.take() {
                let _ = gate.entered.send(());
                gate.release
                    .recv_timeout(TIMEOUT)
                    .map_err(|_| DspError::InputShape)?;
            }
            if self.fail_warmup {
                return Err(DspError::InputShape);
            }
        } else {
            self.evidence.live_renders.fetch_add(1, Ordering::Relaxed);
        }
        Ok(input
            .iter()
            .map(|channel| channel.iter().map(|sample| sample * 2.0).collect())
            .collect())
    }
}

fn prepare(
    params: Arc<RevoxParams>,
    evidence: Arc<Evidence>,
    channels: usize,
    gate: Option<WarmupGate>,
    failure: Option<Failure>,
) -> (Receiver<RevoxDspState>, thread::JoinHandle<()>) {
    let geometry = Geometry::new(48_000.0).unwrap();
    params.status.configure(48_000.0, geometry.latency as u32);
    let (send, receive) = mpsc::sync_channel(1);
    let control = thread::spawn(move || {
        let control_thread = thread::current().id();
        let mut gate = gate;
        let worker = Worker::with_prepared_factory(
            48_000.0,
            geometry,
            params.status.clone(),
            2048,
            channels,
            move |layout| {
                let owner = thread::current().id();
                assert_ne!(
                    owner, control_thread,
                    "model load ran on the control thread"
                );
                evidence.layouts.lock().unwrap().push(layout);
                if failure == Some(Failure::Load) {
                    return Err(DspError::InputShape);
                }
                Ok(Double {
                    evidence: evidence.clone(),
                    owner,
                    warmup: true,
                    gate: gate.take(),
                    fail_warmup: failure == Some(Failure::Warmup),
                })
            },
        );
        let state = RevoxDspState {
            lane: Some(worker.lane.clone()),
            worker: Some(worker),
            latency: geometry.latency,
            sample_rate: 48_000.0,
            generation: params.status.generation(),
            prepared_channels: (params.status.state() == ProcessingState::Ready)
                .then_some(channels),
            ..RevoxDspState::default()
        };
        let _ = send.send(state);
    });
    (receive, control)
}

fn finish_prepare(
    receive: Receiver<RevoxDspState>,
    control: thread::JoinHandle<()>,
) -> RevoxDspState {
    // A failed preparation publication must fail this test without joining a
    // control thread that is still waiting inside the constructor.
    let state = receive
        .recv_timeout(TIMEOUT)
        .expect("control preparation did not finish");
    control.join().unwrap();
    state
}

fn region(
    mut state: RevoxDspState,
    params: Arc<RevoxParams>,
    channels: usize,
    input: [f32; 2],
) -> RevoxDspState {
    let lane = state.lane.as_ref().unwrap().clone();
    let latency = state.latency;
    let (send, receive) = mpsc::sync_channel(1);
    let callback = thread::spawn(move || {
        let mut output = vec![[f32::NAN; 2]; latency + PACKET_FRAMES];
        assert_realtime_clean(|| {
            let _section = truce::rt::RtSection::enter();
            for frame in &mut output {
                *frame = state.sample(&params, input, channels);
            }
        });
        let _ = send.send((state, output));
    });
    let completed = receive.recv_timeout(TIMEOUT);
    if completed.is_err() {
        lane.stop.store(true, Ordering::Release);
    }
    callback.join().unwrap();
    let (state, output) = completed.expect("prepared audio did not produce its delayed output");
    assert!(output[..latency].iter().all(|frame| *frame == [0.0; 2]));
    for frame in &output[latency..] {
        for channel in 0..2 {
            let expected = if channel < channels {
                input[channel] * 2.0
            } else {
                0.0
            };
            assert!((frame[channel] - expected).abs() < 1e-7);
        }
    }
    state
}

#[test]
fn known_layout_waits_for_worker_warmup_before_audio_and_same_layout_reset_reuses_it() {
    let params = Arc::new(RevoxParams::default());
    let evidence = Arc::new(Evidence::default());
    let (entered_send, entered_receive) = mpsc::sync_channel(1);
    let (release_send, release_receive) = mpsc::sync_channel(1);
    let (receive, control) = prepare(
        params.clone(),
        evidence.clone(),
        2,
        Some(WarmupGate {
            entered: entered_send,
            release: release_receive,
        }),
        None,
    );
    entered_receive
        .recv_timeout(TIMEOUT)
        .expect("worker warmup did not start before live input");
    let early_return = receive.recv_timeout(Duration::from_millis(50));
    let warmup_state = params.status.state();
    let live_renders = evidence.live_renders.load(Ordering::Relaxed);
    release_send.send(()).unwrap();
    assert!(matches!(early_return, Err(mpsc::RecvTimeoutError::Timeout)));
    assert_eq!(warmup_state, ProcessingState::Loading);
    assert_eq!(live_renders, 0);
    let state = finish_prepare(receive, control);
    assert_eq!(params.status.state(), ProcessingState::Ready);
    assert_eq!(*evidence.layouts.lock().unwrap(), [2]);
    assert_eq!(evidence.warmups.load(Ordering::Relaxed), 1);

    let lane = state.lane.as_ref().unwrap().clone();
    let mut state = region(state, params.clone(), 2, [0.125, -0.25]);
    assert_realtime_clean(|| {
        let _section = truce::rt::RtSection::enter();
        state.reset(&params, 48_000.0, Some(2));
    });
    assert!(Arc::ptr_eq(state.lane.as_ref().unwrap(), &lane));
    assert_eq!(params.status.state(), ProcessingState::Ready);
    let _state = region(state, params.clone(), 2, [-0.375, 0.0625]);
    assert_eq!(*evidence.layouts.lock().unwrap(), [2]);
    assert_eq!(evidence.warmups.load(Ordering::Relaxed), 1);
    assert!(evidence.live_renders.load(Ordering::Relaxed) >= 2);
}

#[test]
fn known_layout_change_prepares_only_the_new_batch_before_the_next_audio_region() {
    let params = Arc::new(RevoxParams::default());
    let evidence = Arc::new(Evidence::default());
    let (receive, control) = prepare(params.clone(), evidence.clone(), 2, None, None);
    let mut state = finish_prepare(receive, control);
    assert_eq!(*evidence.layouts.lock().unwrap(), [2]);
    assert_eq!(evidence.live_renders.load(Ordering::Relaxed), 0);

    state.reset(&params, 48_000.0, Some(1));
    assert_eq!(params.status.state(), ProcessingState::Ready);
    assert_eq!(*evidence.layouts.lock().unwrap(), [2, 1]);
    assert_eq!(evidence.warmups.load(Ordering::Relaxed), 2);
    assert_eq!(evidence.live_renders.load(Ordering::Relaxed), 0);
    let _state = region(state, params, 1, [0.25, 0.0]);
    assert_eq!(*evidence.layouts.lock().unwrap(), [2, 1]);
    assert_eq!(evidence.warmups.load(Ordering::Relaxed), 2);
}

#[test]
fn initial_load_or_warmup_failure_is_terminal_before_audio_without_a_late_retry() {
    for failure in [Failure::Load, Failure::Warmup] {
        let params = Arc::new(RevoxParams::default());
        let evidence = Arc::new(Evidence::default());
        let (receive, control) = prepare(params.clone(), evidence.clone(), 1, None, Some(failure));
        let mut state = finish_prepare(receive, control);
        assert_eq!(params.status.state(), ProcessingState::Failed);
        assert!(state.worker.as_ref().unwrap().is_running());
        for _ in 0..state.latency + PACKET_FRAMES {
            assert_eq!(state.sample(&params, [0.25, 0.0], 1), [0.0; 2]);
        }
        assert_eq!(params.status.state(), ProcessingState::Failed);
        assert_eq!(*evidence.layouts.lock().unwrap(), [1]);
        assert_eq!(
            evidence.warmups.load(Ordering::Relaxed),
            usize::from(failure == Failure::Warmup)
        );
        assert_eq!(evidence.live_renders.load(Ordering::Relaxed), 0);
    }
}
