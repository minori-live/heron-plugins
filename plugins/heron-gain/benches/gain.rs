//! Compare the original per-sample processor against the production block path.

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use heron_gain::{GainDspState, GainParams, GainParamsParamId as P, HeronGain};
use truce::prelude::*;

fn original(params: &GainParams, buffer: &mut AudioBuffer, context: &mut ProcessContext) {
    for sample in 0..buffer.num_samples() {
        let db = params.gain.read();
        let gain = if db <= -90.0 {
            0.0
        } else if db.abs() <= f32::EPSILON {
            1.0
        } else {
            db_to_linear(db)
        };
        for channel in 0..buffer.num_output_channels() {
            let input = buffer.input(channel.min(buffer.num_input_channels() - 1))[sample];
            buffer.output(channel)[sample] = input * gain;
        }
    }
    if buffer.num_output_channels() > 0 {
        context.set_meter(P::MeterLeft, buffer.output_peak(0));
    }
    if buffer.num_output_channels() > 1 {
        context.set_meter(P::MeterRight, buffer.output_peak(1));
    }
}

fn gain(c: &mut Criterion) {
    let events = EventList::default();
    let mut output_events = EventList::default();
    let transport = TransportInfo::default();
    let meters = |_, value| {
        black_box(value);
    };
    for smoothing in [false, true] {
        let name = if smoothing {
            "gain/ramp"
        } else {
            "gain/steady"
        };
        let mut group = c.benchmark_group(name);
        group.sample_size(20);
        group.warm_up_time(Duration::from_secs(1));
        group.measurement_time(Duration::from_secs(2));
        for (inputs, outputs) in [(1, 1), (1, 2), (2, 2)] {
            for frames in [64, 257, 1024] {
                group.throughput(Throughput::Elements((frames * outputs) as u64));
                let input = vec![0.25; frames];
                let input_refs: Vec<_> = (0..inputs).map(|_| input.as_slice()).collect();
                let mut output = vec![vec![0.0; frames]; outputs];
                let mut output_refs: Vec<_> = output.iter_mut().map(Vec::as_mut_slice).collect();
                let mut buffer =
                    AudioBuffer::from_slices_checked(&input_refs, &mut output_refs, frames);
                let params = GainParams::default();
                params.set_sample_rate(48_000.0);
                params.set_plain(P::Gain.into(), -12.0);
                params.snap_smoothers();
                let mut state = GainDspState::default();
                let mut context =
                    ProcessContext::new(&transport, 48_000.0, frames, &mut output_events)
                        .with_meters(&meters);
                let shape = format!("{inputs}x{outputs}/{frames}");
                for optimized in [false, true] {
                    let backend = if optimized { "block_simd" } else { "original" };
                    group.bench_function(BenchmarkId::new(backend, &shape), |b| {
                        let mut process = || {
                            if optimized {
                                HeronGain::process(
                                    &mut state,
                                    black_box(&params),
                                    black_box(&mut buffer),
                                    &events,
                                    &mut context,
                                );
                            } else {
                                original(black_box(&params), black_box(&mut buffer), &mut context);
                            }
                            black_box(buffer.output(0));
                        };
                        if smoothing {
                            b.iter_batched(
                                || {
                                    // Reset outside timing before each process
                                    // call, including when one block spans the
                                    // ramp endpoint. Batching several resets
                                    // first would instead measure steady gain.
                                    params.set_plain(P::Gain.into(), -12.0);
                                    params.snap_smoothers();
                                    params.set_plain(P::Gain.into(), 6.0);
                                },
                                |()| process(),
                                BatchSize::PerIteration,
                            );
                        } else {
                            b.iter(process);
                        }
                    });
                }
            }
        }
        group.finish();
    }
}

criterion_group!(benches, gain);
criterion_main!(benches);
