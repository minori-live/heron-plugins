# ADR 0002: Release artifact pinning

- Status: Accepted
- Date: 2026-10-11
- Owners: project maintainers
- Scope: producer release assets and Heron's VST3 dependency lock; updates the
  artifact-pinning part of [ADR 0001](0001-independent-plugin-releases.md)

## Context

Heron consumes complete bundles from immutable GitHub releases. A separate
checksum catalog, per-asset lock digests and digest-keyed caches duplicate the
release inventory across producer and consumer. The contract uses
version, source commit and filenames, with validation at the native artifact
boundary.

## Decision

While AAX is disabled, publish exactly ten runtime/symbol ZIPs and one macOS DMG.
Keep GitHub artifact attestations, Apple signing/notarization and immutable
published releases. Generate no separate checksum catalog.

Heron's lock records the suite version, full source commit and filenames of the
four VST3 runtime archives and their matching symbol archives. Updates require a
published stable release, resolve the tag's source commit and reject downgrades
or changed source commits/filenames for the same version, including pending
upgrades. Cache entries use version, platform and filename.

Before staging, validate archive paths, schema, version/commit/platform,
complete bundle identities and actual executable architectures. Real VST3
probes establish exported identities and loading behavior. Symbols retain
matching Debug IDs for every final architecture slice. AAX retains its pinned
SDK version, authorized download/archive input, license and validator/PACE
activation gates.

Package-manager and tool-generated integrity metadata, historical state fixture
provenance, vendored source provenance and native Debug IDs keep their existing
roles.

## Consequences

The producer and consumer maintain one release-file contract. Archive/native
validation catches malformed, incompatible or incomplete bundles; the consumer
no longer compares downloads against an independent per-asset digest.
Published assets and tags must remain immutable. Existing projects, parameters
and native identities retain their compatibility commitments.

## Verification

Check the exact release asset set and consumer lock update behavior. Exercise
cache reuse, malformed archives, manifest/platform mismatches, final native
probes and runtime/symbol Debug ID matching. Signed macOS release bundles still
require notarization and dual-architecture AU validation.
