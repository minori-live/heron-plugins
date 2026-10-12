# Artifact contract

One suite version identifies all four plug-ins. The public producer repository
is `minori-live/heron-plugins`; a published release tag is `v<VERSION>`. Published
assets are immutable. A release has five runtime ZIP files and one macOS DMG
while AAX is disabled. The publisher requires this exact asset set and creates GitHub artifact
attestations for it.

Platform IDs are `windows-x64`, `linux-x64`, `linux-arm64` and
`macos-universal`. Runtime assets are
`heron-plugins-<version>-<platform>-<format>.zip`. Formats are `vst3`,
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

Heron locks the four VST3 runtimes, recording the suite version, full source
commit and four asset filenames. The consumer resolves the tag's commit and
requires a published stable release with all four runtime files. Older releases
remain usable through their runtime archives. Updates reject downgrades and changes to
the source commit or asset names of an already pinned version, including pending
upgrades.

Heron caches downloads by version, platform and filename. It validates archive
paths, manifests, complete bundle contents and actual executable architectures
before staging. Real VST3 probes verify exported identities and loading behavior.
Plug-in symbols are neither published nor uploaded to Sentry. Heron's own
application symbol uploads remain independent.
AU/AAX downloads serve other DAWs and remain outside Heron's built-in dependency
lock. See [ADR 0002](adr/0002-release-artifact-pinning.md) for pinning and
[ADR 0005](adr/0005-runtime-only-releases.md) for the runtime-only release contract.
