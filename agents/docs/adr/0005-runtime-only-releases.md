# ADR 0005: Runtime-only releases

- Status: Accepted
- Date: 2026-10-12
- Owners: project maintainers
- Scope: plug-in release assets and Heron's dependency lock
- Supersedes: symbol distribution in [ADR 0001](0001-independent-plugin-releases.md)
  and [ADR 0002](0002-release-artifact-pinning.md)

## Context

Plug-in Sentry integration is deferred. Symbol archives and their validation
pipeline were added before that integration was required.

## Decision

Publish five runtime ZIPs and one macOS DMG while AAX is disabled. Remove symbol
archive generation, publication, download and upload, together with the Sentry
CLI dependency and symbol-specific verification. Compiler-generated debug files
remain available for local debugging.

Heron locks only the four VST3 runtime archives. Existing version, source commit
and runtime filenames retain their identities; obsolete symbol entries in old
locks are ignored. Heron's own Sentry integration remains unchanged.

## Alternatives rejected

Uploading symbols directly to Sentry would introduce the deferred integration.
Retaining a symbol-distribution pipeline would keep work without a current
consumer.

## Consequences

Release builds require no Sentry credentials. Runtime identity, architecture,
signing, notarization and plug-in loading checks remain in place. Existing
published releases are not modified.

## Verification

Check the runtime-only release inventory and consumer lock normalization.
Existing native bundle probes and release signing checks retain their roles.

## Reconsider when

Plug-in diagnostics has an approved symbol-distribution requirement.
