import { readdir, readFile, lstat, writeFile, open } from "node:fs/promises"
import { join } from "node:path"
import { run } from "./process.ts"
import { auVersion } from "./version.ts"
import type { Format } from "./catalog.ts"

export async function stampVersion(bundle: string, version: string, format: Format) {
  if (process.platform !== "darwin") return
  const path = join(bundle, "Contents/Info.plist")
  let plist = await readFile(path, "utf8")
  const core = version.split("-")[0]!
  const bundleVersion = /(<key>CFBundleVersion<\/key>\s*<string>)[^<]*(<\/string>)/
  if (!bundleVersion.test(plist)) throw new Error("Missing bundle version field")
  plist = plist.replace(bundleVersion, `$1${core}$2`)
  if (!plist.includes("<key>CFBundleShortVersionString</key>")) {
    plist = plist.replace(
      "<key>CFBundleVersion</key>",
      `<key>CFBundleShortVersionString</key><string>${core}</string>\n<key>CFBundleVersion</key>`
    )
  }
  plist = plist.replace(
    /(<key>CFBundleShortVersionString<\/key>\s*<string>)[^<]*(<\/string>)/,
    `$1${core}$2`
  )
  if (format === "au2") {
    if (!/(<key>version<\/key>\s*<integer>)\d+(<\/integer>)/.test(plist))
      throw new Error("Missing AU component version")
    plist = plist.replace(
      /(<key>version<\/key>\s*<integer>)\d+(<\/integer>)/,
      `$1${auVersion(version)}$2`
    )
  }
  await writeFile(path, plist)
  run("plutil", ["-lint", path])
  for (const field of ["CFBundleVersion", "CFBundleShortVersionString"])
    if (
      run("plutil", ["-extract", field, "raw", "-o", "-", path], { capture: true }).trim() !== core
    )
      throw new Error(`Incorrect ${field}`)
}

export async function machoFiles(directory: string): Promise<string[]> {
  const files: string[] = []
  for (const entry of await readdir(directory)) {
    const path = join(directory, entry)
    const info = await lstat(path)
    if (info.isSymbolicLink()) continue
    if (info.isDirectory()) files.push(...(await machoFiles(path)))
    else if (info.isFile()) {
      const handle = await open(path, "r")
      const bytes = Buffer.alloc(4)
      try {
        await handle.read(bytes, 0, 4, 0)
      } finally {
        await handle.close()
      }
      const magic = bytes.toString("hex")
      if (["cffaedfe", "cefaedfe", "feedfacf", "feedface", "cafebabe", "bebafeca"].includes(magic))
        files.push(path)
    }
  }
  return files
}

export async function signMacBundle(bundle: string, release: boolean) {
  const identity = release ? process.env.TRUCE_SIGNING_IDENTITY : "-"
  if (!identity || (release && !identity.startsWith("Developer ID Application:"))) {
    throw new Error("Release requires a Developer ID Application signing identity")
  }
  for (const path of [...(await machoFiles(bundle)), bundle]) {
    run("codesign", [
      "--force",
      "--sign",
      identity,
      ...(release ? ["--options", "runtime", "--timestamp"] : []),
      path
    ])
  }
  run("codesign", ["--verify", "--deep", "--strict", bundle])
}

export function notarize(path: string) {
  const key = process.env.APPLE_API_KEY
  const keyId = process.env.APPLE_API_KEY_ID
  const issuer = process.env.APPLE_API_ISSUER
  if (!key || !keyId || !issuer)
    throw new Error("Release requires all Apple notarization credentials")
  const result = run(
    "xcrun",
    [
      "notarytool",
      "submit",
      path,
      "--key",
      key,
      "--key-id",
      keyId,
      "--issuer",
      issuer,
      "--wait",
      "--output-format",
      "json"
    ],
    { capture: true }
  )
  const response = JSON.parse(result) as { status?: string; id?: string }
  if (response.status !== "Accepted")
    throw new Error(`Notarization rejected: ${response.id ?? "unknown submission"}`)
  console.log(`Notarization accepted: ${response.id}`)
}
