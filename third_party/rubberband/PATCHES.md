# Rubber Band vendor notes

Pinned release: **4.0.0** (see `SOURCE.json`).

Builds use upstream `single/RubberBandSingle.cpp` (built-in FFT / BQResampler,
`NO_THREADING`) via `crates/rubberband-sys`.

## Owned patches

### `R3LiveShifter::measureResamplerDelay` preallocated scratch

Upstream allocates two heap `std::vector<float>` buffers inside
`measureResamplerDelay()`. That helper also runs from `setPitchScale` while
`m_firstProcess` is true, which Heron Repitch must be able to reach from the
audio thread on the first callback after construction or `reset`.

Patch: size `m_delayMeasureIn` / `m_delayMeasureOut` in `createResamplers`
(off the audio thread) and reuse them in `measureResamplerDelay` without
growing. Validated by Repitch `rt-paranoid` process tests and package builds.
