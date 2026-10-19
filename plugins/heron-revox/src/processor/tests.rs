use super::*;
use crate::{HeronRevox, stream::Renderer};
use heron_revox_dsp::DspError;
use std::sync::{
    Mutex,
    atomic::AtomicUsize,
    mpsc::{self, Receiver, SyncSender},
};
use truce::prelude::{
    EventList, Params, PluginLogic, ProcessContext, ProcessMode, ProcessStatus, TransportInfo,
};
use truce_test::assert_realtime_clean;

const TIMEOUT: Duration = Duration::from_secs(3);

fn geometry(latency: usize) -> Geometry {
    Geometry {
        context: 32,
        hop: 128,
        overlap: 32,
        latency,
    }
}

fn prepared<R: Renderer + 'static>(
    params: &RevoxParams,
    latency: usize,
    load: impl FnMut(usize) -> Result<R, DspError> + Send + 'static,
) -> RevoxDspState {
    params.status.configure(48_000.0, latency as u32);
    let worker = Worker::with_factory(48_000.0, geometry(latency), params.status.clone(), 4, load);
    RevoxDspState {
        lane: Some(worker.lane.clone()),
        worker: Some(worker),
        latency,
        sample_rate: 48_000.0,
        generation: params.status.generation(),
        ..RevoxDspState::default()
    }
}

struct Gate {
    entered: SyncSender<()>,
    release: Receiver<()>,
}

fn gate() -> (Gate, Receiver<()>, SyncSender<()>) {
    let (entered_send, entered_receive) = mpsc::sync_channel(1);
    let (release_send, release_receive) = mpsc::sync_channel(1);
    (
        Gate {
            entered: entered_send,
            release: release_receive,
        },
        entered_receive,
        release_send,
    )
}

impl Gate {
    fn wait(self) -> Result<(), DspError> {
        let _ = self.entered.send(());
        self.release
            .recv_timeout(TIMEOUT)
            .map_err(|_| DspError::InputShape)
    }
}

struct Double;

impl Renderer for Double {
    fn render_channels(&mut self, input: &[Vec<f32>], _: f32) -> Result<Vec<Vec<f32>>, DspError> {
        Ok(input
            .iter()
            .map(|channel| channel.iter().map(|sample| sample * 2.0).collect())
            .collect())
    }
}

fn callback_result<R: Send + 'static>(
    lane: &Arc<Lane>,
    callback: impl FnOnce() -> R + Send + 'static,
) -> R {
    let (send, receive) = mpsc::sync_channel(1);
    let callback = thread::spawn(move || {
        let _ = send.send(callback());
    });
    let completed = receive.recv_timeout(TIMEOUT);
    // A regression in either wait loop must fail without hanging the suite.
    lane.stop.store(true, Ordering::Release);
    callback.join().unwrap();
    completed.expect("audio callback did not finish")
}

fn process_block<'a>(
    state: &mut RevoxDspState,
    params: &RevoxParams,
    mode: ProcessMode,
    input: &'a [&'a [f32]; 2],
    output: &'a mut [&'a mut [f32]; 2],
) {
    let length = input[0].len();
    let mut buffer = AudioBuffer::from_slices_checked(input, output, length);
    let events = EventList::with_capacity(0);
    let mut output_events = EventList::with_capacity(0);
    let transport = TransportInfo::default();
    let mut context = ProcessContext::new(&transport, 48_000.0, length, &mut output_events)
        .with_process_mode(mode);
    assert_realtime_clean(|| {
        let _section = truce::rt::RtSection::enter();
        assert_eq!(
            HeronRevox::process(state, params, &mut buffer, &events, &mut context),
            ProcessStatus::Normal
        );
    });
}

fn signal(index: usize) -> [f32; 2] {
    [
        0.125 + index as f32 / 8192.0,
        -(0.0625 + index as f32 / 16384.0),
    ]
}

