//! Bounded per-instance latency/sample-rate shared with the editor footer.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub struct ProcessingStatus {
    latency: AtomicU32,
    sample_rate: AtomicU64,
}

impl Default for ProcessingStatus {
    fn default() -> Self {
        Self {
            latency: AtomicU32::new(0),
            sample_rate: AtomicU64::new(48_000.0_f64.to_bits()),
        }
    }
}

impl ProcessingStatus {
    pub fn latency_samples(&self) -> u32 {
        self.latency.load(Ordering::Relaxed)
    }

    pub fn sample_rate(&self) -> f64 {
        f64::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }

    pub(crate) fn configure(&self, sample_rate: f64, latency: u32) {
        self.sample_rate
            .store(sample_rate.to_bits(), Ordering::Relaxed);
        self.latency.store(latency, Ordering::Relaxed);
    }
}
