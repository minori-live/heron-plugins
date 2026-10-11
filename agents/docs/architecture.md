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
install conflicting signal, panic, logging or crash handlers. This release has
no plug-in-side Sentry configuration, upload or runtime reporting. Offline debug
and source archive generation does not initialize a reporting client; Heron
owns uploading its pinned VST3 symbols to its existing project.

Do not silently change a creative setting, bypass a high-load processor or alter
host transport in response to load. Ordinary failures retain an explicit last
coherent or unavailable state and provide bounded evidence for control/UI
feedback. A native crash can affect the loading DAW; no helper-recovery promise
is part of this suite.

## Decision governance and evidence

Changes to threading, state persistence, ABI/dependency foundations, real-time
assumptions, compatibility commitments or material parameter interaction
semantics require an ADR under [ADR governance](adr/README.md). Accepted
decisions update this current-state document; proposed work is not current
architecture.

Use [engineering standards](engineering-standards.md) and
[testing policy](testing.md) for implementation and evidence. Platform CI
validates final unpacked bundles; native event tests cover owned editor lifetimes.