#[test]
fn all_host_modes_wait_for_loading_and_inference_and_produce_identical_wet_audio() {
    struct GatedDouble {
        warmup: bool,
        inference: Option<Gate>,
    }
    impl Renderer for GatedDouble {
        fn render_channels(
            &mut self,
            input: &[Vec<f32>],
            shift: f32,
        ) -> Result<Vec<Vec<f32>>, DspError> {
            if self.warmup {
                self.warmup = false;
            } else if let Some(gate) = self.inference.take() {
                gate.wait()?;
            }
            Double.render_channels(input, shift)
        }
    }

    let mut reference = None;
    for mode in [
        ProcessMode::Realtime,
        ProcessMode::Buffered,
        ProcessMode::Offline,
    ] {
        let params = Arc::new(RevoxParams::default());
        let (loading, loading_entered, loading_release) = gate();
        let (inference, inference_entered, inference_release) = gate();
        let mut gates = Some((loading, inference));
        let mut state = prepared(&params, 256, move |_| {
            let (loading, inference) = gates.take().unwrap();
            loading.wait()?;
            Ok(GatedDouble {
                warmup: true,
                inference: Some(inference),
            })
        });
        assert_eq!(HeronRevox::latency(&state), 256);
        let lane = state.lane.as_ref().unwrap().clone();
        let callback_params = params.clone();
        let (preroll_send, preroll_receive) = mpsc::sync_channel(1);
        let (send, receive) = mpsc::sync_channel(1);
        let callback = thread::spawn(move || {
            let input: [[f32; 1024]; 2] =
                std::array::from_fn(|channel| std::array::from_fn(|index| signal(index)[channel]));
            let mut output = [[f32::NAN; 1024]; 2];
            let [left, right] = &mut output;
            process_block(
                &mut state,
                &callback_params,
                mode,
                &[&input[0][..256], &input[1][..256]],
                &mut [&mut left[..256], &mut right[..256]],
            );
            let _ = preroll_send.send(());
            process_block(
                &mut state,
                &callback_params,
                mode,
                &[&input[0][256..], &input[1][256..]],
                &mut [&mut left[256..], &mut right[256..]],
            );
            let _ = send.send(output);
        });

        let entered_loading = loading_entered.recv_timeout(TIMEOUT).is_ok();
        let preroll_completed = preroll_receive.recv_timeout(TIMEOUT).is_ok();
        let loading_state = params.status.state();
        let mut completed = receive.recv_timeout(Duration::from_millis(50)).ok();
        let waited_loading = completed.is_none();
        let _ = loading_release.send(());
        let entered_inference = waited_loading && inference_entered.recv_timeout(TIMEOUT).is_ok();
        let inference_state = params.status.state();
        if completed.is_none() {
            completed = receive.recv_timeout(Duration::from_millis(50)).ok();
        }
        let waited_inference = completed.is_none();
        let _ = inference_release.send(());
        if completed.is_none() {
            completed = receive.recv_timeout(TIMEOUT).ok();
        }
        lane.stop.store(true, Ordering::Release);
        callback.join().unwrap();

        assert!(
            entered_loading && preroll_completed,
            "{mode:?}: loading/preroll failed"
        );
        assert_eq!(loading_state, ProcessingState::Loading);
        assert!(
            waited_loading,
            "{mode:?}: emitted audio while the model was loading"
        );
        assert!(
            entered_inference && waited_inference,
            "{mode:?}: bypassed pending inference"
        );
        assert_eq!(inference_state, ProcessingState::Ready);
        let output = completed.expect("processing did not resume after inference was released");
        for (channel, samples) in output.iter().enumerate() {
            for (index, actual) in samples.iter().enumerate() {
                let expected = if index < 256 {
                    0.0
                } else {
                    signal(index - 256)[channel] * 2.0
                };
                assert!(
                    (actual - expected).abs() < 6e-8,
                    "{mode:?}, sample {index}, channel {channel}: {actual} != {expected}",
                );
            }
        }
        if let Some(reference) = &reference {
            assert_eq!(&output, reference, "host mode changed rendered audio");
        } else {
            reference = Some(output);
        }
    }
}

