# Development environment

This document adapts Heron's runtime and execution conventions to the plug-in
producer. It describes the tools and commands actually configured in this
repository. The Rust workspace contains four plug-ins, shared Iced/Truce UI and
reusable plug-in DSP; the Node package owns TypeScript build and release tools.

Adapted on 2026-10-10 from Heron's
[environment conventions](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/environment.md)
at source commit `fef5cdb165bb322e07f2a280fea8372aa5e22393`.
The [documentation index](README.md) defines local authority; tool versions and
commands below were checked against this producer's own configuration.

## Locked toolchain

`mise.toml` declares tool release lines and `mise.lock` resolves concrete
versions and download artifacts. The checked-in tools are:

| Tool                                        | Role                                                    |
| ------------------------------------------- | ------------------------------------------------------- |
| Node.js 26                                  | Execute erasable TypeScript producer scripts directly   |
| pnpm 12.10                                  | Install the root JavaScript tooling package             |
| Stable Rust, rustfmt, Clippy and llvm-tools | Compile, format, lint and instrument the Rust workspace |
| CMake 4.3                                   | Build the C++ final-bundle identity/loading probes      |
| cargo-truce 6.3                             | Build and package the pinned Truce plug-in formats      |
| cargo-llvm-cov 0.8                          | Collect Rust coverage                                   |
| cargo-shear 1.13                            | Audit owned Rust dependency use                         |
| APM 0.33.0                                  | Resolve and deploy reusable agent skills                |

The lock currently resolves Node.js 26.11.1, pnpm 12.10.0 and Rust 1.99.0;
`Cargo.toml` declares edition 2024 and a minimum Rust version of 1.98. The files,
not this version snapshot, are authoritative after a toolchain update.

From the repository root, install the locked tools and JavaScript dependencies:

```sh
mise install --locked
mise run install
apm install --frozen
```

`pnpm-lock.yaml` separately locks the JavaScript dependencies. `mise run install`
uses `pnpm install --frozen-lockfile --prefer-offline`. Build, format, lint,
coverage and full-check mise tasks depend on this installation task. The
Rust-only `check-fast` task does not install pnpm dependencies.

`apm.yml` declares the agent skill dependencies and `apm.lock.yaml` pins their
resolution. Deployed skills under `.agents/skills/` are checked into Git so a
fresh checkout can use them without a network install. `apm install --frozen`
refreshes those deployed copies from the lockfile; update the declarations and
lockfile rather than editing installed copies. Keep `apm_modules/` ignored as
local APM cache. The installed `stss` skill tightens defensive prose while
preserving facts and meaningful limits.

## Shell and command conventions

Run project commands from a login shell so normal startup files activate mise
and the repository-managed tools. On Windows, use PowerShell 7 (`pwsh`), not
Windows PowerShell 5.1 (`powershell.exe`). Automation and IDE subprocesses must
select the same repository environment. Verify the selected tools with:

```sh
node --version
pnpm --version
rustc --version
cargo --version
cmake --version
```

The configured root commands are:

```sh
mise run check
mise run check-fast
mise run build
mise run format
mise run format-check
mise run lint
mise run ci:coverage
```

`mise run check` is the full local validation path: suite version consistency,
Oxfmt, Oxlint, strict TypeScript and script tests, Rust fmt, cargo-shear,
all-target workspace Clippy, workspace tests and four separate plug-in
`rt-paranoid` test runs. `mise run check-fast` runs Rust formatting, all-target
Clippy and workspace tests; it omits JavaScript and the separate paranoid runs.
CI runs source checks once on Linux and uses coverage for each platform's Rust
test pass. Native packaging checks the final bundles; AU is built on macOS and
AAX is checked by its dedicated release job when enabled.

Use existing leaf commands for narrower work:

```sh
pnpm check
pnpm format:check
pnpm lint
pnpm version:check
cargo test --locked -p heron-eq
cargo test --locked -p heron-eq --features rt-paranoid
cargo check --workspace --locked --no-default-features --features au
cargo check --workspace --locked --no-default-features --features aax
```

Run each plug-in's `rt-paranoid` tests separately, as configured in mise; their
allocation instrumentation must not introduce multiple global allocators into
one test binary. Optional Rust feature checks do not establish native AU/AAX
wrapper, SDK, signing or loading behavior. Interactive regressions marked ignored
require their stated platform/desktop environment and are not claimed by an
ordinary test run.

## Native platform prerequisites

The shared CI setup in
[`setup-build`](../../.github/actions/setup-build/action.yml) is the concrete
reference for platform dependencies, SDK checkout and compiler caches.

