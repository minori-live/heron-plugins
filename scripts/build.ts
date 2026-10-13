import { cp, mkdir, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { archiveTree, extractTree } from "./archive.ts"
import {
  assetName,
  bundleName,
  hostPlatform,
  manifestPlugins,
  plugins,
  type Format
} from "./catalog.ts"
import { root, run, sourceCommit, target } from "./process.ts"
import { stageLicenses, stageRevoxModelNotices } from "./licenses.ts"
import { prepareRevoxRuntime, stageRevoxRuntime } from "./revox-runtime.ts"
import { notarize, signMacBundle, stampVersion } from "./sign.ts"
import { readVersion } from "./version.ts"
import { validateBundles } from "./validate.ts"
import { aaxEnabled, prepareAax, signAax, validateAax } from "./aax.ts"
import { createMacDmg } from "./dmg.ts"

if (import.meta.main) {
  const args = process.argv.slice(2)
  if (args.some((arg) => !["--release", "--aax", "--no-aax", "--only-aax"].includes(arg)))
    throw new Error("Usage: build.ts [--release] [--aax | --no-aax | --only-aax]")
  if (args.filter((arg) => ["--aax", "--no-aax", "--only-aax"].includes(arg)).length > 1)
    throw new Error("Choose only one AAX build mode")
  const release = args.includes("--release")
  const platform = hostPlatform()
  const version = await readVersion()
  const commit = sourceCommit()
  const requireCleanSource = () => {
    if (
      release &&
      run("git", ["status", "--porcelain", "--untracked-files=normal"], { capture: true }).trim()
    )
      throw new Error("Release requires a clean committed source tree and lockfiles")
  }
  requireCleanSource()
  run("cargo", ["metadata", "--locked", "--all-features", "--format-version", "1"], {
    capture: true
  })
  await rm(join(target, "release-assets"), { recursive: true, force: true })
  run(process.execPath, [join(root, "scripts/version.ts"), "check"])
  const formats: Format[] = args.includes("--only-aax")
    ? ["aax"]
    : platform === "macos-universal"
      ? ["vst3", "au2"]
      : ["vst3"]
  if (
    args.includes("--aax") ||
    args.includes("--only-aax") ||
    (release && !args.includes("--no-aax") && !platform.startsWith("linux") && (await aaxEnabled()))
  ) {
    if (!(await aaxEnabled())) throw new Error("AAX is explicitly disabled in release-config.json")
    await prepareAax(release)
    if (platform.startsWith("linux")) throw new Error("AAX is unsupported on Linux")
    if (!formats.includes("aax")) formats.push("aax")
  }
  // A release format must include the whole suite, including Revox.
  for (const format of formats) manifestPlugins(format)
  const revoxRuntime = await prepareRevoxRuntime(platform)
  const env: NodeJS.ProcessEnv = {
    CARGO_INCREMENTAL: "0",
    MACOSX_DEPLOYMENT_TARGET: "14.2"
  }
  if (platform === "macos-universal") {
    run("rustup", ["target", "add", "aarch64-apple-darwin", "x86_64-apple-darwin"])
    // The pinned package path links real MH_BUNDLE VST3 binaries for BOTH slices.
    // Its temporary installers are build intermediates, never release assets.
    for (const plugin of plugins)
      run(
        "cargo",
        [
          "truce",
          "package",
          "-p",
          plugin.crate,
          "--formats",
          formats.join(","),
          "--universal",
          "--no-notarize",
          "--no-sign",
          "--no-suite",
          "--target-cpu",
          "baseline"
        ],
        {
          env: {
            ...env,
            TRUCE_SIGNING_IDENTITY: "-"
          }
        }
      )
  } else {
    for (const plugin of plugins) {
      if (formats.includes("vst3"))
        run("cargo", ["truce", "build", "-p", plugin.crate, "--vst3", "--target-cpu", "baseline"], {
          env
        })
      if (formats.includes("aax"))
        run("cargo", ["truce", "build", "-p", plugin.crate, "--aax", "--target-cpu", "baseline"], {
          env
        })
    }
  }
  // cargo-truce does not expose --locked for every internal Cargo invocation.
  requireCleanSource()
  for (const format of formats) {
    const stage = join(target, "release-stage", platform, format)
    await rm(stage, { recursive: true, force: true })
    await mkdir(join(stage, "bundles"), { recursive: true })
    for (const plugin of plugins) {
      const name = bundleName(plugin, format)
      const source =
        platform === "macos-universal"
          ? join(target, "package/macos/plugin", plugin.slug, name)
          : join(target, "bundles", name)
      const bundle = join(stage, "bundles", name)
      await cp(source, bundle, { recursive: true, verbatimSymlinks: true })
      await stampVersion(bundle, version, format)
      await stageLicenses(bundle, version, commit)
      if (plugin.slug === "revox") {
        await stageRevoxModelNotices(bundle)
        await stageRevoxRuntime(bundle, platform, revoxRuntime)
      }
      if (platform === "macos-universal") await signMacBundle(bundle, release)
    }
    await writeFile(
      join(stage, "bundle-manifest.json"),
      `${JSON.stringify({ schemaVersion: 1, version, sourceCommit: commit, platform, plugins: manifestPlugins(format) }, null, 2)}\n`
    )
    if (format === "aax" && release) await signAax(stage)
    const output = join(target, "release-assets", assetName(version, platform, format))
    await archiveTree(stage, output)
    if (release && platform === "macos-universal") notarize(output)
    const unpacked = join(target, "archive-check", platform, format)
    await rm(unpacked, { recursive: true, force: true })
    await mkdir(unpacked, { recursive: true })
    await extractTree(output, unpacked)
    if (platform === "macos-universal")
      for (const plugin of plugins)
        run("codesign", [
          "--verify",
          "--deep",
          "--strict",
          join(unpacked, "bundles", bundleName(plugin, format))
        ])
    if (format === "aax") await validateAax(unpacked)
    else await validateBundles(unpacked, platform, format, version)
    console.log(`Produced ${output}`)
  }
  if (platform === "macos-universal" && !args.includes("--only-aax"))
    await createMacDmg(version, release, formats)
}