#[test]
fn full_input_queue_waits_without_losing_audio_or_its_original_pitch_timestamp() {
    struct ShiftGain;
    impl Renderer for ShiftGain {
        fn render_channels(
            &mut self,
            input: &[Vec<f32>],
            shift: f32,
        ) -> Result<Vec<Vec<f32>>, DspError> {
            Ok(input
                .iter()
                .map(|channel| {
                    channel
                        .iter()
                        .map(|sample| sample * (2.0 + shift))
                        .collect()
                })
                .collect())
        }
    }

    let params = Arc::new(RevoxParams::default());
    let (loading, loading_entered, loading_release) = gate();
    let mut loading = Some(loading);
    let mut state = prepared(&params, 1024, move |_| {
        loading.take().unwrap().wait()?;
        Ok(ShiftGain)
    });
    let lane = state.lane.as_ref().unwrap().clone();
    let mut output = [[0.0; 2]; 1792];
    assert_realtime_clean(|| {
        let _section = truce::rt::RtSection::enter();
        for (index, frame) in output[..128].iter_mut().enumerate() {
            *frame = state.sample(&params, signal(index), 2);
        }
    });
    let entered_loading = loading_entered.recv_timeout(TIMEOUT).is_ok();
    assert_realtime_clean(|| {
        let _section = truce::rt::RtSection::enter();
        for (index, frame) in output[128..640].iter_mut().enumerate() {
            *frame = state.sample(&params, signal(index + 128), 2);
        }
    });
    params.set_plain(0, 3.0);
    params.set_plain(1, 50.0);
    assert_realtime_clean(|| {
        let _section = truce::rt::RtSection::enter();
        for (index, frame) in output[640..767].iter_mut().enumerate() {
            *frame = state.sample(&params, signal(index + 640), 2);
        }
    });
    let full = lane.input.is_full();
    let callback_params = params.clone();
    let (started_send, started_receive) = mpsc::sync_channel(1);
    let (send, receive) = mpsc::sync_channel(1);
    let callback = thread::spawn(move || {
        let _ = started_send.send(());
        assert_realtime_clean(|| {
            let _section = truce::rt::RtSection::enter();
            for (index, frame) in output[767..].iter_mut().enumerate() {
                *frame = state.sample(&callback_params, signal(index + 767), 2);
            }
        });
        let _ = send.send(output);
    });
    let callback_started = started_receive.recv_timeout(TIMEOUT).is_ok();
    let mut completed = receive.recv_timeout(Duration::from_millis(50)).ok();
    let waited = completed.is_none();
    let loading_state = params.status.state();
    // The queued packet began at +3.5. A later automation update must not
    // replace that setting while its final sample waits for queue capacity.
    params.set_plain(0, -2.0);
    params.set_plain(1, -25.0);
    let _ = loading_release.send(());
    if completed.is_none() {
        completed = receive.recv_timeout(TIMEOUT).ok();
    }
    lane.stop.store(true, Ordering::Release);
    callback.join().unwrap();

    assert!(entered_loading && full && callback_started);
    assert!(
        waited,
        "full input queue returned before worker capacity was available"
    );
    assert_eq!(loading_state, ProcessingState::Loading);
    let output = completed.expect("full input queue did not resume after model loading");
    for (index, actual) in output.iter().enumerate() {
        let expected = if index < 1024 {
            [0.0; 2]
        } else {
            let source = index - 1024;
            let gain = if source < 640 {
                2.0
            } else if source < 672 {
                let phase = (source - 640 + 1) as f32 / 33.0;
                let wet = 0.5 - 0.5 * (std::f32::consts::PI * phase).cos();
                2.0 * (1.0 - wet) + 5.5 * wet
            } else {
                5.5
            };
            signal(source).map(|sample| sample * gain)
        };
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
fn unsupported_rate_is_silent_and_clears_previous_latency() {
    let params = RevoxParams::default();
    let mut state = prepared(&params, 256, |_| Ok(Double));
    state.reset(&params, 22_050.0, None);
    assert_eq!(state.latency_samples(), 0);
    assert_eq!(params.status.state(), ProcessingState::UnsupportedRate);
    assert_realtime_clean(|| {
        let _section = truce::rt::RtSection::enter();
        assert_eq!(state.sample(&params, [0.25, -0.125], 2), [0.0; 2]);
    });
}

#[test]
fn model_loading_and_inference_failures_release_waiting_audio_as_silence() {
    struct FailingInference {
        warmup: bool,
    }
    impl Renderer for FailingInference {
        fn render_channels(
            &mut self,
            input: &[Vec<f32>],
            shift: f32,
        ) -> Result<Vec<Vec<f32>>, DspError> {
            if self.warmup {
                self.warmup = false;
                Double.render_channels(input, shift)
            } else {
                Err(DspError::InputShape)
            }
        }
    }

    for load_fails in [true, false] {
        let params = RevoxParams::default();
        let mut state = prepared(&params, 256, move |_| {
            if load_fails {
                Err(DspError::ModelContract { model: "Revox" })
            } else {
                Ok(FailingInference { warmup: true })
            }
        });
        let lane = state.lane.as_ref().unwrap().clone();
        let status = callback_result(&lane, move || {
            assert_realtime_clean(|| {
                let _section = truce::rt::RtSection::enter();
                for _ in 0..1024 {
                    assert_eq!(state.sample(&params, [0.25, -0.125], 2), [0.0; 2]);
                }
            });
            params.status.state()
        });
        assert_eq!(status, ProcessingState::Failed);
    }
}

#[test]
fn layout_changes_retire_partial_input_and_old_wet_without_callback_allocation() {
    let params = RevoxParams::default();
    let mut state = prepared(&params, 256, |_| Ok(Double));
    state.channels = 1;
    let lane = state.lane.as_ref().unwrap().clone();
    // Switching back to mono must not make a previously retired mono packet
    // eligible again, even when both old layouts still have queued output.
    for channels in [1, 2] {
        let mut old = Packet::new(0, channels, 0.0);
        old.len = PACKET_FRAMES;
        old.samples.fill([0.9, -0.8]);
        assert!(lane.output.push(old).is_ok());
    }
    callback_result(&lane, move || {
        assert_realtime_clean(|| {
            let _section = truce::rt::RtSection::enter();
            for _ in 0..4 {
                assert_eq!(state.sample(&params, [0.1, 0.0], 1), [0.0; 2]);
            }
            for _ in 0..9 {
                assert_eq!(state.sample(&params, [0.2, -0.3], 2), [0.0; 2]);
            }
            for index in 0..768 {
                let output = state.sample(&params, [0.4, 0.0], 1);
                let expected = if index < 256 { [0.0; 2] } else { [0.8, 0.0] };
                for channel in 0..2 {
                    assert!((output[channel] - expected[channel]).abs() < 1e-6);
                }
            }
        });
    });
}

#[test]
fn worker_selects_and_replaces_one_batch_for_mono_stereo_mono_without_mixing() {
    struct DoubleBatch {
        channels: usize,
        active: Arc<AtomicUsize>,
    }
    impl Renderer for DoubleBatch {
        fn render_channels(
            &mut self,
            input: &[Vec<f32>],
            shift: f32,
        ) -> Result<Vec<Vec<f32>>, DspError> {
            assert_eq!(input.len(), self.channels);
            Double.render_channels(input, shift)
        }
    }
    impl Drop for DoubleBatch {
        fn drop(&mut self) {
            self.active.fetch_sub(1, Ordering::SeqCst);
        }
    }

    let params = RevoxParams::default();
    let loaded = Arc::new(Mutex::new(Vec::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let worker_loaded = loaded.clone();
    let worker_active = active.clone();
    let mut state = prepared(&params, 256, move |channels| {
        assert_eq!(worker_active.fetch_add(1, Ordering::SeqCst), 0);
        worker_loaded.lock().unwrap().push(channels);
        Ok(DoubleBatch {
            channels,
            active: worker_active.clone(),
        })
    });
    let lane = state.lane.as_ref().unwrap().clone();
    let status = callback_result(&lane, move || {
        assert_realtime_clean(|| {
            let _section = truce::rt::RtSection::enter();
            for (channels, input) in [(1, [0.15, 0.0]), (2, [-0.2, 0.35]), (1, [0.4, 0.0])] {
                for index in 0..768 {
                    let output = state.sample(&params, input, channels);
                    let expected = if index < 256 {
                        [0.0; 2]
                    } else {
                        input.map(|sample| sample * 2.0)
                    };
                    for channel in 0..2 {
                        assert!((output[channel] - expected[channel]).abs() < 1e-6);
                    }
                }
            }
        });
        params.status.state()
    });
    assert_eq!(status, ProcessingState::Ready);
    assert_eq!(*loaded.lock().unwrap(), [1, 2, 1]);
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[test]
fn short_layout_regions_drain_retired_output_before_input_backpressure() {
    let params = RevoxParams::default();
    let mut state = prepared(&params, 512, |_| Ok(Double));
    let lane = state.lane.as_ref().unwrap().clone();
    callback_result(&lane, move || {
        assert_realtime_clean(|| {
            let _section = truce::rt::RtSection::enter();
            // Every region produces wet packets but ends before any is due.
            // Retired output must drain so both bounded queues cannot deadlock.
            for region in 0..8 {
                let (channels, input) = if region % 2 == 0 {
                    (1, [0.2, 0.0])
                } else {
                    (2, [0.2, -0.3])
                };
                for _ in 0..384 {
                    assert_eq!(state.sample(&params, input, channels), [0.0; 2]);
                }
            }
            // The final region's first packet must survive preroll retirement.
            for index in 384..768 {
                let output = state.sample(&params, [0.2, -0.3], 2);
                let expected = if index < 512 { [0.0; 2] } else { [0.4, -0.6] };
                for channel in 0..2 {
                    assert!((output[channel] - expected[channel]).abs() < 1e-6);
                }
            }
        });
    });
}
