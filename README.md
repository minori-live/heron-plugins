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
image. AAX remains explicitly disabled until its SDK, signing service and Pro
Tools validation are ready.

Download the runtime ZIP for your platform and format, then install the complete
bundles from its `bundles/` directory in your DAW's plug-in location. macOS users
can use the DMG's format folders and installation shortcuts. Quit and restart
the DAW after installing. Heron bundles its own checksum-pinned VST3 release and
upgrades it with application updates.

## Plug-in screenshots

Native editors on macOS. Gain, Sine and Metronome show their default settings;
EQ shows a three-band example with the spectrum display.

### Heron Gain

<img src="assets/screenshots/heron-gain.png" alt="Heron Gain editor with gain control and stereo output meters" width="380">

### Heron EQ

<img src="assets/screenshots/heron-eq.png" alt="Heron EQ editor with three EQ bands, a spectrum display and selected-band controls" width="1040">

### Heron Sine

<img src="assets/screenshots/heron-sine.png" alt="Heron Sine editor with output, attack and release controls" width="520">

### Heron Metronome

<img src="assets/screenshots/heron-metronome.png" alt="Heron Metronome editor with output, accent tone, beat tone and decay controls" width="600">

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

`mise run build` creates development bundle archives in `target/release-assets/`.
macOS builds and validates both Intel and Apple Silicon slices; Intel validation
on Apple Silicon requires Rosetta. Development macOS
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

GitHub downloads contain runtime bundles and installers. Plug-ins do not
publish symbols or integrate with Sentry.

See [CI and release operations](agents/docs/ci.md),
[the artifact contract](agents/docs/artifacts.md) and
[the migration decision](agents/docs/adr/0001-independent-plugin-releases.md).
The [EQ implementation notes](agents/docs/heron-eq.md) preserve its DSP, editor
and compatibility contracts.
Licensed under [GPL-3.0-only](LICENSE); bundles include third-party notices.

## Heron Revox development build

Heron Revox is a new vocal resynthesis and pitch-shifting effect using FCPE and
Revox Vocoder 1.5. It has its own VST3/AUv2 identity and is currently packaged
separately from the four-plug-in release suite above. The development packaging
entry point supports Apple Silicon macOS 14.2 or later. Intel macOS has no
matching ORT prebuilt archive; Windows and Linux plug-in bundles have not yet
been validated.

Acquire and export the pinned models using the commands in the
[Revox development notes](agents/docs/environment.md), then run:

```sh
pnpm build:revox
pnpm build:revox --au
```

The default builds VST3; `--au` also builds an AUv2 component and runs `auval`.
The ZIP and verified bundles are under `target/revox/bundles/` and
`target/revox/archive-check/macos-arm64/bundles/`. Copy the complete VST3 bundle
to `~/Library/Audio/Plug-Ins/VST3/` or the AU component to
`~/Library/Audio/Plug-Ins/Components/`, then restart the DAW. These development
bundles use ad-hoc signing and are not notarized release downloads.

The inference graphs are embedded; Python and model downloads are unnecessary
on the playback machine. Fixed processing windows automatically use batch 1 for
mono or batch 2 for stereo, preserving each channel without additional batching
delay. Each bundle includes the WebGPU Dawn library, FCPE's
MIT license, the Revox model provenance using its GPL-3.0-only option, and the
ONNX Runtime/Dawn license notices. AAX remains disabled by the suite's existing
licensed-SDK and signing requirements.
