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
- GitHub artifacts and releases contain runtime bundles and installers only.
  Plug-ins do not publish symbol archives or integrate with Sentry. Future
  reporting needs an independent ADR and must preserve real-time constraints
  and the host DAW's panic/signal/log/crash-handler ownership.
- Keep Truce and its wrappers pinned together. Default plugin features remain
  `vst3`; `au` and `aax` are optional, platform-specific release formats.
- `VERSION`, `workspace.package.version` and package release metadata must agree.
  Use repository version tooling when changing the suite version.

## Development and verification

Use the locked `mise` toolchain from a login shell. Windows shell commands use
PowerShell 7 (`pwsh`). Follow `agents/docs/ci.md` for the release matrix and gates.
See [the environment guide](agents/docs/environment.md) for setup and platform
prerequisites. `mise run check` is the full validation path; use narrower owned
checks when they establish the required evidence.

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
Follow [the testing policy](agents/docs/testing.md) when adding, changing or
auditing tests and coverage.

## Engineering and documentation

Production Rust must handle external failures explicitly. Keep unsafe blocks
small and document ownership, lifetime and thread-affinity assumptions. New
production files must stay below 800 lines; 1200 lines is a hard split threshold.
Split by feature and ownership. Tests/generated/ABI sources may be justified
exceptions.

Follow [the engineering standards](agents/docs/engineering-standards.md) for
implementation and review, [architecture](agents/docs/architecture.md) for
thread/native boundaries, and [interaction design](agents/docs/interaction-design.md)
and [the design system](agents/docs/design-system.md) for native editors.
Logic Pro is the default gesture and EQ workflow reference. Material deviations
need a documented decision; stable parameter/state compatibility remains required.

Keep repository-wide agent instructions here. Architecture, development, CI and
ADRs belong under `agents/docs/`. Create an ADR for persistence, threading,
real-time, foundational dependency or compatibility changes. This repository has
no public website. Preserve upstream third-party licenses and record any pinned
patches and their validation. Do not edit installed skills under `.agents/skills/`.

## Supporting notes

- [Documentation authority and full standards index](agents/docs/README.md)
- [Engineering standards](agents/docs/engineering-standards.md)
- [Testing policy](agents/docs/testing.md)
- [Architecture and real-time constraints](agents/docs/architecture.md)
- [Native editor interaction design](agents/docs/interaction-design.md)
- [Native editor design system](agents/docs/design-system.md)
- [Development environment](agents/docs/environment.md)
- [CI and release operations](agents/docs/ci.md)
- [Published artifact contract](agents/docs/artifacts.md)
- [EQ implementation and verification](agents/docs/heron-eq.md)
- [ADR lifecycle and template](agents/docs/adr/README.md)
