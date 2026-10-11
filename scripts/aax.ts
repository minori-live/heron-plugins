import {
  cp,
  mkdir,
  readFile,
  readdir,
  stat,
  writeFile,
  rm,
  rename,
  lstat,
  realpath
} from "node:fs/promises"
import { join, dirname } from "node:path"
import { tmpdir } from "node:os"
import { root, run } from "./process.ts"
import { bundleName, plugins } from "./catalog.ts"

export interface AaxConfig {
  enabled: boolean
  sdkVersion: string | null
}
export async function aaxConfig(): Promise<AaxConfig> {
  const config = JSON.parse(await readFile(join(root, "release-config.json"), "utf8")) as {
    aax: AaxConfig
  }
  return config.aax
}
export async function aaxEnabled() {
  return (await aaxConfig()).enabled
}

async function sdkRoot(directory: string): Promise<string> {
  try {
    await stat(join(directory, "Interfaces/AAX.h"))
    await stat(join(directory, "Extensions/Topology/AAX_CMonolithicParameters.cpp"))
    return directory
  } catch {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        try {
          return await sdkRoot(join(directory, entry.name))
        } catch {
          /* Try the next SDK directory. */
        }
      }
    }
    throw new Error("AAX archive does not contain the complete compatible SDK")
  }
}

export async function prepareAax(release: boolean) {
  const config = await aaxConfig()
  if (!config.enabled) throw new Error("AAX is disabled")
  if (!config.sdkVersion) throw new Error("Pin the accepted AAX SDK version before enabling AAX")
  const metadata = JSON.parse(
    run("cargo", ["metadata", "--locked", "--format-version", "1"], { capture: true })
  ) as { packages: { name: string; manifest_path: string }[] }
  const adapter = metadata.packages.find((pkg) => pkg.name === "truce-aax")
  if (!adapter) throw new Error("Missing pinned AAX adapter")
  const adapterSource = await readFile(join(dirname(adapter.manifest_path), "src/lib.rs"), "utf8")
  if (/let descriptor = TruceAaxDescriptor\s*\{[\s\S]*?version:\s*1\s*,/.test(adapterSource))
    throw new Error(
      "AAX activation requires the reviewed suite-version descriptor patch; preserve ABI version 10"
    )
  if (process.env.AAX_SDK_LICENSE_ACCEPTED !== "true")
    throw new Error("AAX_SDK_LICENSE_ACCEPTED is required for the selected SDK license")
  const archive = join(tmpdir(), "heron-plugins-aax-sdk.zip")
  const source = process.env.AAX_SDK_ARCHIVE
  let bytes: Buffer
  if (source) bytes = await readFile(source)
  else {
    if (!process.env.AAX_SDK_URL) throw new Error("Provide AAX_SDK_ARCHIVE or AAX_SDK_URL")
    const response = await fetch(process.env.AAX_SDK_URL, {
      signal: AbortSignal.timeout(120_000),
      headers: process.env.AAX_SDK_TOKEN
        ? { Authorization: `Bearer ${process.env.AAX_SDK_TOKEN}` }
        : {}
    })
    if (!response.ok) throw new Error(`AAX SDK download failed (${response.status})`)
    bytes = Buffer.from(await response.arrayBuffer())
  }
  await writeFile(archive, bytes)
  const entries = run("tar", ["-tf", archive], { capture: true }).trim().split("\n")
  if (
    entries.some(
      (entry) => entry.startsWith("/") || entry.includes("\\") || entry.split("/").includes("..")
    )
  )
    throw new Error("Unsafe AAX SDK archive paths")
  const directory = join(tmpdir(), `heron-plugins-aax-sdk-${config.sdkVersion}`)
  await rm(directory, { recursive: true, force: true })
  await mkdir(directory, { recursive: true })
  run("tar", ["-xf", archive, "-C", directory])
  process.env.AAX_SDK_PATH = await realpath(await sdkRoot(directory))
  if (!process.env.AAX_VALIDATOR)
    throw new Error(
      "AAX_VALIDATOR must select Avid's full validator; load-only fallback is prohibited"
    )
  if (release) {
    for (const name of ["PACE_ACCOUNT", "PACE_SIGN_ID"])
      if (!process.env[name]) throw new Error(`AAX release requires ${name}`)
    if (process.env.PACE_SIGNING_SERVICE_READY !== "true")
      throw new Error(
        "Provision the official PACE signing service and cloud authorization before enabling AAX"
      )
    run("wraptool", ["--version"])
  }
}

export async function signAax(stage: string) {
  const account = process.env.PACE_ACCOUNT!
  for (const plugin of plugins) {
    const bundle = join(stage, "bundles", bundleName(plugin, "aax"))
    run("wraptool", [
      "sign",
      "--account",
      account,
      "--signid",
      process.env.PACE_SIGN_ID!,
      "--allowsigningservice",
      ...(process.platform === "darwin" ? ["--dsigharden", "--dsig1-compat", "off"] : []),
      "--in",
      bundle,
      "--out",
      bundle
    ])
    // Confirm this command against the accepted PACE SDK during AAX onboarding.
    run("wraptool", [
      "verify",
      "--in",
      process.platform === "win32"
        ? join(bundle, "Contents/x64", `${plugin.name}.aaxplugin`)
        : bundle
    ])
    if (process.platform === "darwin") run("codesign", ["--verify", "--deep", "--strict", bundle])
  }
}

export async function validateAax(stage: string) {
  if (!process.env.AAX_VALIDATOR) throw new Error("Missing full AAX validator")
  const install =
    process.platform === "darwin"
      ? "/Library/Application Support/Avid/Audio/Plug-Ins"
      : join(
          process.env.CommonProgramFiles ?? "C:\\Program Files\\Common Files",
          "Avid/Audio/Plug-Ins"
        )
  await mkdir(install, { recursive: true })
  const backups: { installed: string; backup: string; exists: boolean }[] = []
  try {
    for (const plugin of plugins) {
      const installed = join(install, bundleName(plugin, "aax"))
      const backup = `${installed}.heron-validation-backup-${process.pid}`
      const exists = await lstat(installed).then(
        () => true,
        () => false
      )
      if (exists) await rename(installed, backup)
      backups.push({ installed, backup, exists })
      await cp(
        join(stage, "bundles", bundleName(plugin, "aax")),
        join(install, bundleName(plugin, "aax")),
        { recursive: true, verbatimSymlinks: true }
      )
    }
    const output = run("cargo", ["truce", "validate", "--aax", "--aax-tests", "full"], {
      capture: true,
      stderr: true
    })
    if (output.includes("SKIP") || output.includes("pluginrunner"))
      throw new Error("AAX validation skipped required release tests")
    for (const plugin of plugins) {
      const line = output.split("\n").find((line) => line.includes(`${plugin.name} ... PASS (`))
      const tally = line?.match(/PASS \((\d+)\/(\d+)\)/)
      if (!tally || Number(tally[1]) === 0 || tally[1] !== tally[2])
        throw new Error(`Missing complete nonzero AAX validation results for ${plugin.name}`)
    }
    console.log(output)
  } finally {
    for (const { installed, backup, exists } of backups.reverse()) {
      await rm(installed, { recursive: true, force: true })
      if (exists) await rename(backup, installed)
    }
  }
}
