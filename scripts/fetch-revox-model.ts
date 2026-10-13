import { createHash, randomUUID } from "node:crypto"
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { target } from "./process.ts"

// Revox 1.5, including the author's GPL-3.0-only licensing option.
const revision = "59a27b71358a2de4b22acda1b07d89d02bf0de79"
const sha256 = "e7a94cf11388ad12ba9ba1316029fdcbf15f77e342559bd1e9d6971a3f8ee020"
const base = `https://huggingface.co/minori-live/revox-vocoder-1/resolve/${revision}`
const directory = join(target, "revox", "models")

async function download(name: string): Promise<Buffer> {
  const response = await fetch(`${base}/${name}`)
  if (!response.ok) throw new Error(`Revox ${name}: HTTP ${response.status}`)
  return Buffer.from(await response.arrayBuffer())
}

if (import.meta.main) {
  await mkdir(directory, { recursive: true })
  const destination = join(directory, "revox.onnx")
  const existing = await readFile(destination).catch((error: NodeJS.ErrnoException) => {
    if (error.code !== "ENOENT") throw error
    return null
  })
  const bytes = existing ?? (await download("vocoder.onnx"))
  if (createHash("sha256").update(bytes).digest("hex") !== sha256)
    throw new Error(`Revox checksum mismatch: ${destination}`)
  if (!existing) {
    const temporary = `${destination}.${randomUUID()}.tmp`
    try {
      await writeFile(temporary, bytes)
      await rename(temporary, destination)
    } finally {
      await rm(temporary, { force: true })
    }
  }
  for (const name of ["README.md", "manifest.json", "validation.json"])
    await writeFile(join(directory, `revox-${name}`), await download(name))
  console.log(`Revox 1.5 verified: ${destination}`)
}
