# Agent Guide

Heron Plugins owns Heron Gain, Heron EQ, Heron Sine, Heron Metronome and their shared Rust
UI. Heron consumes released VST3 artifacts; this repository also publishes AUv2
components for macOS. AAX is opt-in and requires the licensed SDK and PACE release
signing prerequisites described in `agents/docs/ci.md`.

## Ownership and compatibility

- Keep this Rust workspace independent of Heron's Electron, audio engine, host,
  runtime and project database. Do not introduce source dependencies on Heron.
- Preserve `truce.toml` vendor/bundle IDs, VST3 class IDs, AU manufacturer/subtype
  codes, parameter IDs, units/ranges and saved-state meaning. Identity or state
  changes require an explicit compatibility decision and regression evidence.
- Heron Metronome is a MIDI-triggered click instrument. Heron's tempo/count-in
  scheduler remains owned by Heron; other DAWs send accent note 84 and beat note 72.
- Keep allocation, filesystem/network access, UI work and blocking synchronization
  out of audio callbacks. Recoverable failures use typed `Result` values; never
  unwind through plugin ABI, platform or audio callbacks.
- This release has no plugin-side runtime reporting or Sentry upload setup.
  Generate debug/source archives offline; Heron owns uploading its pinned VST3
  symbols. Future reporting needs an independent ADR and must preserve real-time
  constraints and the host DAW's panic/signal/log/crash-handler ownership.
- Keep Truce and its wrappers pinned together. Default plugin features remain
  `vst3`; `au` and `aax` are optional, platform-specific release formats.
- `VERSION`, `workspace.package.version` and package release metadata must agree.
  Use repository version tooling when changing the suite version.

## Development and verification

Use the locked `mise` toolchain from a login shell. Windows shell commands use
PowerShell 7 (`pwsh`). Follow `agents/docs/ci.md` for the release matrix and gates.

Rust validation at the owned boundary:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --locked -p heron-gain --features rt-paranoid
cargo test --locked -p heron-eq --features rt-paranoid
cargo test --locked -p heron-sine --features rt-paranoid
cargo test --locked -p heron-metronome --features rt-paranoid
```

Tests must prove plugin-owned behavior: DSP output, MIDI/sample timing, resource
bounds, editor composition, release identity and historical state compatibility.
Do not add upstream SDK conformance tests, incidental markup/file inventories or
duplicate evidence. Historical fixtures are immutable evidence from the old
Heron implementation; do not regenerate them from current plugin code to make a
compatibility failure pass. Their provenance and expected values live beside the
regressions. Heron owns artifact-consumer and host integration tests.

## Engineering and documentation

Production Rust must handle external failures explicitly. Keep unsafe blocks
small and document ownership, lifetime and thread-affinity assumptions. New
production files must stay below 800 lines; 1200 lines is a hard split threshold.
Split by feature and ownership. Tests/generated/ABI sources may be justified
exceptions.

Keep repository-wide agent instructions here. Architecture, development, CI and
ADRs belong under `agents/docs/`. Create an ADR for persistence, threading,
real-time, foundational dependency or compatibility changes. This repository has
no public website. Preserve upstream third-party licenses and record any pinned
patches and their validation. Do not edit installed skills under `.agents/skills/`.
