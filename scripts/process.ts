import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import { join } from "node:path"

export const root = fileURLToPath(new URL("../", import.meta.url))
export const target = join(root, "target")

export function run(
  command: string,
  args: string[],
  options: { cwd?: string; env?: NodeJS.ProcessEnv; capture?: boolean; stderr?: boolean } = {}
): string {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? root,
    env: { ...process.env, ...options.env },
    stdio: options.capture ? ["ignore", "pipe", "pipe"] : "inherit",
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024
  })
  if (result.error) throw result.error
  if (result.status !== 0) {
    if (options.capture) process.stderr.write(result.stderr ?? "")
    throw new Error(`${command} failed (${result.status ?? result.signal})`)
  }
  return `${result.stdout ?? ""}${options.stderr ? (result.stderr ?? "") : ""}`
}

export function sourceCommit() {
  const commit = run("git", ["rev-parse", "HEAD"], { capture: true }).trim()
  if (!/^[a-f0-9]{40}$/.test(commit)) throw new Error("Invalid producer source commit")
  return commit
}
