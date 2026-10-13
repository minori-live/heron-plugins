import { appendFile, cp, mkdir, rm, stat, writeFile } from "node:fs/promises"
import { homedir } from "node:os"
import { join } from "node:path"
import { bundleName, revox, type Format } from "./catalog.ts"
import { archiveTree, extractTree } from "./archive.ts"
import { waitForAuRegistration, withInstalledAuBundles } from "./au-registration.ts"
import { stageLicenses } from "./licenses.ts"
import { root, run, sourceCommit, target } from "./process.ts"
import { signMacBundle, stampVersion } from "./sign.ts"
import { auVersion, readVersion } from "./version.ts"

const platform = "macos-arm64"
const dawnName = "libwebgpu_dawn.dylib"

function dependencies(binary: string) {
  const identity = run("otool", ["-D", binary], { capture: true })
    .split("\n")
    .slice(1)
    .map((line) => line.trim())
  return run("otool", ["-L", binary], { capture: true })
    .split("\n")
    .slice(1)
    .filter((line) => line.trim())
    .map((line) => line.trim().split(" (")[0]!)
    .filter((dependency) => !identity.includes(dependency))
}

async function linkVst3(bundle: string, dawn: string, env: NodeJS.ProcessEnv) {
  // Truce's all-load relink pulls duplicate compiler-runtime implementations
  // from ORT and Rust. Keep the VST3 entry points explicitly, then let the
  // linker select their transitive archive members as in a normal native link.
  const release = join(target, "aarch64-apple-darwin/release")
  const library = join(release, "libheron_revox.dylib")
  const links: string[] = []
  for (const dependency of dependencies(library)) {
    const framework = dependency.match(/^\/System\/Library\/Frameworks\/([^/]+)\.framework\//)
    const system = dependency.match(/^\/usr\/lib\/lib(.+?)(?:\.[A-Z]|\.\d+)*\.dylib$/)
    if (framework) links.push("-framework", framework[1]!)
    else if (system) links.push(`-l${system[1]}`)
    else if (dependency === `@rpath/${dawnName}`) links.push(dawn)
    else throw new Error(`Unsupported Revox link dependency: ${dependency}`)
  }
  const exports = [
    "_GetPluginFactory",
    "_BundleEntry",
    "_bundleEntry",
    "_BundleExit",
    "_bundleExit"
  ]
  run(
    "/usr/bin/clang",
    [
      "-bundle",
      "-arch",
      "arm64",
      "-mmacosx-version-min=14.2",
      "-ObjC",
      "-Wl,-dead_strip",
      "-Wl,-headerpad_max_install_names",
      ...exports.flatMap((name) => [`-Wl,-u,${name}`, `-Wl,-exported_symbol,${name}`]),
      join(release, "libheron_revox.a"),
      ...links,
      "-o",
      join(bundle, "Contents/MacOS", revox.name)
    ],
    { env }
  )
}

async function buildVst3(bundle: string, env: NodeJS.ProcessEnv) {
  run(
    "cargo",
    [
      "build",
      "--release",
      "--locked",
      "-p",
      revox.crate,
      "--no-default-features",
      "--features",
      "vst3",
      "--target",
      "aarch64-apple-darwin"
    ],
    { env }
  )
  await mkdir(join(bundle, "Contents/MacOS"), { recursive: true })
  await writeFile(
    join(bundle, "Contents/Info.plist"),
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>${revox.name}</string>
<key>CFBundleIdentifier</key><string>${revox.builtinId}</string>
<key>CFBundleName</key><string>${revox.name}</string>
<key>CFBundlePackageType</key><string>BNDL</string>
<key>CFBundleVersion</key><string>1</string>
</dict></plist>\n`
  )
}

async function stageRuntime(bundle: string, format: Format, env: NodeJS.ProcessEnv) {
  const executable = join(bundle, "Contents/MacOS", revox.name)
  const frameworks = join(bundle, "Contents/Frameworks")
  await mkdir(frameworks, { recursive: true })
  // ort's copy-dylibs output is a symlink into its verified archive cache.
  // Dereference it: installed plug-ins must not depend on a builder's cache.
  const candidates = [
    join(target, "aarch64-apple-darwin/release", dawnName),
    join(target, "release", dawnName)
  ]
  let source: string | undefined
  for (const candidate of candidates)
    if (
      await stat(candidate).then(
        () => true,
        () => false
      )
    ) {
      source = candidate
      break
    }
  if (!source) throw new Error("The ORT build did not produce its WebGPU Dawn runtime")
  if (format === "vst3") await linkVst3(bundle, source, env)
  const dawn = join(frameworks, dawnName)
  await cp(source, dawn, { dereference: true })
  run("install_name_tool", ["-id", `@rpath/${dawnName}`, dawn])
  run("install_name_tool", [
    "-change",
    `@rpath/${dawnName}`,
    `@loader_path/../Frameworks/${dawnName}`,
    executable
  ])
  if (!dependencies(executable).includes(`@loader_path/../Frameworks/${dawnName}`))
    throw new Error("Revox did not link the bundled WebGPU Dawn runtime")
  for (const binary of [executable, dawn]) {
    if (run("lipo", ["-archs", binary], { capture: true }).trim() !== "arm64")
      throw new Error(`Revox development bundle must contain only arm64: ${binary}`)
    for (const dependency of dependencies(binary)) {
      if (
        !dependency.startsWith("/System/Library/") &&
        !dependency.startsWith("/usr/lib/") &&
        dependency !== `@loader_path/../Frameworks/${dawnName}` &&
        dependency !== `@rpath/${dawnName}`
      )
        throw new Error(`Unbundled native dependency: ${dependency}`)
    }
  }
}

async function stageModelNotices(bundle: string) {
  const resources = join(bundle, "Contents/Resources")
  const licenses = join(resources, "licenses/revox-models-and-runtime")
  await mkdir(licenses, { recursive: true })
  await cp(join(root, "scripts/revox/licenses"), licenses, { recursive: true })
  await cp(join(target, "revox/fcpe-upstream/LICENSE"), join(licenses, "FCPE-LICENSE"))
  await cp(join(root, "scripts/revox/fcpe-source.json"), join(licenses, "fcpe-source.json"))
  await cp(join(target, "revox/fcpe-export.json"), join(licenses, "fcpe-export.json"))
  await cp(join(target, "revox/fcpe-plugin-export.json"), join(licenses, "fcpe-plugin-export.json"))
  for (const name of ["revox-README.md", "revox-manifest.json", "revox-plugin-manifest.json"])
    await cp(join(target, "revox/models", name), join(licenses, name))
  await appendFile(
    join(resources, "THIRD_PARTY_NOTICES.txt"),
    "\nFCPE checkpoint and source: MIT; see licenses/revox-models-and-runtime/FCPE-LICENSE.\n" +
      "Revox Vocoder 1.5: GPL-3.0-only licensing option selected; see LICENSE and the model README.\n" +
      "ONNX Runtime 1.31.0 and WebGPU Dawn: accompanying native runtime licenses and third-party notices.\n" +
      "Both inference graphs are embedded in the plug-in executable; Python is not required at runtime.\n"
  )
}

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
    if (format === "vst3") await buildVst3(bundle, env)
    else {
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
    }
    await stampVersion(bundle, version, format)
    await stageRuntime(bundle, format, env)
    await stageLicenses(bundle, version, commit)
    await stageModelNotices(bundle)
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
