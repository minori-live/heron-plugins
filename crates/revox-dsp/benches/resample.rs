//! Benchmarks the actual 2.12-second FCPE input window, without model inference.

use std::{hint::black_box, time::Duration};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use heron_revox_dsp::{PLUGIN_WINDOW_SAMPLES, fir};

#[path = "support/original_resample.rs"]
mod original;
#[path = "../src/resample.rs"]
#[allow(
    unused_imports,
    reason = "The harness-free benchmark does not run the included unit tests."
)]
mod resample;

fn bench_resample(c: &mut Criterion) {
    let input: Vec<_> = (0..PLUGIN_WINDOW_SAMPLES)
        .map(|i| (i as f32 * 0.031).sin() * 0.3)
        .collect();
    let original = original::PitchResampler::new();
    let simd = resample::PitchResampler::new();
    let reference = original.process(&input);
    assert_eq!(original.process_contiguous(&input), reference);
    assert!(
        simd.process(&input)
            .iter()
            .zip(&reference)
            .all(|(a, b)| (a - b).abs() < 1e-6)
    );

    let mut group = c.benchmark_group("revox_pitch_fir_2_12s");
    group.throughput(Throughput::Elements(PLUGIN_WINDOW_SAMPLES as u64));
    group.bench_function("original_scalar", |b| {
        b.iter(|| black_box(original.process(black_box(&input))))
    });
    group.bench_function("contiguous_scalar", |b| {
        b.iter(|| black_box(original.process_contiguous(black_box(&input))))
    });
    group.bench_function("simd", |b| {
        b.iter(|| black_box(simd.process(black_box(&input))))
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(2));
    targets = bench_resample
}
criterion_main!(benches);
