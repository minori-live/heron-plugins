# CI and release operations

`CI` runs reusable Test and Build workflows for pushes to main and pull requests.
The stable required check is `Gate`; it succeeds only when both workflows pass.
The project has no website or Pages deployment.

Test runs Linux x64, Windows x64 and macOS: formatting, lint, TypeScript and
artifact-contract checks, Rust fmt/Clippy/tests, historical state compatibility,
and each plug-in's separate real-time allocation checks. Rust coverage is
uploaded with the three Heron-compatible platform flags, then Codecov publishes
merged statuses. A full optional-adapter check does not imply licensed AAX
wrapper validation.

Build packages VST3 on Windows x64, Linux x64, Linux arm64 and universal macOS;
macOS also packages AUv2 and a disk image. PR smoke builds disable LTO but retain
full matching debug information. Tag releases keep thin LTO. The shared setup
uses the locked mise toolchain and pnpm lockfile, pinned VST3 SDK and sccache
version. Compiler-cache writes come from main/tags; PRs read shared caches.
Check jobs cache Cargo targets, while packaging jobs cache downloads. Bundles
and symbols are retained as workflow artifacts for 90 days.

## Native artifact validation

The macOS packaging path links real VST3 MH_BUNDLE executables for both slices;
plain cross-target cargo-truce build output is insufficient for that boundary.
Before building, Cargo metadata resolves with `--locked --all-features`.
Formal releases require a clean committed source tree and lockfiles; the tree is
checked again after cargo-truce's internal builds. Dirty development builds are
smoke evidence and cannot be published as source-pinned releases.
Deployment target is macOS 14.2 and target CPU is baseline. Metadata stamping,
license insertion, universal linking and all executable changes happen before
signing. VST3 symbols are generated from each final linked MH_BUNDLE, because its
UUID differs from the original Cargo dylib. AU symbols preserve Cargo's packed
DWARF: packaging only copies/lipos those dylibs, and every original slice must
match the final module UUID before merging. No UUID/address rewriting is allowed.
Both formats must resolve the actual final process-function address to an owned
Rust source line on each architecture, including EQ's `processor.rs`. A matching
UUID or generic debug-information flag alone cannot establish completeness.

After archiving, the runtime ZIP is unpacked and verified. Every VST3 bundle's
actual factory must expose the expected fixed class ID. All four AU components
must have unique per-plugin Objective-C editor/container classes in both final
Mach-O slices. They are registered together, checked for identity/version and instantiated, then
`auval` must succeed under both arm64 and Rosetta x86_64. Validation restores any
pre-existing suite installation. The DMG contains complete format bundles and
installation shortcuts. Its signed release is notarized, stapled and checked by
Gatekeeper. ZIPs are notarized and distributed unchanged; their Apple ticket is
resolved online because ZIP files cannot be stapled.

