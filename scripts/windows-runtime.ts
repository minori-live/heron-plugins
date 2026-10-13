import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { run } from "./process.ts"

let buildTools: Promise<{ generator: string; dumpbin: string }> | undefined

export function windowsStaticRuntimeEnvironment(): NodeJS.ProcessEnv {
  // Cargo gives encoded flags priority over RUSTFLAGS. Truce's baseline mode
  // preserves both; cc reads crt-static from Cargo and builds its C++ shim /MT.
  const encoded = process.env.CARGO_ENCODED_RUSTFLAGS
  if (encoded !== undefined)
    return {
      CARGO_ENCODED_RUSTFLAGS: `${encoded ? `${encoded}\u001f` : ""}-Ctarget-feature=+crt-static`
    }
  const flags =
    process.env.RUSTFLAGS ??
    process.env.CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS ??
    process.env.CARGO_BUILD_RUSTFLAGS ??
    ""
  return { RUSTFLAGS: `${flags} -C target-feature=+crt-static`.trim() }
}

export function windowsBuildTools() {
  return (buildTools ??= findWindowsBuildTools())
}

async function findWindowsBuildTools() {
  const vswhere = join(
    process.env["ProgramFiles(x86)"] ?? "C:\\Program Files (x86)",
    "Microsoft Visual Studio/Installer/vswhere.exe"
  )
  const installations = JSON.parse(
    run(
      vswhere,
      [
        "-latest",
        "-products",
        "*",
        "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-format",
        "json"
      ],
      { capture: true }
    )
  ) as { installationPath: string; installationVersion: string }[]
  const installation = installations[0]
  if (!installation) throw new Error("Visual C++ build tools are required for Windows bundles")
  const major = Number(installation.installationVersion.split(".")[0])
  const generator =
    major === 17 ? "Visual Studio 17 2022" : major === 18 ? "Visual Studio 18 2026" : undefined
  if (!generator) throw new Error(`Unsupported Visual Studio major version: ${major}`)
  const toolVersion = (
    await readFile(
      join(
        installation.installationPath,
        "VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt"
      ),
      "utf8"
    )
  ).trim()
  if (!/^\d+(?:\.\d+)+$/.test(toolVersion)) throw new Error("Invalid Visual C++ tools version")
  const dumpbin = join(
    installation.installationPath,
    "VC/Tools/MSVC",
    toolVersion,
    "bin/Hostx64/x64/dumpbin.exe"
  )
  return { generator, dumpbin }
}

export async function assertNoExternalMsvcRuntime(binary: string) {
  const { dumpbin } = await windowsBuildTools()
  const dependencies = run(dumpbin, ["/DEPENDENTS", binary], { capture: true })
  const external = dependencies.match(/\b(?:MSVCP|MSVCR|VCRUNTIME|CONCRT)\d[^\s]*\.dll\b/gi)
  if (external)
    throw new Error(
      `${binary} requires an unbundled Visual C++ runtime: ${[...new Set(external)].join(", ")}`
    )
}
