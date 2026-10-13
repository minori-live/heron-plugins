//! Bounded per-instance processing state shared with the editor.

use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ProcessingState {
    Loading,
    Ready,
    Overloaded,
    Failed,
    UnsupportedRate,
}

pub struct ProcessingStatus {
    state: AtomicU8,
    latency: AtomicU32,
    sample_rate: AtomicU64,
}

impl Default for ProcessingStatus {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(ProcessingState::Loading as u8),
            latency: AtomicU32::new(0),
            sample_rate: AtomicU64::new(48_000.0_f64.to_bits()),
        }
    }
}

impl ProcessingStatus {
    pub fn state(&self) -> ProcessingState {
        match self.state.load(Ordering::Acquire) {
            1 => ProcessingState::Ready,
            2 => ProcessingState::Overloaded,
            3 => ProcessingState::Failed,
            4 => ProcessingState::UnsupportedRate,
            _ => ProcessingState::Loading,
        }
    }

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
        self.set(ProcessingState::Loading);
    }

    pub(crate) fn set(&self, state: ProcessingState) {
        self.state.store(state as u8, Ordering::Release);
    }

    pub(crate) fn available(&self, available: bool) {
        let (from, to) = if available {
            (ProcessingState::Overloaded, ProcessingState::Ready)
        } else {
            (ProcessingState::Ready, ProcessingState::Overloaded)
        };
        let _ =
            self.state
                .compare_exchange(from as u8, to as u8, Ordering::AcqRel, Ordering::Relaxed);
    }
}
