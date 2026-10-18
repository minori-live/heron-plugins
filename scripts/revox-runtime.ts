import { createHash } from "node:crypto"
import { appendFile, cp, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises"
import { dirname, join } from "node:path"
import { hostPlatform, type Platform } from "./catalog.ts"
import { run, target } from "./process.ts"
import { assertNoExternalMsvcRuntime, windowsBuildTools } from "./windows-runtime.ts"

const version = "1.31.0"
const sourceCommit = "402979ac1bef89dcd55add41eaa7277c407599eb"
const repository = "https://github.com/microsoft/onnxruntime"
const directory = join(target, "revox/runtime")
const archives = {
  "linux-x64": {
    name: `onnxruntime-linux-x64-${version}.tgz`,
    sha256: "cc5c72baf5ae5c8238a6841f0897227be2d02826b9cf98eaf02fdefaeeb03a57",
    library: `libonnxruntime.so.${version}`
  },
  "linux-arm64": {
    name: `onnxruntime-linux-aarch64-${version}.tgz`,
    sha256: "c5b8b3cca31f3d643a3b313b8f42e2d0f16a4fcefd02cda343d97bd5306afaab",
    library: `libonnxruntime.so.${version}`
  },
  "macos-arm64": {
    name: `onnxruntime-osx-arm64-${version}.tgz`,
    sha256: "1031b36dd3bdaa8d976a6130d9cf2a05e9be320b93ae804f8427b91dc3432afc",
    library: `libonnxruntime.${version}.dylib`
  }
} as const
type RuntimePlatform = Platform | "macos-arm64" | "macos-x64"

export function runtimeBundlePath(platform: RuntimePlatform): string {
  if (platform.startsWith("macos-")) return "Contents/Frameworks/libonnxruntime.dylib"
  if (platform === "windows-x64") return "Contents/x86_64-win/onnxruntime.dll"
  return `Contents/${platform === "linux-x64" ? "x86_64" : "aarch64"}-linux/libonnxruntime.so`
}

function runtimeName(platform: RuntimePlatform): string {
  if (platform.startsWith("macos-")) return "libonnxruntime.dylib"
  return platform === "windows-x64" ? "onnxruntime.dll" : "libonnxruntime.so"
}

async function exists(path: string) {
  return stat(path).then(
    () => true,
    (error: NodeJS.ErrnoException) => {
      if (error.code !== "ENOENT") throw error
      return false
    }
  )
}

function sleepMs(ms: number) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms)
}

/** Shallow-fetch the pinned ORT release. Prefer the release tag: GitHub's
 *  upload-pack reliably advertises refs/tags/*, while bare-SHA --depth=1
 *  fetches intermittently fail with "not our ref" even when the commit is the
 *  tag tip (seen on Windows Test runners). */
function fetchPinnedOnnxRuntimeSource(source: string) {
  const tag = `v${version}`
  const attempts = 4
  let lastError: unknown
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      run("git", ["fetch", "--depth=1", "origin", "tag", tag], { cwd: source })
      run("git", ["checkout", "--force", "--detach", "FETCH_HEAD"], { cwd: source })
      return
    } catch (error) {
      lastError = error
      try {
        // Rare fallback if the tag ref moves or upload-pack flaps: retry the
        // pinned SHA once per attempt before backing off.
        run("git", ["fetch", "--depth=1", "origin", sourceCommit], { cwd: source })
        run("git", ["checkout", "--force", "--detach", sourceCommit], { cwd: source })
        return
      } catch (shaError) {
        lastError = shaError
      }
      if (attempt === attempts) break
      sleepMs(1_000 * 2 ** (attempt - 1))
    }
  }
  throw lastError instanceof Error
    ? lastError
    : new Error(`ONNX Runtime source fetch failed after ${attempts} attempts`)
}

async function digest(path: string) {
  return createHash("sha256")
    .update(await readFile(path))
    .digest("hex")
}

function macDependencies(binary: string) {
  return run("otool", ["-L", binary], { capture: true })
    .split("\n")
    .slice(1)
    .filter((line) => /^\s/.test(line))
    .map((line) => line.trim().split(" (")[0]!)
    .filter((dependency) => dependency !== "@rpath/libonnxruntime.dylib")
}

