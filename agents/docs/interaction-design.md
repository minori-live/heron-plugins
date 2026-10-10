# Plug-in interaction design

This specification adapts
[Heron's product interaction design](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/interaction-design.md)
at source commit `fef5cdb165bb322e07f2a280fea8372aa5e22393` for native plug-in
editors in external DAWs. [AGENTS.md](../../AGENTS.md) and the
[documentation index](README.md) define repository authority.
[The design system](design-system.md) governs visual states, tokens and
accessibility; [architecture](architecture.md) governs the host, parameter,
state and real-time boundaries. These are requirements, not a claim of complete
keyboard, accessibility or cross-host validation in the migrated implementation.

## Behavioral reference and discoverability

Logic Pro is the default reference for equivalent plug-in parameter gestures
and host/editor expectations. Preserve familiar DAW behavior without copying a
host's branding or visual skin. A lasting departure that changes parameter
interaction or state meaning needs a documented reason and, when material, an
ADR. Cross-platform input, accessibility, host ABI and real-time safety take
precedence when explicitly documented.

A musician familiar with another DAW must be able to discover the editor's
ordinary operations without source code or developer instructions. Labels,
units, readouts, tooltips and contextual recovery guidance may explain them.
Do not claim usability validation until an actual musician completes the
applicable adjustment, automation and recall journey in a supported host.

Keep repeatedly adjusted parameters on their owning surface. Current gain,
frequency, tone, decay, level and enabled/bypassed state must be readable without
opening a generic menu. Menus hold occasional commands or terminal choices;
non-modal panels hold contextual configuration. Neither substitutes for room
for a primary parameter.

## Parameter gesture contract

- A continuous pointer edit begins one host gesture, publishes bounded preview
  values and ends that gesture once. Changes to multiple parameters balance
  each affected parameter's host edits.
- Keep the numerical value and unit readable during adjustment. Mapping and
  sensitivity follow the parameter's complete useful range, including fractional
  dB steps; fine control must not require impractically long travel.
- Provide keyboard fine adjustment and a documented larger step, plus direct
  numeric entry where precision matters. A pointer-only custom canvas does not
  satisfy the keyboard requirement.
- Default reset and numeric-entry gestures are consistent for equivalent
  controls. The general reference is double-click reset; an established
  control-specific numeric-entry gesture must be named and retained rather than
  silently swapped during visual refactoring.
- Define Escape, focus-loss, closure and state-recall behavior. Cancellation
  reconciles the retained value and balances the host gesture; old pointer or
  asynchronous messages cannot resume an ended edit.
- User preview, host acceptance and completed gesture are distinguishable.
  Never present a stale draft as the committed value after host recall or
  rejected preparation.
- Wheel adjustment is an explicit control policy and must not steal the host's
  scrolling or unrelated editor navigation. Do not add global wheel mutation
  or silently change the existing hover/focus policy.

The host owns automation recording, Undo/Redo and complete plug-in settings.
An editor must not add a competing project history or assume that every DAW
records previews identically. Host-originated automation/recall updates the
editor without manufacturing user edit events. Automation, state recall and
editor closure are verified separately from rendered appearance.

The current shared `ParameterKnob` proves pointer begin/update/end and visible
value behavior. Its custom canvas does not currently establish keyboard control,
default reset, numeric entry or platform accessibility support. Those remain
requirements for improvements and must not be claimed as migration evidence.

## Native input, scaling and lifetime

Honor the DAW's parent window, platform thread affinity and accepted resize/scale
contract. Use logical editor coordinates for layout and parameter gestures;
convert physical/client input through the same scale used to draw the controls.
The new size must update both painting and hit testing, including after repeated
grow/shrink cycles, host zoom changes and replacement native child windows.

Retain focus and stable widget identity while a gesture is active. Native focus
loss must reach the interaction state machine. Closing or reopening an editor
balances outstanding host edits, revokes stale callbacks and clears only that
editor's transient audition ownership. A host's reentrant destruction policy
requires its own evidence; passing a queued-callback test is not universal DAW
lifecycle validation.

Do not interpret a successful screenshot as proof of native focus delivery,
mouse routing, IME operation, scaling or automation recording. Each has a
separate observable acceptance path.

## EQ editor contract

The graph is the primary surface. Follow [Heron EQ](heron-eq.md) for its existing
parameter and numerical acceptance, including these relationships:

- At the 1040 by 700 logical minimum, compact top/footer bars leave the graph
  dominant. Avoid a permanent inspector, output column or 24-button band bank.
- Selecting a band shows frequency, gain and Q in a compact panel at the lower
  center of the graph. Deselecting all bands removes it. Node movement must not
  relocate the panel or replace its widget identity during rotary edits.
- Shape, channel placement and applicable cut/band-pass roll-off stay with the
  selected band's controls. Additional actions and MIDI learning open on request.
- Processing, analyzer and output panels open from compact bars, stay inside
  the accepted window and close without changing the EQ.
- Node selection and curves remain legible at 24 bands. Selected, hovered,
  bypassed and solo-audition states are different; color alone cannot carry them.
- Numeric readouts and units remain visible. Current EQ rotary double-click
  opens numeric editing, Command/Control-click resets, and Shift provides fine
  adjustment. Preserve this established exception and disclose it in control
  guidance. The current rotary also supports hover wheel stepping; changing that
  policy needs explicit review under the wheel rule above.
- Copy/paste is validated band interchange, not a replacement for host project
  recall. Capacity or current-sample-rate failure leaves bands, selection and
  destination globals unchanged.

Frequency positions use a logarithmic domain: equal ratios occupy equal
distances. Frequency and Q rotaries use their domain mapping; EQ gain uses a
linear dB axis. Drawing, drag interpretation, snapping and readouts must use the
same conversions. Display ranges do not silently rewrite stored settings.
The right spectrum dBFS axis is separate from the left EQ-gain axis.

Measured Pre/Post belong to this instance's input/output; External and Match
use its routed sidechain. Editable static response, actual processor state,
latency and measured spectrum are distinct. Pending or failed FIR preparation
must not make a stale/ideal curve appear to describe the latest finite kernel.
Changing smoothing, tilt or channel projection does not change raw Match data.

Sidechain Match validates the whole candidate at the current sample rate before
publishing parameters or resetting selection. Failure leaves all bands, output
settings, selection and any existing host edit intact. Failure atomicity is
separate from Undo support.

Operation feedback uses one optional, closeable notice. Close or Escape dismisses
it; another menu or new Match clears the previous notice. Dismissal does not
write parameters or end an active edit. Disabled actions publish no mutation.
Panel close and destructive band deletion have separate names and meanings.

## Feedback and unavailable states

Use the least interruptive layer that protects the user's work: a readable
inline state or non-modal notice for an ordinary failed edit; a contextual panel
for explanation; a dialog only when continuation requires an immediate choice.
Meter changes are visual telemetry, not a stream of spoken alerts or dialogs.

A warning names the affected parameter/operation, observable consequence,
whether settings/audio changed and the next user-controlled action. Keep titles
stable while phases change. Never show success before the owned commit point,
offer cancellation for work that cannot be cancelled, or dismiss an operation
while pretending an uncertain mutation was aborted.

Plug-ins do not own device-loss decisions, channel routing or host project
recovery. Do not automatically change a creative setting, transport or bypass
in response to load. A plug-in editor that cannot open must not reset the
instance's saved parameters; loading/recovery policy remains with the host.

## Review and delivery evidence

- The primary parameter and current signal state are discoverable on the owning
  surface; labels/units explain specialized gestures.
- Pointer, keyboard, numeric entry and accessible value convey the same domain.
- Host begin/end counts, cancellation and authoritative recall remain balanced.
- Multiple instances have independent selection, meters, spectra and audition.
- Overlay dismissal, focus loss, resize and closure have explicit outcomes.
- A failed operation preserves its documented coherent state and explains a
  useful recovery action.
- Actual VST3 and AUv2 hosts validate simultaneous loading, editor interaction,
  parameter automation and save/reopen. Validator success alone is insufficient.

Current targeted EQ tests and historical Heron native resize/focus regressions
provide narrow evidence described in [Heron EQ](heron-eq.md). They do not claim
all controls meet this specification or all DAWs pass it. Record manual host
results through [CI operations](ci.md) and follow [testing policy](testing.md).
AAX acceptance remains deferred while that release format is disabled.
