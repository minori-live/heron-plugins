//! Bounded per-instance processing state shared with the editor.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ProcessingState {
    Loading = 0,
    Ready = 1,
    Failed = 3,
    UnsupportedRate = 4,
}

pub struct ProcessingStatus {
    // Publish generation and state together so a retiring job cannot overwrite
    // a reset that occurs between a generation check and a status update.
    state: AtomicU64,
    latency: AtomicU32,
    sample_rate: AtomicU64,
}

impl Default for ProcessingStatus {
    fn default() -> Self {
        Self {
            state: AtomicU64::new(ProcessingState::Loading as u64),
            latency: AtomicU32::new(0),
            sample_rate: AtomicU64::new(48_000.0_f64.to_bits()),
        }
    }
}

impl ProcessingStatus {
    pub fn state(&self) -> ProcessingState {
        Self::decode_state(self.state.load(Ordering::Acquire))
    }

    fn decode_state(value: u64) -> ProcessingState {
        match value as u8 {
            1 => ProcessingState::Ready,
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

    pub(crate) fn generation(&self) -> u64 {
        self.state.load(Ordering::Acquire) >> 8
    }

    pub(crate) fn configure(&self, sample_rate: f64, latency: u32) -> (u64, ProcessingState) {
        self.sample_rate
            .store(sample_rate.to_bits(), Ordering::Relaxed);
        self.latency.store(latency, Ordering::Relaxed);
        self.advance_region()
    }

    pub(crate) fn begin_region(&self) -> u64 {
        self.advance_region().0
    }

    fn advance_region(&self) -> (u64, ProcessingState) {
        let previous = self
            .state
            .try_update(Ordering::AcqRel, Ordering::Acquire, |previous| {
                Some(((previous >> 8).wrapping_add(1) << 8) | ProcessingState::Loading as u64)
            })
            .unwrap_or_else(|previous| previous);
        (
            (previous >> 8).wrapping_add(1) & (u64::MAX >> 8),
            Self::decode_state(previous),
        )
    }

    pub(crate) fn set(&self, state: ProcessingState) {
        let _ = self.set_for_generation(self.generation(), state);
    }

    pub(crate) fn set_for_generation(&self, generation: u64, state: ProcessingState) -> bool {
        self.state
            .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current >> 8 == generation).then_some((generation << 8) | state as u64)
            })
            .is_ok()
    }
}
