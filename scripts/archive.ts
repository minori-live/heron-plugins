import { createWriteStream } from "node:fs"
import { chmod, lstat, readdir, readlink, mkdir, symlink } from "node:fs/promises"
import { createRequire } from "node:module"
import { dirname, join, relative, resolve, sep } from "node:path"
import { pipeline } from "node:stream/promises"
import { Readable, Transform } from "node:stream"
import { ZipFile } from "yazl"

export function safeSymlink(entry: string, destination: string, archiveRoot: string): string {
  if (
    !destination ||
    destination.includes("\\") ||
    destination.startsWith("/") ||
    /^[A-Za-z]:/.test(destination)
  ) {
    throw new Error(`Unsafe bundle symlink: ${entry}`)
  }
  const inside = relative(resolve(archiveRoot), resolve(dirname(entry), destination))
  if (inside === ".." || inside.startsWith(`..${sep}`))
    throw new Error(`Escaping bundle symlink: ${entry}`)
  return destination
}

// Keep Unix modes and symlinks intact: flattening PACE seals breaks AAX loading.
export async function archiveTree(source: string, output: string) {
  const zip = new ZipFile()
  const time = new Date("2000-01-01T00:00:00Z")
  async function visit(directory: string, prefix: string) {
    for (const name of (await readdir(directory)).sort()) {
      const absolute = join(directory, name)
      const path = prefix ? `${prefix}/${name}` : name
      const info = await lstat(absolute)
      if (info.isSymbolicLink()) {
        const destination = safeSymlink(absolute, await readlink(absolute), source)
        zip.addBuffer(Buffer.from(destination), path, { mode: 0o120777, mtime: time })
      } else if (info.isDirectory()) {
        zip.addEmptyDirectory(path, { mode: 0o40755, mtime: time })
        await visit(absolute, path)
      } else if (info.isFile()) {
        zip.addFile(absolute, path, { mode: 0o100000 | (info.mode & 0o777), mtime: time })
      } else throw new Error(`Unsupported archive entry: ${path}`)
    }
  }
  await mkdir(dirname(output), { recursive: true })
  const finished = pipeline(zip.outputStream, createWriteStream(output))
  // Attach rejection handling before directory traversal, then await it below.
  void finished.catch(() => {})
  try {
    await visit(source, "")
    zip.end()
    await finished
  } catch (error) {
    ;(zip.outputStream as Readable).destroy(error as Error)
    throw error
  }
}

interface ZipEntry {
  path: string
  type: "File" | "Directory"
  flags: number
  externalFileAttributes: number
  uncompressedSize: number
  stream(): Readable
}
const unzip = createRequire(import.meta.url)("unzipper") as {
  Open: { file(path: string): Promise<{ files: ZipEntry[] }> }
}

// GNU tar cannot read ZIPs, and Git Bash interprets Windows drive letters as
// remote hosts. Verify the exact published bytes with one streaming Node reader.
export async function extractTree(archive: string, destination: string): Promise<void> {
  const { files } = await unzip.Open.file(archive)
  if (files.length === 0 || files.length > 65_000) throw new Error("Invalid bundle entry count")
  const names = new Set<string>()
  let expanded = 0
  const entries = files.map((entry) => {
    const path = entry.path.replace(/\/$/, "")
    if (
      !path ||
      path.includes("\\") ||
      path.includes(":") ||
      Array.from(path).some((character) => character.charCodeAt(0) < 32) ||
      path.split("/").some((part) => !part || part === "." || part === "..")
    )
      throw new Error(`Unsafe bundle archive path: ${entry.path}`)
    if (names.has(path.toLowerCase())) throw new Error(`Duplicate bundle archive path: ${path}`)
    names.add(path.toLowerCase())
    const mode = entry.externalFileAttributes >>> 16
    const type = mode & 0o170000
    if (![0, 0o100000, 0o040000, 0o120000].includes(type) || (entry.flags & 1) !== 0)
      throw new Error(`Unsupported bundle archive entry: ${path}`)
    if (
      !Number.isSafeInteger(entry.uncompressedSize) ||
      entry.uncompressedSize < 0 ||
      entry.uncompressedSize > 2 * 1024 ** 3
    )
      throw new Error(`Oversized bundle archive entry: ${path}`)
    expanded += entry.uncompressedSize
    if (expanded > 8 * 1024 ** 3) throw new Error("Oversized expanded bundle archive")
    return { entry, path, mode, type }
  })
  await mkdir(destination, { recursive: true })
  if ((await readdir(destination)).length !== 0)
    throw new Error("Bundle extraction requires an empty destination")
  // Materialize links last so files cannot be written through an archive link.
  const links: Array<{ path: string; target: string }> = []
  for (const { entry, path, mode, type } of entries) {
    const output = join(destination, path)
    if (entry.type === "Directory" || type === 0o040000) {
      await mkdir(output, { recursive: true })
      continue
    }
    await mkdir(dirname(output), { recursive: true })
    if (type === 0o120000) {
      if (entry.uncompressedSize > 4096) throw new Error(`Oversized bundle symlink: ${path}`)
      const chunks: Buffer[] = []
      let size = 0
      for await (const chunk of entry.stream()) {
        size += chunk.length
        if (size > entry.uncompressedSize) throw new Error(`Bundle symlink size mismatch: ${path}`)
        chunks.push(chunk)
      }
      if (size !== entry.uncompressedSize) throw new Error(`Bundle symlink size mismatch: ${path}`)
      const target = safeSymlink(output, Buffer.concat(chunks).toString("utf8"), destination)
      links.push({ path: output, target })
      continue
    }
    let written = 0
    const bounded = new Transform({
      transform(chunk: Buffer, _encoding, callback) {
        written += chunk.length
        callback(
          written > entry.uncompressedSize
            ? new Error(`Bundle archive size mismatch: ${path}`)
            : null,
          chunk
        )
      }
    })
    await pipeline(entry.stream(), bounded, createWriteStream(output, { flags: "wx" }))
    if (written !== entry.uncompressedSize) throw new Error(`Bundle archive size mismatch: ${path}`)
    await chmod(output, mode & 0o777 || 0o644)
  }
  for (const link of links) await symlink(link.target, link.path)
}
