import { createHash } from "node:crypto"
import { appendFile, lstat, readFile, readdir, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { assetName, platforms, type Format } from "./catalog.ts"
import { aaxEnabled } from "./aax.ts"
import { readVersion } from "./version.ts"

if (import.meta.main) {
  const directory = process.argv[2]
  if (!directory) throw new Error("Usage: release-assets.ts <asset-directory>")
  const version = await readVersion()
  const expected: string[] = []
  for (const platform of platforms) {
    const formats: Format[] = platform === "macos-universal" ? ["vst3", "au2"] : ["vst3"]
    if ((await aaxEnabled()) && !platform.startsWith("linux")) formats.push("aax")
    for (const format of formats)
      for (const symbols of [false, true])
        expected.push(assetName(version, platform, format, symbols))
  }
  expected.push(`heron-plugins-${version}-macos-universal.dmg`)
  expected.sort()
  const actual = (await readdir(directory)).sort()
  if (JSON.stringify(actual) !== JSON.stringify(expected))
    throw new Error(
      `Incomplete release asset set: expected ${expected.join(", ")}; got ${actual.join(", ")}`
    )
  const checksums: string[] = []
  for (const name of expected) {
    const path = join(directory, name)
    if (!(await lstat(path)).isFile())
      throw new Error(`Release asset is not a regular file: ${name}`)
    checksums.push(
      `${createHash("sha256")
        .update(await readFile(path))
        .digest("hex")}  ${name}`
    )
  }
  await writeFile(join(directory, "SHA256SUMS"), `${checksums.join("\n")}\n`)
  if (process.env.GITHUB_OUTPUT)
    await appendFile(
      process.env.GITHUB_OUTPUT,
      `subjects<<ASSETS\n${[...expected, "SHA256SUMS"].map((name) => join(directory, name)).join("\n")}\nASSETS\nasset_names=${JSON.stringify([...expected, "SHA256SUMS"])}\n`
    )
}
