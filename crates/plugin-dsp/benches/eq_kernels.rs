//! Compare the old indexing, contiguous scalar layout, and production SIMD.
//! Private production modules are included directly; no benchmark API is shipped.

use std::{hint::black_box, time::Duration};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fearless_simd::{Fallback, Level, dispatch};
#[path = "../src/eq/oversampling.rs"]
mod oversampling;

use oversampling::{OVERSAMPLE_TAPS, OversamplingFir, oversampling_kernel};

struct ScalarFir {
    kernel: [f64; OVERSAMPLE_TAPS],
    history: [[f64; OVERSAMPLE_TAPS]; 2],
    cursor: usize,
}

impl ScalarFir {
    fn new() -> Self {
        Self {
            kernel: oversampling_kernel(),
            history: [[0.0; OVERSAMPLE_TAPS]; 2],
            cursor: 0,
        }
    }

    // Original production indexing and tap accumulation order.
    fn original(&mut self, input: (f64, f64)) -> (f64, f64) {
        self.history[0][self.cursor] = input.0;
        self.history[1][self.cursor] = input.1;
        let mut result = [0.0; 2];
        for (channel, output) in result.iter_mut().enumerate() {
            for tap in 0..OVERSAMPLE_TAPS {
                *output += self.kernel[tap]
                    * self.history[channel]
                        [(self.cursor + OVERSAMPLE_TAPS - tap) % OVERSAMPLE_TAPS];
            }
        }
        self.cursor = (self.cursor + 1) % OVERSAMPLE_TAPS;
        (result[0], result[1])
    }
}

struct MirroredScalarFir {
    kernel: [f64; OVERSAMPLE_TAPS],
    history: [[f64; OVERSAMPLE_TAPS * 2]; 2],
    cursor: usize,
}

impl MirroredScalarFir {
    fn new() -> Self {
        Self {
            kernel: oversampling_kernel(),
            history: [[0.0; OVERSAMPLE_TAPS * 2]; 2],
            cursor: 0,
        }
    }

    // Same mirrored layout as production, but a sequential scalar reduction.
    fn mirrored(&mut self, input: (f64, f64)) -> (f64, f64) {
        self.cursor = if self.cursor == 0 {
            OVERSAMPLE_TAPS - 1
        } else {
            self.cursor - 1
        };
        let mut result = [0.0; 2];
        for ((history, sample), output) in self
            .history
            .iter_mut()
            .zip([input.0, input.1])
            .zip(&mut result)
        {
            history[self.cursor] = sample;
            history[self.cursor + OVERSAMPLE_TAPS] = sample;
            for (coefficient, sample) in self
                .kernel
                .iter()
                .zip(&history[self.cursor..self.cursor + OVERSAMPLE_TAPS])
            {
                *output += coefficient * sample;
            }
        }
        (result[0], result[1])
    }
}

fn eq_kernels(criterion: &mut Criterion) {
    let input: Vec<_> = (0..256)
        .map(|index| ((index as f64 * 0.071).sin(), (index as f64 * 0.13).cos()))
        .collect();
    let mut fir = criterion.benchmark_group("eq_fir_33_stereo");
    fir.sample_size(20);
    fir.warm_up_time(Duration::from_secs(1));
    fir.measurement_time(Duration::from_secs(2));
    fir.throughput(Throughput::Elements(input.len() as u64));
    let mut original = ScalarFir::new();
    fir.bench_function("original_ring_scalar", |bencher| {
        bencher.iter(|| {
            for &input in black_box(&input) {
                black_box(original.original(input));
            }
        })
    });
    let mut mirrored = MirroredScalarFir::new();
    fir.bench_function("mirrored_scalar", |bencher| {
        bencher.iter(|| {
            for &input in black_box(&input) {
                black_box(mirrored.mirrored(input));
            }
        })
    });
    for (name, level) in [
        ("mirrored_fallback", Level::Fallback(Fallback::new())),
        ("mirrored_host_simd", Level::new()),
    ] {
        let mut processor = OversamplingFir::new();
        fir.bench_function(name, |bencher| {
            bencher.iter(|| {
                // Isolate arithmetic with one dispatch for the batch. The
                // end-to-end benchmark includes NaturalPhase frame dispatch.
                dispatch!(level, simd => {
                    for &input in black_box(&input) { black_box(processor.process(simd, input)); }
                });
            })
        });
    }
    fir.finish();
}

criterion_group!(benches, eq_kernels);
criterion_main!(benches);
