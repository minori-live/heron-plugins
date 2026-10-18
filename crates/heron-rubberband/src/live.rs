//! RubberBandLiveShifter wrapper with a fixed-capacity host block adapter.

use rubberband_sys as ffi;
use thiserror::Error;

/// Live shifter window / quality choice.
///
/// LiveShifter always uses the R3-live engine. `Faster` selects the short
/// window; `Finer` selects the medium window (closer to the Stretcher's
/// `OptionEngineFiner` quality trade-off without leaving the low-latency path).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
    Faster = 0,
    #[default]
    Finer = 1,
}

impl Quality {
    pub fn from_discrete(value: i32) -> Self {
        if value <= 0 {
            Self::Faster
        } else {
            Self::Finer
        }
    }

    fn live_window_option(self) -> ffi::RubberBandLiveOptions {
        match self {
            Self::Faster => ffi::RubberBandLiveOptionWindowShort,
            Self::Finer => ffi::RubberBandLiveOptionWindowMedium,
        }
    }
}

/// Construction-time LiveShifter options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveOptions {
    pub formant_preserved: bool,
    pub quality: Quality,
    pub channels_together: bool,
}

impl Default for LiveOptions {
    fn default() -> Self {
        Self {
            formant_preserved: true,
            quality: Quality::Finer,
            channels_together: true,
        }
    }
}

