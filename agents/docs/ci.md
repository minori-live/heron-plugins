# CI and release operations

`CI` (`.github/workflows/ci.yml`) runs on pull requests, pushes to `main`, and
manual `workflow_dispatch`. It calls the reusable Test and Build workflows, then
reports their combined result through the stable `Gate` job. `Gate` succeeds only
when both workflows succeed. Configure the `Gate` check (shown under the `CI`
workflow) as the only required status check for pull requests so matrix job names
can change without breaking branch protection. The project has no website or
Pages deployment.

Test runs release-tooling tests, Clippy, Rust tests with coverage, historical
state compatibility and each plug-in's separate real-time allocation checks on
Linux x64, Windows x64 and macOS. Formatting, source lint, TypeScript, version and
dependency checks run once on Linux. Coverage provides the workspace Rust test
pass on each platform. Codecov merges the three platform reports.

Build packages VST3 on Windows x64, Linux x64, Linux arm64 and universal macOS;
macOS also packages AUv2 and a disk image. PR smoke builds disable LTO; tag
releases keep thin LTO. The shared setup
uses the locked mise toolchain, pnpm lockfile and pinned sccache version. Only
packaging jobs fetch the pinned VST3 SDK for bundle probes. Compiler-cache writes
come from main/tags; PRs read shared caches.
Check jobs cache Cargo targets, while packaging jobs cache downloads. Runtime
bundles and installers are retained as workflow artifacts for 90 days.

## Native artifact validation

The macOS packaging path links real VST3 MH_BUNDLE executables for both slices;
plain cross-target cargo-truce build output is insufficient for that boundary.
Before building, Cargo metadata resolves with `--locked --all-features`.
Formal releases require a clean committed source tree and lockfiles; the tree is
checked again after cargo-truce's internal builds. Dirty development builds are
smoke evidence and cannot be published as source-pinned releases.
Deployment target is macOS 14.2 and target CPU is baseline. Metadata stamping,
license insertion, universal linking and all executable changes happen before
signing.

After archiving, the runtime ZIP is unpacked and verified. Every VST3 bundle's
actual factory must expose the expected fixed class ID. Revox additionally runs
actual embedded-model pitch-shift rendering from the extracted bundle on every
platform and both macOS slices, including forced CPU fallback on macOS. All five AU components
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
identities, wrong versions and instance failures fail immediately. All five
instances must remain alive together before each architecture's `auval` checks.
Restoration also updates the directory timestamp. Validation preserves prior
installations and never deletes the user's global AU caches. Use a clean local
validation environment if an older cached suite reports a different version.

The shared Revox model job prepares and verifies the FCPE/vocoder graphs and
their license/provenance files before Test and Build. `scripts/revox-runtime.ts`
downloads SHA-256-pinned official ONNX Runtime 1.31.0 distributions for Linux and
Apple Silicon. Windows builds a CPU library with static MSVC CRT linkage and
checks its imports to reject an external Visual C++ runtime dependency. macOS
packaging builds the Intel CPU library from the same pinned upstream commit
`402979ac1bef89dcd55add41eaa7277c407599eb`, using Python 3.12, CMake 3.28 or newer
and the same macOS 14.2 deployment target, then combines both slices with `lipo`.
Windows selects the installed Visual Studio generator and native tools through
`vswhere`. Source builds disable
telemetry, Python bindings and upstream unit-test targets; final native model
probes exercise the shipped runtime. `REVOX_PYTHON` can select the build Python
executable explicitly; otherwise it uses `python` on Windows and `python3` on macOS.
The source build's dependencies remain pinned by upstream's release CMake files.
Model, runtime, license or native inference failures stop the build; they cannot
produce an archive containing only the other four plug-ins.

## Apple release credentials

Configure these repository secrets before a signed tag release:

| Secret                     | Purpose                                                         |
| -------------------------- | --------------------------------------------------------------- |
| `MAC_CSC_LINK`             | Base64 Developer ID Application PKCS#12 certificate/private key |
| `MAC_CSC_KEY_PASSWORD`     | PKCS#12 password                                                |
| `APPLE_API_KEY_P8`         | Complete App Store Connect `.p8` PEM private key or its Base64  |
| `APPLE_API_KEY_ID`         | API key ID                                                      |
| `APPLE_API_ISSUER`         | API issuer ID                                                   |
| `CODECOV_TOKEN`            | Coverage token, when needed by repository visibility/settings   |
| `HERON_PLUGINS_SYNC_TOKEN` | Producer-only scoped token for Heron upgrade branches and PRs   |
| `TAP_SYNC_TOKEN`           | Scoped token to notify `homebrew-tap` of stable releases        |

The API key must be a complete, unencrypted PKCS#8 EC private key, including
`BEGIN PRIVATE KEY` and `END PRIVATE KEY` lines. Raw PEM and strict standard
Base64 of that complete PEM are accepted; line endings are normalized. Partial
PEM, encrypted keys, another key type and malformed Base64 fail before keychain
mutation or compilation. The log identifies only `PEM` or `Base64`, never key
contents or detailed parser errors.

