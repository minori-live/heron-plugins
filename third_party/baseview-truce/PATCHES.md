# Heron patches: native editor scale and focus

Source: published `baseview-truce` **0.1.1-truce.13**, using its normalized Cargo
manifest and published MIT/Apache-2.0 licenses.

- Registry checksum: `5dc830ece0415c778e700c97d38bb28f4d8af0280f9d140f50ea094d0fed3e27`
- Upstream repository: <https://github.com/truce-audio/baseview>
- Upstream commit: `a860a62b6471a549f113026b236242a90a3c9d8e`

## Windows embedded host scale and focus

Windows implementation changes are confined to `src/win/window.rs`:

- A window opened with a pinned `ScaleFactor` accepts valid host scale updates.
  The scale cell updates immediately so queued logical resizes use the latest
  factor. A deferred `ScaleChanged` task emits `Resized` after the handler borrow
  ends, using its current physical extent without scaling that extent again.
- A nested native message pump leaves deferred tasks queued while the handler
  is borrowed. The outer dispatch drains them after that borrow ends, so a
  `ScaleChanged` notification cannot be popped and silently ignored.
- Native `Focused` and `Unfocused` transitions use the same FIFO queue. A
  reentrant focus change during rendering retains every transition until the
  handler callback returns, so the Iced bridge can cancel interrupted gestures.
  This addresses the [focus reentry review of PR #222](https://github.com/minori-live/heron/pull/222#discussion_r4215168764).
  Heron's `native_focus` regression verifies real loss/gain transitions are
  delivered exactly once, in order, after the active frame callback returns.
- Pinned windows ignore `WM_DPICHANGED`'s DPI-only suggested rectangle because
  the host owns their accepted physical extent. System-scale windows retain the
  published OS DPI behavior.

`src/window.rs` documents the corrected public scale-update behavior.

This pairs with Heron's `truce-gui` patch. Registry cache markers, the packaged lockfile and VCS
metadata are omitted; provenance from the published VCS metadata is recorded
above. Remove both patches together after an upstream release supplies the
equivalent Windows behavior and Heron's packaged native-editor and
`native_focus` regressions pass with that release.

## macOS Retina backing notifications

`src/macos/view.rs` registers AppKit's argument-free
[`viewDidChangeBackingProperties`](https://developer.apple.com/documentation/appkit/nsview/viewdidchangebackingproperties%28%29?language=objc)
selector and matching callback signature. The published selector included a
colon, so AppKit never called it when a view attached or changed display backing.
The callback preserves NSView's superclass behavior, waits until the view is
attached (and its baseview state initialized), and delivers the updated physical
size through the existing deferrable event queue. Reentrant native layout cannot
borrow an already-active window handler.

The paired `truce-iced` patch initializes from the attached parent's backing
scale and prevents host zoom from replacing macOS pixel density. The owned
`heron-eq` `native_scale` regression checks native attach notifications and actual
Metal drawable extents on a Retina Mac:

```sh
cargo test --locked -p heron-eq --test native_scale -- --ignored
```

Before the patches, a host's 1x content-scale announcement produced a 1120×760
drawable in a 2240×1520 Retina backing. Afterward the drawable is 2240×1520
through attach and late 1x/1.25x announcements, and 2560×1640 after a resize to
1280×820 logical points. Moving the live editor between 1x and 2x displays is the
additional manual check for an actual display transition. Remove this macOS
patch with its Iced counterpart when a coupled upstream release supplies the
same behavior and the native regression passes.
