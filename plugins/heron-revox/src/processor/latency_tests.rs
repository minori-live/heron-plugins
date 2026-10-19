use super::*;
use crate::{HeronRevox, stream::Renderer};
use heron_revox_dsp::DspError;
use std::sync::mpsc;
use truce::prelude::PluginLogic;

struct Identity;

impl Renderer for Identity {
    fn render_channels(&mut self, input: &[Vec<f32>], _: f32) -> Result<Vec<Vec<f32>>, DspError> {
        Ok(input.to_vec())
    }
}

fn signal(position: usize, sample_rate: f64) -> f32 {
    (0.125 + 0.0625 * (std::f64::consts::TAU * 997.0 * position as f64 / sample_rate).sin()) as f32
}

fn gcd(mut first: usize, mut second: usize) -> usize {
    while second != 0 {
        (first, second) = (second, first % second);
    }
    first
}

#[test]
fn every_host_rate_reports_the_sample_offset_and_finishes_all_hop_packet_alignments() {
    for (sample_rate, expected_latency) in [
        (44_100.0, 51_284),
        (48_000.0, 55_808),
        (88_200.0, 102_440),
        (96_000.0, 111_488),
        (176_400.0, 204_752),
        (192_000.0, 222_848),
    ] {
        let params = RevoxParams::default();
        let geometry = Geometry::new(sample_rate).unwrap();
        let generation = params
            .status
            .configure(sample_rate, geometry.latency as u32);
        let worker =
            Worker::with_factory(sample_rate, geometry, params.status.clone(), 2048, |_| {
                Ok(Identity)
            });
        let lane = worker.lane.clone();
        let mut state = RevoxDspState {
            lane: Some(lane.clone()),
            worker: Some(worker),
            latency: geometry.latency,
            sample_rate,
            generation,
            ..RevoxDspState::default()
        };
        assert_eq!(HeronRevox::latency(&state) as usize, expected_latency);

        // At 44.1/88.2/176.4 kHz the hop does not align with packet boundaries.
        // Visit a complete residue cycle plus the repeated first alignment;
        // rounding only the first window's readiness up to 128 is insufficient.
        let hop_count = PACKET_FRAMES / gcd(geometry.hop, PACKET_FRAMES) + 1;
        let length = expected_latency + hop_count * geometry.hop;
        let input: Vec<_> = (0..length)
            .map(|index| signal(index, sample_rate))
            .collect();
        let (send, receive) = mpsc::sync_channel(1);
        let callback = thread::spawn(move || {
            let mut output = vec![f32::NAN; length];
            let mut start = 0;
            for block in [1, 257, 127, 511].into_iter().cycle() {
                let end = (start + block).min(length);
                let inputs = [&input[start..end]];
                let mut outputs = [&mut output[start..end]];
                let mut buffer =
                    AudioBuffer::from_slices_checked(&inputs, &mut outputs, end - start);
                state.process(&params, &mut buffer);
                start = end;
                if start == length {
                    break;
                }
            }
            let _ = send.send(output);
        });
        let completed = receive.recv_timeout(Duration::from_secs(15));
        // Missing lookahead must fail the test rather than hang the test host:
        // stopping the lane releases either callback queue wait before joining.
        lane.stop.store(true, Ordering::Release);
        callback.join().unwrap();
        let output = completed.unwrap_or_else(|error| {
            panic!("{sample_rate} Hz: callback could not finish its queued audio: {error}")
        });

        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(
            output[..expected_latency]
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert!(
            output[expected_latency].is_finite() && output[expected_latency] > 0.01,
            "{sample_rate} Hz: output did not start at its reported sample offset"
        );
        // The identity renderer keeps the real window assembly, both resamplers,
        // stitching and queues. Ignore only the initial step's FIR transient.
        // 1e-4 allows polyphase coefficient/phase quantization at supported rates;
        // a one-sample shift of this 997 Hz tone exceeds that budget.
        let error = output[expected_latency + PACKET_FRAMES..]
            .iter()
            .enumerate()
            .map(|(index, sample)| (sample - signal(index + PACKET_FRAMES, sample_rate)).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            error < 1e-4,
            "{sample_rate} Hz: output differs from its reported input timestamp by {error}"
        );
    }
}
