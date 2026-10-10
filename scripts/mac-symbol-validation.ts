import type { Plugin } from "./catalog.ts"
import { run } from "./process.ts"

export function rustProcessAddress(symbols: string, plugin: Plugin): string {
  const owner = plugin.crate.replaceAll("-", "_")
  const method = plugin.slug === "eq" ? "EqDspState7process" : "PluginLogic7process"
  const matches = symbols
    .split("\n")
    .filter((line) => line.includes(owner) && line.endsWith(method))
    .map((line) => /^([0-9a-f]+)\s+[tT]\s+/.exec(line)?.[1])
    .filter((address) => address !== undefined)
  if (matches.length !== 1) throw new Error(`Missing unique Rust process symbol: ${plugin.crate}`)
  return `0x${matches[0]}`
}

export function assertRustSourceLine(lookup: string, plugin: Plugin): string {
  const file = plugin.slug === "eq" ? "processor.rs" : "lib.rs"
  const source = new RegExp(
    `Line info: file '(?:[^']*/)?${file.replace(".", "\\.")}', line ([1-9][0-9]*)\\b`
  ).exec(lookup)
  if (!lookup.includes(`plugins/${plugin.crate}/src/`) || !source)
    throw new Error(`Incomplete owned Rust function/source lines: ${plugin.crate}`)
  return `${file}:${source[1]}`
}

export function verifyMacRustSourceLines(binary: string, debugFile: string, plugin: Plugin) {
  for (const architecture of ["arm64", "x86_64"]) {
    // Probe a real address in the final linked executable, rather than trusting UUID alone.
    const address = rustProcessAddress(
      run("nm", ["-arch", architecture, binary], { capture: true }),
      plugin
    )
    const line = assertRustSourceLine(
      run("dwarfdump", [`--arch=${architecture}`, `--lookup=${address}`, debugFile], {
        capture: true
      }),
      plugin
    )
    console.log(`Verified Rust symbols: ${plugin.crate}/${architecture} ${address} -> ${line}`)
  }
}