async function downloadedRuntime(platform: keyof typeof archives) {
  const pinned = archives[platform]
  const archive = join(directory, pinned.name)
  await mkdir(directory, { recursive: true })
  if (!(await exists(archive))) {
    const response = await fetch(`${repository}/releases/download/v${version}/${pinned.name}`, {
      signal: AbortSignal.timeout(300_000)
    })
    if (!response.ok) throw new Error(`ONNX Runtime download failed: HTTP ${response.status}`)
    const bytes = Buffer.from(await response.arrayBuffer())
    if (createHash("sha256").update(bytes).digest("hex") !== pinned.sha256)
      throw new Error(`ONNX Runtime checksum mismatch: ${pinned.name}`)
    await writeFile(archive, bytes)
  }
  if ((await digest(archive)) !== pinned.sha256)
    throw new Error(`ONNX Runtime checksum mismatch: ${pinned.name}`)
  const extracted = join(directory, `${platform}-archive`)
  await rm(extracted, { recursive: true, force: true })
  await mkdir(extracted, { recursive: true })
  {
    const entries = run("tar", ["-tzf", archive], { capture: true }).trim().split("\n")
    if (
      entries.some(
        (entry) => entry.startsWith("/") || entry.includes("\\") || entry.split("/").includes("..")
      )
    )
      throw new Error("Unsafe ONNX Runtime archive paths")
    run("tar", ["-xzf", archive, "-C", extracted])
  }
  const entries = await readdir(extracted, { withFileTypes: true })
  if (entries.length !== 1 || !entries[0]!.isDirectory())
    throw new Error("Unexpected ONNX Runtime archive layout")
  const source = join(extracted, entries[0]!.name)
  const output = join(directory, platform)
  await mkdir(output, { recursive: true })
  // Dereference upstream SONAME links: the installed bundle owns real bytes.
  await cp(join(source, "lib", pinned.library), join(output, runtimeName(platform)), {
    dereference: true
  })
  const notices = join(output, "notices")
  await mkdir(notices, { recursive: true })
  for (const name of ["LICENSE", "ThirdPartyNotices.txt", "Privacy.md"])
    await cp(join(source, name), join(notices, name))
  await writeFile(
    join(output, "runtime-source.json"),
    `${JSON.stringify(
      {
        version,
        repository,
        sourceCommit,
        platform,
        selectedProviders: platform === "macos-arm64" ? ["CoreML", "CPU"] : ["CPU"],
        archive: pinned.name,
        archiveSha256: pinned.sha256,
        librarySha256: await digest(join(output, runtimeName(platform)))
      },
      null,
      2
    )}\n`
  )
  return output
}

async function sourceRuntime(platform: "windows-x64" | "macos-x64") {
  const windows = platform === "windows-x64"
  if (process.platform !== (windows ? "win32" : "darwin"))
    throw new Error(`${platform} runtime must be built on its native operating system`)
  const pythonCommand = process.env.REVOX_PYTHON ?? (windows ? "python" : "python3")
  const windowsTools = windows ? await windowsBuildTools() : undefined
  const python = run(
    pythonCommand,
    [
      "-c",
      "import sys; assert sys.version_info >= (3, 10), 'ONNX Runtime requires Python 3.10 or newer'; print(sys.executable)"
    ],
    { capture: true }
  ).trim()
  const source = join(directory, "onnxruntime-source")
  if (!(await exists(join(source, ".git")))) {
    await mkdir(source, { recursive: true })
    run("git", ["init", source])
    if (windows) run("git", ["config", "core.longpaths", "true"], { cwd: source })
    run("git", ["remote", "add", "origin", repository], { cwd: source })
  }
  fetchPinnedOnnxRuntimeSource(source)
  if (
    run("git", ["rev-parse", "HEAD"], { cwd: source, capture: true }).trim() !== sourceCommit ||
    run("git", ["status", "--porcelain", "--untracked-files=no", "--ignore-submodules=all"], {
      cwd: source,
      capture: true
    }).trim()
  )
    throw new Error("ONNX Runtime source is not the clean pinned release commit")
  run(
    "git",
    [
      ...(windows ? ["-c", "core.longpaths=true"] : []),
      "submodule",
      "update",
      "--init",
      "--recursive",
      "--depth=1"
    ],
    { cwd: source }
  )
  const submodules = run("git", ["submodule", "status", "--recursive"], {
    cwd: source,
    capture: true
  })
  if (submodules.split(/\r?\n/).some((line) => /^[+U-]/.test(line)))
    throw new Error("ONNX Runtime submodules do not match the pinned source tree")
  run(
    "git",
    [
      ...(windows ? ["-c", "core.longpaths=true"] : []),
      "submodule",
      "foreach",
      "--recursive",
      "git diff --quiet HEAD"
    ],
    { cwd: source }
  )
  const build = join(directory, `${platform}-build`)
  const definitions = [
    // ONNX uses find_package(Python3), while ORT sets Python_EXECUTABLE.
    // Pin both to the interpreter that actually ran the version check.
    `Python3_EXECUTABLE=${python.replaceAll("\\", "/")}`,
    "onnxruntime_BUILD_UNIT_TESTS=OFF",
    "onnxruntime_USE_COREML=OFF",
    ...(!windows ? ["CMAKE_OSX_ARCHITECTURES=x86_64", "CMAKE_OSX_DEPLOYMENT_TARGET=14.2"] : [])
  ]
  run(
    python,
    [
      join(source, "tools/ci_build/build.py"),
      "--build_dir",
      build,
      "--config",
      "Release",
      "--update",
      "--build",
      "--build_shared_lib",
      "--parallel",
      "3",
      "--skip_tests",
      "--skip_submodule_sync",
      "--no_telemetry",
      ...(windows
        ? ["--enable_msvc_static_runtime", "--cmake_generator", windowsTools!.generator]
        : []),
      "--targets",
      "onnxruntime",
      "--compile_no_warning_as_error",
      "--cmake_extra_defines",
      ...definitions
    ],
    { cwd: source }
  )
  const output = join(directory, platform)
  await mkdir(join(output, "notices"), { recursive: true })
  const library = join(output, runtimeName(platform))
  await cp(
    join(build, "Release", windows ? "Release/onnxruntime.dll" : `libonnxruntime.${version}.dylib`),
    library,
    {
      dereference: true
    }
  )
  if (windows) await assertNoExternalMsvcRuntime(library)
  for (const name of ["LICENSE", "ThirdPartyNotices.txt"])
    await cp(join(source, name), join(output, "notices", name))
  await cp(join(source, "docs/Privacy.md"), join(output, "notices/Privacy.md"))
  await writeFile(
    join(output, "runtime-source.json"),
    `${JSON.stringify(
      {
        version,
        repository,
        sourceCommit,
        platform,
        selectedProviders: ["CPU"],
        build: {
          configuration: "Release",
          staticMsvcRuntime: windows,
          telemetry: false,
          definitions
        },
        librarySha256: await digest(library)
      },
      null,
      2
    )}\n`
  )
  return output
}

