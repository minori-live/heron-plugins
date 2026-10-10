# Testing policy

Status: Accepted\
Owner: project maintainers\
Last reviewed: 2026-10-10

Adapted on 2026-10-10 from Heron's
[`agents/docs/testing.md`](https://github.com/minori-live/heron/blob/6686b9bdf17cabf053d32a669a4726effa26c9cb/agents/docs/testing.md),
reviewed 2026-09-06, at `6686b9bdf17cabf053d32a669a4726effa26c9cb`.
That policy was adapted from TypeApe's `agents/docs/testing.md`, reviewed
2026-08-24, at `a214188b22c3062b8fdb52a24f8297f6c4039b20`. This repository adopts
the same ownership, narrowest-boundary, fixture, coverage and review rules for
its Rust plug-ins, DSP, Iced UI and TypeScript release tooling. Application-only
Vue, Pinia, Electron, project-database and IPC examples do not transfer ownership
of Heron's host or runtime into this repository.

Tests are executable evidence for behavior Heron Plugins owns. They are not a
second conformance suite for dependencies, UI frameworks, runtimes, plug-in SDKs,
operating systems or remote services. This policy refines the quality floor in
[Engineering standards](engineering-standards.md).

## Ownership rule

A test MUST identify a plug-in-suite requirement or regression risk beyond an
upstream dependency working as documented. Owned behavior includes:

- DSP calculations, invariants, parameter transitions and saved-state semantics;
- per-instance isolation, editor/worker lifetime and thread-affinity boundaries;
- MIDI timing, parameter normalization, ABI mapping and external failure policy;
- retry, cancellation, fallback, compatibility, recovery and commit semantics;
- public editor controls, visible states and normalized parameter-edit intents;
- plug-in audio output, processor preparation and real-time resource bounds;
- build/release artifacts and upgrade-PR effects this repository controls.

Tests MUST NOT freeze upstream internals, SVG paths, markup, error wording,
framework reactivity or widget plumbing, operating-system defaults, or SDK/parser
conformance. An integration contract is ours on our side: prove our mapping,
normalized result or failure policy, not an unrelated dependency's implementation.

Heron's audio runtime, plug-in host, transport/count-in scheduling, application
project persistence and artifact consumer remain Heron-owned. Producer tests
MUST NOT duplicate those suites or claim that a passing plug-in test establishes
their integration. Reusing Heron's artifact consumer from release automation
does not move its implementation or consumer-contract tests here.

## Decision procedure

Before adding or retaining a test, answer:

1. Which exact plug-in-suite requirement or regression does it prove?
2. What is the narrowest boundary at which that behavior is observable?
3. Would a conforming dependency change in internals, widget composition, assets,
   wording or equivalent output break it? If so, narrow or remove it.
4. Is that requirement proved elsewhere? Retain a broader test only for a
   distinct owned boundary risk.
5. Do assertions inspect public output or policy, rather than only execution or
   a mock being called?
6. If coverage is the motivation, is this a minimal public-contract smoke for
   an otherwise-unexercised thin path, under the limits below?

A test with no owned answer to question 1 MUST be removed. Deleting upstream or
duplicate evidence does not inherently require a replacement. Test names and
review evidence should make the owned behavior clear without mandatory
annotations or a separate test registry.

## Boundary guidance

### DSP, parameters and adapters

Use deterministic inputs and outputs for owned DSP calculations, portable-state
decoders, parameter validation and processor/editor state machines.
Boundary/invalid cases need distinct risks, not combinatorial enumeration for
its own sake. Assert measured audio, sample offsets, accepted configuration,
parameter values or declared latency at the narrowest useful boundary.
Numerical tolerances MUST follow the owned signal/compatibility requirement;
do not widen them to make a regression pass.

Adapters normally use controlled fakes at the dependency boundary and assert
actual request contents, normalization or policy. Do not instantiate an unrelated
parser, client or SDK merely to test its documented behavior. Dependency upgrades
get regressions for our broken usage. Host edit-balance tests can use a controlled
host bridge when the risk is the plug-in's begin/write/end ordering; an actual
native attachment is justified only for a separate ABI or window-lifetime risk.

### Shared UI and native editors

Use public control inputs, user actions, normalized parameter-edit messages and
plug-in instance state. Do not test Iced widget forwarding, framework state
retention, native keyboard/focus defaults or upstream accessibility behavior
unless the plug-in or a repository-owned toolkit patch adds a distinct rule.
Plug-in-specific numeric gestures, cancellation, disabled/busy policy, balanced
host edits, solo cleanup and focus-loss recovery remain valuable.

Exact classes, widget nesting, snapshots, icon markup and SVG data are not
contracts by default. Only accepted design, accessibility, security or release
requirements justify exact values. Assert semantic state or layout behavior
rather than the particular widgets implementing it. Shared UI tests own reusable
gesture evidence; individual editor tests own parameter mapping, composition
and instance isolation. A broad editor test MUST NOT repeat every gesture already
tested in the shared component.

Native render evidence establishes the displayed state and accepted layout.
Live-window event tests establish owned mouse routing, resize and focus behavior.
Neither a screenshot nor a synthetic event alone establishes hardware audio or
correct operation in every DAW. Keep the manual host acceptance boundary in
[CI and release operations](ci.md).

### Integration and release artifacts

Use actual native modules, process/window integration or final published
artifacts for risks absent at a narrower boundary: fixed VST3/AU identity,
exported buses, saved-state ABI, final architectures, matching debug IDs, archive
links/modes, signing, or an owned editor attachment. Assert our bytes, restored
parameters, audio output, final bundle contents or visible result. Plain-object
`JSON.parse(JSON.stringify(fixture))` equality proves JSON, not our encoder.

Historical state and parameter fixtures remain justified: they exercise our
compatibility with the old implementation, including non-default values and
audio. Normative portable-preset fixtures protect our schema and numerical
interoperability and are distinct from a library round-trip test. Final bundle
probes and AU validation test our registration, packaging and simultaneous
loading; they do not establish general VST3/AU SDK conformance.

Keep allocation, bounded preparation/publication/retirement, concurrency and
signal-correctness tests at the plug-in processor or owned worker boundary.
Heron owns host graph publication, runtime scheduling and application recovery.
Keep native GPU, platform hardware and DAW/soak evidence where CI cannot reproduce
the environment. Neither setup inconvenience nor upstream involvement makes a
data-integrity, compatibility or real-time regression disposable. AAX remains
disabled until its licensed SDK, signing and official host validation gates are
met; optional adapter checks MUST NOT claim licensed release acceptance.

The upgrade coordinator owns the resulting Heron branch/PR state and retry
policy, not Heron's lock parser or artifact-consumer contract. For branch
freshness, leases and idempotence, prefer a temporary real Git repository and
bare remote; fake only the GitHub API and consumer boundary when appropriate.
Assert final commit/tree, lock bytes and converged PR state, rather than merely
freezing a command list or counting mocks.

### Static content and configuration

Static checks may enforce normative security, ownership, release metadata,
localization completeness or generated-artifact consistency. Prefer existing
lint, type, schema and build checks when they express the invariant directly.
Do not freeze file inventories, historical symbol absence, prose, line wrapping
or particular registration syntax instead of testing the live contract.

## Fixtures, mocks and coverage

Mocks and fixtures are arrangements, not evidence. Call assertions MUST identify
an owned request, effect, ordering guarantee or failure policy. Keep fixtures
minimal and explain normative, regression or legacy-comparison intent where it
matters. Behavior-defining values remain explicit; generated/randomized failure
cases MUST be reproducible and report their seed or equivalent reproduction input.

Legacy fixtures MUST preserve their recorded source commit, version, generation
procedure and expected parameter/audio values. A suite version after migration
does not rewrite their original Heron provenance. Do not regenerate historical
fixtures from current code, replace non-default evidence with defaults, or change
expected values to make a compatibility failure pass. An intentional identity or
state-format change requires a documented compatibility decision and regression
evidence; adding a new fixture does not erase the old requirement.

Coverage is a discovery tool, not evidence of correctness. A metric alone never
justifies a test without a meaningful assertion. One representative smoke is
allowed for an otherwise-unexercised thin path. The maintainer's 2026-09-06
clarification also applies to supported public presentation variants and optional
composition entry points when removing duplicated tests would otherwise lose
their coverage. Check the public control's accessible name or visible label,
supplied value or composed edit intent; do not assert SVG paths, internal wrappers
or framework implementation details. Keep full gesture, failure and lifecycle
scenarios at the shared behavior boundary, run once rather than repeated for
every appearance. Do not enumerate arbitrary control/parameter combinations or
add unrelated examples to offset a coverage loss.

The maintainer's coverage-preservation requirement remains in force during
migration and cleanup: coverage MUST NOT decrease. Compare the same crate or
script suite, metric, instrumentation and exclusions before and after; account
for moved source paths without changing the population being compared. Do not
lower thresholds, exclude code, or pad the suite to satisfy this constraint.
First reduce redundant cases to the limited contract smoke above, or add evidence
for a real missing owned behavior. If a conflict remains, report the measured
change and resolve that constraint explicitly.

## Representative review decisions

| Candidate                                                               | Decision | Reason                                                              |
| ----------------------------------------------------------------------- | -------- | ------------------------------------------------------------------- |
| JSON round-trip of a hand-written literal                               | Remove   | No owned serializer, normalizer or compatibility boundary executes. |
| Iced forwards a widget message or an SDK parses its own parameter field | Remove   | Dependency behavior; test our adapter mapping if needed.            |
| Processor replacement retains its active owner until bounded retirement | Keep     | We own lifetime and real-time safety.                               |
| Legacy state bytes restore fixed parameter IDs and measured audio       | Keep     | Saved-state and sound compatibility are ours.                       |
| Rejected band paste leaves parameters and selection unchanged           | Keep     | Atomicity of our edit operation is the requirement.                 |
| A fixed list of source files or fixture names                           | Remove   | Incidental inventory, not a behavioral contract.                    |
| Final module UUIDs match every archived architecture's debug symbols    | Keep     | The producer owns final release bytes and symbol completeness.      |
| Exact upstream icon paths or widget nesting                             | Remove   | Asset/composition internals are not our requirement.                |
| Editor closure balances its gestures and clears only its instance solo  | Keep     | We own the editor's cleanup and instance-isolation policy.          |
| Repeated upgrade retries converge to one PR without replacing edits     | Keep     | The producer owns retry, lease and external-effect reconciliation.  |

## Review and maintenance

Review evidence MUST connect tests to requirements/risks and explain intentional
deletions. Remove upstream conformance checks, duplicated narrower behavior,
incidental static assertions, mock-only evidence, unjustified input permutations
and coverage padding. Replace them only when an actual owned behavior lacks
evidence. Normal repository review is sufficient; do not create another approval
or annotation framework.
