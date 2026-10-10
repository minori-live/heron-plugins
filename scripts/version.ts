import { readFile, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { root } from "./process.ts"

export function validateVersion(value: string): string {
  if (
    !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/.test(value)
  ) {
    throw new Error(`Invalid suite version: ${value}`)
  }
  return value
}

export async function readVersion() {
  return validateVersion((await readFile(join(root, "VERSION"), "utf8")).trim())
}

export function auVersion(version: string): number {
  const parts = version.split("-")[0]!.split(".").map(Number)
  if (parts[0]! > 65535 || parts[1]! > 255 || parts[2]! > 255) {
    throw new Error("Suite version cannot fit the AU component version")
  }
  return parts[0]! * 65536 + parts[1]! * 256 + parts[2]!
}

if (import.meta.main) {
  const command = process.argv[2] ?? "check"
  if (command !== "check" && command !== "sync") throw new Error("Expected check or sync")
  const version = await readVersion()
  const packagePath = join(root, "package.json")
  const cargoPath = join(root, "Cargo.toml")
  const pkg = JSON.parse(await readFile(packagePath, "utf8")) as { version: string }
  const cargo = await readFile(cargoPath, "utf8")
  const section = cargo.match(/\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/)?.[1]
  const cargoVersion = section?.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
  if (!cargoVersion) throw new Error("Missing workspace.package.version")
  if (command === "sync") {
    pkg.version = version
    await writeFile(packagePath, `${JSON.stringify(pkg, null, 2)}\n`)
    await writeFile(
      cargoPath,
      cargo.replace(section!, section!.replace(/^version\s*=\s*"[^"]+"/m, `version = "${version}"`))
    )
  } else if (pkg.version !== version || cargoVersion !== version) {
    throw new Error("VERSION, package.json and Cargo.toml must agree")
  }
  console.log(version)
}