On Linux, install a C/C++ toolchain and the GUI development libraries used by
the setup action: Cairo, Fontconfig, FreeType, GLib/Pango, X11/XCB and related
cursor/input/RandR libraries, xkbcommon, Wayland and wayland-protocols. The
action records the exact Ubuntu package list. Use native x64 and arm64 build
hosts for their corresponding release targets. Native editor event tests need
a usable graphical desktop; a successful headless build does not exercise it.

On Windows, use the MSVC Rust target with Visual Studio's Desktop development
with C++ workload and a Windows SDK so Cargo and CMake can compile/link native
code and the C++ VST3 probe. This workspace is a plug-in producer and has no
cpal device runtime, ASIO build requirement or Heron native addon.

On macOS, provide Xcode command-line build tools and the Apple SDK/tooling used
by CMake, clang, `lipo`, `dsymutil`, codesign, `auval` and `hdiutil`. Packaging
builds both `aarch64-apple-darwin` and `x86_64-apple-darwin`; the build script
adds both Rust targets and uses macOS 14.2 as the deployment target. Intel
validation on Apple Silicon requires Rosetta. CI installs it with
`softwareupdate --install-rosetta --agree-to-license`.

## SDKs, patches and packaging

Fetch the pinned VST3 SDK before building final-artifact probes:

```sh
mise run sdk:vst3
mise run build
```

The fetch script creates an independent checkout at `third_party/vst3sdk`, pins
the commit recorded in that script and initializes recursive submodules. That
SDK directory is ignored by Git. `fixtures/CMakeLists.txt` builds this
repository's C++ VST3 probe and, on macOS, AU probe against the required native
headers/framework. This workspace does not generate Heron's Bindgen or N-API
bindings.

The tracked `third_party/baseview-truce`, `third_party/truce-gui` and
`third_party/truce-iced` directories are separate from the downloaded SDK.
`Cargo.toml` selects these GUI patches through `[patch.crates-io]`. Preserve
their licenses and `PATCHES.md` provenance; a clean checkout needs their full
sources. Keep Truce's pinned wrappers and paired Windows scale/focus patches
consistent when upgrading them.

`scripts/catalog.ts` owns producer bundle names and manifest identities for
Gain, EQ, Sine and Metronome. Default plug-in features build VST3. A macOS
producer build packages VST3 and AUv2 for both architectures, plus a DMG;
Windows x64 and Linux x64/arm64 package VST3. AAX is explicitly disabled in
`release-config.json` and requires the separate licensed prerequisites in
[CI and release operations](ci.md).

`mise run build` writes runtime archives and installers to
`target/release-assets/`. It also uses `target/release-stage/`,
`target/archive-check/` and `target/bundle-probe/` for
staging and final-archive checks. Each run clears the release asset output.
macOS development bundles use ad-hoc signing; tag releases keep thin LTO, require
a clean committed source tree and use Developer ID signing/notarization.

AU validation temporarily installs all four complete components under the
current user's `Library/Audio/Plug-Ins/Components`, restores existing bundles
afterward and verifies both architectures with simultaneous instances and
`auval`. Use a controlled local validation session; the script preserves prior
installations and does not reset global AU caches. See
[artifact contracts](artifacts.md) and
[release operations](ci.md) for the exact gates and credentials.

GitHub artifacts and releases contain runtime bundles and installers. Plug-ins
do not publish symbol archives or integrate with Sentry.

## Formatting, checking and generated output

Hand-authored Node scripts and configuration use TypeScript. Node executes the
erasable syntax directly; the root `tsconfig.json` enables `erasableSyntaxOnly`
and checks these sources without emitting JavaScript. `pnpm check` performs
that strict type check and runs the owned Node script tests.

Oxfmt formats owned TypeScript, JSON, YAML, Markdown and other supported source
formats; Oxlint is the primary Node/TypeScript lint path. Their configurations
exclude build output, dependencies and `third_party/` snapshots. Rustfmt and
Clippy cover the owned Rust workspace. Keep generated bundles, symbols,
coverage, temporary SDK extraction and other build artifacts out of Git and
documentation directories. Source-size thresholds in
[Engineering standards](engineering-standards.md) are review requirements;
this repository currently has no automatic source-size check command.

## Revox DSP experiment

The Revox crate can be exercised without building a plug-in or opening a DAW.
Run from this repository root in a login shell. Acquire the pinned Revox model:

```sh
node scripts/fetch-revox-model.ts
```

FCPE's official repository does not provide this ONNX interface. Export its
checksum-verified MIT checkpoint with the development-only Python environment:

```sh
uv venv --python 3.12 target/revox/venv
uv pip install --python target/revox/venv/bin/python --require-hashes -r scripts/revox/requirements.lock
target/revox/venv/bin/python scripts/revox/export_fcpe.py
target/revox/venv/bin/python scripts/revox/prepare_vocoder.py
```

