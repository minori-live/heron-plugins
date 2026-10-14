# Engineering standards

These standards adapt Heron's engineering policy to the independently released
Rust plug-in suite, its Iced editors, Truce adapters and TypeScript producer
tooling. They apply to hand-authored production code, tests, build tooling and
architecture changes. `MUST`, `MUST NOT`, `SHOULD` and `MAY` are normative.

Adopted on 2026-10-10 from Heron's
[engineering standards](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/engineering-standards.md)
at source commit `fef5cdb165bb322e07f2a280fea8372aa5e22393`.
The [documentation index](README.md) defines local authority and adoption.

Prefer explicit ownership and checkable constraints over abstractions whose
safe use depends on oral history. Mechanical rules should become repository
checks; judgment belongs in normal review. The current checks are listed below.
A normative rule does not imply that a mechanical gate already exists.

## Change discipline

- A change MUST have one stated user, product, maintenance or reliability
  outcome. Incidental cleanup must remain reviewable.
- New behavior MUST define applicable normal, unavailable, busy, failed and
  recovery states, including what happens when a host rejects an operation.
- A bug fix MUST first add an automated reproduction that fails for the reported
  behavior. When automation is impossible, the change description MUST explain
  why and give a repeatable manual check with the required host and platform.
- Implementation, tests, agent-facing documentation and installation or usage
  instructions MUST change together when their shared behavior changes.
- `TODO` and `FIXME` comments MUST link to a tracked issue. Deferred correctness
  work and exceptions need an owner and a durable tracking reference.
- Preserve upstream licenses, provenance and patch notes when changing vendored
  code. Do not edit installed skill copies under `.agents/skills/`.

## Ownership and dependency direction

Keep this workspace independent of Heron's application, audio engine, plug-in
host, project persistence and release tooling. Heron consumes released VST3
artifacts; a source dependency on Heron would break that boundary. Split modules
by a named feature or runtime owner, rather than generic `types`, `helpers` or
`utils` buckets.

### DSP and plug-in behavior

- `crates/plugin-dsp` owns reusable DSP and serializable EQ configuration. It
  MUST remain usable without a DAW, Truce format wrapper or editor window.
- Each `plugins/heron-*` implementation owns its processing, parameters, MIDI
  mapping, saved state and editor composition. Keep lifecycle and composition
  entry points thin; substantial behavior belongs with its owning feature.
- Parameter IDs, units, ranges, defaults, normalization, automation flags and
  saved-state meaning are compatibility contracts. Vendor/bundle IDs, VST3
  class IDs and AU registration identities MUST remain stable unless an
  explicit compatibility decision and regression evidence authorize a change.
- Keep `truce.toml`, actual format exports and `scripts/catalog.ts` consistent.
  A manifest alone does not prove that a native binary exports its claimed ID.
- Heron Metronome owns MIDI-triggered click behavior. Heron's tempo, transport
  and count-in scheduler remain owned by Heron.

### Shared editor layers

- `crates/iced-ui` owns reusable visual tokens and presentation primitives. It
  MUST NOT own DSP, a DAW session or plug-in lifecycle policy.
- `plugins/heron-plugin-ui` owns shared plug-in controls and their parameter
  gesture behavior. Per-plug-in editors compose these controls with their own
  parameters and observable state.
- Editors MUST respect the host's window, content-scale and lifecycle contracts.
  UI work, clipboard operations, file dialogs, response calculations and FFT
  presentation stay outside the audio callback.
- Host automation gestures require balanced begin/end handling, including
  cancellation, focus loss and editor teardown. Closing an editor MUST NOT
  leave an active gesture or a worker referring to an unloaded instance.
- `third_party/baseview-truce`, `third_party/truce-gui` and
  `third_party/truce-iced` are pinned upstream snapshots with owned patches.
  Record each patch's purpose, source/license provenance, regression evidence
  and removal condition in its `PATCHES.md`; upgrade coupled patches together.

### Producer tooling and native adapters

- `scripts/` owns build orchestration, identity manifests, archive safety,
  signing and release reconciliation. The native
  `fixtures/` probes validate the final binary boundary.
- Keep format-specific packaging and platform installation behavior in their
  owning adapters. An entry point should compose those adapters rather than
  accumulate their implementations.
- Preserve complete bundles, licenses and resources. Validate unpacked runtime
  archives against the modules actually shipped.
- Heron owns its artifact-consumer and host integration contract. The producer
  upgrade coordinator may execute that consumer from trusted Heron main, but
  MUST NOT execute code from a pending upgrade branch.
