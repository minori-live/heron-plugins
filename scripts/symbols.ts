import { cp, mkdir, readdir, writeFile, readFile } from "node:fs/promises"
import { basename, join, relative } from "node:path"
import { run, root, target } from "./process.ts"
import { plugins, bundleName, type Format, type Platform } from "./catalog.ts"
import { machoFiles } from "./sign.ts"
import { sanitizeSdkSources } from "./source-bundles.ts"
import { aaxConfig } from "./aax.ts"

interface DebugInfo {
  variants: { debug_id: string; arch: string }[]
  features: string
  is_usable: boolean
}
export interface SymbolModule {
  modulePath: string
  binaryPath: string
  debugId: string
  arch: "x64" | "arm64"
}

const cli = join(
  root,
  "node_modules/.bin",
  process.platform === "win32" ? "sentry-cli.cmd" : "sentry-cli"
)
export function sentry(args: string[]) {
  // Node cannot spawn .cmd without a shell. Invoke the package's JS entry on Windows.
  return process.platform === "win32"
    ? run(process.execPath, [join(root, "node_modules/@sentry/cli/bin/sentry-cli"), ...args], {
        capture: true
      })
    : run(cli, args, { capture: true })
}
export function debugInfo(path: string): DebugInfo {
  return JSON.parse(sentry(["debug-files", "check", "--json", path])) as DebugInfo
}

function arch(value: string): "x64" | "arm64" {
  if (value === "x86_64" || value === "amd64") return "x64"
  if (value === "arm64" || value === "aarch64") return "arm64"
  throw new Error(`Unexpected native architecture ${value}`)
}

export async function stageSymbols(
  stage: string,
  destination: string,
  version: string,
  commit: string,
  platform: Platform,
  format: Format
) {
  const modules: SymbolModule[] = []
  await mkdir(join(destination, "symbols"), { recursive: true })
  for (const plugin of plugins) {
    const bundle = join(stage, "bundles", bundleName(plugin, format))
    const symbolDir = join(destination, "symbols", plugin.slug)
    await mkdir(symbolDir, { recursive: true })
    let binaries: string[]
    if (platform === "macos-universal") binaries = await machoFiles(bundle)
    else if (format === "aax")
      binaries = [
        join(bundle, "Contents/x64", `${plugin.name}.aaxplugin`),
        join(bundle, "Contents/Resources", `${plugin.crate.replaceAll("-", "_")}_aax.dll`)
      ]
    else {
      const subdir =
        platform === "windows-x64"
          ? "x86_64-win"
          : `${platform === "linux-arm64" ? "aarch64" : "x86_64"}-linux`
      binaries = [
        join(
          bundle,
          "Contents",
          subdir,
          `${plugin.name}.${platform === "windows-x64" ? "vst3" : "so"}`
        )
      ]
    }
    for (const binary of binaries) {
      const executable = debugInfo(binary)
      const expectedArches =
        platform === "macos-universal"
          ? ["arm64", "x64"]
          : [platform === "linux-arm64" ? "arm64" : "x64"]
      if (
        JSON.stringify(executable.variants.map((slice) => arch(slice.arch)).sort()) !==
        JSON.stringify(expectedArches)
      )
        throw new Error(`Incomplete final module architectures: ${binary}`)
      let debugFile: string
      if (platform === "macos-universal") {
        const dsym = join(symbolDir, `${basename(binary)}.dSYM`)
        run("dsymutil", [binary, "-o", dsym])
        debugFile = join(dsym, "Contents/Resources/DWARF", basename(binary))
      } else if (platform === "windows-x64") {
        const stem = plugin.crate.replaceAll("-", "_")
        const wrapper = format === "aax" && binary.endsWith(".aaxplugin")
        const name = wrapper
          ? "TruceAAXTemplate.pdb"
          : `${stem}${format === "aax" ? "_aax" : ""}.pdb`
        debugFile = join(symbolDir, name)
        const source = wrapper
          ? join(target, "tmp/aax-template/build/TruceAAXTemplate.pdb")
          : join(target, "release", name)
        await cp(source, debugFile)
      } else {
        debugFile = join(symbolDir, `${plugin.name}.so.debug`)
        await cp(binary, debugFile)
      }
      const symbols = debugInfo(debugFile)
      if (!symbols.is_usable || !symbols.features.includes("debug"))
        throw new Error(`Missing full debug information for ${binary}`)
      for (const slice of executable.variants) {
        const match = symbols.variants.find(
          (entry) => entry.debug_id === slice.debug_id && arch(entry.arch) === arch(slice.arch)
        )
        if (!match) throw new Error(`Binary/symbol Debug ID mismatch for ${binary}`)
        modules.push({
          modulePath: relative(destination, debugFile).replaceAll("\\", "/"),
          binaryPath: relative(stage, binary).replaceAll("\\", "/"),
          debugId: slice.debug_id,
          arch: arch(slice.arch)
        })
      }
      sentry(["debug-files", "bundle-sources", "--output", symbolDir, debugFile])
      if (format === "aax" && !(await aaxConfig()).publishSdkSources) {
        if (!process.env.AAX_SDK_PATH)
          throw new Error("Missing licensed SDK root for source filtering")
        await sanitizeSdkSources(symbolDir, process.env.AAX_SDK_PATH)
      }
      if (!(await readdir(symbolDir)).some((name) => name.endsWith(".src.zip")))
        throw new Error(`Missing source bundle for ${binary}`)
    }
    const sources = (await readdir(symbolDir))
      .filter((name) => name.endsWith(".src.zip"))
      .flatMap((name) => debugInfo(join(symbolDir, name)).variants)
    for (const module of modules.filter((module) =>
      module.modulePath.startsWith(`symbols/${plugin.slug}/`)
    ))
      if (
        !sources.some(
          (source) => source.debug_id === module.debugId && arch(source.arch) === module.arch
        )
      )
        throw new Error(`Missing matching source bundle: ${module.binaryPath}/${module.arch}`)
  }
  if (
    modules.length !==
    plugins.length * (platform === "macos-universal" ? 2 : 1) * (format === "aax" ? 2 : 1)
  )
    throw new Error("Incomplete format symbols")
  await writeFile(
    join(destination, "symbols-manifest.json"),
    `${JSON.stringify({ schemaVersion: 1, version, sourceCommit: commit, platform, format, modules }, null, 2)}\n`
  )
}

export async function verifySymbolIds(stage: string, destination: string) {
  const manifest = JSON.parse(
    await readFile(join(destination, "symbols-manifest.json"), "utf8")
  ) as { modules: SymbolModule[] }
  for (const module of manifest.modules) {
    const info = debugInfo(join(stage, module.binaryPath))
    if (
      !info.variants.some(
        (slice) => slice.debug_id === module.debugId && arch(slice.arch) === module.arch
      )
    )
      throw new Error(`Post-signing module Debug ID mismatch for ${module.binaryPath}`)
  }
}
