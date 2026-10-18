import { test } from "node:test"
import assert from "node:assert/strict"
import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { archiveTree, safeSymlink } from "./archive.ts"
import { auVersion } from "./version.ts"
import { manifestPlugins, assetName, bundleName, revox, repitch } from "./catalog.ts"

test("VST3 suite identities preserve saved Heron projects", () => {
  assert.deepEqual(
    manifestPlugins("vst3").map((entry) => [entry.builtinId, entry.nativeId]),
    [
      ["live.minori.heron.gain", "46774F504DF84B4AC1F308AB88DD3677"],
      ["live.minori.heron.eq", "8A8341D5CA36B6C9A9572788F40EBB9F"],
      ["live.minori.heron.sine", "C1351DFA4DDD4B4AC1F30896F6D9DF76"],
      ["live.minori.heron.metronome", "8CD16A11027ACC7FDF0C1419E86D1024"],
      ["live.minori.heron.revox", "801E1EF7D1FBA46DD7EE039C0317622C"],
      ["live.minori.heron.repitch", "B3C6C5BCC36E4A1173BC693A14112A03"]
    ]
  )
  assert.equal(
    assetName("0.6.4", "macos-universal", "vst3"),
    "heron-plugins-0.6.4-macos-universal-vst3.zip"
  )
  assert.deepEqual(
    manifestPlugins("au2").map((entry) => [entry.builtinId, entry.nativeId]),
    [
      ["live.minori.heron.gain", "aufx:HRGN:HRON"],
      ["live.minori.heron.eq", "aumf:HREQ:HRON"],
      ["live.minori.heron.sine", "aumu:HRSN:HRON"],
      ["live.minori.heron.metronome", "aumu:HRMT:HRON"],
      ["live.minori.heron.revox", "aufx:HRRV:HRON"],
      ["live.minori.heron.repitch", "aufx:HRRP:HRON"]
    ]
  )
})

test("Revox preserves its development identity in the released suite", () => {
  assert.equal(revox.builtinId, "live.minori.heron.revox")
  assert.equal(revox.nativeId, "801E1EF7D1FBA46DD7EE039C0317622C")
  assert.equal(`${revox.auType}:${revox.fourcc}:HRON`, "aufx:HRRV:HRON")
  assert.equal(bundleName(revox, "vst3"), "Heron Revox.vst3")
  assert.equal(bundleName(revox, "au2"), "Heron Revox.component")
  assert.throws(() => manifestPlugins("aax"), /complete release suite/)
})

test("Repitch preserves its development identity in the released suite", () => {
  assert.equal(repitch.builtinId, "live.minori.heron.repitch")
  assert.equal(repitch.nativeId, "B3C6C5BCC36E4A1173BC693A14112A03")
  assert.equal(`${repitch.auType}:${repitch.fourcc}:HRON`, "aufx:HRRP:HRON")
  assert.equal(bundleName(repitch, "vst3"), "Heron Repitch.vst3")
  assert.equal(bundleName(repitch, "au2"), "Heron Repitch.component")
})

test("AU component version changes with the suite version and rejects overflow", () => {
  assert.equal(auVersion("0.6.4"), 1540)
  assert.equal(auVersion("1.2.3-rc.1"), 66051)
  assert.throws(() => auVersion("1.256.0"))
  assert.throws(() => auVersion("1.0.256"))
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
