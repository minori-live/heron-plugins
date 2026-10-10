import { mkdir } from "node:fs/promises"
import { delimiter, dirname, join } from "node:path"
import { root, run } from "./process.ts"

export async function collectCoverage(workspace = root, execute = run) {
  const output = join(workspace, "coverage/rust/lcov.info")
  await mkdir(dirname(output), { recursive: true })
  // On Windows mise installs cargo tools below an additional bin directory.
  // Cargo discovers its subcommands through PATH, not through mise's shim list.
  const llvmCov = execute("mise", ["which", "cargo-llvm-cov"], { capture: true }).trim()
  if (!llvmCov) throw new Error("Missing locked cargo-llvm-cov executable")
  execute("cargo", ["llvm-cov", "--workspace", "--locked", "--lcov", "--output-path", output], {
    cwd: workspace,
    env: { PATH: `${dirname(llvmCov)}${delimiter}${process.env.PATH ?? ""}` }
  })
}

if (import.meta.main) await collectCoverage()
