# Native plug-in design system

This specification adapts
[Heron's design system](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/design-system.md)
at source commit `fef5cdb165bb322e07f2a280fea8372aa5e22393` to Rust/Iced native
editors. [AGENTS.md](../../AGENTS.md) and the [documentation index](README.md)
define repository authority. [Interaction design](interaction-design.md)
governs workflow, control exposure and DAW gesture semantics; visual consistency
does not justify hiding a frequently adjusted parameter.

## Visual thesis and boundaries

Editors should feel precise, calm and signal-led. Neutral large surfaces keep
the user's attention on sound and measured data; color explains focus, selection,
audio/MIDI, warnings and action. Decoration, motion and elevation must explain
state or hierarchy rather than compete with the graph or value being edited.

The local source of shared native foundations is `crates/iced-ui`; reusable
plug-in controls live in `plugins/heron-plugin-ui`. An editor composes these
primitives and supplies its instance's state/intent. Shared UI must not acquire
transport, project persistence, device policy or another plug-in instance's
parameters. Avoid separate generic component systems for each plug-in.

This is a native specification: logical editor coordinates, toolkit focus,
platform accessibility and accepted host scale replace browser layout and DOM
mechanisms. Native windows must not import Heron's application UI runtime or
assume the host supplies its themes/fonts. [Architecture](architecture.md)
defines the parameter, callback and lifetime boundary.

## Semantic foundations

Use named semantic roles for canvas, surfaces, controls, text, borders, actions,
selection, focus, feedback and signal. `Appearance::palette()` currently resolves
a `SemanticPalette` with these roles:

| Role group      | Current native fields                                       |
| --------------- | ----------------------------------------------------------- |
| Surface         | `canvas`, `canvas_subtle`, `surface`, `surface_raised`      |
| Control         | `control`, `control_hover`, `control_pressed`               |
| Text            | `text`, `text_muted`, `text_subtle`                         |
| Border          | `border`, `border_strong`                                   |
| Action          | `action`, `action_hover`, `action_pressed`, `action_text`   |
| Selection/focus | `selection`, `selection_hover`, `selection_border`, `focus` |
| Feedback/signal | `success`, `warning`, `danger`, `audio`, `midi`             |

Resolve colors from shared roles. Centralize a justified signal/domain palette
in an owned style/token source; do not scatter raw colors through new ordinary
forms, overlays or buttons. Runtime band/signal colors enter through typed
values and must have a non-color representation of the same state. A domain
canvas may use measured geometry and composited glow, but its focus, hit testing
and interaction states still follow the shared behavior contract.

Shared spacing currently uses a four-unit scale: `XS=4`, `SM=8`, `MD=12`,
`LG=16`, `XL=24`. Typography roles currently expose caption 11, control 12,
compact body 13 and panel title 15 in logical Iced units. These are existing
compact roles, not a declaration that every caption or physical target is
accessible at every host scale. Do not copy browser CSS pixels into physical
native dimensions; test actual layout at the accepted content/display scale.

Typography remains role-based: interface text explains controls; aligned
musical/numerical data remains easy to compare; headings establish hierarchy.
Preserve readable dB, Hz/kHz, Q, milliseconds and percentages. Use appropriate
glyph coverage, shaping, fallback and IME behavior for supported text. Font
loading/licensing belongs to the non-real-time native implementation; the
current editors receive the font from `ParamCache` and do not establish that
Heron's bundled web font families or localization pipeline migrated with them.

## Current theme behavior and exceptions

`crates/iced-ui` defines dark and light appearances. The shared plug-in
`palette()`/`theme()` deliberately select dark appearance. Heron EQ has an owned
semantic palette override in `plugins/heron-eq/src/editor/style.rs`, including
its neutral spectrum and band colors. This existing product identity is an
explicit local palette, not permission to introduce arbitrary new colors in
unrelated controls.

Do not claim a host theme switch propagates to plug-in content or that the suite
has validated light editors merely because a light palette exists. If light
appearance or system-following themes are introduced, all supported controls,
overlays, focus states and signal palettes must be reviewed together. A theme
change must not change parameters, selection ownership or host gesture state.

## Reusable controls and visual states

Use an existing native primitive before creating another knob, meter, numeric
entry or icon button. Shared controls own drawing and reusable interaction;
the plug-in translates normalized values, units and host edit intent. An
equivalent visual control must not independently reimplement parameter mapping
or host gesture semantics.

Each interactive control defines its applicable default, hover, focus, pressed,
selected, disabled, busy and invalid states. Focus is distinct from selected;
pressed is momentary and selected remains current after activation. Disabled
controls stay named/readable and publish no edit. Busy prevents duplicate
commitment while retaining the action label; invalid shows the specific recovery
beside the value or in its single owned notice.

Actions, exclusive modes, object selection and continuous values are different
behaviors. Use concrete text for ordinary actions and stable selected state for
mode choices. An icon action needs a name and a discoverable text explanation;
a tooltip supplements, rather than replaces, the accessible name. Never put an
essential action inside a tooltip.

Numeric controls keep the range, current value, unit and default explicit. Direct
entry validates a complete value without penalizing intermediate typing. Fine
and larger keyboard steps are documented. A field error cannot silently leave
the display implying that its draft was accepted. Knobs scale drag against the
useful parameter domain; meters consume bounded display values and never write
parameters or calculate processing on the UI's behalf.

The current shared `ParameterKnob` and `LevelMeter` centralize drawing and
pointer/value behavior. Their existence does not prove platform accessibility,
keyboard operation or reset/numeric-entry coverage. EQ has domain-specific
rotaries with additional interaction behavior; see
[interaction design](interaction-design.md) for the established gestures and
current gaps.

## Panels, overlays and feedback

Use toolkit-owned overlays/panels with explicit focus, Escape, dismissal and
restoration behavior. Keep layers and anchors owned by editor composition;
new widgets must not invent unrelated overlay stacking rules. A non-modal panel
contains persistent contextual controls; a menu contains commands or terminal
choices; a dialog represents an interrupting decision.

Titles name a stable action; descriptions identify the affected object and
consequence. Changing counts or progress phases belong in body content.
Destructive confirmation names the action and consequence and has an explicit
safe Cancel path. Closing a panel and deleting a band use distinct icons/names.

Status explains the object, outcome, settings/audio safety and next action.
Keep useful asynchronous status readable, with platform announcements where
supported; never announce every rapidly changing meter/spectrum value. Use
determinate progress only for known progress and offer cancellation only when
the owned operation can actually abort. Dismissal must not imply an uncertain
state mutation was rolled back.

## Density, coordinates and graph composition

Standard forms/configuration favor roughly 32–48 logical-unit controls; compact
editor chrome may be denser, with at least a 24 by 24 logical-unit interaction
target or an explicitly reviewed accessible equivalent. These native sizing
rules adapt Heron's density intent; they are not a claim that logical units
are CSS pixels or that minimum size alone proves accessibility.

Graphs may retain specialized domain geometry; surrounding labels, contextual
controls and panels remain readable and operable at supported minimum size.
Avoid horizontal scrolling for ordinary forms or toolbar-only tasks. Test long
labels, numeric drafts and 200% native magnification or an equivalent available
text-scaling mechanism, recording the actual method and host/display scale.
Increased scale must not hide the only way to close or configure a panel.
Do not shrink text until all controls fit at the expense of readability.

EQ uses a logarithmic frequency axis, a linear EQ-gain axis and a separate
spectrum dBFS axis. Drawn nodes, labels, hit testing and dragging use the same
geometry. Maintain selected-band controls at a stable lower-center anchor and
review actual 24-band overlap. A coordinate transformation must not fabricate
new extrema, move the measured data, bridge unavailable measurement gaps or hide
a pending/failed response as current.
See [Heron EQ](heron-eq.md) for finite-response and spectrum-display contracts.

Painting and input share the accepted logical/physical scale. Review repeated
resize, host zoom/DPI and focus transitions against live native attachments;
PNG dimensions alone do not prove interactive bounds. Preserve widget identity
and retained interaction state across layout changes.

## Accessibility requirement and honest evidence

Heron's WCAG 2.2 AA visual/interaction baseline remains the design benchmark,
adapted to native platform semantics. Required outcomes are:

- visible focus for every operable element and a programmatic name, role,
  value, range and state through the supported accessibility mechanism;
- full keyboard operation, including fine numeric control and a way out of
  canvas-only or overlay interactions;
- readable text and non-text control contrast, with no information conveyed
  by color alone;
- focus containment/restoration for modal decisions and deliberate focus
  handling for non-modal panels;
- usable non-canvas controls at supported host/display scaling and 200%
  magnification or equivalent text scaling,
  without clipped numeric entry or inaccessible dismissal;
- reduced motion for nonessential animation, while retaining signal evidence;
- no flood of announcements from real-time telemetry.

Native screenshots and widget tests cannot establish screen-reader support,
keyboard completeness, contrast across every state, reduced-motion behavior or
200% magnification by themselves. The migrated custom canvases have outstanding
keyboard/platform-accessibility gaps; current fixed-dark content also has no
light-theme acceptance claim. Keep those limitations explicit and do not label
the native suite WCAG-conformant from the existence of semantic palettes.

New or changed controls must meet the applicable requirements or document the
remaining gap, consequence and owned follow-up; existing gaps are not a design
exception for new behavior. Review native platform output and owned editor
attachment tests.

## Contribution evidence

- Reuse the shared native primitive or record why a domain-specific control is
  needed. Keep instance/host policy outside shared drawing.
- Centralize semantic tokens and document domain/runtime-color exceptions.
- Cover applicable disabled/busy/error states, long text, selected/focus states,
  numerical bounds, keyboard interaction and cancellation.
- Review overlay anchoring, Escape, focus loss and restoration.
- Review minimum size, host scaling and actual painted/hit-test agreement.
- Exercise multiple plug-in instances and host automation/recall independently.
- Record the format, OS, scale and scenario for native evidence; distinguish
  rendered appearance from actual input/lifetime validation.

Follow [testing policy](testing.md), [engineering standards](engineering-standards.md)
and [CI operations](ci.md). Targeted EQ native render/event tests are review
surfaces; there is currently no independent all-control native catalog or
automated accessibility certification. Cross-platform VST3/AUv2 releases use
final bundle validation; AAX remains disabled.
