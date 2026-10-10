# Architecture decision records

ADRs record durable choices, their rationale and rejected alternatives. Current
implementation details and verification evidence belong in the owning documents
linked from the [documentation index](../README.md).

## Current records

| Record                                                                          | Status   | Scope                                                                                              |
| ------------------------------------------------------------------------------- | -------- | -------------------------------------------------------------------------------------------------- |
| [0001 — Independent plugin suite releases](0001-independent-plugin-releases.md) | Accepted | Source ownership, compatibility, formats, immutable artifacts and producer-owned Heron upgrade PRs |
| [0002 — Release artifact pinning](0002-release-artifact-pinning.md)             | Accepted | Version/commit/filename locks, release assets and consumer validation                              |

Accepted means the decision is chosen. It does not claim that all formats,
implementation work or manual host acceptance are complete. Proposed decisions
do not authorize a compatibility commitment.

## When a decision record is required

Create an ADR before changing process/thread ownership, native ABI or host
protocols, saved-state meaning, real-time safety, foundational dependencies,
long-term identity/artifact compatibility or material interaction semantics
departing from the Logic reference or accepted architecture.

Small implementation choices, routine dependency patches, reversible refactors
within one owner, test deduplication and issue triage do not need separate ADRs.
Prefer one cohesive decision over a record for each implementation step.

## Lifecycle and review

Use four digits and a short kebab-case title; the next local record is **0003**.
Copy [the template](template.md). Status is Proposed, Accepted, Superseded by
ADR-NNNN or Rejected. Accepted records change only for editorial corrections,
links and explicit implementation-scope clarification. A changed decision
requires a new record and a supersession link; do not renumber existing history.

Record owners, context, a testable decision, rejected alternatives, consequences,
verification requirements and reconsideration triggers. Link the issue or PR
when available. Normal repository review is sufficient.

## Migration provenance

This lifecycle was adopted on 2026-10-10 from Heron's
[ADR governance](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/adr/README.md).
Heron's ADR numbers belong to its own namespace. Its historical
[native EQ decision](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/adr/0029-native-heron-eq.md)
and [EQ editor decision](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/adr/0030-eq-graph-and-floating-controls.md)
remain source history for the migrated contracts. Maintain their plugin-owned
current rules in [architecture](../architecture.md),
[interaction design](../interaction-design.md) and [EQ notes](../heron-eq.md).
Heron retains the application/host decisions; do not import its entire ADR
series or treat the same number as a local decision.
