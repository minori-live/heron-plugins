import { cp, mkdir, rm, symlink, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { plugins, bundleName, type Format } from "./catalog.ts"
import { run, target } from "./process.ts"
import { notarize } from "./sign.ts"

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
  console.log(`Produced ${output}`)
}
