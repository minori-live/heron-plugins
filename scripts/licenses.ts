import { createHash } from "node:crypto"
import { appendFile, cp, mkdir, readFile, readdir, writeFile } from "node:fs/promises"
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

export async function stageRevoxModelNotices(bundle: string) {
  const resources = join(bundle, "Contents/Resources")
  const licenses = join(resources, "licenses/revox-models-and-runtime")
  await mkdir(licenses, { recursive: true })
  // These files are produced by the checksum-verifying model preparation jobs.
  // Missing provenance fails packaging just like a missing embedded graph.
  await cp(join(root, "scripts/revox/licenses"), licenses, { recursive: true })
  await cp(join(target, "revox/fcpe-upstream/LICENSE"), join(licenses, "FCPE-LICENSE"))
  await cp(join(root, "scripts/revox/fcpe-source.json"), join(licenses, "fcpe-source.json"))
  for (const name of ["fcpe-export.json", "fcpe-plugin-export.json"])
    await cp(join(target, "revox", name), join(licenses, name))
  for (const name of ["revox-README.md", "revox-manifest.json", "revox-plugin-manifest.json"])
    await cp(join(target, "revox/models", name), join(licenses, name))
  const modelLicenseHash = createHash("sha256")
    .update(await readFile(join(licenses, "revox-README.md")))
    .digest("hex")
  if (modelLicenseHash !== "30fa86773fd71296deb16e92dcc19eb7409498e5bc8d6f39f615d45b50dd6ec5")
    throw new Error("Revox model license evidence does not match the pinned dual-license README")
  await writeFile(
    join(licenses, "license-selection.json"),
    `${JSON.stringify(
      {
        model: "Revox Vocoder 1.5",
        selectedLicense: "GPL-3.0-only",
        revision: "59a27b71358a2de4b22acda1b07d89d02bf0de79",
        source:
          "https://huggingface.co/minori-live/revox-vocoder-1/blob/59a27b71358a2de4b22acda1b07d89d02bf0de79/README.md",
        readmeSha256: modelLicenseHash,
        note: "The pinned README offers GPL-3.0-only as an alternative; the unmodified upstream manifest still records CC-BY-NC-SA-4.0. This GPL suite selects the README's GPL-3.0-only option."
      },
      null,
      2
    )}\n`
  )
  await appendFile(
    join(resources, "THIRD_PARTY_NOTICES.txt"),
    "\nFCPE checkpoint and source: MIT; see licenses/revox-models-and-runtime/FCPE-LICENSE.\n" +
      "Revox Vocoder 1.5: GPL-3.0-only licensing option selected; see LICENSE and the model README.\n" +
      "ONNX Runtime 1.31.0: MIT; see accompanying runtime license, third-party notices and runtime-source.json.\n" +
      "Both inference graphs are embedded in the plug-in executable; Python and model downloads are not required at runtime.\n"
  )
}