Apple credentials are imported into a temporary keychain and removed in an
always-run cleanup step. Before compiling, `notarytool store-credentials`
validates the key, key ID and issuer using the `heron-plugins-release` profile
in that temporary keychain. Authentication errors produce a fixed diagnostic;
submissions continue to use the private-key file directly. The pipeline requires a valid Developer ID Application
identity, accepted notarization, verified bundle seals and stapled DMG. Missing
credentials fail the release; ad-hoc development builds stay available.

To troubleshoot credentials before a full build, manually dispatch Build with
`validate_apple_credentials` enabled. This mode runs the same normalization,
certificate import and authentication preflight, then cleans up; it does not
package or publish artifacts. The original `v0.6.4` tag at `9b8c303` predates
raw-PEM support and still requires Base64 of the complete PEM. Success with a
newer credential-only workflow does not change that tag's credential handling.

## Publishing and synchronization

Synchronize VERSION/package/Cargo metadata and Cargo.lock before creating
`v<VERSION>`. The tag must match VERSION. Release runs Test and signed Build,
requires the exact asset set, creates public GitHub artifact attestations and
creates/updates a draft release. An already published release
is never overwritten. Review native validation evidence and publish the
draft explicitly.
GitHub Immutable Releases is enabled for this repository, so publishing locks
its assets and tag at the service boundary. Draft updates also require the exact
remote asset set, remove obsolete producer-owned draft attachments, and refuse
unexpected manual attachments. Publish only after the upload job has completed;
do not publish during a rerun. See
[GitHub's immutable release setting](https://docs.github.com/en/rest/repos/repos#enable-immutable-releases).

Publishing a stable release runs `update-heron.yml` in this producer repository
and `notify-homebrew-tap.yml` for the macOS Cask. Prereleases do not run either.
Configure `HERON_PLUGINS_SYNC_TOKEN` only here as a fine-grained PAT owned by
`minori-live`, selecting only `minori-live/heron` with Contents and Pull requests
read/write permissions. No Heron secret, dispatch receiver, Actions permission
or token forwarding is needed. The scoped PAT pushes the upgrade branch and
creates/updates the PR so normal Heron CI runs.

`TAP_SYNC_TOKEN` is a separate fine-grained PAT whose only selected repository
is `minori-live/homebrew-tap`, with Contents and Pull requests write. This
repository uses it only to dispatch `heron-plugins-release-published`. The Tap
owns version/digest validation, the Cask update and its release PR. See
[Homebrew distribution](homebrew-distribution.md).

The serialized job checks out the coordinator from producer main and the
consumer from trusted Heron main. Heron main must already contain the artifact
consumer and an initialized release lock; otherwise the job fails with a
bootstrap instruction. It never runs scripts from an upgrade branch. The job
reuses Heron's `pnpm plugins:update --latest` to resolve the stable release,
source commit and filenames, followed by `plugins:prepare` for the actual Linux
x64 runtime archive. Preparation validates its manifest, complete bundle set
and binary architecture. An existing pending
branch's lock participates in downgrade and same-version source/filename checks.
PR CI covers the remaining platforms and native integration.

The job maintains one rolling `dsh0416/update-heron-plugins` branch and PR. It
refuses unrelated pending edits, checks main/branch freshness, and updates with
an exact `--force-with-lease`. Identical pending commits are reused while the PR
title and body are reconciled independently, so retrying after a successful push
and failed PR creation recovers without another commit. A main update rebuilds
the lock-only branch from the current main; a main lock that already includes the
release supersedes any open upgrade PR. The PR requires manual merge. Heron's
application release controls when users receive the new built-in suite.

Upgrade-job failure never revokes or modifies a published plug-in release. After
bootstrap, credentials, API availability or branch freshness is repaired, run
`Maintain the Heron plug-in upgrade PR` manually with an empty version to resolve
the latest stable release. An explicit version is available for controlled
retries and still cannot downgrade main or a pending upgrade. Deploy this
workflow on producer main before publishing releases that should trigger it.

## AAX activation

`release-config.json` keeps `aax.enabled=false`. Enabling it requires a separate
review with the accepted compatible SDK version, redistribution
terms, PACE tools/cloud authorization and Avid developer-validator access.
Do not redistribute the licensed SDK archive or confidential SDK source.
The pinned Truce AAX descriptor hardcodes `version=1`; preflight rejects
activation until a reviewed minimal suite-version patch fixes the descriptor
while retaining ABI version 10. A source-adapter check does not prove licensed
AAX release readiness.

The SDK must include Interfaces/AAX.h and
Extensions/Topology/AAX_CMonolithicParameters.cpp, not just a partial interfaces
snapshot. Set `AAX_SDK_ARCHIVE` or authorized `AAX_SDK_URL`/`AAX_SDK_TOKEN`, and
`AAX_SDK_LICENSE_ACCEPTED=true`. The downloader rejects unsafe archive paths,
clears old extraction output and rejects missing SDK files. Configure Avid's full
DigiShell validator in `AAX_VALIDATOR`.

Mac/Windows AAX builds contain two actual modules per plug-in: the C++ wrapper
and the Rust sidecar. Validate installed final archives with
`cargo truce validate --aax --aax-tests full`. Each plug-in must
produce a nonzero passing test tally; missing-validator/load-only/skip fallbacks
are forbidden. Existing installations are restored afterward.

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
normal macOS DMG, and contributes two additional runtime ZIPs. The DMG
contains VST3 and AU; AAX uses separate ZIP downloads.
