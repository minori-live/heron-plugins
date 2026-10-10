import { lstat, mkdir, rename } from "node:fs/promises"
import { join } from "node:path"
import { root, run, target } from "./process.ts"

const commit = "9fad9770f2ae8542ab1a548a68c1ad1ac690abe0"
if (import.meta.main) {
  const destination = join(root, "third_party/vst3sdk")
  let configured = await lstat(join(destination, ".git")).then(
    () => true,
    () => false
  )
  if (configured) {
    try {
      run("git", ["-C", destination, "rev-parse", "--git-dir"], { capture: true })
    } catch {
      // A copied submodule can retain a gitdir pointer into its former repository.
      // Keep its source recoverable and create an independent pinned checkout.
      await mkdir(join(target, "sdk-backups"), { recursive: true })
      await rename(destination, join(target, "sdk-backups", `vst3sdk-${Date.now()}`))
      configured = false
    }
  }
  if (!configured) {
    await mkdir(destination, { recursive: true })
    run("git", ["init", destination])
    run("git", [
      "-C",
      destination,
      "remote",
      "add",
      "origin",
      "https://github.com/steinbergmedia/vst3sdk.git"
    ])
    run("git", ["-C", destination, "fetch", "--depth", "1", "origin", commit])
    run("git", ["-C", destination, "checkout", "--force", "--detach", "FETCH_HEAD"])
  }
  if (run("git", ["-C", destination, "rev-parse", "HEAD"], { capture: true }).trim() !== commit)
    throw new Error("Unexpected VST3 SDK commit")
  run("git", ["-C", destination, "submodule", "update", "--init", "--recursive", "--depth", "1"])
}
