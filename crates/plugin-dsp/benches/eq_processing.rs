//! End-to-end audio work only; preparation and buffer allocation stay outside timing.
//! Uses the public processor API so this benchmark also runs on the pre-SIMD tree.

use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use heron_plugin_dsp::eq::{
    EqBand, EqChannel, EqConfig, EqProcessor, LinearPhaseResolution, ProcessingMode,
};

fn config(mode: ProcessingMode, bands: usize, resolution: LinearPhaseResolution) -> EqConfig {
    EqConfig {
        processing_mode: mode,
        linear_phase_resolution: resolution,
        bands: (0..bands)
            .map(|index| EqBand {
                id: index as u32 + 1,
                frequency_hz: 80.0 * 1.2_f64.powi(index as i32),
                gain_db: if index % 2 == 0 { 3.0 } else { -3.0 },
                channel: [EqChannel::Stereo, EqChannel::Mid, EqChannel::Side][index % 3],
                ..EqBand::default()
            })
            .collect(),
        ..EqConfig::default()
    }
}

fn eq_processing(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("eq_processing");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    let mut cases = Vec::new();
    cases.push((
        "zero_latency_6_bands".to_owned(),
        config(ProcessingMode::ZeroLatency, 6, LinearPhaseResolution::High),
    ));
    for bands in [0, 6, 24] {
        cases.push((
            format!("natural_{bands}_bands"),
            config(
                ProcessingMode::NaturalPhase,
                bands,
                LinearPhaseResolution::High,
            ),
        ));
    }
    for resolution in [
        LinearPhaseResolution::Low,
        LinearPhaseResolution::High,
        LinearPhaseResolution::Maximum,
    ] {
        cases.push((
            format!("linear_{resolution:?}_6_bands"),
            config(ProcessingMode::LinearPhase, 6, resolution),
        ));
    }
    for frames in [64, 256] {
        let input: Vec<_> = (0..frames)
            .map(|index| {
                let phase = index as f32 * 0.123;
                (phase.sin() * 0.3, (phase * 1.7).cos() * 0.2)
            })
            .collect();
        group.throughput(Throughput::Elements(frames as u64));
        for (name, config) in &cases {
            let mut processor = EqProcessor::prepare(config, 48_000.0).expect("valid benchmark EQ");
            // Fill all FIR histories before measuring sustained processing.
            for index in 0..16_384 {
                let (left, right) = input[index % frames];
                black_box(processor.process_stereo(left, right));
            }
            group.bench_function(BenchmarkId::new(name, frames), |bencher| {
                bencher.iter(|| {
                    for &(left, right) in black_box(&input) {
                        black_box(processor.process_stereo(left, right));
                    }
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, eq_processing);
criterion_main!(benches);
