# Plug-in architecture and real-time constraints

This is the producer's architecture contract. It adapts
[Heron's architecture and real-time constraints](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/architecture.md)
at source commit `fef5cdb165bb322e07f2a280fea8372aa5e22393` to an independently
released native plug-in suite. Repository instructions remain in
[AGENTS.md](../../AGENTS.md); the [documentation index](README.md) identifies
the local specifications and their authority. Requirements below govern new
work; a requirement is not evidence that every existing format or host passes it.

## Runtime and ownership

```text
DAW host
  -> pinned Truce format adapter (VST3 or AUv2)
       -> per-instance parameters and serialized state
       -> plug-in processor -> runtime-independent DSP
       -> native editor -> shared Rust/Iced visual primitives
       -> bounded telemetry and preparation handoffs
```

The plug-in runs inside the loading host's address space. The DAW owns audio
devices, transport, scheduling, project persistence, automation recording and
the supported editor attachment lifecycle. This repository owns the plug-in's
processing, parameters, state interpretation, native editor and release bundles.
It must remain usable without Heron's application services or source tree.

| Owner                                                             | Responsibility                                                                                      |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `plugins/heron-gain`, `heron-sine`, `heron-metronome`, `heron-eq` | Stable plug-in identity, parameter contract, processor and editor composition                       |
| `crates/plugin-dsp`                                               | Runtime-independent EQ configuration, coefficients, response and processing                         |
| `crates/iced-ui`                                                  | Rust semantic palettes, spacing and typography roles                                                |
| `plugins/heron-plugin-ui`                                         | Shared plug-in visual primitives and parameter-message adapters                                     |
| Pinned Truce adapters                                             | Format ABI, host parameter/state bridge and native editor integration                               |
| Producer scripts                                                  | Version metadata, staging, universal composition, validation, signing and archives                  |
| Heron repository                                                  | Its audio runtime, plug-in host/probes, Metronome scheduler, consumer and project integration tests |

Heron Metronome is a MIDI-triggered click instrument. Accent note 84 and beat
note 72 are part of its input contract. It does not claim a device or run a
second tempo/count-in scheduler; the DAW supplies timed events. Heron retains
its own scheduling policy.

The release formats are VST3 for all four supported platform targets and AUv2
for universal macOS. AAX adapters are optional and release jobs remain disabled
until the licensed SDK, signing and host-validation gates are satisfied. See
[CI operations](ci.md) and [ADR-0001](adr/0001-independent-plugin-releases.md).

## Dependency direction

Editor composition reads an instance's parameter/telemetry contract and sends
typed edit intent through its host bridge. Shared visual primitives own drawing
and reusable interaction behavior, not transport, project persistence or another
instance's state. DSP code does not depend on editor widgets or format-specific
host implementation types.

Do not introduce source dependencies on Heron's host, device/runtime or database
crates. Do not add a helper process, application IPC layer or host restart policy
to compensate for in-process plug-in ownership. The release artifact contract
is Heron's dependency boundary. See
[the artifact contract](artifacts.md).

## Parameter and saved-state contract

Native identities and state interpretation are persisted compatibility
commitments. Preserve VST3 class IDs, AU manufacturer/subtype codes, bundle IDs,
parameter IDs, defaults, units, ranges and normalized-value mappings. Never
reuse a retired parameter ID for a new meaning. A suite version bump alone
does not authorize changing historical state or automation semantics.

State decoding validates external data before publishing a replacement. Define
the commit point and recoverable failure behavior for compound state, preset,
band-paste and matching operations; failed preparation must not partially replace
the current configuration. Host recall is authoritative over stale editor drafts
and asynchronous results. Use explicit revision/generation checks where results
can outlive the state they were prepared for.

Opaque format state and the EQ portable JSON envelope are different contracts.
The latter describes fitted band interchange and is not an interchangeable VST3
or AU state chunk. Retain its validation and stable-band identity policy. See
[Heron EQ](heron-eq.md) for the current numerical and state boundaries.

The migrated non-default fixtures are historical evidence: Gain, Sine and
Metronome originate from Heron 0.6.4; EQ state, parameter and audio evidence
originates from Heron 0.6.5. Do not regenerate them with current code to make a
regression pass. Producer tests prove plug-in behavior and compatibility; Heron
tests prove project reopening, host integration and artifact consumption.

## Host automation and native editor lifetime

Continuous user edits send a balanced host `begin_edit` / value updates /
`end_edit` sequence for each affected parameter. The host owns recording and
Undo/Redo grouping. Do not turn each preview into a separate user commitment,
invent a second project history, or confuse a host automation update with a new
user gesture. Cancellation, state recall, focus loss and editor closure must
have explicit edit-balancing and value-reconciliation behavior.

Control and editor calls obey the format and platform's thread-affinity
requirements. A callback bridge, native child window and editor worker belong
to an explicit instance/lifetime. Revoke stale permissions before asynchronous
teardown can publish another edit; close or replace an editor without retaining
callbacks into destroyed host resources. Opening multiple instances must not
make a process-global registry into an ambient "current plug-in" selector.

EQ currently implements this with `HeronEqEditor`, an exact-bridge session match,
revocable callbacks and generation-owned solo audition. Its close path balances
edits and clears only that session's audition ownership. This is a concrete EQ
implementation, not proof that every editor or every external DAW has identical
reentrant destruction behavior. Changes to bridge synchronization require
explicit lock-order and host-reentry review; control-thread locks are never
permission to acquire the same locks in audio processing.

Native attachment, resize and scale negotiation must share one coordinate
contract. The plug-in converts host/client geometry to logical Iced layout and
physical rendering consistently, including hit testing after accepted resizes.
On Windows, the loading host owns process DPI awareness and the thread DPI
context supplied to embedded editors. Window initialization must not override
either policy; standalone executables initialize their own DPI policy before
creating windows. See [ADR-0004](adr/0004-host-owned-windows-dpi.md).
Do not preserve an obsolete old-size/new-size pointer ratio after drawing the
new layout. Keep pinned toolkit patches and their provenance together; remove
them only after equivalent upstream behavior passes the relevant native checks.

## Real-time boundary

- Keep callback work bounded, allocation-free and free of deallocation that can
  invoke an allocator or destroy a heap-owning processor.
- Keep filesystem/network access, UI work, font loading, clipboard access,
  serialization, logging and blocking synchronization off audio callbacks.
- Use fixed-capacity state, atomic snapshots or bounded non-blocking handoffs
  where processing meets parameters, events, telemetry and preparation.
- Allocate voices, histories and processing capacity at an appropriate
  non-real-time preparation boundary. External event/block counts do not
  authorize unbounded processing, growth or string lookup in the callback.
- Define overflow, dropped/coalesced requests and stale-generation outcomes.
  Diagnostics use bounded counters/state and are read outside processing.
- Never hold a callback-visible lock across host, platform, editor or filesystem
  calls. Changing thread ownership or real-time assumptions requires an ADR.

The EQ audio path captures bounded samples/metadata; FFT and display work occur
outside processing. New processors are prepared off the callback and handed
over with bounded warm-up, transition and retirement ownership. The old owner
must be reclaimed off the callback. Reported latency follows the accepted
processor, not a newer unaccepted editor preview.

The editor's Linear Phase response preview has a separate worker with latest-only
request/completion slots and generation checks. It prepares the exact finite FIR
outside UI drawing and audio processing. Only the current completion may be
displayed. Worker creation failure, pending work and shutdown have explicit
states; worker joining/destruction must never migrate to the audio callback.

## Display evidence and instance independence

Editable target, accepted audio configuration, measured spectra, processing
status and reported latency are separate facts. Immediate response previews may
describe current edit intent while audio still uses an accepted older processor.
Pending/failed preparation must not display a stale result as the current target.
Display smoothing, axis ranges and channel projection do not alter the raw
measurements used by matching or the processing configuration.

Every meter, spectrum, draft, selection, audition and asynchronous result belongs
to its instance. An editor must not borrow the visible state of a different
instance because it shares a native ID or library. Closing the editor does not
destroy persistent plug-in parameters or stop audio owned by the host.

## Failure and diagnostics

Production Rust returns typed `Result` values for recoverable input, state,
filesystem and preparation failures. Do not use `panic!`, `unwrap()` or
unexplained `expect()` for external failures. Panic must not unwind through a
plug-in ABI, platform callback or audio callback. Review upstream wrapper
behavior when changing a format boundary; producer unit tests alone do not
establish fault containment inside arbitrary hosts.

The host owns process lifetime and fatal-error handling. Plug-ins must not
install conflicting signal, panic, logging or crash handlers. Plug-ins do not
integrate with Sentry or publish symbol archives. Heron's application diagnostics
remain owned by Heron. See [ADR 0005](adr/0005-runtime-only-releases.md).

Do not silently change a creative setting, bypass a high-load processor or alter
host transport in response to load. Ordinary failures retain an explicit last
coherent or unavailable state and provide bounded evidence for control/UI
feedback. A native crash can affect the loading DAW; no helper-recovery promise
is part of this suite.

## Heron Revox

`crates/revox-dsp` owns an experimental 48 kHz mono FCPE-to-Revox analysis and
resynthesis pipeline. `plugins/heron-revox` adapts it to mono/stereo host audio
and a two-control native editor. Revox is an independent development bundle; the
four existing suite release identities remain unchanged. A library-lifetime ORT
environment is shared across instances; every instance owns its model sessions.
This strong environment owner supports reopening after all instances close. Loading,
feature extraction, inference, synthesis and destruction run outside the audio
callback; these operations allocate and may block. No Python runtime or helper
process is used by the Rust DSP. Python is used only to export the upstream FCPE
checkpoint and produce independent feature references during development.

The pinned FCPE export maps time-major 128-band Mel features to 360 pitch-bin
probabilities. Input is antialiased from 48 kHz to 16 kHz with a centered FIR;
FCPE's physical 5 ms-offset frame centers are aligned to Revox's 0 ms-origin
10 ms grid. Only adjoining reliable voiced frames interpolate in log frequency.
Low-confidence nonsilent frames have `pitch_valid=false`; measured silence
(20 ms RMS below -60 dBFS) is reliable unvoiced. This is an explicit adapter
policy, not an additional confidence output supplied by FCPE.

Revox 1.5 receives unchanged magnitude-Mel features and pitch scaled by
`2^(semitones/12)` over the -13 to +13 semitone combined range. Zero semitones
still synthesizes a new waveform. Seeded unit-variance noise is supplied directly;
the model applies its own noise gain. Centered iSTFT preserves the exact input
sample count without normalization or clipping. Model errors are typed and
propagated to the caller. The plug-in adapter exposes failure and uses delayed
dry audio as described below.

ORT enables every EP in the largest compatible prebuilt set for each supported
target. The default priority is CoreML, WebGPU, CPU on Apple Silicon; TensorRT
RTX, TensorRT, CUDA, DirectML, CPU on Windows x64; TensorRT RTX, TensorRT, CUDA,
CPU on Linux x64/arm64; and DirectML, CPU on Windows arm64. All successfully
registered EPs stay on the same session in order, allowing operator-level
fallback. A registration failure is recorded and does not prevent later EPs
from registering. CPU is the final fallback; there is no backend mode selector.

If the combined graph cannot initialize, preparation discards that session and
tries a fresh CPU session. Its diagnostic names the entire attempted chain,
because ORT does not reliably identify the responsible provider. Backend reports
record the ordered providers registered on the accepted session plus failed
attempts; they do not prove that every operator used a GPU. Inference errors
after loading propagate to the caller. No global logging subscriber is installed.

"Ready to use" in the [ORT EP guide](https://ort.pyke.io/perf/execution-providers)
means an individual EP has a prebuilt distribution. The pinned rc.14
[distribution table](https://github.com/pykeio/ort/blob/1ee1f842e8a381153ede7a506f01734e545854e0/ort-sys/build/download/dist.tsv)
does not provide arbitrary unions. Windows/Linux WebGPU, OpenVINO, and Linux x64
MIGraphX come in separate archives and cannot be added to the default NVIDIA
archive. We do not use `lax-feature-matching` to disguise missing native EPs or
compile a custom ORT. Intel macOS has no rc.14 prebuilt distribution.

Both models are noncausal. FCPE's symmetric convolutions require up to 92 frames
of context per side, and its GroupNorm also depends on the whole input window.
Revox resets excitation phase between calls. The plug-in worker uses fixed
2.12-second windows (960 ms left context, 160 ms output step, 40 ms overlap and
960 ms right context), crops the center, and uses a raised-cosine overlap to
join successive outputs. This reduces hard seams but does not restore the model's
missing continuous excitation state or prove perceptually transparent vocals.
Host rates 44.1/48/88.2/96/176.4/192 kHz are converted to/from the model's 48 kHz
by a centered polyphase sinc on the worker. Stereo channels are synthesized
separately; mono stays mono. No channels are summed.

The host receives a constant latency of 1.48 seconds plus 128 host samples,
including a 320 ms inference allowance. This first version targets track
processing, not low-latency live monitoring. The callback sends/receives fixed
128-frame stereo packets through two preallocated 2048-packet queues. Packets
carry absolute input sample positions; stale output is discarded, future output
is held until its matching delayed position, and missing input starts a new
context region. Pitch settings are captured with input packets and selected at
the output window's starting timestamp, quantized to the 160 ms processing step.

Real-time and buffered callbacks never wait, allocate, load a model or run ORT.
Loading, missed deadlines and model failure produce latency-aligned dry audio;
a 5 ms transition smooths wet/dry recovery. The editor distinguishes loading,
processing, overload, failure and unsupported sample rate. A worker failure stays
failed until host reactivation/reset. Unsupported rates have zero latency and
pass input through. Host-declared offline rendering waits for the same worker's
matching output, preserving output under faster-than-realtime bounce; known
missing intervals use delayed dry without waiting for unsubmitted future audio.
Reset/destruction stop and join the previous worker off the realtime callback.

Truce 6.3 prepares DSP at VST3 activation, so the initial latency query must follow
`setActive(true)`. Heron's VST3 host already uses that order; hosts that only query
before activation are not covered by this development validation. The packaged
audio probe follows activation, checks 71168 samples at 48 kHz and measures the
combined pitch shift through the actual native binary.

The editor reuses Heron EQ's shared rotary drawing, chrome and palette. Stable
parameter IDs are 0 for integer Semitones (-12..12, default 0) and 1 for integer
Cents (-100..100, default 0). The combined shift is semitones + cents / 100;
zero still invokes the vocoder. Only these two values persist. Editor revision,
worker queues and processing status are instance-local transient data. Pointer,
keyboard and numeric edits balance host gestures, including focus loss, recall
and native editor closure. Native canvas accessibility nodes and non-color
invalid-input feedback are tracked in
[issue #11](https://github.com/minori-live/heron-plugins/issues/11), owned by
@dsh0416; render and gesture evidence do not establish screen-reader support.
See [development commands](environment.md#revox-dsp-experiment).

The exact `ort` 2.0.0-rc.14 pin is a deliberate prerelease dependency: the requested
modern ORT execution-provider API is only published on its 2.0 release-candidate
line. Keep the Rust wrapper and bundled native runtime resolution together in
`Cargo.lock`. FCPE source/weight checksums and Python export dependencies are
locked under `scripts/revox/`; Revox's immutable revision and model checksum are
in `scripts/fetch-revox-model.ts`. We use the official FCPE MIT checkpoint and
Revox's GPL-3.0-only license option. Downloaded license texts remain beside the
development model artifacts. The Revox build verifies both model hashes and
embeds their bytes. Its independent bundle contains the model provenance/licenses
and ORT/Dawn notices; its Dawn library is copied into the bundle and linked by
loader-relative path. No user-selected model path or Python runtime is needed.
The macOS VST3 package uses an owned selective archive link with explicit VST3
entry points: Truce's all-load link pulls duplicate ORT/Rust compiler-runtime
symbols. The final unpacked bundle must pass both identity and model-audio probes.

## Decision governance and evidence

Changes to threading, state persistence, ABI/dependency foundations, real-time
assumptions, compatibility commitments or material parameter interaction
semantics require an ADR under [ADR governance](adr/README.md). Accepted
decisions update this current-state document; proposed work is not current
architecture.

Use [engineering standards](engineering-standards.md) and
[testing policy](testing.md) for implementation and evidence. Platform CI
validates final unpacked bundles; native event tests cover owned editor lifetimes.