- Plug-ins share the host DAW process. Do not take ownership of its global
  logging, panic, signal or crash handlers. Plug-ins do not integrate with Sentry.
  Future reporting needs a separate architecture decision.

## Real-time and thread boundaries

- The audio callback MUST NOT allocate or free heap resources, log,
  access the filesystem/network, perform UI work or acquire a lock that a
  control, host, editor, worker or filesystem operation can hold.
- Gain, EQ, Sine and Metronome callbacks MUST NOT wait, and their work per audio
  block MUST be bounded.
- Revox MUST retain audio and wait for worker queue capacity and matching
  processed audio in every host process mode, as specified in
  [architecture](architecture.md#heron-revox). Model loading, inference and
  allocation remain on its worker; the DAW owns deadline handling.
- All control/publication queues MUST have bounded memory.
  Define capacities and the policy for overflow, stale generations and dropped
  work; expose a bounded state or observation rather than silently losing an
  operation whose outcome matters.
- Prepare expensive DSP, buffers and analysis on control/worker threads. A
  callback may accept a bounded prepared result; reclamation and recovery stay
  off the callback. Cancellation must prevent obsolete work from replacing a
  newer accepted configuration.
- Platform UI and host callbacks MUST obey their thread affinity. Instance
  teardown must stop or reconcile owned workers and callbacks before their
  resources disappear. Blocking joins belong on the appropriate control thread.
- Allocation invariants need explicit evidence for the changed owned callback
  path. Passing ordinary unit tests does not establish real-time safety.

## Stateful boundary work

For fallible native lifecycle, state restoration, prepared DSP publication,
staging, signing or repository-update workflows, document and test the
applicable parts of:

1. states and legal transitions;
2. instance/resource identity and revision or generation rules;
3. the single accepted or durable commit point;
4. prepare/abort cleanup;
5. data and resource ownership after every failure;
6. idempotency or an operation-status reconciliation path; and
7. a recoverable or explicitly quarantined failure state.

A timeout with an unknown mutation outcome MUST NOT be treated as proof that
nothing committed. Exceptions, Rust panics, ambient current-instance state and
free-form strings MUST NOT become native protocol semantics. Use the existing
typed host/Truce result contract and structured context; internal causes may be
retained outside real-time work. Do not introduce a new process or IPC layer
merely to implement these rules.

## Error handling and unsafe code

### TypeScript

An internal build script may throw when one caller owns the whole operation
and its commit outcome remains known. Catch to recover, preserve a meaningful
cause, translate at an ownership boundary or perform cleanup. Do not catch just
to print a warning and continue past a required build, signature or validation
failure. Reconcile uncertain remote writes before claiming success or retrying.

Prefer hand-authored `.ts` scripts and configuration. Run erasable TypeScript
directly with the locked Node.js runtime and check it with the root
`tsconfig.json`, which enables `erasableSyntaxOnly`, strict checking and
`noUncheckedIndexedAccess`. Avoid TypeScript syntax that requires runtime
transformation. Reserve `.mjs` and `.cjs` for generated output or consumers that
require those extensions.

### Rust and native callbacks

- Recoverable host, input, state, filesystem and lifecycle failures return typed
  `Result` values or the existing native adapter's stable result code.
- Production code MUST NOT use `panic!`, `unwrap()` or `expect()` to handle
  runtime input or external failure. An invariant `expect()` MUST explain the
  proof next to the call. Tests may use panic-based assertions and setup.
- A panic MUST NOT unwind through a plug-in ABI, platform callback or audio
  callback. Callback failure becomes a bounded state, counter or non-blocking
  control signal; recovery never runs in the callback.
- Keep `unsafe` blocks small and state their safety preconditions, ownership,
  lifetime, aliasing and thread-affinity assumptions at the practical boundary.
  An `unsafe` function still needs explicit blocks for unsafe operations.
- Raw host pointers, native window handles and asynchronous worker references
  MUST have an explicit validity period and teardown owner.

## Source size and cohesion

Physical line count is a review signal, not a substitute for design judgment.

- A new hand-authored production file MUST NOT enter above 800 lines.
- Material changes to an existing production file above 800 lines require
  ownership and cohesion review.
- A hand-authored production file above 1200 lines MUST be split. An exception
  requires a linked issue and MUST NOT permit further growth.
- Tests, generated sources, ABI declarations and mechanically derived FFI MAY
  exceed the thresholds when splitting would reduce auditability. Identify the
  exception in review; size alone does not establish one. Review upstream
  snapshots by provenance and keep owned patches focused and documented.
- Extract named behavior, features, protocols or runtime ownership. Generic
  file buckets or arbitrary line-count cuts do not satisfy the rule.
- Binary entry points, package barrels and editor/build composition roots
  SHOULD be substantially smaller than the general threshold.

These thresholds are normative review requirements. This repository currently
has no source-size inventory command or automatic 800/1200-line gate. Do not
claim that `pnpm lint` or CI enforces them.

## Dependencies and reproducibility

Use stable published runtime and dependency releases. Prerelease, nightly,
canary or unpublished versions require an explicit need and a documented
tradeoff. Existing pinned vendor patches retain their recorded provenance.

Keep Truce, its format wrappers and matching GUI integrations compatible and
pinned together. Commit relevant `Cargo.lock`, `pnpm-lock.yaml` and `mise.lock`
changes with their declarations. Refresh `mise.lock` through `mise lock`, not
hand editing. See [Development environment](environment.md) for actual version
and installation commands. Suite versions are independent of Heron's app
version; synchronize them through this repository's version tooling.

## Testing by ownership and risk

[Testing policy](testing.md) chooses the narrowest useful owned boundary.
Tests MUST prove this suite's DSP, MIDI timing, parameter/state compatibility,
resource bounds, editor behavior, artifact shape or failure/recovery policy.
Do not build a second conformance suite for Iced, Truce, SDKs, operating systems
or GitHub. Coverage alone does not establish correctness.

| Owned change                                                     | Primary evidence                                                                               |
| ---------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| DSP calculation, parameter mapping or state transition           | Focused deterministic tests with relevant boundary and invalid cases                           |
| Serialized state, preset interchange, archive or release adapter | Real owned bytes/storage or contract tests; historical fixtures where compatibility matters    |
| Editor gesture, cancellation, focus or host automation           | Public control/state tests; native host evidence for a distinct platform boundary              |
| Audio callback, queue or DSP publication                         | Allocation invariant and concurrency/overflow tests; benchmark or soak appropriate to the risk |
| Native identity, wrapper, signing or installation                | Final-archive probes and platform CI                                                           |

Tests assert observable output and stable contracts, not private call order,
incidental widget trees or source inventories. Retain broader tests only for a
distinct boundary risk. Do not regenerate historical fixtures from the current
implementation to make a compatibility regression pass.

## Editor completion

New editor behavior MUST use shared tokens/controls or document why a dedicated
control is needed; define its applicable default, hover, focus, pressed,
selected, disabled, busy, invalid, empty and error states. Support applicable
keyboard actions and focus restoration. Verify supported themes, host content
scale and declared window sizes; errors need specific recovery behavior.

Preserve the existing host parameter gestures and cancellation semantics.
Changes to material interaction semantics need a documented decision.

## Decisions, documentation and exceptions

Maintain current design and behavior in the existing owning documents and
explain implementation choices and validation in the PR. Follow
[ADR governance](adr/README.md) for maintainer-directed creation, acceptance,
and amendments; a changed technical domain alone does not require a record.
A policy exception MUST link to an issue. An issue link is sufficient at the
current project scale; do not invent a parallel waiver bureaucracy.

Exceptions are narrow: they identify the file, rule, and reason. They MUST NOT
silently authorize similar code elsewhere or allow the affected scope to grow.

Keep repository-wide instructions in `AGENTS.md` and architecture, development,
testing, release and agent documentation under `agents/docs/`. README and
installation instructions describe the shipped suite. This repository has no
public website. Do not store generated output, temporary notes, benchmark
artifacts or secrets in documentation directories.

## Current mechanical checks and review

`mise run check` currently runs version consistency, Oxfmt, Oxlint, strict
TypeScript/script tests, Rust formatting, cargo-shear, all-target Clippy,
workspace tests and each plug-in's separate `rt-paranoid` tests. CI additionally
checks optional Rust adapters and packages/validates final native artifacts.
See [environment](environment.md) and [CI operations](ci.md) for their scope.

Thread ownership, unsafe preconditions, compatibility decisions, source-size
exceptions and TODO tracking remain normative review requirements.

Review each change for clear ownership, stable native/state contracts, one
commit point, known cleanup/failure state, bounded real-time work, correct
thread affinity, meaningful tests, cohesive source-size decisions, accurate
documentation and tracked deferred work.
