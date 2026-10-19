# Heron patch: negotiated main-bus preparation

Source: published `truce-core` **6.3.0**, using its normalized Cargo manifest.

- Registry checksum: `5ecd892fb0f82fdfbf801b4aab7f443f3c51deac572366b8bebfbda49117f1df`
- Upstream repository: <https://github.com/truce-audio/truce>
- Upstream commit: `ff6b573c7d845638656b03bbe4b4436559dd9725`
- Upstream path: `crates/truce-core`
- Published license metadata: `LicenseRef-TruceLicense-1.0`

The published crate omitted its license text. `LICENSE` is copied from the
existing `third_party/truce-iced/LICENSE`, which retains that exact upstream
commit's license (Git blob `b759333b753ac8483217e2983fe0afe804c96b6a`).
Registry cache markers, the packaged lockfile, VCS metadata and the unused
original Cargo manifest are omitted; the published VCS provenance is recorded
above.

`AudioConfig` gains `main_bus_channels: Option<(u32, u32)>` and the builder
`with_main_bus_channels(input, output)`. The tuple contains negotiated main-bus
input/output widths and excludes auxiliary buses. `AudioConfig::new` sets it to
`None`, preserving existing adapters' unknown-layout behavior. The configuration
remains `Copy`, and plugin reset signatures remain unchanged.

The paired `truce-au` patch supplies the AUv2 host's validated widths during
Initialize and Reset. The static shell forwards this configuration to plugin
logic, allowing Revox to choose its actual mono or stereo model batch before
audio rendering begins. It does not infer a layout from the supported-layout
list or initialize both model batches.

All other published Rust sources and normalized Cargo metadata are unchanged.
Keep the paired AU/core patches together when upgrading Truce. Remove this
extension after an upstream release supplies negotiated layout at preparation
time and Revox's installed AU initialization and reset regressions pass with
that release.
