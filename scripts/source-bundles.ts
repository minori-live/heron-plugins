import { readFile, readdir, writeFile } from "node:fs/promises"
import { join, posix } from "node:path"
import { unzipSync, zipSync } from "fflate"

interface SourceManifest {
  files: Record<string, { path?: string }>
  [key: string]: unknown
}
function normalize(path: string) {
  return posix.normalize(path.replaceAll("\\", "/")).toLowerCase()
}

export function omitSdkSources(bytes: Buffer, sdk: string): Buffer {
  if (bytes.subarray(0, 8).toString("hex") !== "5359534202000000")
    throw new Error("Unsupported Sentry source bundle format")
  const files = unzipSync(bytes)
  const manifestBytes = files["manifest.json"]
  if (!manifestBytes) throw new Error("Missing source bundle manifest")
  const manifest = JSON.parse(Buffer.from(manifestBytes).toString("utf8")) as SourceManifest
  const prefix = `${normalize(sdk).replace(/\/$/, "")}/`
  for (const [key, entry] of Object.entries(manifest.files)) {
    if (entry.path && `${normalize(entry.path)}/`.startsWith(prefix)) {
      delete manifest.files[key]
      delete files[key]
    }
  }
  files["manifest.json"] = Buffer.from(JSON.stringify(manifest))
  const zip = Buffer.from(zipSync(files, { mtime: new Date("2000-01-01T00:00:00Z") }))
  // SYSB's eight-byte header participates in ZIP absolute offsets.
  const end = zip.length - 22
  if (zip.readUInt32LE(end) !== 0x06054b50) throw new Error("Unexpected source ZIP trailer")
  const central = zip.readUInt32LE(end + 16)
  let position = central
  while (position < end) {
    if (zip.readUInt32LE(position) !== 0x02014b50) throw new Error("Invalid source ZIP directory")
    zip.writeUInt32LE(zip.readUInt32LE(position + 42) + 8, position + 42)
    position +=
      46 +
      zip.readUInt16LE(position + 28) +
      zip.readUInt16LE(position + 30) +
      zip.readUInt16LE(position + 32)
  }
  zip.writeUInt32LE(central + 8, end + 16)
  return Buffer.concat([bytes.subarray(0, 8), zip])
}

export async function sanitizeSdkSources(directory: string, sdk: string) {
  for (const name of await readdir(directory)) {
    if (!name.endsWith(".src.zip")) continue
    const path = join(directory, name)
    await writeFile(path, omitSdkSources(await readFile(path), sdk))
  }
}
