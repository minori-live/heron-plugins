import { cp, mkdir, rm, rename, lstat } from "node:fs/promises"
import { join } from "node:path"
import { homedir } from "node:os"
import { bundleName, plugins, type Format, type Platform } from "./catalog.ts"
import { root, run, target } from "./process.ts"
import { debugInfo } from "./symbols.ts"
import { auVersion } from "./version.ts"

async function validateAu(stage: string, build: string, version: string) {
  for (const architecture of ["arm64", "x86_64"]) {
    const registeredClasses = new Set<string>()
    for (const plugin of plugins) {
      const binary = join(
        stage,
        "bundles",
        bundleName(plugin, "au2"),
        "Contents/MacOS",
        plugin.name
      )
      const classes = run("nm", ["-arch", architecture, "-j", binary], { capture: true })
        .split("\n")
        .filter((name) => /^_OBJC_CLASS_\$_Truce(AUCocoaViewProxy|AuFixedContainer)_/.test(name))
        .sort()
      const expected = [
        `_OBJC_CLASS_$_TruceAUCocoaViewProxy_${plugin.slug}`,
        `_OBJC_CLASS_$_TruceAuFixedContainer_${plugin.slug}`
      ].sort()
      if (JSON.stringify(classes) !== JSON.stringify(expected))
        throw new Error(`AU editor class namespace mismatch: ${plugin.name}/${architecture}`)
      for (const name of classes) {
        if (registeredClasses.has(name)) throw new Error(`Duplicate AU editor class: ${name}`)
        registeredClasses.add(name)
      }
    }
  }
  const install = join(homedir(), "Library/Audio/Plug-Ins/Components")
  await mkdir(install, { recursive: true })
  const backups: { installed: string; backup: string; exists: boolean }[] = []
  try {
    // Register the complete suite together and restore any pre-existing installation.
    for (const plugin of plugins) {
      const name = bundleName(plugin, "au2")
      const installed = join(install, name)
      const backup = `${installed}.heron-validation-backup-${process.pid}`
      const exists = await lstat(installed).then(
        () => true,
        () => false
      )
      if (exists) await rename(installed, backup)
      backups.push({ installed, backup, exists })
      await cp(join(stage, "bundles", name), installed, { recursive: true, verbatimSymlinks: true })
    }
    for (const architecture of ["arm64", "x86_64"]) {
      run("arch", [
        `-${architecture}`,
        join(build, "au-probe"),
        ...plugins.flatMap((plugin) => [
          plugin.auType,
          plugin.fourcc,
          "HRON",
          String(auVersion(version))
        ])
      ])
      for (const plugin of plugins) {
        const type = plugin.auType
        const output = run(
          "arch",
          [`-${architecture}`, "/usr/bin/auval", "-v", type, plugin.fourcc, "HRON"],
          { capture: true, stderr: true }
        )
        if (!output.includes("VALIDATION SUCCEEDED"))
          throw new Error(`AU validation failed for ${plugin.name}/${architecture}`)
        console.log(`AU identity, version, load and auval passed: ${plugin.name}/${architecture}`)
      }
    }
  } finally {
    for (const { installed, backup, exists } of backups.reverse()) {
      await rm(installed, { recursive: true, force: true })
      if (exists) await rename(backup, installed)
    }
  }
}

export async function validateBundles(
  stage: string,
  platform: Platform,
  format: Format,
  version: string
) {
  const build = join(target, "bundle-probe")
  run("cmake", [
    "-S",
    join(root, "fixtures"),
    "-B",
    build,
    ...(platform === "macos-universal"
      ? ["-DCMAKE_OSX_ARCHITECTURES=arm64;x86_64", "-DCMAKE_OSX_DEPLOYMENT_TARGET=14.2"]
      : []),
    "-DCMAKE_BUILD_TYPE=Release"
  ])
  run("cmake", ["--build", build, "--config", "Release"])
  if (format === "au2") {
    await validateAu(stage, build, version)
    return
  }
  const probe =
    platform === "windows-x64" ? join(build, "Release/vst3-probe.exe") : join(build, "vst3-probe")
  for (const plugin of plugins) {
    const bundle = join(stage, "bundles", bundleName(plugin, format))
    const binary =
      platform === "macos-universal"
        ? join(bundle, "Contents/MacOS", plugin.name)
        : join(
            bundle,
            "Contents",
            platform === "windows-x64"
              ? "x86_64-win"
              : platform === "linux-arm64"
                ? "aarch64-linux"
                : "x86_64-linux",
            `${plugin.name}.${platform === "windows-x64" ? "vst3" : "so"}`
          )
    if (platform === "macos-universal") {
      const info = debugInfo(binary)
      if (
        info.variants.length !== 2 ||
        !info.variants.some((entry) => entry.arch === "arm64") ||
        !info.variants.some((entry) => entry.arch === "x86_64")
      )
        throw new Error("Missing universal plugin architecture")
      for (const architecture of ["arm64", "x86_64"])
        run("arch", [`-${architecture}`, probe, binary, plugin.nativeId])
    } else run(probe, [binary, plugin.nativeId])
  }
}