impl LiveOptions {
    fn to_bits(self) -> ffi::RubberBandOptions {
        let mut options = self.quality.live_window_option();
        if self.formant_preserved {
            options |= ffi::RubberBandLiveOptionFormantPreserved;
        }
        if self.channels_together {
            options |= ffi::RubberBandLiveOptionChannelsTogether;
        }
        options
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PrepareError {
    #[error("sample rate must be positive")]
    InvalidSampleRate,
    #[error("channel count must be 1 or 2")]
    InvalidChannels,
    #[error("max host block must be positive")]
    InvalidMaxBlock,
    #[error("Rubber Band LiveShifter failed to construct")]
    ConstructFailed,
    #[error("Rubber Band reported an empty block size")]
    EmptyBlock,
}

/// Single-owner realtime pitch shifter. Not `Sync`; do not call concurrently.
pub struct LivePitchShifter {
    state: ffi::RubberBandLiveState,
    channels: usize,
    block_size: usize,
    start_delay: usize,
    pitch_scale: f64,
    formant_preserved: bool,
    input_ring: Vec<f32>,
    output_ring: Vec<f32>,
    input_len: usize,
    output_len: usize,
    output_read: usize,
    block_in: Vec<f32>,
    block_out: Vec<f32>,
    in_ptrs: Vec<*const f32>,
    out_ptrs: Vec<*mut f32>,
}

// SAFETY: Rubber Band LiveShifter instances are independently owned. Moving the
// wrapper across threads is fine; concurrent `shift` / `setPitchScale` is not.
unsafe impl Send for LivePitchShifter {}

impl LivePitchShifter {
    /// Allocate the native shifter and fixed scratch rings. Not realtime-safe.
    pub fn prepare(
        sample_rate: u32,
        channels: usize,
        max_host_block: usize,
        options: LiveOptions,
        initial_pitch_scale: f64,
    ) -> Result<Self, PrepareError> {
        if sample_rate == 0 {
            return Err(PrepareError::InvalidSampleRate);
        }
        if !(1..=2).contains(&channels) {
            return Err(PrepareError::InvalidChannels);
        }
        if max_host_block == 0 {
            return Err(PrepareError::InvalidMaxBlock);
        }
        // Keep debug level at 0 so the default logger stays RT-safe.
        // SAFETY: sets a process-wide default; no live state is required.
        unsafe {
            ffi::rubberband_live_set_default_debug_level(0);
        }
        // SAFETY: constructs a new owned LiveShifter; null is checked below.
        let state =
            unsafe { ffi::rubberband_live_new(sample_rate, channels as u32, options.to_bits()) };
        if state.is_null() {
            return Err(PrepareError::ConstructFailed);
        }
        let scale = if initial_pitch_scale.is_finite() && initial_pitch_scale > 0.0 {
            initial_pitch_scale
        } else {
            1.0
        };
        // SAFETY: `state` is a freshly created non-null LiveShifter.
        unsafe {
            ffi::rubberband_live_set_pitch_scale(state, scale);
            ffi::rubberband_live_set_debug_level(state, 0);
        }
        // SAFETY: `state` remains valid until deleted below on failure.
        let block_size = unsafe { ffi::rubberband_live_get_block_size(state) } as usize;
        if block_size == 0 {
            // SAFETY: deletes the just-created state on the empty-block path.
            unsafe {
                ffi::rubberband_live_delete(state);
            }
            return Err(PrepareError::EmptyBlock);
        }
        // Query Rubber Band's own start delay after construction with the
        // initial pitch scale already applied (see getStartDelay in
        // R3LiveShifter.cpp). Do not invent a separate theoretical formula.
        // SAFETY: pitch scale was set before querying start delay.
        let start_delay = unsafe { ffi::rubberband_live_get_start_delay(state) } as usize;
        // Rings sized for one engine block plus one worst-case host block so
        // process never grows them.
        let ring_capacity = (block_size + max_host_block) * channels;
        let mut shifter = Self {
            state,
            channels,
            block_size,
            start_delay,
            pitch_scale: scale,
            formant_preserved: options.formant_preserved,
            input_ring: vec![0.0; ring_capacity],
            output_ring: vec![0.0; ring_capacity],
            input_len: 0,
            output_len: 0,
            output_read: 0,
            block_in: vec![0.0; block_size * channels],
            block_out: vec![0.0; block_size * channels],
            in_ptrs: vec![std::ptr::null(); channels],
            out_ptrs: vec![std::ptr::null_mut(); channels],
        };
        // Grow BQResampler ratio-change scratch across the in-range pitch
        // extremes and clear LiveShifter's m_firstProcess off the audio thread
        // so the first realtime setPitchScale/shift never heap-allocates.
        shifter.warm_realtime_path(scale);
        Ok(shifter)
    }

    /// Off-audio-thread: grow BQResampler ratio-change scratch across the
    /// in-range pitch extremes, then `rubberband_live_reset` so the host still
    /// sees a cold start. `m_firstProcess` stays true afterward; the next
    /// realtime `setPitchScale` (if any) re-measures using the preallocated
    /// delay scratch, and the first `shift` applies the usual start pad.
    fn warm_realtime_path(&mut self, initial_scale: f64) {
        let frames = self.block_size;
        let channels = self.channels;
        let silence = vec![0.0_f32; frames * channels];
        let mut discarded = vec![0.0_f32; frames * channels];
        for scale in [0.5_f64, 2.0, initial_scale] {
            self.set_pitch_scale(scale);
            self.process_interleaved(&silence, &mut discarded, frames, channels);
        }
        // SAFETY: exclusive owned LiveShifter; reset is off the audio thread.
        unsafe {
            ffi::rubberband_live_reset(self.state);
        }
        self.input_len = 0;
        self.output_len = 0;
        self.output_read = 0;
        // Force a post-reset pitch apply so delay measurement runs with the
        // preallocated scratch while m_firstProcess is true again.
        self.pitch_scale = if (initial_scale - 1.0).abs() < f64::EPSILON {
            0.5
        } else {
            1.0
        };
        self.set_pitch_scale(initial_scale);
        self.start_delay = self.start_delay();
    }

    pub fn block_size(&self) -> usize {
        self.block_size
    }

    /// Rubber Band `getStartDelay()` for the current pitch scale / options.
    ///
    /// Always re-queries the native shifter so pitch changes are reflected.
    pub fn start_delay(&self) -> usize {
        // SAFETY: exclusive owned LiveShifter; matches RubberBandLiveShifter::getStartDelay.
        unsafe { ffi::rubberband_live_get_start_delay(self.state) as usize }
    }

    /// Host-reported latency in samples — exactly Rubber Band's start delay.
    pub fn latency_samples(&self) -> u32 {
        self.start_delay() as u32
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// RT-safe when not concurrent with `process_*`. Skips the native call when
    /// the scale is unchanged so Rubber Band never re-enters ratio setup.
    pub fn set_pitch_scale(&mut self, scale: f64) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        if (scale - self.pitch_scale).abs() < f64::EPSILON {
            return;
        }
        // SAFETY: `self.state` is owned and exclusive for the lifetime of `self`.
        unsafe {
            ffi::rubberband_live_set_pitch_scale(self.state, scale);
        }
        self.pitch_scale = scale;
        // Pitch scale is an input to getStartDelay(); refresh the cached peek
        // used only for prepare-time capacity sizing.
        self.start_delay = self.start_delay();
    }

    /// RT-safe formant option toggle.
    pub fn set_formant_preserved(&mut self, preserved: bool) {
        if preserved == self.formant_preserved {
            return;
        }
        self.formant_preserved = preserved;
        let option = if preserved {
            ffi::RubberBandLiveOptionFormantPreserved
        } else {
            ffi::RubberBandLiveOptionFormantShifted
        };
        // SAFETY: exclusive ownership; formant option may change at any time.
        unsafe {
            ffi::rubberband_live_set_formant_option(self.state, option);
        }
    }

    /// Reset internal buffers while retaining the current pitch/formant settings.
    /// Not realtime-safe: Rubber Band re-measures resampler delay and this
    /// re-warms ratio-change scratch afterward.
    pub fn reset(&mut self) {
        let scale = self.pitch_scale;
        self.warm_realtime_path(scale);
    }

    /// Process interleaved host audio for `frames` frames.
    ///
    /// `input` and `output` must each hold `frames * channels` samples and must
    /// not alias. Remaining capacity in the rings is guaranteed by `prepare`.
    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: usize,
        channels: usize,
    ) {
        debug_assert_eq!(channels, self.channels);
        debug_assert_eq!(input.len(), frames * channels);
        debug_assert_eq!(output.len(), frames * channels);
        if frames == 0 || channels == 0 {
            return;
        }

        let need = frames * channels;
        debug_assert!(self.input_len + need <= self.input_ring.len());
        self.input_ring[self.input_len..self.input_len + need].copy_from_slice(&input[..need]);
        self.input_len += need;

        while self.input_len >= self.block_size * channels {
            self.shift_one_block();
        }

        self.drain_output(output, frames, channels);
    }

    fn shift_one_block(&mut self) {
        let channels = self.channels;
        let block = self.block_size;
        for frame in 0..block {
            for channel in 0..channels {
                self.block_in[channel * block + frame] =
                    self.input_ring[frame * channels + channel];
            }
        }
        let consumed = block * channels;
        self.input_ring.copy_within(consumed..self.input_len, 0);
        self.input_len -= consumed;

        for channel in 0..channels {
            self.in_ptrs[channel] = self.block_in[channel * block..(channel + 1) * block].as_ptr();
            self.out_ptrs[channel] =
                self.block_out[channel * block..(channel + 1) * block].as_mut_ptr();
        }
        // SAFETY: planar pointers reference `block_in` / `block_out` for exactly
        // `block` frames per channel; buffers do not alias; exclusive ownership.
        unsafe {
            ffi::rubberband_live_shift(self.state, self.in_ptrs.as_ptr(), self.out_ptrs.as_ptr());
        }

        let out_need = block * channels;
        debug_assert!(self.output_len - self.output_read + out_need <= self.output_ring.len());
        if self.output_read > 0 {
            self.output_ring
                .copy_within(self.output_read..self.output_len, 0);
            self.output_len -= self.output_read;
            self.output_read = 0;
        }
        for frame in 0..block {
            for channel in 0..channels {
                self.output_ring[self.output_len + frame * channels + channel] =
                    self.block_out[channel * block + frame];
            }
        }
        self.output_len += out_need;
    }

    fn drain_output(&mut self, output: &mut [f32], frames: usize, channels: usize) {
        let need = frames * channels;
        let available = self.output_len - self.output_read;
        if available >= need {
            output[..need]
                .copy_from_slice(&self.output_ring[self.output_read..self.output_read + need]);
            self.output_read += need;
            return;
        }
        if available > 0 {
            output[..available]
                .copy_from_slice(&self.output_ring[self.output_read..self.output_len]);
            self.output_read = self.output_len;
            output[available..need].fill(0.0);
        } else {
            output[..need].fill(0.0);
        }
    }
}

impl Drop for LivePitchShifter {
    fn drop(&mut self) {
        if !self.state.is_null() {
            // SAFETY: frees the owned LiveShifter exactly once.
            unsafe {
                ffi::rubberband_live_delete(self.state);
            }
            self.state = std::ptr::null_mut();
        }
    }
}
