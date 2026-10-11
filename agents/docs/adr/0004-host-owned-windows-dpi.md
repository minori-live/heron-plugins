# ADR-0004: Host-owned Windows DPI awareness

- Status: Accepted
- Date: 2026-10-11
- Owners: project maintainers
- Scope: Windows native editor process and thread DPI ownership

## Context

The shared baseview window initializer calls `SetProcessDpiAwarenessContext`
after creating a window, including when a plug-in is embedded in a DAW. This
attempts to change the loading host's process default and discards the result.
A user reports that opening Heron plug-ins makes the entire Studio One UI
small. The call is confirmed in source; its success and contribution to that
report have not been verified on the affected Windows installation.

## Decision

The loading host owns process DPI awareness and the thread DPI context supplied
to embedded editors. Remove the process-level setter from baseview's shared
window initializer without replacing it with a thread-level override. Continue
using the existing host content-scale and native window resize contracts.

A standalone executable owns its DPI initialization through its application
manifest or startup code, before window creation. The pinned cargo-truce 6.3.0
standalone tooling already embeds a DPI-aware manifest. This suite publishes
plug-in bundles, not standalone executables.

## Alternatives rejected

Keeping the setter and checking its result still attempts to mutate a host-owned
process setting. Replacing it with a forced per-monitor thread context can
conflict with a host's deliberate mixed-DPI or bitmap-scaling mode.

## Consequences

Opening an editor no longer requests a different process DPI policy. A custom
standalone executable without a manifest must supply its own policy instead of
depending on the library's late initialization. Plug-in identity, parameters,
saved state, rendering scale negotiation and audio processing are unchanged.

## Verification

Compile the patched Windows backend and inspect the editor initialization path.
The reported Studio One behavior requires a Windows interactive session with
the affected host settings; a DPI-aware test process can reject the old setter,
so merely asserting equal DPI values before and after opening would also pass
before this fix. The repeatable host check and current evidence are recorded in
[the baseview patch notes](../../../third_party/baseview-truce/PATCHES.md#windows-dpi-ownership).

## Reconsider when

A supported host requires an explicit, scoped editor DPI context. Any such
exception must preserve the host's process policy and restore its thread context.
