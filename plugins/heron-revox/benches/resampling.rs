//! Host-rate conversion with the real model window and retained 200 ms output.
//! Each case includes output allocation; model inference is deliberately absent.

use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use heron_revox_dsp::PLUGIN_WINDOW_SAMPLES;

#[path = "support/original_resampling.rs"]
mod original;
#[path = "../src/resampling.rs"]
#[allow(
    unused_imports,
    dead_code,
    reason = "The harness-free benchmark does not run the included unit tests."
)]
mod resampling;

fn bench_resampling(c: &mut Criterion) {
    for host_rate in [44_100, 48_000, 96_000, 192_000] {
        for (direction, from, to) in [
            ("to_model", host_rate, 48_000),
            ("from_model", 48_000, host_rate),
        ] {
            let input_len = if from == 48_000 {
                PLUGIN_WINDOW_SAMPLES
            } else {
                from * 212 / 100
            };
            let output_len = to * 212 / 100;
            let input: Vec<_> = (0..input_len)
                .map(|i| (i as f32 * 0.031).sin() * 0.3)
                .collect();
            let original = original::Resampler::new(from as f64, to as f64);
            let simd = resampling::Resampler::new(from as f64, to as f64);
            let reference = original.convert(&input, output_len);
            assert_eq!(original.convert_contiguous(&input, output_len), reference);
            let actual = simd.convert(&input, output_len);
            assert!(
                actual
                    .iter()
                    .zip(&reference)
                    .all(|(a, b)| (a - b).abs() < 1e-6)
            );

            let mut group = c.benchmark_group(format!("revox_host_resampling/{direction}"));
            // Input duration stays 2.12 s for all variants, including cropped
            // output; ns/window is the comparable metric for deadline headroom.
            group.throughput(Throughput::Elements(input_len as u64));
            group.bench_with_input(
                BenchmarkId::new("original_scalar", host_rate),
                &input,
                |b, input| b.iter(|| black_box(original.convert(black_box(input), output_len))),
            );
            group.bench_with_input(
                BenchmarkId::new("contiguous_scalar", host_rate),
                &input,
                |b, input| {
                    b.iter(|| black_box(original.convert_contiguous(black_box(input), output_len)))
                },
            );
            group.bench_with_input(
                BenchmarkId::new("simd_full", host_rate),
                &input,
                |b, input| b.iter(|| black_box(simd.convert(black_box(input), output_len))),
            );
            if direction == "from_model" {
                let range = to * 96 / 100..to * 116 / 100;
                assert_eq!(
                    simd.convert_range(&input, range.clone()),
                    actual[range.clone()]
                );
                group.bench_with_input(
                    BenchmarkId::new("simd_cropped", host_rate),
                    &input,
                    |b, input| {
                        b.iter(|| black_box(simd.convert_range(black_box(input), range.clone())))
                    },
                );
            }
            group.finish();
        }
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(2));
    targets = bench_resampling
}
criterion_main!(benches);
