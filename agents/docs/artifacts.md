# Artifact contract

One suite version identifies all four plug-ins. The public producer repository
is `minori-live/heron-plugins`; a published release tag is `v<VERSION>`. Published
assets are immutable. A release has ten ZIP files, one macOS DMG and SHA256SUMS
while AAX is disabled.

Platform IDs are `windows-x64`, `linux-x64`, `linux-arm64` and
`macos-universal`. Runtime assets are
`heron-plugins-<version>-<platform>-<format>.zip`; corresponding symbol assets are
`heron-plugins-<version>-<platform>-<format>-symbols.zip`. Formats are `vst3`,
`au2` (macOS only) and gated `aax` (macOS/Windows only).

A runtime ZIP has exactly `bundles/` and `bundle-manifest.json` at its root.
Each complete native bundle contains its license, THIRD_PARTY_NOTICES.txt and
dependency license texts under `Contents/Resources/`. Archive creation retains
executable modes and relative symlinks and rejects links escaping the root.

```json
{
  "schemaVersion": 1,
  "version": "0.6.4",
  "sourceCommit": "<40 lowercase hex characters>",
  "platform": "macos-universal",
  "plugins": [
    {
      "builtinId": "live.minori.heron.gain",
      "name": "Heron Gain",
      "kind": "effect",
      "format": "vst3",
      "nativeId": "46774F504DF84B4AC1F308AB88DD3677",
      "bundleName": "Heron Gain.vst3"
    }
  ]
}
```

The actual manifest lists all four catalog entries. VST3 native IDs use Heron's
GUID display order; the binary probe checks the exported COM-layout bytes after
the same field-order conversion. AU IDs use `type:subtype:manufacturer`; EQ's
MIDI input requires `aumf:HREQ:HRON`. AAX IDs
use eight-hex-digit `manufacturer:product:native-layout-ids`, with multiple
registered layout IDs separated by commas; the catalog records the pinned
Truce 6.3 wrapper's actual mono/stereo registration.

A symbol ZIP has `symbols/` and `symbols-manifest.json` at its root. Its manifest
contains `schemaVersion`, `version`, `sourceCommit`, `platform`, `format` and
`modules`. Each module records `modulePath`, `binaryPath`, `debugId` and `arch`.
`modulePath` points to a real DWARF/PDB/ELF debug file inside `symbols/`;
`binaryPath` points to its matching executable inside the runtime ZIP. A
universal DWARF file has separate records for its `x64` and `arm64` slices.
Source bundles are associated with these debug IDs and archived under
`symbols/`. Final binary/debug IDs and full debug information are checked
offline before archiving; a format's symbols never include another format.
For macOS VST3, DWARF is generated from the final linked MH_BUNDLE. AU packaging
copies/lipos Cargo dylibs, so it merges their original complete packed DWARF only
after each slice's UUID matches the final AU module. Neither UUIDs nor addresses
are rewritten. Every final plug-in process function address must resolve to a
positive line in its owned Rust source on both architectures; EQ additionally
requires `processor.rs`. This rejects partial dSYMs that retain a matching UUID
but have lost Rust compilation units after intermediate objects are removed.

Heron locks only the four VST3 runtimes and corresponding VST3 symbol assets,
including full SHA-256 digests and source commit. It verifies the download before
staging complete bundles, never rebuilding plugin source. Its release pipeline
uploads these symbols using Heron's own project credentials. AU/AAX downloads
serve other DAWs and remain outside Heron's built-in dependency lock.