export async function prepareRevoxRuntime(platform: RuntimePlatform): Promise<string> {
  if (platform === "macos-x64" || platform === "windows-x64") return sourceRuntime(platform)
  if (platform !== "macos-universal") return downloadedRuntime(platform)
  const arm64 = await downloadedRuntime("macos-arm64")
  const x64 = await sourceRuntime("macos-x64")
  const output = join(directory, platform)
  await mkdir(output, { recursive: true })
  const library = join(output, runtimeName(platform))
  run("lipo", [
    "-create",
    join(arm64, runtimeName(platform)),
    join(x64, runtimeName(platform)),
    "-output",
    library
  ])
  await cp(join(arm64, "notices"), join(output, "notices"), { recursive: true })
  await writeFile(
    join(output, "runtime-source.json"),
    `${JSON.stringify(
      {
        version,
        repository,
        sourceCommit,
        platform,
        selectedProviders: { arm64: ["CoreML", "CPU"], x86_64: ["CPU"] },
        slices: [
          JSON.parse(await readFile(join(arm64, "runtime-source.json"), "utf8")),
          JSON.parse(await readFile(join(x64, "runtime-source.json"), "utf8"))
        ],
        librarySha256: await digest(library)
      },
      null,
      2
    )}\n`
  )
  return output
}

export async function stageRevoxRuntime(
  bundle: string,
  platform: RuntimePlatform,
  prepared: string
) {
  const relative = runtimeBundlePath(platform)
  const destination = join(bundle, relative)
  await mkdir(dirname(destination), { recursive: true })
  await cp(join(prepared, runtimeName(platform)), destination, { dereference: true })
  if (platform.startsWith("macos-")) {
    run("install_name_tool", ["-id", "@rpath/libonnxruntime.dylib", destination])
    const actual = run("lipo", ["-archs", destination], { capture: true })
      .trim()
      .split(/\s+/)
      .sort()
    const expected =
      platform === "macos-universal"
        ? ["arm64", "x86_64"]
        : [platform === "macos-arm64" ? "arm64" : "x86_64"]
    if (JSON.stringify(actual) !== JSON.stringify(expected))
      throw new Error("ONNX Runtime architecture mismatch")
    for (const dependency of macDependencies(destination))
      if (!dependency.startsWith("/System/Library/") && !dependency.startsWith("/usr/lib/"))
        throw new Error(`Unbundled ONNX Runtime dependency: ${dependency}`)
  }
  const notices = join(bundle, "Contents/Resources/licenses/revox-models-and-runtime")
  await mkdir(notices, { recursive: true })
  await cp(join(prepared, "notices"), join(notices, "onnxruntime-distribution"), {
    recursive: true
  })
  const provenance = JSON.parse(await readFile(join(prepared, "runtime-source.json"), "utf8"))
  await writeFile(
    join(notices, "runtime-source.json"),
    `${JSON.stringify(
      {
        ...provenance,
        bundlePath: relative,
        preSigningLibrarySha256: await digest(destination)
      },
      null,
      2
    )}\n`
  )
}

if (import.meta.main) {
  const args = process.argv.slice(2)
  const platform = (args[0] ??
    (process.platform === "darwin"
      ? process.arch === "arm64"
        ? "macos-arm64"
        : "macos-x64"
      : hostPlatform())) as RuntimePlatform
  if (
    args.length > 1 ||
    ![...Object.keys(archives), "windows-x64", "macos-universal", "macos-x64"].includes(platform)
  )
    throw new Error(
      "Usage: revox-runtime.ts [windows-x64|linux-x64|linux-arm64|macos-arm64|macos-x64|macos-universal]"
    )
  const prepared = await prepareRevoxRuntime(platform)
  const environment = `ORT_DYLIB_PATH=${join(prepared, runtimeName(platform))}`
  if (process.env.GITHUB_ENV) await appendFile(process.env.GITHUB_ENV, `${environment}\n`)
  console.log(environment)
}
