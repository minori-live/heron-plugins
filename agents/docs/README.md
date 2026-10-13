# Agent development notes

This directory owns the engineering and maintenance rules for Heron Plugins.
[AGENTS.md](../../AGENTS.md) is the concise entry point; these local documents
provide the normative detail needed to work on the suite independently of Heron.
Keep architecture, development, CI, review guidance and ADRs here. This repository
has no website workspace. Do not put temporary notes, generated output, build
artifacts or benchmark results in this directory.

## Standards and operations

- [Engineering standards](engineering-standards.md)
- [Testing policy](testing.md)
- [Architecture and real-time constraints](architecture.md)
- [Native editor interaction design](interaction-design.md)
- [Native editor design system](design-system.md)
- [Development environment](environment.md)
- [CI and release operations](ci.md)
- [Published artifact contract](artifacts.md)
- [EQ implementation and verification](heron-eq.md)
- [Architecture decisions and their lifecycle](adr/README.md)

## Adoption and authority

On 2026-10-10, this repository adopted the applicable rules from Heron's
[agent documentation](https://github.com/minori-live/heron/blob/fef5cdb165bb322e07f2a280fea8372aa5e22393/agents/docs/README.md).
Each migrated standard records its source. These are maintained here as local
plugin rules; future Heron documentation changes do not silently amend them.
Heron's Electron, Vue, application database, audio-device runtime and plug-in
host rules continue to apply in Heron. Their implementation and tests do not
move into this workspace.

The rules apply to all new work and materially changed code or behavior.
Existing code is not permission to repeat a conflicting pattern. Bring the
affected scope into compliance or link a narrowly scoped tracking issue.
Documented gaps in editor accessibility or theme support remain
gaps; a normative requirement does not establish that a feature is delivered.

Apply the documents by responsibility:

1. Engineering standards and testing policy govern implementation, module
   cohesion, errors, review evidence, owned tests and exceptions.
2. Architecture governs source ownership, dependency direction, native ABI,
   worker/editor lifetime and real-time assumptions.
3. Interaction design governs parameter gestures, control discovery, feedback
   and recovery; the design system governs native visual primitives and
   accessibility.
4. Accepted ADRs preserve durable decisions. CI operations and the artifact
   contract define release gates and producer/consumer compatibility.

A proposed ADR is not authority. Maintain current rules in the owning documents
as part of authorized work. When an accepted ADR changes a durable decision,
update its owning current-state document in the same change. Historical
implementation details in an ADR do not independently freeze the current design;
clarify misleading details with a dated amendment and a link as described in
[ADR governance](adr/README.md). Genuine conflicts about durable commitments
require resolution within the maintainer's direction; neither a documentation
edit nor an agent-written status label authorizes a change outside the task.
Raise unresolved substantive conflicts and continue unaffected work. ADR
creation is not a routine approval stage. Accepted architecture is distinct
from completed implementation.

Mechanical checks belong in `mise run check` or the appropriate narrower command.
Rules without an implemented mechanical check still require review; do not claim
CI enforces them. Judgment-based rules are recorded in the pull-request
governance review. Exceptions and deferred correctness work require a linked
issue; no separate waiver process is required at the current project scale.
