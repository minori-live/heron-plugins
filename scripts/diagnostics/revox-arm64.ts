// Temporary branch-only investigation. These outputs are not release assets.
import { createHash } from "node:crypto"
import { createReadStream, createWriteStream } from "node:fs"
import { cp, mkdir, readFile, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { Readable } from "node:stream"
import { pipeline } from "node:stream/promises"
import { archiveTree, extractTree } from "../archive.ts"
import { bundleName, revox } from "../catalog.ts"
import { stageLicenses, stageRevoxModelNotices } from "../licenses.ts"
import { run, sourceCommit, target } from "../process.ts"
import { prepareRevoxRuntime, runtimeBundlePath, stageRevoxRuntime } from "../revox-runtime.ts"
import { readVersion } from "../version.ts"

const platform = "linux-arm64"
const output = join(target, "revox-diagnostics")
const evidence = join(output, "evidence")
const name = bundleName(revox, "vst3")
const binaryRelative = "Contents/aarch64-linux/Heron Revox.so"
const baselineName = "heron-plugins-0.8.1-linux-arm64-vst3.zip"
const baselineSha256 = "0145d959d20d490dad93cfa33a9b9d70bf38f2503b20fec11e2ae3a5102bb634"
const baselineCommit = "87c5afaa126a9d12fd58c065568ba37a0ed73291"

async function sha256(path: string) {
  const hash = createHash("sha256")
  for await (const bytes of createReadStream(path)) hash.update(bytes)
  return hash.digest("hex")
}

async function recordBundle(label: string, bundle: string, details: object) {
  await writeFile(
    join(evidence, `${label}-bundle.json`),
    `${JSON.stringify(
      {
        ...details,
        binary: join(bundle, binaryRelative),
        binarySha256: await sha256(join(bundle, binaryRelative)),
        runtimeSha256: await sha256(join(bundle, runtimeBundlePath(platform)))
      },
      null,
      2
    )}\n`
  )
}

async function baseline() {
  const archive = join(output, baselineName)
  const url = `https://github.com/minori-live/heron-plugins/releases/download/v0.8.1/${baselineName}`
  const response = await fetch(url, { signal: AbortSignal.timeout(300_000) })
  if (!response.ok || !response.body)
    throw new Error(`Baseline release download failed: HTTP ${response.status}`)
  const reader = response.body.getReader()
  const chunks = async function* () {
    try {
      while (true) {
        const { done, value } = await reader.read()
        if (done) return
        yield value
      }
    } finally {
      reader.releaseLock()
    }
  }
  await pipeline(Readable.from(chunks()), createWriteStream(archive, { flags: "wx" }))
  if ((await sha256(archive)) !== baselineSha256)
    throw new Error("Published v0.8.1 ARM64 archive checksum mismatch")
  const directory = join(output, "baseline")
  await extractTree(archive, directory)
  const manifest = JSON.parse(await readFile(join(directory, "bundle-manifest.json"), "utf8"))
  if (
    manifest.version !== "0.8.1" ||
    manifest.sourceCommit !== baselineCommit ||
    manifest.platform !== platform ||
    !manifest.plugins.some(
      (plugin: { builtinId: string; nativeId: string; bundleName: string }) =>
        plugin.builtinId === revox.builtinId &&
        plugin.nativeId === revox.nativeId &&
        plugin.bundleName === name
    )
  )
    throw new Error("Published v0.8.1 ARM64 manifest identity mismatch")
  await cp(join(directory, "bundle-manifest.json"), join(evidence, "baseline-manifest.json"))
  await recordBundle("baseline", join(directory, "bundles", name), {
    url,
    archiveSha256: baselineSha256,
    sourceCommit: baselineCommit
  })
}

async function build() {
  const requireCleanSource = () => {
    if (run("git", ["status", "--porcelain", "--untracked-files=normal"], { capture: true }).trim())
      throw new Error("Diagnostic build requires committed source and unchanged lockfiles")
  }
  requireCleanSource()
  run("cargo", ["metadata", "--locked", "--all-features", "--format-version", "1"], {
    capture: true
  })
  const version = await readVersion()
  const commit = sourceCommit()
  const runtime = await prepareRevoxRuntime(platform)
  // Identical release profile and baseline CPU selection to the published build.
  // Do not opt into the smoke build's disabled LTO or extra codegen units.
  if (Object.keys(process.env).some((key) => key.startsWith("CARGO_PROFILE_RELEASE_")))
    throw new Error("Unexpected release-profile override in the diagnostic build")
  run("cargo", ["truce", "build", "-p", revox.crate, "--vst3", "--target-cpu", "baseline"], {
    env: { CARGO_INCREMENTAL: "0" }
  })
  requireCleanSource()
  const stage = join(output, "instrumented")
  const bundle = join(stage, "bundles", name)
  await cp(join(target, "bundles", name), bundle, { recursive: true, verbatimSymlinks: true })
  await stageLicenses(bundle, version, commit)
  await stageRevoxModelNotices(bundle)
  await stageRevoxRuntime(bundle, platform, runtime)
  await recordBundle("instrumented", bundle, {
    diagnosticOnly: true,
    version,
    sourceCommit: commit,
    platform,
    profile: "release (unmodified Cargo.toml thin-LTO settings)",
    targetCpu: "baseline"
  })
  await archiveTree(stage, join(evidence, "instrumented-revox-diagnostic.zip"))
}

if (process.platform !== "linux" || process.arch !== "arm64")
  throw new Error("This diagnostic must run natively on Linux ARM64")
await mkdir(evidence, { recursive: true })
const args = process.argv.slice(2)
if (args.length !== 1 || !["baseline", "build"].includes(args[0]!))
  throw new Error("Usage: revox-arm64.ts baseline|build")
if (args[0] === "baseline") await baseline()
else await build()
