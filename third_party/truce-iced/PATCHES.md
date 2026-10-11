# Heron patches: native editor focus and Retina density

Source: published `truce-iced` **6.3.0**, using its normalized Cargo manifest.

- Registry checksum: `ebce0f17416819460410786fba2f3f6c18f5514f7013da2fe6bcc20a3b4d5295`
- Upstream repository: <https://github.com/truce-audio/truce>
- Upstream commit: `ff6b573c7d845638656b03bbe4b4436559dd9725`
- Upstream path: `crates/truce-iced`
- Published license metadata: `LicenseRef-TruceLicense-1.0`

The published crate omitted its license text. `LICENSE` is copied from that
exact upstream commit (Git blob `b759333b753ac8483217e2983fe0afe804c96b6a`).

## Windows native editor focus-loss interruption

On Windows, `src/editor.rs::IcedBaseviewHandler::on_event` forwards baseview's
native `Focused` and `Unfocused` events to Iced's pending window-event queue.
Heron EQ already uses `Unfocused` to cancel its active gesture, restore the retained value and balance
host parameter edits. Without the bridge, moving focus during a native curve
drag leaves its edited value and open host gesture behind. This was observed
in the packaged Windows editor interruption regression while verifying #220.
The paired `Focused` event restores Iced text-input cursor and IME behavior
after the interruption; forwarding only focus loss would strand inputs in the
unfocused state.

Registry cache markers, the packaged lockfile and VCS metadata are
omitted; provenance from the published VCS metadata is recorded above. Remove
this patch when an upstream release supplies the equivalent paired event bridge and
Heron's packaged focus-loss interruption regression passes with that release.

## macOS Retina density

`src/editor.rs::IcedEditor::open` initializes render density from the actual
parent NSWindow before creating the Metal surface, rather than trusting a
main-screen default or a prior host content-scale announcement. Once attached,
AppKit backing notifications own the render density; `set_scale_factor` no longer
replaces it with host zoom. Pre-open overrides remain supported for deterministic
1x/2x offscreen captures.

This restores the existing macOS `SystemScaleFactor` policy. Host content zoom
and Retina backing density are different quantities: Truce's macOS editor sizes
remain logical points, and the previous setter changed raster resolution without
changing the view's logical extent. This patch does not introduce a new zoom or
resize policy. Windows and Linux scale handling is unchanged.

The paired baseview patch restores AppKit backing-change notifications. On an
interactive Retina Mac, run:

```sh
cargo test --locked -p heron-eq --test native_scale -- --ignored
```

The regression failed before the fix with 1120×760 drawable pixels where the
native backing required 2240×1520. It now confirms 2240×1520 after opening and
late host 1x/1.25x callbacks, followed by 2560×1640 for a 1280×820 logical resize.
It also checks baseview's native attach notification separately from Iced's
surface initialization. Remove the coupled macOS patches when an upstream
release supplies this behavior and the same native regression passes.
