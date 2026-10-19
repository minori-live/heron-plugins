//! RubberBandStretcher wrapper for realtime pitch / time work (Heron shared).

use rubberband_sys as ffi;

use crate::live::PrepareError;

/// Construction options for the classic Stretcher (includes R2/R3 engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StretcherOptions {
    pub engine_finer: bool,
    pub formant_preserved: bool,
    pub channels_together: bool,
    pub pitch_high_consistency: bool,
    pub window_short: bool,
}

impl Default for StretcherOptions {
    fn default() -> Self {
        Self {
            engine_finer: true,
            formant_preserved: false,
            channels_together: true,
            pitch_high_consistency: true,
            window_short: false,
        }
    }
}

impl StretcherOptions {
    fn to_bits(self) -> ffi::RubberBandOptions {
        let mut options = ffi::RubberBandOptionProcessRealTime as ffi::RubberBandOptions
            | ffi::RubberBandOptionThreadingNever as ffi::RubberBandOptions;
        if self.engine_finer {
            options |= ffi::RubberBandOptionEngineFiner as ffi::RubberBandOptions;
        }
        if self.formant_preserved {
            options |= ffi::RubberBandOptionFormantPreserved as ffi::RubberBandOptions;
        }
        if self.channels_together {
            options |= ffi::RubberBandOptionChannelsTogether as ffi::RubberBandOptions;
        }
        if self.pitch_high_consistency {
            options |= ffi::RubberBandOptionPitchHighConsistency as ffi::RubberBandOptions;
        }
        if self.window_short {
            options |= ffi::RubberBandOptionWindowShort as ffi::RubberBandOptions;
        }
        options
    }
}

/// Thin realtime Stretcher owner for Heron resampling / stretch callers.
pub struct RealtimeStretcher {
    state: ffi::RubberBandState,
    channels: usize,
    start_delay: usize,
}

// SAFETY: independently owned Stretcher; not Sync for concurrent process calls.
unsafe impl Send for RealtimeStretcher {}

impl RealtimeStretcher {
    pub fn prepare(
        sample_rate: u32,
        channels: usize,
        options: StretcherOptions,
        time_ratio: f64,
        pitch_scale: f64,
        max_process_size: u32,
    ) -> Result<Self, PrepareError> {
        if sample_rate == 0 {
            return Err(PrepareError::InvalidSampleRate);
        }
        if !(1..=2).contains(&channels) {
            return Err(PrepareError::InvalidChannels);
        }
        // SAFETY: process-wide default debug level; no live state required.
        unsafe {
            ffi::rubberband_set_default_debug_level(0);
        }
        // SAFETY: constructs an owned stretcher; null is checked below.
        let state = unsafe {
            ffi::rubberband_new(
                sample_rate,
                channels as u32,
                options.to_bits(),
                if time_ratio.is_finite() && time_ratio > 0.0 {
                    time_ratio
                } else {
                    1.0
                },
                if pitch_scale.is_finite() && pitch_scale > 0.0 {
                    pitch_scale
                } else {
                    1.0
                },
            )
        };
        if state.is_null() {
            return Err(PrepareError::ConstructFailed);
        }
        // SAFETY: `state` is a freshly created non-null stretcher.
        unsafe {
            ffi::rubberband_set_max_process_size(state, max_process_size.max(1));
            ffi::rubberband_set_debug_level(state, 0);
        }
        // SAFETY: pitch/time ratios were set on construction before querying delay.
        let start_delay = unsafe { ffi::rubberband_get_start_delay(state) } as usize;
        Ok(Self {
            state,
            channels,
            start_delay,
        })
    }

    pub fn engine_version(&self) -> i32 {
        // SAFETY: exclusive owned stretcher pointer.
        unsafe { ffi::rubberband_get_engine_version(self.state) }
    }

    pub fn start_delay(&self) -> usize {
        self.start_delay
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn set_pitch_scale(&mut self, scale: f64) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        // SAFETY: exclusive ownership of `self.state`.
        unsafe {
            ffi::rubberband_set_pitch_scale(self.state, scale);
        }
    }

    pub fn set_time_ratio(&mut self, ratio: f64) {
        let ratio = if ratio.is_finite() && ratio > 0.0 {
            ratio
        } else {
            1.0
        };
        // SAFETY: exclusive ownership of `self.state`.
        unsafe {
            ffi::rubberband_set_time_ratio(self.state, ratio);
        }
    }

    /// # Safety
    /// `input` must contain `self.channels` pointers each valid for `samples`.
    pub unsafe fn process(&mut self, input: &[*const f32], samples: u32, final_: bool) {
        debug_assert_eq!(input.len(), self.channels);
        // SAFETY: caller guarantees planar input validity; exclusive ownership.
        unsafe {
            ffi::rubberband_process(self.state, input.as_ptr(), samples, i32::from(final_));
        }
    }

    pub fn available(&self) -> i32 {
        // SAFETY: exclusive owned stretcher pointer.
        unsafe { ffi::rubberband_available(self.state) }
    }

    /// # Safety
    /// `output` must contain `self.channels` mutable pointers each valid for `samples`.
    pub unsafe fn retrieve(&mut self, output: &[*mut f32], samples: u32) -> u32 {
        debug_assert_eq!(output.len(), self.channels);
        // SAFETY: caller guarantees planar output validity; exclusive ownership.
        unsafe { ffi::rubberband_retrieve(self.state, output.as_ptr(), samples) }
    }

    pub fn reset(&mut self) {
        // SAFETY: exclusive ownership of `self.state`.
        unsafe {
            ffi::rubberband_reset(self.state);
        }
        // SAFETY: state remains valid after reset.
        self.start_delay = unsafe { ffi::rubberband_get_start_delay(self.state) } as usize;
    }
}

impl Drop for RealtimeStretcher {
    fn drop(&mut self) {
        if !self.state.is_null() {
            // SAFETY: frees the owned stretcher exactly once.
            unsafe {
                ffi::rubberband_delete(self.state);
            }
            self.state = std::ptr::null_mut();
        }
    }
}
