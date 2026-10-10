# ADR-0002: EQ Nyquist display domain

- Status: Accepted
- Date: 2026-10-11
- Owners: project maintainers
- Scope: current EQ editor and non-real-time display data; parameter and saved-state compatibility remain unchanged
- Related: [PR #4](https://github.com/minori-live/heron-plugins/pull/4)

## Context

The EQ graph used a fixed 10 Hz–30 kHz axis. At lower sample rates it displayed
invalid response above Nyquist under a translucent mask; at higher rates the
same cap omitted valid measured audio. Removing the mask alone would retain
an invalid frequency domain or leave valid frequencies undisplayed. The chosen
domain also changes pointer-to-frequency mapping, so it needs an explicit
interaction contract.

## Decision

The logarithmic graph spans 10 Hz to half the current sample rate without a
fixed upper display cap. Response curves, measured spectra, collision shading,
ticks, snapping and pointer mapping use that domain. Analyzer and cached FIR
response grids extend to actual Nyquist. Frozen spectra retain their captured
data but display only frequencies within the current domain.

Band nodes whose center frequency exceeds current Nyquist are hidden and cannot
be point- or box-selected. A sample-rate change cancels an active graph pointer
gesture and balances its host edit; later pointer motion cannot resume that
gesture using the new axis.

The existing 1 Hz–30 kHz saved and automated band parameter range, IDs and
saved-state meaning remain unchanged. Manual frequency editing still starts at
10 Hz; saved frequencies below 10 Hz retain the existing left-edge clamping
for drawing and interaction. A sample-rate or display-range change does not
rewrite band settings. This decision changes the editor and non-real-time
display data, not audio callback ownership, real-time processing or the
persistence protocol.

## Alternatives rejected

### Keep the fixed axis and cover invalid frequencies

A mask preserves screen positions but still allocates graph space to an
invalid domain and can expose misleading digital responses beneath it.

### Cap the axis at the lesser of Nyquist and 30 kHz

This hides invalid low-rate frequencies but omits valid audio above 30 kHz at
high sample rates. The stored band parameter limit does not define the measured
spectrum's display limit.

## Consequences

Valid frequencies fill the graph at every supported sample rate: 96 kHz audio
ends at 48 kHz, and 192 kHz audio ends at 96 kHz. A band's pixel position changes
when the sample rate changes. Cancelling an active graph gesture prevents that
remapping from changing an ongoing edit. Frequency controls retain their
existing parameter range even when the graph shows higher measured frequencies.

## Verification

Owned regressions cover low- and high-rate axis endpoints, matching graph
gestures, hidden-node selection, sample-rate gesture cancellation, frozen
spectrum filtering, measured tones above 30 kHz and cached FIR response through
Nyquist. Native before/after captures at normal and minimum editor sizes verify
the actual graph, endpoint labels and removal of the mask.

## Reconsider when

A supported sample-rate range or a deliberate band-parameter compatibility
change requires a different usable frequency domain, or host interaction
evidence requires a different policy for sample-rate changes during gestures.
