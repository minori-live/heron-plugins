//! Checked-in FFI for the vendored Rubber Band C API (`rubberband-c.h`).
//!
//! Regenerated offline from `third_party/rubberband/rubberband/rubberband-c.h`
//! so CI never needs bindgen/libclang. Covers both the realtime Stretcher and
//! LiveShifter surfaces so Heron can share this crate for resampling.

#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code
)]

use std::os::raw::{c_int, c_uint};

pub const RUBBERBAND_VERSION: &str = "4.0.0";
pub const RUBBERBAND_API_MAJOR_VERSION: u32 = 3;
pub const RUBBERBAND_API_MINOR_VERSION: u32 = 0;

pub type RubberBandOption = c_uint;
pub type RubberBandOptions = c_int;
pub type RubberBandLiveOptions = c_int;

pub const RubberBandOptionProcessOffline: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionProcessRealTime: RubberBandOption = 0x0000_0001;
pub const RubberBandOptionStretchElastic: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionStretchPrecise: RubberBandOption = 0x0000_0010;
pub const RubberBandOptionTransientsCrisp: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionTransientsMixed: RubberBandOption = 0x0000_0100;
pub const RubberBandOptionTransientsSmooth: RubberBandOption = 0x0000_0200;
pub const RubberBandOptionDetectorCompound: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionDetectorPercussive: RubberBandOption = 0x0000_0400;
pub const RubberBandOptionDetectorSoft: RubberBandOption = 0x0000_0800;
pub const RubberBandOptionPhaseLaminar: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionPhaseIndependent: RubberBandOption = 0x0000_2000;
pub const RubberBandOptionThreadingAuto: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionThreadingNever: RubberBandOption = 0x0001_0000;
pub const RubberBandOptionThreadingAlways: RubberBandOption = 0x0002_0000;
pub const RubberBandOptionWindowStandard: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionWindowShort: RubberBandOption = 0x0010_0000;
pub const RubberBandOptionWindowLong: RubberBandOption = 0x0020_0000;
pub const RubberBandOptionSmoothingOff: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionSmoothingOn: RubberBandOption = 0x0080_0000;
pub const RubberBandOptionFormantShifted: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionFormantPreserved: RubberBandOption = 0x0100_0000;
pub const RubberBandOptionPitchHighSpeed: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionPitchHighQuality: RubberBandOption = 0x0200_0000;
pub const RubberBandOptionPitchHighConsistency: RubberBandOption = 0x0400_0000;
pub const RubberBandOptionChannelsApart: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionChannelsTogether: RubberBandOption = 0x1000_0000;
pub const RubberBandOptionEngineFaster: RubberBandOption = 0x0000_0000;
pub const RubberBandOptionEngineFiner: RubberBandOption = 0x2000_0000;

pub const RubberBandLiveOptionWindowShort: RubberBandLiveOptions = 0x0000_0000;
pub const RubberBandLiveOptionWindowMedium: RubberBandLiveOptions = 0x0010_0000;
pub const RubberBandLiveOptionFormantShifted: RubberBandLiveOptions = 0x0000_0000;
pub const RubberBandLiveOptionFormantPreserved: RubberBandLiveOptions = 0x0100_0000;
pub const RubberBandLiveOptionChannelsApart: RubberBandLiveOptions = 0x0000_0000;
pub const RubberBandLiveOptionChannelsTogether: RubberBandLiveOptions = 0x1000_0000;

#[repr(C)]
pub struct RubberBandState_ {
    _unused: [u8; 0],
}
pub type RubberBandState = *mut RubberBandState_;

#[repr(C)]
pub struct RubberBandLiveState_ {
    _unused: [u8; 0],
}
pub type RubberBandLiveState = *mut RubberBandLiveState_;

