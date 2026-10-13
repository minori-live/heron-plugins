use super::*;
use crate::{GainParamsParamId as P, Plugin};
use std::time::Duration;
use truce_test::{InputSource, assert_no_audio_alloc, driver};

fn params(db: f64) -> GainParams {
    let params = GainParams::default();
    params.set_sample_rate(48_000.0);
    params.set_plain(P::Gain.into(), db);
    params.snap_smoothers();
    params
}

// The pre-SIMD Gain's scalar audio calculation, including its cutoff and
// exact-unity special case. Keep this independent of the production kernel.
fn reference(params: &GainParams, input: &[Vec<f32>], outputs: usize) -> Vec<Vec<f32>> {
    let mut output = vec![vec![0.0; input[0].len()]; outputs];
    for sample in 0..input[0].len() {
        let db = params.gain.read();
        let gain = if db <= -90.0 {
            0.0
        } else if db.abs() <= f32::EPSILON {
            1.0
        } else {
            db_to_linear(db)
        };
        for (channel, output) in output.iter_mut().enumerate() {
            output[sample] = input[channel.min(input.len() - 1)][sample] * gain;
        }
    }
    output
}

fn run(
    state: &mut GainDspState,
    params: &GainParams,
    input: &[Vec<f32>],
    outputs: usize,
) -> (Vec<Vec<f32>>, [f32; 2]) {
    let frames = input[0].len();
    let mut output = vec![vec![0.0; frames]; outputs];
    let inputs: Vec<&[f32]> = input.iter().map(Vec::as_slice).collect();
    let mut outputs: Vec<&mut [f32]> = output.iter_mut().map(Vec::as_mut_slice).collect();
    let mut buffer = AudioBuffer::from_slices_checked(&inputs, &mut outputs, frames);
    let peaks = state.process(params, &mut buffer);
    (output, peaks)
}

fn signal(frames: usize, channels: usize) -> Vec<Vec<f32>> {
    (0..channels)
        .map(|channel| {
            (0..frames)
                .map(|sample| ((sample * 17 % 61) as f32 - 30.0) / (31.0 + channel as f32))
                .collect()
        })
        .collect()
}

#[test]
fn block_gain_matches_original_across_layouts_and_tails() {
    for level in [Level::fallback(), Level::new()] {
        for (inputs, outputs) in [(1, 1), (1, 2), (2, 2)] {
            for frames in [0, 1, 3, 17, 255, 256, 257, 1025] {
                let input = signal(frames, inputs);
                for db in [-90.0, -12.0, 0.0, 24.0] {
                    let reference = reference(&params(db), &input, outputs);
                    let mut state = GainDspState {
                        level,
                        ..Default::default()
                    };
                    let (actual, peaks) = run(&mut state, &params(db), &input, outputs);
                    assert_eq!(actual, reference, "{inputs}->{outputs}, {frames}, {db}");
                    for (channel, output) in actual.iter().enumerate() {
                        let expected = output
                            .iter()
                            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
                        assert_eq!(peaks[channel], expected);
                    }
                }
            }
        }
    }
}

#[test]
fn linear_ramps_share_each_sample_and_snap_exactly_at_chunk_boundaries() {
    for level in [Level::fallback(), Level::new()] {
        let params = params(-12.0);
        let scalar = self::params(-12.0);
        let mut state = GainDspState {
            level,
            ..Default::default()
        };
        // Edits interrupt ramps; the final long span includes the 480-sample
        // endpoint and multiple constant chunks after the exact target snap.
        for (target, frames) in [(6.0, 257), (-90.0, 17), (0.0, 1025), (24.0, 513)] {
            params.set_plain(P::Gain.into(), target);
            scalar.set_plain(P::Gain.into(), target);
            let input = signal(frames, 1);
            let expected = reference(&scalar, &input, 2);
            let (actual, _) = run(&mut state, &params, &input, 2);
            assert_eq!(actual, expected);
            assert_eq!(params.gain.read().to_bits(), scalar.gain.read().to_bits());
        }
        // A change smaller than is_smoothing's threshold still needs one read
        // to snap the smoother; reading current() would keep the old value.
        params.set_plain(P::Gain.into(), 24.0 - 0.000_02);
        scalar.set_plain(P::Gain.into(), 24.0 - 0.000_02);
        let input = signal(17, 2);
        let (actual, _) = run(&mut state, &params, &input, 2);
        assert_eq!(actual, reference(&scalar, &input, 2));
    }
}

#[test]
fn unity_preserves_sample_bits_and_peak_meter_propagates_nan() {
    for level in [Level::fallback(), Level::new()] {
        let mut input = signal(257, 2);
        input[0][0] = -0.0;
        input[0][1] = f32::INFINITY;
        input[1][2] = f32::NEG_INFINITY;
        // Exercise vector lanes and the one-sample tail separately.
        input[0][7] = f32::from_bits(0x7fc0_1234);
        input[1][256] = f32::from_bits(0x7fc0_5678);
        let mut state = GainDspState {
            level,
            ..Default::default()
        };
        let (actual, peaks) = run(&mut state, &params(0.0), &input, 2);
        for (actual, expected) in actual.iter().flatten().zip(input.iter().flatten()) {
            assert_eq!(actual.to_bits(), (*expected * 1.0).to_bits());
        }
        assert!(peaks.iter().all(|peak| peak.is_nan()));
    }
}

#[test]
fn host_automation_preserves_subblock_offsets_without_allocations() {
    let render = |block_size| {
        driver!(Plugin)
            .sample_rate(48_000.0)
            .block_size(block_size)
            .duration(Duration::from_millis(40))
            .input(InputSource::Constant(0.25))
            .script(|script| {
                // Keep event gaps above Truce's configured coalescing floor;
                // these are actual sub-block boundaries in both render runs.
                script.wait_samples(64);
                script.set_param(P::Gain, 6.0);
                script.wait_samples(257);
                script.set_param(P::Gain, -90.0);
                script.wait_samples(513);
                script.set_param(P::Gain, 0.0);
            })
            .run()
    };
    let expected = render(1);
    let actual = assert_no_audio_alloc(|| render(1024));
    assert_eq!(actual.output, expected.output);
    assert_eq!(&actual.output[0][..64], &[0.25; 64]);
    assert_ne!(actual.output[0][64].to_bits(), 0.25_f32.to_bits());
    assert_eq!(actual.output[0], actual.output[1]);
}
