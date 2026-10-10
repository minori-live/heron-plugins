import { cp, mkdir, readdir, writeFile } from "node:fs/promises"
import { dirname, join, basename, resolve } from "node:path"
import { root, run, target } from "./process.ts"

export async function stageLicenses(bundle: string, version: string, commit: string) {
  const resources = join(bundle, "Contents/Resources")
  await mkdir(resources, { recursive: true })
  await cp(join(root, "LICENSE"), join(resources, "LICENSE"))
  const destination = join(target, "dependency-licenses")
  await mkdir(destination, { recursive: true })
  const metadata = JSON.parse(
    run("cargo", ["metadata", "--locked", "--format-version", "1"], { capture: true })
  ) as {
    workspace_members: string[]
    packages: {
      id: string
      name: string
      version: string
      license: string | null
      license_file: string | null
      manifest_path: string
      repository: string | null
    }[]
  }
  const lines = [
    `Heron Plugins ${version}, source commit ${commit}`,
    "Copyright Heron contributors. GPL-3.0-only; see LICENSE.",
    "Corresponding source: https://github.com/minori-live/heron-plugins",
    "Each dependency remains licensed under its own terms. LicenseRef entries refer to the accompanying upstream license text.",
    ""
  ]
  for (const pkg of metadata.packages
    .filter((pkg) => !metadata.workspace_members.includes(pkg.id))
    .sort((a, b) => a.name.localeCompare(b.name))) {
    const directory = dirname(pkg.manifest_path)
    const candidates = pkg.license_file
      ? [pkg.license_file]
      : (await readdir(directory)).filter((name) =>
          /^(?:LICENSE|LICENCE|COPYING|NOTICE)(?:[.-]|$)/i.test(name)
        )
    if (!pkg.license && candidates.length === 0)
      throw new Error(`Missing dependency license: ${pkg.name}`)
    lines.push(
      `${pkg.name} ${pkg.version}: ${pkg.license ?? "see upstream license"}${pkg.repository ? ` (${pkg.repository})` : ""}`
    )
    const output = join(destination, `${pkg.name}-${pkg.version}`)
    await mkdir(output, { recursive: true })
    for (const file of candidates)
      await cp(resolve(directory, file), join(output, basename(file)), { recursive: true })
  }
  await cp(destination, join(resources, "licenses"), { recursive: true })
  await writeFile(join(resources, "THIRD_PARTY_NOTICES.txt"), `${lines.join("\n")}\n`)
}
