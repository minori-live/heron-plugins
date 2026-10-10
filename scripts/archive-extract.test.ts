import assert from "node:assert/strict"
import {
  chmod,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  readlink,
  rm,
  symlink,
  writeFile
} from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { test } from "node:test"
import { zipSync } from "fflate"
import { ZipFile } from "yazl"
import { createWriteStream } from "node:fs"
import { pipeline } from "node:stream/promises"
import { archiveTree, extractTree } from "./archive.ts"

test("published ZIP verification restores exact bytes, executable modes and internal symlinks", async (context) => {
  const workspace = await mkdtemp(join(tmpdir(), "heron-plugins-zip-"))
  context.after(() => rm(workspace, { recursive: true, force: true }))
  const source = join(workspace, "source")
  const bundle = join(source, "bundles/Heron EQ.vst3/Contents/MacOS")
  await mkdir(bundle, { recursive: true })
  const binary = Buffer.alloc(256 * 1024, 0x42)
  await writeFile(join(bundle, "Heron EQ"), binary)
  await chmod(join(bundle, "Heron EQ"), 0o755)
  const manifest = '{"schemaVersion":1,"version":"0.6.4"}\n'
  await writeFile(join(source, "bundle-manifest.json"), manifest)
  if (process.platform !== "win32") await symlink("Heron EQ", join(bundle, "current"))
  const archive = join(workspace, "Heron suite.zip")
  await archiveTree(source, archive)
  const unpacked = join(workspace, "unpacked")
  await extractTree(archive, unpacked)
  assert.deepEqual(
    await readFile(join(unpacked, "bundles/Heron EQ.vst3/Contents/MacOS/Heron EQ")),
    binary
  )
  assert.equal(await readFile(join(unpacked, "bundle-manifest.json"), "utf8"), manifest)
  if (process.platform !== "win32") {
    const executable = await lstat(join(unpacked, "bundles/Heron EQ.vst3/Contents/MacOS/Heron EQ"))
    assert.equal(executable.mode & 0o777, 0o755)
    assert.equal(
      await readlink(join(unpacked, "bundles/Heron EQ.vst3/Contents/MacOS/current")),
      "Heron EQ"
    )
  }
})

test("bundle ZIP verification rejects escaping paths, ambiguous names and existing destinations", async (context) => {
  const workspace = await mkdtemp(join(tmpdir(), "heron-plugins-zip-policy-"))
  context.after(() => rm(workspace, { recursive: true, force: true }))
  for (const [index, paths] of [
    ["../escaped"],
    ["/escaped"],
    ["C:/escaped"],
    ["bundles\\escaped"],
    ["a", "A"]
  ].entries()) {
    const archive = join(workspace, `unsafe-${index}.zip`)
    await writeFile(
      archive,
      zipSync(Object.fromEntries(paths.map((path) => [path, Buffer.from("bytes")])))
    )
    await assert.rejects(extractTree(archive, join(workspace, `output-${index}`)))
  }
  const archive = join(workspace, "valid.zip")
  await writeFile(archive, zipSync({ valid: Buffer.from("bytes") }))
  const occupied = join(workspace, "occupied")
  await mkdir(occupied)
  await writeFile(join(occupied, "retained"), "old")
  await assert.rejects(extractTree(archive, occupied), /empty destination/)
  assert.equal(await readFile(join(occupied, "retained"), "utf8"), "old")
})

test("archive links cannot make extraction write outside the verification directory", async (context) => {
  const workspace = await mkdtemp(join(tmpdir(), "heron-plugins-zip-links-"))
  context.after(() => rm(workspace, { recursive: true, force: true }))
  const archive = join(workspace, "unsafe.zip")
  const zip = new ZipFile()
  const finished = pipeline(zip.outputStream, createWriteStream(archive))
  zip.addBuffer(Buffer.from("../../escaped"), "bundles/link", { mode: 0o120777 })
  zip.addBuffer(Buffer.from("binary"), "bundles/link/binary", { mode: 0o100755 })
  zip.end()
  await finished
  await assert.rejects(extractTree(archive, join(workspace, "unpacked")), /Escaping bundle symlink/)
  await assert.rejects(lstat(join(workspace, "escaped")), { code: "ENOENT" })
})