AU validation copies each complete bundle outside the scanned Components folder,
then atomically installs it and updates the Components directory timestamp.
[Apple documents that directory timestamp changes refresh the cached registry](https://developer.apple.com/library/archive/documentation/MusicAudio/Conceptual/AudioUnitProgrammingGuide/AudioUnitDevelopmentFundamentals/AudioUnitDevelopmentFundamentals.html).
Fresh probe processes allow up to 30 seconds for missing registrations; duplicate
identities, wrong versions and instance failures fail immediately. All four
instances must remain alive together before each architecture's `auval` checks.
Restoration also updates the directory timestamp. Validation preserves prior
installations and never deletes the user's global AU caches. Use a clean local
validation environment if an older cached suite reports a different version.

Automated identity/load/auval checks do not exercise the editor inside Logic.
Before publishing the draft, manually test installation, editor open/close,
automation, preset/state restore, project reopen, offline bounce and MIDI
metronome notes in the supported DAWs. Preserve the result in release notes;
artifact attestations prove the producing workflow and bytes, not manual DAW QA.

## Apple release credentials

Configure these repository secrets before a signed tag release:

| Secret                     | Purpose                                                         |
| -------------------------- | --------------------------------------------------------------- |
| `MAC_CSC_LINK`             | Base64 Developer ID Application PKCS#12 certificate/private key |
| `MAC_CSC_KEY_PASSWORD`     | PKCS#12 password                                                |
| `APPLE_API_KEY_P8`         | Base64 App Store Connect API private key                        |
| `APPLE_API_KEY_ID`         | API key ID                                                      |
| `APPLE_API_ISSUER`         | API issuer ID                                                   |
| `CODECOV_TOKEN`            | Coverage token, when needed by repository visibility/settings   |
| `HERON_PLUGINS_SYNC_TOKEN` | Scoped token for dispatching and opening Heron upgrade PRs      |

Apple credentials are imported into a temporary keychain and removed in an
always-run cleanup step. The pipeline requires a valid Developer ID Application
identity, accepted notarization, verified bundle seals and stapled DMG. Missing
credentials fail the release; ad-hoc development builds stay available.
No plugin-side Sentry DSN, organization/project, upload token or runtime capture
is configured in this release. The local Sentry CLI only checks debug files and
generates source bundles; it does not make telemetry or upload requests.

## Publishing and synchronization

Synchronize VERSION/package/Cargo metadata and Cargo.lock before creating
`v<VERSION>`. The tag must match VERSION. Release runs Test and signed Build,
requires the exact asset set, writes SHA256SUMS, creates public GitHub artifact
attestations and creates/updates a draft release. An already published release
is never overwritten. Review native/manual validation evidence and publish the
draft explicitly.
GitHub Immutable Releases is enabled for this repository, so publishing locks
its assets and tag at the service boundary. Draft updates also require the exact
remote asset set, remove obsolete producer-owned draft attachments, and refuse
unexpected manual attachments. Publish only after the upload job has completed;
do not publish during a rerun. See
[GitHub's immutable release setting](https://docs.github.com/en/rest/repos/repos#enable-immutable-releases).

Publishing a stable release sends `heron-plugins-release-published` to
`minori-live/heron` with `client_payload.version`. Prereleases do not dispatch.
Configure the same `HERON_PLUGINS_SYNC_TOKEN` in both repositories, scoped to
Heron contents and pull-request write permissions. The receiver uses that token
to push its dependency branch and create a normal CI-triggering PR. Heron
resolves the latest complete stable release, preventing delayed notifications
from rolling the dependency backward. Its own application release controls when
users receive the new built-in suite.

## AAX activation

`release-config.json` keeps `aax.enabled=false`. Enabling it requires a separate
review with the accepted compatible SDK version and SHA-256, redistribution
terms, PACE tools/cloud authorization and Avid developer-validator access.
Do not redistribute the licensed SDK archive or confidential SDK source.
The pinned Truce AAX descriptor hardcodes `version=1`; preflight rejects
activation until a reviewed minimal suite-version patch fixes the descriptor
while retaining ABI version 10. A source-adapter check does not prove licensed
AAX release readiness.

The SDK must include Interfaces/AAX.h and
Extensions/Topology/AAX_CMonolithicParameters.cpp, not just a partial interfaces
snapshot. Set `AAX_SDK_ARCHIVE` or authorized `AAX_SDK_URL`/`AAX_SDK_TOKEN`, and
`AAX_SDK_LICENSE_ACCEPTED=true`. The downloader checks the pinned archive digest,
rejects unsafe archive paths, clears old extraction output and rejects missing
SDK files. Configure Avid's full DigiShell validator in `AAX_VALIDATOR`.

Mac/Windows AAX builds contain two actual modules per plug-in: the C++ wrapper
and the Rust sidecar. Both need matching full debug files; macOS needs both
architectures for each. Wrapper CXXFLAGS include debug information and Windows
linking requires full PDB generation. Activation must verify actual wrapper
PDB/DWARF IDs and source redistribution handling, then validate installed final
archives with `cargo truce validate --aax --aax-tests full`. Each plug-in must
produce a nonzero passing test tally; missing-validator/load-only/skip fallbacks
are forbidden. Existing installations are restored afterward.
Source generation removes licensed SDK files and their manifest entries by
default (`publishSdkSources=false`), then verifies the rewritten bundles' debug
IDs. Changing that setting requires explicit SDK source redistribution rights.
Debug extraction happens before the PACE seal; IDs are checked again after
signing, and bundles are never re-signed afterward.

Release requires `PACE_ACCOUNT`, `PACE_SIGN_ID` and
`PACE_SIGNING_SERVICE_READY=true` on a provisioned runner. The signing command
uses Truce's supported `wraptool sign --allowsigningservice` path. Obtain the
accepted SDK's official PACE service setup, cloud authorization/session lifecycle
and signature-verification instructions before activating this job; private
tool installation/session commands must be implemented from that documentation.
Do not infer them from community snippets. Verify retail Pro Tools load after
PACE signing; Pro Tools Developer's unsigned-load capability is development
evidence only. Do not modify or Apple-resign bundles after the PACE seal.
The separate AAX release job is disabled by source configuration initially and
runs only for tag releases on `heron-plugins-macos-aax` and
`heron-plugins-windows-aax` self-hosted runners. Configure `AAX_SDK_URL`, optional
`AAX_SDK_TOKEN`, `PACE_ACCOUNT`, `PACE_SIGN_ID_MACOS` and
`PACE_SIGN_ID_WINDOWS` secrets, plus `AAX_SDK_LICENSE_ACCEPTED`,
`AAX_VALIDATOR_MACOS`, `AAX_VALIDATOR_WINDOWS` and
`PACE_SIGNING_SERVICE_READY` variables after official onboarding. Normal package
jobs use `--no-aax`; the dedicated job uses `--only-aax`, cannot overwrite the
normal macOS DMG, and contributes four additional runtime/symbol ZIPs. The DMG
contains VST3 and AU; AAX uses separate ZIP downloads.
