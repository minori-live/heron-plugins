# Heron Plugins

Heron Gain, Heron EQ, Heron Sine and Heron Metronome are independently released plug-ins
for Heron and other DAWs. The suite shares one version and preserves the existing
Heron VST3 identities, parameter IDs and saved states.

| Plug-in         | Role                                                     |
| --------------- | -------------------------------------------------------- |
| Heron Gain      | Gain effect                                              |
| Heron EQ        | Parametric equalizer with spectrum display               |
| Heron Sine      | MIDI sine synthesizer                                    |
| Heron Metronome | MIDI click instrument; note 84 accents and note 72 beats |

GitHub Releases provide VST3 bundles for Windows x64, Linux x64, Linux arm64 and
universal macOS. macOS also includes AUv2 components and an installation disk
image. Each format has a separate debug-symbol archive. AAX remains explicitly
disabled until its SDK, signing service and Pro Tools validation are ready.

Download the runtime ZIP for your platform and format, then install the complete
bundles from its `bundles/` directory in your DAW's plug-in location. macOS users
can use the DMG's format folders and installation shortcuts. Quit and restart
the DAW after installing. Heron bundles its own checksum-pinned VST3 release and
upgrades it with application updates.

## Development

Use a login shell and the locked `mise` toolchain. On Windows, use PowerShell 7.
Follow [the development environment](agents/docs/environment.md) for platform
prerequisites and [the engineering and testing standards](agents/docs/README.md)
for implementation and review. The suite maintains these rules locally.

```sh
mise install
mise run install
mise run sdk:vst3
mise run check
mise run build
```

`mise run build` creates development bundles, symbols and archives in
`target/release-assets/`. macOS builds and validates both Intel and Apple Silicon
slices; Intel validation on Apple Silicon requires Rosetta. Development macOS
bundles use ad-hoc signing. `node scripts/build.ts --release` requires valid Apple
release credentials and produces signed, notarized macOS downloads.

Change `VERSION`, then run `pnpm version:sync`, `cargo check --workspace` and
`pnpm version:check` to synchronize suite metadata and the Cargo lockfile before
tagging. Tag pushes run all checks and create a draft GitHub Release. Publishing
a stable release runs this repository's workflow to validate the release using
Heron's artifact consumer and maintain one manually merged Heron upgrade PR.
The scoped synchronization token lives only in this repository. If Heron main
is not yet bootstrapped or an upgrade attempt fails, retry the workflow manually;
the published plug-in release remains available.

There is no plugin-side Sentry reporting or upload configuration in this
release. Symbol/source archives are generated offline; Heron's release pipeline
uploads its locked VST3 symbols to Heron's existing crash-reporting project.

See [CI and release operations](agents/docs/ci.md),
[the artifact contract](agents/docs/artifacts.md) and
[the migration decision](agents/docs/adr/0001-independent-plugin-releases.md).
The [EQ implementation notes](agents/docs/heron-eq.md) preserve its DSP, editor
and compatibility contracts.
Licensed under [GPL-3.0-only](LICENSE); bundles include third-party notices.
