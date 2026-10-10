import { test } from "node:test"
import assert from "node:assert/strict"
import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { createHash } from "node:crypto"
import { archiveTree, safeSymlink } from "./archive.ts"
import { auVersion } from "./version.ts"
import { manifestPlugins, assetName } from "./catalog.ts"
import { verifySdkDigest } from "./aax.ts"
import { omitSdkSources } from "./source-bundles.ts"
import { unzipSync } from "fflate"

test("VST3 suite identities preserve saved Heron projects", () => {
  assert.deepEqual(
    manifestPlugins("vst3").map((entry) => [entry.builtinId, entry.nativeId]),
    [
      ["live.minori.heron.gain", "46774F504DF84B4AC1F308AB88DD3677"],
      ["live.minori.heron.eq", "8A8341D5CA36B6C9A9572788F40EBB9F"],
      ["live.minori.heron.sine", "C1351DFA4DDD4B4AC1F30896F6D9DF76"],
      ["live.minori.heron.metronome", "8CD16A11027ACC7FDF0C1419E86D1024"]
    ]
  )
  assert.equal(
    assetName("0.6.4", "macos-universal", "vst3", true),
    "heron-plugins-0.6.4-macos-universal-vst3-symbols.zip"
  )
  assert.deepEqual(
    manifestPlugins("au2").map((entry) => [entry.builtinId, entry.nativeId]),
    [
      ["live.minori.heron.gain", "aufx:HRGN:HRON"],
      ["live.minori.heron.eq", "aumf:HREQ:HRON"],
      ["live.minori.heron.sine", "aumu:HRSN:HRON"],
      ["live.minori.heron.metronome", "aumu:HRMT:HRON"]
    ]
  )
})

test("AU component version changes with the suite version and rejects overflow", () => {
  assert.equal(auVersion("0.6.4"), 1540)
  assert.equal(auVersion("1.2.3-rc.1"), 66051)
  assert.throws(() => auVersion("1.256.0"))
  assert.throws(() => auVersion("1.0.256"))
})

test("AAX SDK content must match the accepted pinned archive", () => {
  const bytes = Buffer.from("licensed SDK archive")
  verifySdkDigest(bytes, createHash("sha256").update(bytes).digest("hex"))
  assert.throws(() =>
    verifySdkDigest(Buffer.from("changed"), createHash("sha256").update(bytes).digest("hex"))
  )
  assert.throws(() => verifySdkDigest(bytes, "not-a-checksum"))
})

test("published source bundles omit confidential SDK content and its manifest entries", async () => {
  const bytes = await readFile(new URL("./fixtures/source-bundle.src.zip", import.meta.url))
  const filtered = omitSdkSources(bytes, "/licensed/AAX_SDK")
  const files = unzipSync(filtered)
  assert.deepEqual(Object.keys(files).sort(), ["files/workspace/plugin.rs", "manifest.json"])
  const manifest = JSON.parse(Buffer.from(files["manifest.json"]!).toString("utf8")) as {
    debug_id: string
    files: Record<string, unknown>
  }
  assert.equal(manifest.debug_id, "1b3afa8f-9b30-395b-af42-84dd8fcd3ca7")
  assert.deepEqual(Object.keys(manifest.files), ["files/workspace/plugin.rs"])
  assert.equal(filtered.includes(Buffer.from("SDK source fixture")), false)
  assert.throws(() => omitSdkSources(Buffer.from("unknown format"), "/licensed/AAX_SDK"))
})

test("bundle archives reject links escaping their published root", async () => {
  const directory = await mkdtemp(join(tmpdir(), "heron-plugin-archive-"))
  try {
    const source = join(directory, "stage")
    await mkdir(source)
    assert.equal(safeSymlink(join(source, "link"), "file", source), "file")
    assert.throws(() => safeSymlink(join(source, "link"), "../secret", source))
    assert.throws(() => safeSymlink(join(source, "link"), "/secret", source))
    if (process.platform !== "win32") {
      await symlink("../secret", join(source, "link"))
      await assert.rejects(archiveTree(source, join(directory, "unsafe.zip")))
    }
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test("published ZIP bytes are reproducible for the same bundle tree", async () => {
  const directory = await mkdtemp(join(tmpdir(), "heron-plugin-archive-"))
  try {
    const source = join(directory, "stage")
    await mkdir(join(source, "bundles"), { recursive: true })
    await writeFile(join(source, "bundle-manifest.json"), '{"schemaVersion":1}\n')
    await writeFile(join(source, "bundles/plugin"), "native binary fixture")
    await archiveTree(source, join(directory, "first.zip"))
    await archiveTree(source, join(directory, "second.zip"))
    assert.deepEqual(
      await readFile(join(directory, "first.zip")),
      await readFile(join(directory, "second.zip"))
    )
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})
