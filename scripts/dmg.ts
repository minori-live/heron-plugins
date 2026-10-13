import { createHash } from "node:crypto"
import { createReadStream } from "node:fs"
import {
  cp,
  lstat,
  mkdir,
  mkdtemp,
  readdir,
  readlink,
  rm,
  rmdir,
  symlink,
  writeFile
} from "node:fs/promises"
import { join } from "node:path"
import { plugins, bundleName, type Format } from "./catalog.ts"
import { run, target } from "./process.ts"
import { notarize } from "./sign.ts"

async function fileHash(path: string) {
  const hash = createHash("sha256")
  for await (const chunk of createReadStream(path)) hash.update(chunk)
  return hash.digest("hex")
}

async function verifyTree(source: string, mounted: string): Promise<void> {
  const expected = await lstat(source)
  const actual = await lstat(mounted)
  if (expected.isSymbolicLink()) {
    if (!actual.isSymbolicLink() || (await readlink(source)) !== (await readlink(mounted)))
      throw new Error(`DMG symlink differs from staged bundle: ${mounted}`)
    return
  }
  if ((expected.mode & 0o111) !== (actual.mode & 0o111))
    throw new Error(`DMG executable permissions differ from staged bundle: ${mounted}`)
  if (expected.isDirectory()) {
    if (!actual.isDirectory()) throw new Error(`DMG directory missing: ${mounted}`)
    const names = (await readdir(source)).sort()
    if (JSON.stringify(names) !== JSON.stringify((await readdir(mounted)).sort()))
      throw new Error(`DMG directory entries differ from staged bundle: ${mounted}`)
    for (const name of names) await verifyTree(join(source, name), join(mounted, name))
  } else if (
    !expected.isFile() ||
    !actual.isFile() ||
    expected.size !== actual.size ||
    (await fileHash(source)) !== (await fileHash(mounted))
  ) {
    throw new Error(`DMG file differs from staged bundle: ${mounted}`)
  }
}

async function verifyDmgContents(output: string, source: string) {
  const mountpoint = await mkdtemp(join(target, "dmg-check-"))
  let attached = false
  try {
    run("hdiutil", ["attach", "-readonly", "-nobrowse", "-mountpoint", mountpoint, output])
    attached = true
    // Check all staged contents, allowing filesystem-owned metadata at the volume root.
    for (const name of await readdir(source))
      await verifyTree(join(source, name), join(mountpoint, name))
  } finally {
    if (attached) run("hdiutil", ["detach", mountpoint])
    // Never recursively remove a mountpoint, including after an attach/detach failure.
    await rmdir(mountpoint)
  }
  console.log(`Verified complete DMG bundles and installation shortcuts: ${output}`)
}

export async function createMacDmg(version: string, release: boolean, formats: Format[]) {
  const source = join(target, "dmg-stage")
  await rm(source, { recursive: true, force: true })
  await mkdir(source, { recursive: true })
  for (const format of formats) {
    const label = format === "au2" ? "Audio Units" : format === "aax" ? "AAX" : "VST3"
    const folder = join(source, label)
    await mkdir(folder)
    for (const plugin of plugins)
      await cp(
        join(
          target,
          "archive-check/macos-universal",
          format,
          "bundles",
          bundleName(plugin, format)
        ),
        join(folder, bundleName(plugin, format)),
        { recursive: true, verbatimSymlinks: true }
      )
    const install =
      format === "au2"
        ? "/Library/Audio/Plug-Ins/Components"
        : format === "aax"
          ? "/Library/Application Support/Avid/Audio/Plug-Ins"
          : "/Library/Audio/Plug-Ins/VST3"
    await symlink(install, join(source, `Install ${label}`))
  }
  await writeFile(
    join(source, "INSTALL.txt"),
    `Heron Plugins ${version}\n\nQuit your DAW. Open each format folder and drag its ${plugins.length} plug-in bundles into the matching Install shortcut. macOS may request an administrator password. For a per-user installation, copy VST3 bundles into ~/Library/Audio/Plug-Ins/VST3 and Audio Unit components into ~/Library/Audio/Plug-Ins/Components. Restart the DAW and rescan plug-ins.\n\nHeron installs its own pinned VST3 suite with application updates; use this disk image for other DAWs.\n`
  )
  const output = join(target, "release-assets", `heron-plugins-${version}-macos-universal.dmg`)
  await rm(output, { force: true })
  run("hdiutil", [
    "create",
    "-volname",
    `Heron Plugins ${version}`,
    "-srcfolder",
    source,
    "-format",
    "UDZO",
    output
  ])
  run("hdiutil", ["verify", output])
  if (release) {
    if (!process.env.TRUCE_SIGNING_IDENTITY)
      throw new Error("Missing Developer ID Application identity")
    run("codesign", ["--sign", process.env.TRUCE_SIGNING_IDENTITY, "--timestamp", output])
    notarize(output)
    run("xcrun", ["stapler", "staple", output])
    run("xcrun", ["stapler", "validate", output])
    run("codesign", ["--verify", "--strict", output])
    run("spctl", [
      "--assess",
      "--type",
      "open",
      "--context",
      "context:primary-signature",
      "--verbose=2",
      output
    ])
  }
  await verifyDmgContents(output, source)
  console.log(`Produced ${output}`)
}
