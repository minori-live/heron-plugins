import { createWriteStream } from "node:fs"
import { lstat, readdir, readlink, mkdir } from "node:fs/promises"
import { dirname, join, relative, resolve, sep } from "node:path"
import { pipeline } from "node:stream/promises"
import { Readable } from "node:stream"
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
