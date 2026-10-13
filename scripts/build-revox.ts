import { cp, mkdir, rm, writeFile } from "node:fs/promises"
import { homedir } from "node:os"
import { join } from "node:path"
import { bundleName, revox, type Format } from "./catalog.ts"
import { archiveTree, extractTree } from "./archive.ts"
import { waitForAuRegistration, withInstalledAuBundles } from "./au-registration.ts"
import { stageLicenses, stageRevoxModelNotices } from "./licenses.ts"
import { prepareRevoxRuntime, stageRevoxRuntime } from "./revox-runtime.ts"
import { root, run, sourceCommit, target } from "./process.ts"
import { signMacBundle, stampVersion } from "./sign.ts"
import { auVersion, readVersion } from "./version.ts"

const platform = "macos-arm64"
async function validate(stage: string, formats: Format[], version: string) {
  const build = join(target, "revox/bundle-probe")
  run("cmake", [
    "-S",
    join(root, "fixtures"),
    "-B",
    build,
    "-DCMAKE_OSX_ARCHITECTURES=arm64",
    "-DCMAKE_OSX_DEPLOYMENT_TARGET=14.2",
    "-DCMAKE_CXX_COMPILER=/usr/bin/clang++",
    "-DCMAKE_BUILD_TYPE=Release"
  ])
  run("cmake", ["--build", build, "--config", "Release"])
  if (formats.includes("vst3")) {
    const binary = join(stage, "bundles", bundleName(revox, "vst3"), "Contents/MacOS", revox.name)
    run(join(build, "vst3-probe"), [binary, revox.nativeId])
    run(join(build, "revox-probe"), [binary])
  }
  if (formats.includes("au2")) {
    const name = bundleName(revox, "au2")
    await withInstalledAuBundles(
      join(stage, "bundles"),
      [name],
      join(homedir(), "Library/Audio/Plug-Ins/Components"),
      async () => {
        await waitForAuRegistration(join(build, "au-probe"), "arm64", [
          revox.auType,
          revox.fourcc,
          "HRON",
          String(auVersion(version))
        ])
        const result = run("/usr/bin/auval", ["-v", revox.auType, revox.fourcc, "HRON"], {
          capture: true,
          stderr: true
        })
        if (!result.includes("VALIDATION SUCCEEDED"))
          throw new Error("Heron Revox AU validation failed")
        console.log("Heron Revox AU identity, version and auval passed")
      }
    )
  }
}

if (import.meta.main) {
  const args = process.argv.slice(2)
  if (args.some((arg) => arg !== "--au")) throw new Error("Usage: build-revox.ts [--au]")
  if (process.platform !== "darwin" || process.arch !== "arm64")
    throw new Error("Revox development bundles currently require an Apple Silicon macOS build host")
  const formats: Format[] = args.includes("--au") ? ["vst3", "au2"] : ["vst3"]
  const version = await readVersion()
  const commit = sourceCommit()
  const runtime = await prepareRevoxRuntime(platform)
  const stage = join(target, "revox/bundle-stage", platform)
  await rm(stage, { recursive: true, force: true })
  await mkdir(join(stage, "bundles"), { recursive: true })
  for (const format of formats) {
    // Build and stage each format before the next overwrites Cargo's staticlib.
    // The crate verifies the prepared model checksums before embedding them.
    const name = bundleName(revox, format)
    const bundle = join(stage, "bundles", name)
    const env = {
      MACOSX_DEPLOYMENT_TARGET: "14.2",
      CARGO_INCREMENTAL: "0",
      TRUCE_SIGNING_IDENTITY: "-",
      // A developer shell can expose a package-manager compiler wrapper whose
      // private runtime libraries must never become plug-in dependencies.
      CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER: "/usr/bin/clang",
      CC: "/usr/bin/clang",
      CXX: "/usr/bin/clang++",
      LIBRARY_PATH: "",
      SDKROOT: run("/usr/bin/xcrun", ["--sdk", "macosx", "--show-sdk-path"], {
        capture: true
      }).trim()
    }
    run(
      "cargo",
      [
        "truce",
        "package",
        "-p",
        revox.crate,
        "--formats",
        format,
        "--host-only",
        "--no-notarize",
        "--no-sign",
        "--no-suite",
        "--target-cpu",
        "baseline"
      ],
      { env }
    )
    await cp(join(target, "package/macos/plugin", revox.slug, name), bundle, {
      recursive: true,
      verbatimSymlinks: true
    })
    await stampVersion(bundle, version, format)

    await stageLicenses(bundle, version, commit)
    await stageRevoxModelNotices(bundle)
    await stageRevoxRuntime(bundle, platform, runtime)
    await signMacBundle(bundle, false)
  }
  await writeFile(
    join(stage, "bundle-manifest.json"),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        version,
        sourceCommit: commit,
        platform,
        plugins: formats.map((format) => ({
          builtinId: revox.builtinId,
          name: revox.name,
          kind: revox.kind,
          format,
          nativeId: format === "vst3" ? revox.nativeId : `${revox.auType}:${revox.fourcc}:HRON`,
          bundleName: bundleName(revox, format)
        }))
      },
      null,
      2
    )}\n`
  )
  const output = join(target, "revox/bundles", `heron-revox-${version}-${platform}.zip`)
  await archiveTree(stage, output)
  const unpacked = join(target, "revox/archive-check", platform)
  await rm(unpacked, { recursive: true, force: true })
  await mkdir(unpacked, { recursive: true })
  await extractTree(output, unpacked)
  for (const format of formats)
    run("codesign", [
      "--verify",
      "--deep",
      "--strict",
      join(unpacked, "bundles", bundleName(revox, format))
    ])
  await validate(unpacked, formats, version)
  console.log(`Produced development bundle ${output}`)
}