On Windows use `target/revox/venv/Scripts/python.exe` for the Python executable paths.
`uv` is a development prerequisite for these export commands, not a plug-in
runtime dependency. The exporter validates dynamic frame lengths against the
original PyTorch implementation and writes `fcpe-export.json`, feature references,
the upstream MIT license and the ONNX file under ignored `target/revox/`. It also
exports `fcpe-plugin.onnx`, with a fixed 213-frame window and symbolic `batch`
dimension, plus `fcpe-plugin-export.json` with upstream parity evidence.
`prepare_vocoder.py` verifies the original Revox hash and audits batch-sensitive
axes before writing `models/revox-plugin.onnx` and its manifest. Only input/output
shape annotations change: time is fixed at 212 frames / 101760 noise samples,
and `batch` remains symbolic. Operations and weights are unchanged. Both graphs
are checked for batch sizes 1 and 2. Original variable-length mono probe graphs
remain available as `fcpe.onnx` and `models/revox.onnx`.
Model files are not downloaded by ordinary Cargo tests or at audio-processing time.

Run deterministic frontend, resampling, pitch alignment and backend-policy tests:

```sh
cargo test --locked -p heron-revox-dsp --lib
cargo clippy --locked -p heron-revox-dsp --all-targets -- -D warnings
```

After model acquisition, run actual-model evidence and the performance probe:

On macOS, first set `export MACOSX_DEPLOYMENT_TARGET=14.2`, matching the suite's
release deployment target. The Revox build script locates and links Apple's
compiler runtime required by the bundled CoreML objects.

```sh
cargo test --release --locked -p heron-revox-dsp --test model_pipeline -- --ignored
cargo run --release --locked -p heron-revox-dsp --example revox_probe -- --chunk-ms 320
cargo run --release --locked -p heron-revox-dsp --example revox_probe -- --input vocal-48k-mono.wav --semitones 3 --output-dir target/revox/vocal-up3
```

The probe uses automatic acceleration and a generated 220 Hz harmonic signal.
It writes 32-bit float WAVs and JSON containing loading/processing times, registered
providers and fallbacks, RMS/peak, F0 summaries, and independent-window throughput.
Output F0 uses FCPE again and is not an independent quality assessment. A realtime
factor below one means throughput exceeds audio duration, not that monitoring
latency or window seams are acceptable. `--chunk-ms` intentionally benchmarks
independent windows and does not implement continuous streaming.

The 2026-10-13 Apple M2 Pro run used the default CoreML → WebGPU → CPU chain,
registered successfully for both models, and a 2-second synthetic harmonic signal.
Output retained all 96,000 samples, was finite, and had a median estimated F0 of
220.15 Hz for a 219.98 Hz input at zero shift. First whole-input processing took
981.5 ms, excluding 2061.6 ms of model loading. Seven independent windows of up
to 320 ms had a 20.0 ms median and 199.8 ms maximum processing time. Shape changes
and model warm-up affect these numbers. Windowed output differed from the whole
output by 1.35 times its RMS, so these timings do not establish seamless streaming.
The generated report and WAVs are under ignored `target/revox/probe-default-ep/`.

Default EPs use the largest compatible rc.14 prebuilt archive for each target:

| Target              | Registration priority, ending in CPU        |
| ------------------- | ------------------------------------------- |
| Apple Silicon macOS | CoreML, WebGPU, CPU                         |
| Windows x64         | TensorRT RTX, TensorRT, CUDA, DirectML, CPU |
| Windows arm64       | DirectML, CPU                               |
| Linux x64/arm64     | TensorRT RTX, TensorRT, CUDA, CPU           |

Session creation attempts every entry; unavailable providers leave diagnostics and
later providers still register. CPU is the final fallback, with no backend mode
selector. The report's `registered` array records session registration rather than measured
operator placement. The `fallback` array records individual registration errors
or a failed graph initialization with its complete attempted chain.

Prebuilt does not mean driver-free: ORT's current CUDA binaries require CUDA
13.2 or later and cuDNN 9.23 or later; TensorRT variants also need their matching
runtime libraries. The `copy-dylibs` feature supplies ORT's distributed helper
libraries for development. WebGPU uses Dawn and remains experimental upstream.
Native execution was tested on Apple Silicon; Windows/Linux GPU paths still need
validation on their respective hardware.

