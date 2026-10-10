import assert from "node:assert/strict"
import { mkdtemp, rm, stat } from "node:fs/promises"
import { tmpdir } from "node:os"
import { delimiter, dirname, join } from "node:path"
import { test } from "node:test"
import { collectCoverage } from "./coverage.ts"
import type { run } from "./process.ts"

test("coverage creates its report directory and exposes the locked Cargo subcommand", async (context) => {
  const workspace = await mkdtemp(join(tmpdir(), "heron-plugins-coverage-"))
  context.after(() => rm(workspace, { recursive: true, force: true }))
  const executable = join(workspace, "mise/cargo-llvm-cov/bin/cargo-llvm-cov")
  const calls: Parameters<typeof run>[] = []
  const execute: typeof run = (command, args, options) => {
    calls.push([command, args, options])
    return command === "mise" ? `${executable}\n` : ""
  }
  await collectCoverage(workspace, execute)
  assert((await stat(join(workspace, "coverage/rust"))).isDirectory())
  assert.deepEqual(calls[0], ["mise", ["which", "cargo-llvm-cov"], { capture: true }])
  assert.deepEqual(calls[1]?.[1], [
    "llvm-cov",
    "--workspace",
    "--locked",
    "--lcov",
    "--output-path",
    join(workspace, "coverage/rust/lcov.info")
  ])
  assert.equal(calls[1]?.[2]?.cwd, workspace)
  assert.equal(
    calls[1]?.[2]?.env?.PATH,
    `${dirname(executable)}${delimiter}${process.env.PATH ?? ""}`
  )
})
