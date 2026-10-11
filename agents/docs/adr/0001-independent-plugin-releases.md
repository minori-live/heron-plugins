# ADR 0001: Independent plugin suite releases

Status: accepted

Artifact pinning is updated by
[ADR 0002](0002-release-artifact-pinning.md).

Heron Gain, EQ, Sine, Metronome and their shared UI change more frequently than the
DAW. Building them from Heron's workspace couples iteration, build cost and
release ownership to the desktop application.

Move the complete plugin workspace into `minori-live/heron-plugins` with one
suite version, independently locked build tools, checks, coverage and GitHub
Releases. Keep the existing vendor/bundle identities, VST3 IDs, parameter
contracts, non-default state fixtures and DSP behavior. Heron's scheduler and
plugin host remain in Heron. Runtime archives preserve complete native bundles,
not reconstructed executable-only bundles.

The initial suite is version 0.6.4 independently of Heron's application version.
Its migration includes Heron 0.6.5's EQ and the existing pinned GUI patches.
EQ historical state evidence comes from the original Heron 0.6.5 implementation;
the suite version does not change that fixture's provenance.

Publish VST3 for four platforms, AUv2 for universal macOS and a macOS installation
DMG. AAX is disabled behind explicit SDK/signing/validation gates. Heron pins
released VST3 runtime and symbol assets by version, commit and filename and
stages them for tests/packaging. This producer owns a serialized workflow that
maintains one Heron upgrade PR after a stable release or manual retry. It executes
only trusted Heron main's artifact updater and consumer, preserves main/pending
version, source-commit and asset-name guards, and uses a scoped producer-only PAT for branch and
PR writes. Heron owns the validation contract but has no dispatch receiver or
synchronization credential. Upgrades require manual merge; users receive the
suite with Heron app updates. Other DAWs can install standalone release downloads.

Generate symbols and source bundles per format from final linked modules, check
matching debug IDs and archive them separately. This release introduces no
plugin-side Sentry setup, upload or runtime reporting. Heron uploads its pinned
VST3 symbols to its existing project so host crash reports remain symbolicated.
Any future isolated error-reporting client, consent policy or fatal-handler
integration requires a separate decision that preserves real-time and host
ownership boundaries.

CI follows Heron's tool/cache/coverage/Gate conventions without a website.
Formal macOS releases require Developer ID signing, notarization and a stapled
installation DMG. Required bundle validation runs against unpacked release ZIPs
and both macOS architectures. Published release assets are immutable.
