# Architecture decision records

ADRs record durable choices, their rationale and rejected alternatives. Current
implementation details and verification evidence belong in the owning documents
linked from the [documentation index](../README.md).

## Current records

| Record                                                                          | Status   | Scope                                                                                              |
| ------------------------------------------------------------------------------- | -------- | -------------------------------------------------------------------------------------------------- |
| [0001 — Independent plugin suite releases](0001-independent-plugin-releases.md) | Accepted | Source ownership, compatibility, formats, immutable artifacts and producer-owned Heron upgrade PRs |
| [0002 — Release artifact pinning](0002-release-artifact-pinning.md)             | Accepted | Version/commit/filename locks, release assets and consumer validation                              |
| [0003 — EQ Nyquist display domain](0003-eq-nyquist-display-domain.md)           | Accepted | Sample-rate-dependent graph range, display data and pointer mapping                                |
| [0004 — Host-owned Windows DPI awareness](0004-host-owned-windows-dpi.md)       | Accepted | Host ownership of process and embedded-editor thread DPI policy                                    |
| [0005 — Runtime-only releases](0005-runtime-only-releases.md)                   | Accepted | Runtime-only release assets; plug-in Sentry integration deferred                                   |

Accepted means the decision is chosen. Implementation status belongs in the
owning documents. Proposed decisions do not authorize a compatibility commitment.

## Documentation defaults

Update the existing owning documents as part of authorized work. Choose the
location by the information's purpose:

| Information                                                         | Default location                                        |
| ------------------------------------------------------------------- | ------------------------------------------------------- |
| Current architecture, behavior, and compatibility policy            | Existing architecture, interaction, or feature document |
| A bug's cause, the fix, and validation results                      | Pull request description                                |
| Temporary limitations and tracked follow-up work                    | Issue                                                   |
| A non-obvious local implementation constraint                       | Comment beside the owning code                          |
| A durable choice whose rationale deserves independent future review | ADR, when explicitly requested by a maintainer          |

Do not create a new decision document under another name to bypass these rules.
Update shipped user documentation when user-visible behavior changes, following
its existing scope. Documentation requirements do not imply an ADR requirement.

## Maintainer direction

Maintainers decide when to create an ADR. Agents MUST NOT create one, including
a Proposed draft, without an explicit maintainer request to create that record.
They MUST NOT mark a record Accepted without explicit maintainer acceptance.
Authorization to implement a feature or fix does not imply either action.
A request already given in the task or review is sufficient; do not ask again.

Changes involving process/thread ownership, persistence, cross-process/native
protocols, real-time behavior, foundational dependencies, compatibility, or
interaction semantics do not automatically require an ADR. Neither does the
presence of a related accepted record. Routine fixes, protocol fields, schema
migrations within an existing persistence design, display thresholds, gestures,
dependency updates, and implementation stages belong in their owning documents
and PRs unless a maintainer explicitly requests a decision record.

When a choice appears worth recording separately, briefly explain in the work
summary or PR why future maintainers would need its rationale, which realistic
alternatives carry meaningful tradeoffs, and what would make reversal costly.
These are considerations for the maintainer, not automatic triggers or a
checklist required for every PR.

Continue work already authorized. Do not pause solely to ask whether to create
an ADR, and do not add an ADR approval stage to routine development. Changes
outside the authorized task or to commitments it does not authorize changing
still require maintainer direction, regardless of documentation format.

## Current rules and historical rationale

Current architecture and product rules belong in their owning documents. ADRs
preserve important choices and explain their rationale; they are not a second
copy of the current specification or an implementation log. When an authorized
change affects current rules, update the owning documents in the same change.

An implementation detail does not become a permanent architectural commitment
solely because an accepted ADR mentions it. Correcting a field, threshold,
compatibility edge case, or staged implementation detail does not require a
replacement record. Update the owning document when applicable; if an existing
ADR would mislead readers, add a short dated amendment linking the current
rule and the relevant PR or issue. Preserve the original rationale and identify
what changed rather than silently rewriting the historical choice.

A genuine reversal of a durable decision may warrant a replacement ADR at the
maintainer's direction. Do not silently weaken existing product promises,
ownership boundaries, safety constraints, or compatibility commitments by
editing current documentation or labeling a substantive change a detail. If
such a conflict is outside the authorized scope or cannot be resolved from the
maintainer's direction, raise that specific decision while continuing unaffected
work. A difference in historical implementation detail alone is not a reason
to block work.

## Lifecycle and review

When requested, use a four-digit number above the highest local number
(**0006** next), never fill gaps or reuse retired identities, and choose a
short kebab-case title. Copy [the template](template.md). Status is Proposed,
Accepted, Superseded by ADR-NNNN, or Rejected. Start at Proposed unless the
maintainer has explicitly accepted the decision. Record the request and
acceptance references when available; an agent's status label is not evidence
of maintainer acceptance.

Keep each record focused on one durable choice: context, decision, meaningful
alternatives, consequences, and reconsideration conditions. Link current
specifications and relevant validation evidence rather than duplicating field
catalogs, implementation plans, test checklists, or temporary repair details.
When a replacement is requested and accepted, link both records and identify
which decision it supersedes. Normal repository review is sufficient.

Record identities and statuses stay stable; retrospective consolidation
requires a separate maintainer request.

## Migration provenance

This lifecycle was adopted on 2026-10-10 from Heron's
[ADR governance](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/adr/README.md)
and later aligned with Heron's maintainer-directed governance. Heron's ADR
numbers belong to its own namespace. Its historical
[native EQ decision](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/adr/0029-native-heron-eq.md)
and [EQ editor decision](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/adr/0030-eq-graph-and-floating-controls.md)
remain source history for the migrated contracts. Maintain their plugin-owned
current rules in [architecture](../architecture.md),
[interaction design](../interaction-design.md) and [EQ notes](../heron-eq.md).
Heron retains the application/host decisions; do not import its entire ADR
series or treat the same number as a local decision.
