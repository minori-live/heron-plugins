use super::*;
use truce::prelude::Params;
use truce_test::assert_realtime_clean;

fn prepared(latency: usize) -> RevoxDspState {
    RevoxDspState {
        lane: Some(Arc::new(Lane::new())),
        dry: vec![[0.0; 2]; latency],
        latency,
        mix_step: 1.0,
        ..RevoxDspState::default()
    }
}

#[test]
fn realtime_underrun_preserves_delay_and_stereo_without_allocation_or_blocking() {
    let params = RevoxParams::default();
    params.status.set(ProcessingState::Ready);
    let mut state = prepared(256);
    assert_realtime_clean(|| {
        let _section = truce::rt::RtSection::enter();
        for index in 0..1024 {
            let input = [index as f32 * 0.0001, -(index as f32) * 0.0002];
            let output = state.sample(&params, input, 2, false);
            let expected = if index < 256 {
                [0.0; 2]
            } else {
                [
                    (index - 256) as f32 * 0.0001,
                    -((index - 256) as f32) * 0.0002,
                ]
            };
            assert_eq!(output, expected);
        }
    });
    assert_eq!(params.status.state(), ProcessingState::Overloaded);
}

#[test]
fn timestamped_wet_output_skips_late_packets_and_keeps_future_audio_in_place() {
    let params = RevoxParams::default();
    params.status.set(ProcessingState::Ready);
    let mut state = prepared(128);
    state.position = 256;
    let lane = state.lane.as_ref().unwrap();
    for (start, sample) in [(0, [0.9, 0.8]), (128, [0.3, -0.2]), (384, [0.6, -0.5])] {
        let mut packet = Packet::new(start, 2, 0.0);
        packet.len = 128;
        packet.samples.fill(sample);
        assert!(lane.output.push(packet).is_ok());
    }
    assert_realtime_clean(|| {
        let _section = truce::rt::RtSection::enter();
        for _ in 0..128 {
            assert_eq!(state.sample(&params, [0.1, -0.1], 2, false), [0.3, -0.2]);
        }
        // Source [256,384) was missed: don't play the later packet early.
        for _ in 0..128 {
            assert_eq!(state.sample(&params, [0.1, -0.1], 2, false), [0.1, -0.1]);
        }
        assert_eq!(state.sample(&params, [0.1, -0.1], 2, false), [0.6, -0.5]);
    });
    assert_eq!(params.status.state(), ProcessingState::Ready);
}

#[test]
fn full_input_queue_drops_a_packet_and_preserves_its_parameter_timestamp() {
    let params = RevoxParams::default();
    params.set_plain(0, 3.0);
    params.set_plain(1, 50.0);
    params.status.set(ProcessingState::Ready);
    let mut state = prepared(256);
    let lane = state.lane.as_ref().unwrap().clone();
    while lane.input.push(Packet::new(0, 1, 0.0)).is_ok() {}
    assert_realtime_clean(|| {
        let _section = truce::rt::RtSection::enter();
        for _ in 0..128 {
            state.sample(&params, [0.2, 0.0], 1, false);
        }
    });
    assert_eq!(params.status.state(), ProcessingState::Overloaded);
    assert_eq!(lane.output_floor.load(Ordering::Acquire), 128);
    while lane.input.pop().is_some() {}
    for _ in 0..128 {
        state.sample(&params, [0.2, 0.0], 1, false);
    }
    let packet = lane.input.pop().unwrap();
    assert_eq!(packet.start, 128);
    assert_eq!(packet.shift, 3.5);
    assert_eq!(packet.len, 128);
}

#[test]
fn offline_after_a_gap_uses_delayed_dry_without_waiting_for_future_input() {
    struct Identity;
    impl crate::stream::Renderer for Identity {
        fn render(&mut self, input: &[f32], _: f32) -> Result<Vec<f32>, heron_revox_dsp::DspError> {
            Ok(input.to_vec())
        }
    }

    let params = RevoxParams::default();
    params.status.set(ProcessingState::Ready);
    let geometry = Geometry {
        context: 32,
        hop: 128,
        overlap: 32,
        latency: 256,
    };
    let worker = Worker::with_renderer(Identity, 48_000.0, geometry, params.status.clone());
    let lane = worker.lane.clone();
    let mut state = prepared(geometry.latency);
    state.worker = Some(worker);
    state.lane = Some(lane.clone());
    state.position = geometry.latency as u64;
    state.dry.fill([0.2, -0.1]);
    // The next contiguous region starts at 128, but this packet cannot fill its
    // lookahead window. The worker must announce the gap before producing audio.
    let mut input = Packet::new(128, 2, 0.0);
    input.len = PACKET_FRAMES;
    assert!(lane.input.push(input).is_ok());

    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let callback = thread::spawn(move || {
        let dry = state.sample(&params, [0.0; 2], 2, true);
        // An already rendered packet still wins over the gap boundary.
        let mut wet = Packet::new(1, 2, 0.0);
        wet.len = 1;
        wet.samples[0] = [0.4, -0.3];
        assert!(state.lane.as_ref().unwrap().output.push(wet).is_ok());
        let wet = state.sample(&params, [0.0; 2], 2, true);
        let _ = send.send((dry, wet));
    });
    // A regression must fail rather than leave the test process deadlocked.
    let rendered = receive.recv_timeout(Duration::from_secs(2));
    lane.stop.store(true, Ordering::Release);
    callback.join().unwrap();
    let (dry, wet) = rendered.expect("offline callback waited for an unrecoverable gap");
    assert_eq!(dry, [0.2, -0.1]);
    assert_eq!(wet, [0.4, -0.3]);
    assert_eq!(lane.output_floor.load(Ordering::Acquire), 128);
}

#[test]
fn unsupported_rate_is_visible_passthrough_and_clears_previous_latency() {
    let params = RevoxParams::default();
    let mut state = prepared(128);
    state.reset(&params, 22_050.0);
    assert_eq!(state.latency_samples(), 0);
    assert_eq!(params.status.state(), ProcessingState::UnsupportedRate);
    assert_eq!(
        state.sample(&params, [0.25, -0.125], 2, false),
        [0.25, -0.125]
    );
}