The [pinned prebuilt matrix](https://github.com/pykeio/ort/blob/1ee1f842e8a381153ede7a506f01734e545854e0/ort-sys/build/download/dist.tsv)
offers Windows/Linux WebGPU, OpenVINO, and Linux x64 MIGraphX as separate archives,
not extensions that can be merged into the default NVIDIA archive. Enabling all
those Cargo features together fails distribution resolution. Intel macOS has no
rc.14 prebuilt; it needs a compatible externally supplied ORT build
(`ORT_LIB_PATH`). Revox has a separate development package as described below.

### Heron Revox plug-in

Prepare the models above before running workspace Cargo checks: the plug-in's
build script verifies the pinned SHA-256 values of `fcpe-plugin.onnx` and
`models/revox-plugin.onnx` and embeds one copy of each graph in the native binary.
The Test workflow prepares the graphs once on Apple Silicon and supplies that
same checked artifact to each platform's workspace checks.

```sh
export MACOSX_DEPLOYMENT_TARGET=14.2 # macOS
cargo test --release --locked -p heron-revox --features rt-paranoid --lib
cargo test --release --locked -p heron-revox embedded_models_render -- --ignored
cargo test --release --locked -p heron-revox render_editor_evidence -- --ignored
pnpm build:revox --au
```

`build:revox` builds a separate Apple Silicon VST3, optionally AUv2 with `--au`,
with embedded models, a bundle-local WebGPU Dawn library, licenses and model
provenance. It verifies the extracted bundle, native identities and actual VST3
audio: a four-second 220 Hz harmonic signal with +3 semitones and +50 cents must
produce the combined shift, finite samples and the reported leading latency.
AU builds also pass registration/version checks and `auval`. This is an
ad-hoc-signed development package, separate from the existing universal suite
release; Intel macOS is not supported by the pinned ORT prebuilts. The standard
suite build continues to package its four existing plug-ins. Windows/Linux Rust
paths remain configured but have no Revox native-package validation claim.

The editor has two controls: Semitones (-12..12, step 1) and Cents (-100..100,
step 1), combined additively. Both default to zero and still resynthesize audio.
Double-click a dial or click its readout for numeric input, Command/Control-click
resets, and Shift-drag adjusts finely. Tab selects a dial; arrows change by one,
Shift+arrows by three semitones or ten cents, Home resets, and Enter opens numeric
entry. Escape cancels the edit. Host automation and saved states use IDs 0 and 1.

Supported host rates are 44.1/48/88.2/96/176.4/192 kHz, mono or stereo. Reported
latency is 1.48 seconds plus 128 host samples. Revox targets track processing at
this stage. It uses 160 ms output steps with model context and a 40 ms overlap;
this is not an acceptance claim for transparent vocal seams or live monitoring.
The worker fixes the ORT `batch` dimension to the active channel count: mono uses
1, stereo uses 2. Channels from the same window are processed together, retaining
independent features, pitch and excitation; channels are never summed. It does
not wait for future windows to assemble a batch or add batching latency. Session
preparation and warm-up occur on the worker after the layout is known.
During loading, missed worker deadlines or failure, output is aligned dry audio
and the footer shows the state. Failed processing is retried at host reactivation.
An unsupported rate passes dry audio with zero latency. Offline bounce waits for
worker output; real-time and buffered playback never wait for inference.

## Dependency and suite version changes

Use stable published dependencies and runtimes. Prerelease, nightly, canary or
unpublished versions need an explicit reason and documented tradeoff; preserve
the provenance of existing pinned patches. Declare tool policies in
`mise.toml`, and refresh their resolution through:

```sh
mise lock
mise install --locked
```

Do not hand-edit `mise.lock`. Commit relevant declarations and lockfile updates
together. Use repository-managed pnpm for JavaScript dependency changes and
commit `pnpm-lock.yaml`; commit Rust declaration changes with `Cargo.lock`.
Ordinary Cargo validation uses `--locked` so it cannot silently alter dependency
resolution.

`VERSION` is the suite version's source of truth, independently of Heron's
application version. After editing it, synchronize root package/workspace
metadata and refresh the owned crate versions in the Cargo lockfile:

```sh
pnpm version:sync
cargo check --workspace
pnpm version:check
cargo metadata --locked --all-features --format-version 1
```

The deliberate unlocked Cargo check in this version-update sequence refreshes
`Cargo.lock`; review and commit that change. `version:sync` updates the root
`package.json` and `[workspace.package].version`; member crates inherit the
workspace version. `version:check`, included in `mise run check`, checks those
mirrors. It does not by itself update or inspect `Cargo.lock`. Formal build
preflight additionally resolves Cargo metadata with `--locked --all-features`
and checks release source cleanliness before and after native builds.

Tag/release operations, Apple secrets and the producer-owned Heron upgrade PR
workflow are documented in [CI and release operations](ci.md). Keep credentials
outside tracked configuration and generated credentials outside documentation.
