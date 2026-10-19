# Heron patches: Revox AU preparation and lifecycle serialization

Source: published `truce-au` **6.3.0**, using its normalized Cargo manifest.

- Registry checksum: `d9168fc2f0fa9b8f589e89518e200ddb103020652d317a6d24a44214132278aa`
- Upstream repository: <https://github.com/truce-audio/truce>
- Upstream commit: `ff6b573c7d845638656b03bbe4b4436559dd9725`
- Upstream path: `crates/truce-au`
- Published license metadata: `LicenseRef-TruceLicense-1.0`

The published crate omitted its license text. `LICENSE` is copied from the
existing `third_party/truce-iced/LICENSE`, which retains that exact upstream
commit's license (Git blob `b759333b753ac8483217e2983fe0afe804c96b6a`).
Registry cache markers, the packaged lockfile, VCS metadata and the unused
original Cargo manifest are omitted; the published VCS provenance is recorded
above.

## Negotiated main-bus preparation

AUv2 Initialize and Reset forward the host's validated main input/output widths
through a private callback registered by the Rust adapter. The callback builds
`AudioConfig` with the paired vendored `truce-core` extension
`main_bus_channels: Some((input, output))` before invoking plugin reset. This
lets Revox prepare only the selected mono or stereo model batch before rendering.
The supported-layout list is not used as a substitute for the negotiated layout.

The bridge is internal to the native module. The public `AuCallbacks` table,
`TRUCE_AU_ABI_VERSION`, shared shim headers and AUv3 reset path remain unchanged.
The published reset callback still supplies an unknown layout (`None`).

## Opt-in AUv2 lifecycle gate

The default-off `serialize-lifecycle` feature defines
`TRUCE_AU_SERIALIZE_LIFECYCLE` for the macOS AUv2 C shim. Heron Revox enables the
feature on its optional `truce-au` dependency. A per-instance recursive pthread
mutex serializes Initialize, Uninitialize, Reset, Render and Close. Render owns
the gate throughout input pulling, Rust processing and final output copying;
Initialize cannot replace native buffers while a render uses them. The mutex is
created by the component factory and destroyed before its instance is freed.
Clang cleanup releases ordinary lifecycle guards on every return path.

Revox preserves input and waits for matching processed audio in every host mode.
Serializing its lifecycle does not introduce bypass, dropped audio or an
overload state. Reset stays on the thread that called it; allocation or model
teardown is not deferred into the next audio process block. A reset arriving
during a render waits for that render to finish before clearing its timeline.

The recursive mutex permits same-thread Reset from the shim's host input,
transport and pre/post-render callbacks. Those calls occur outside Rust's
process-state borrow. Revox's processing function does not synchronously call
the host while holding that borrow. Component destruction still relies on the
host ending calls to that instance; the gate does not support recursively
destroying or initializing a component from its active render.

The feature does not change the published path when disabled. Cargo unifies
dependency features in a workspace build, so isolation of produced components
relies on the existing per-plugin `cargo truce package -p ...` release commands.
A workspace-wide AU feature check is a compilation check, not a producer of
independently configured AU components. Keep this feature off other plugins'
dependency declarations; their callbacks retain their nonwaiting contract.

## Failure and validation

Logic playback exposed Reset running while Revox's render was waiting for its
first wet sample. The active callback retained an old due position while reset
cleared its position, channel count and partial input and advanced the
processing generation. The worker retired old-generation packets and waited
for new input that the blocked callback could never enqueue. Truce's release
ownership cell assumed host lifecycle exclusion and allowed the overlapping
mutable state access.

`fixtures/revox-au-probe.cpp` reproduces the ordering at the installed AU
boundary. Its input callback holds a render at the first delayed output while
another host thread calls Reset. Reset must remain pending until that render is
released. The probe then verifies a complete new timeline's reported preroll
and actual +3.5-semitone processed audio. Its process deadline bounds a failing
test; it does not change plugin audio behavior. Run this packaged AU audio probe
alongside `auval`, and verify playback start in Logic.

Remove the paired AU/core patches after an upstream release supplies negotiated
layout at preparation time and equivalent lifecycle serialization, and these
installed-component and Logic playback regressions pass with that release.