unsafe extern "C" {
    pub fn rubberband_new(
        sampleRate: c_uint,
        channels: c_uint,
        options: RubberBandOptions,
        initialTimeRatio: f64,
        initialPitchScale: f64,
    ) -> RubberBandState;
    pub fn rubberband_delete(state: RubberBandState);
    pub fn rubberband_reset(state: RubberBandState);
    pub fn rubberband_get_engine_version(state: RubberBandState) -> c_int;
    pub fn rubberband_set_time_ratio(state: RubberBandState, ratio: f64);
    pub fn rubberband_set_pitch_scale(state: RubberBandState, scale: f64);
    pub fn rubberband_get_time_ratio(state: RubberBandState) -> f64;
    pub fn rubberband_get_pitch_scale(state: RubberBandState) -> f64;
    pub fn rubberband_set_formant_scale(state: RubberBandState, scale: f64);
    pub fn rubberband_get_formant_scale(state: RubberBandState) -> f64;
    pub fn rubberband_get_preferred_start_pad(state: RubberBandState) -> c_uint;
    pub fn rubberband_get_start_delay(state: RubberBandState) -> c_uint;
    pub fn rubberband_get_latency(state: RubberBandState) -> c_uint;
    pub fn rubberband_set_transients_option(state: RubberBandState, options: RubberBandOptions);
    pub fn rubberband_set_detector_option(state: RubberBandState, options: RubberBandOptions);
    pub fn rubberband_set_phase_option(state: RubberBandState, options: RubberBandOptions);
    pub fn rubberband_set_formant_option(state: RubberBandState, options: RubberBandOptions);
    pub fn rubberband_set_pitch_option(state: RubberBandState, options: RubberBandOptions);
    pub fn rubberband_set_expected_input_duration(state: RubberBandState, samples: c_uint);
    pub fn rubberband_get_samples_required(state: RubberBandState) -> c_uint;
    pub fn rubberband_set_max_process_size(state: RubberBandState, samples: c_uint);
    pub fn rubberband_get_process_size_limit(state: RubberBandState) -> c_uint;
    pub fn rubberband_study(
        state: RubberBandState,
        input: *const *const f32,
        samples: c_uint,
        final_: c_int,
    );
    pub fn rubberband_process(
        state: RubberBandState,
        input: *const *const f32,
        samples: c_uint,
        final_: c_int,
    );
    pub fn rubberband_available(state: RubberBandState) -> c_int;
    pub fn rubberband_retrieve(
        state: RubberBandState,
        output: *const *mut f32,
        samples: c_uint,
    ) -> c_uint;
    pub fn rubberband_get_channel_count(state: RubberBandState) -> c_uint;
    pub fn rubberband_calculate_stretch(state: RubberBandState);
    pub fn rubberband_set_debug_level(state: RubberBandState, level: c_int);
    pub fn rubberband_set_default_debug_level(level: c_int);

    pub fn rubberband_live_new(
        sampleRate: c_uint,
        channels: c_uint,
        options: RubberBandOptions,
    ) -> RubberBandLiveState;
    pub fn rubberband_live_delete(state: RubberBandLiveState);
    pub fn rubberband_live_reset(state: RubberBandLiveState);
    pub fn rubberband_live_set_pitch_scale(state: RubberBandLiveState, scale: f64);
    pub fn rubberband_live_get_pitch_scale(state: RubberBandLiveState) -> f64;
    pub fn rubberband_live_set_formant_scale(state: RubberBandLiveState, scale: f64);
    pub fn rubberband_live_get_formant_scale(state: RubberBandLiveState) -> f64;
    pub fn rubberband_live_get_start_delay(state: RubberBandLiveState) -> c_uint;
    pub fn rubberband_live_set_formant_option(
        state: RubberBandLiveState,
        options: RubberBandOptions,
    );
    pub fn rubberband_live_get_block_size(state: RubberBandLiveState) -> c_uint;
    pub fn rubberband_live_shift(
        state: RubberBandLiveState,
        input: *const *const f32,
        output: *const *mut f32,
    );
    pub fn rubberband_live_get_channel_count(state: RubberBandLiveState) -> c_uint;
    pub fn rubberband_live_set_debug_level(state: RubberBandLiveState, level: c_int);
    pub fn rubberband_live_set_default_debug_level(level: c_int);
}
