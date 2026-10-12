import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { parseArgs } from "node:util"
import { run } from "./process.ts"

export const HERON_REPOSITORY = "minori-live/heron"
export const UPGRADE_BRANCH = "dsh0416/update-heron-plugins"
const LOCK_FILE = "heron-plugins.lock.json"
const branchRef = `refs/heads/${UPGRADE_BRANCH}`

export type UpdateCommand = (command: string, args: string[]) => Promise<string>

interface PullRequest {
  number: number
  title: string
  body: string | null
  head: { sha: string; ref: string; repo: { full_name: string } }
  base: { ref: string; repo: { full_name: string } }
}

function releaseMetadata(lock: string): { version: string; sourceCommit: string } {
  // The consumer owns full lock parsing and release validation. These fields only
  // describe the result in the PR; never use them to approve an artifact.
  const value = JSON.parse(lock) as {
    release?: { version?: string; sourceCommit?: string } | null
  }
  if (!value.release)
    throw new Error(
      "Heron main is not bootstrapped with an initialized plug-in lock. Merge and pin the consumer migration, then retry; the published release is unaffected."
    )
  if (
    typeof value.release.version !== "string" ||
    !/^\d+\.\d+\.\d+$/.test(value.release.version) ||
    typeof value.release.sourceCommit !== "string" ||
    !/^[a-f0-9]{40}$/.test(value.release.sourceCommit)
  )
    throw new Error("Missing validated release metadata for the upgrade PR")
  return { version: value.release.version, sourceCommit: value.release.sourceCommit }
}

async function openUpgrade(command: UpdateCommand): Promise<PullRequest | undefined> {
  const pulls = JSON.parse(
    await command("gh", [
      "api",
      "--method",
      "GET",
      `repos/${HERON_REPOSITORY}/pulls`,
      "-f",
      "state=open",
      "-f",
      "base=main",
      "-f",
      `head=minori-live:${UPGRADE_BRANCH}`
    ])
  ) as PullRequest[]
  if (!Array.isArray(pulls) || pulls.length > 1)
    throw new Error("Expected at most one open Heron plug-in upgrade PR")
  const pull = pulls[0]
  if (
    pull &&
    (!Number.isSafeInteger(pull.number) ||
      pull.number <= 0 ||
      pull.head.ref !== UPGRADE_BRANCH ||
      pull.head.repo.full_name !== HERON_REPOSITORY ||
      pull.base.ref !== "main" ||
      pull.base.repo.full_name !== HERON_REPOSITORY)
  )
    throw new Error("Unexpected repository or branch in the upgrade PR")
  return pull
}

async function remoteCommit(command: UpdateCommand, ref: string): Promise<string> {
  const output = (await command("git", ["ls-remote", "--heads", "origin", ref])).trim()
  if (!output) return ""
  const [commit, actualRef, ...extra] = output.split(/\s+/)
  if (!commit || !/^[a-f0-9]{40}$/.test(commit) || actualRef !== ref || extra.length)
    throw new Error("Unexpected remote branch response")
  return commit
}

function pullBody(version: string, sourceCommit: string): string {
  return `Update the bundled VST3 suite to [Heron Plugins ${version}](https://github.com/minori-live/heron-plugins/releases/tag/v${version}), produced from commit \`${sourceCommit}\`.

The lock records the suite version, source commit and runtime release files for macOS universal, Windows x64, Linux x64 and Linux arm64.

The producer prepared the Linux x64 runtime archive using Heron's artifact consumer. Normal Heron pull-request CI verifies native integration and packaging across the supported platforms. Merge this PR manually to deliver the suite with a future Heron application release.
`
}

export async function maintainHeronUpgrade(options: {
  workspace: string
  version?: string
  command?: UpdateCommand
}): Promise<"current" | "updated"> {
  if (
    options.version !== undefined &&
    (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(options.version) ||
      options.version.split(".").some((part) => !Number.isSafeInteger(Number(part))))
  )
    throw new Error("Manual retries require a canonical stable suite version")
  const workspace = resolve(options.workspace)
  const command: UpdateCommand =
    options.command ?? (async (tool, args) => run(tool, args, { cwd: workspace, capture: true }))
  const temporary = await mkdtemp(join(process.env.RUNNER_TEMP ?? tmpdir(), "heron-upgrade-"))
  try {
    if ((await command("git", ["status", "--porcelain"])).trim())
      throw new Error("Heron checkout must be clean before resolving a release")
    await command("gh", ["auth", "setup-git"])
    await command("git", [
      "fetch",
      "--no-tags",
      "origin",
      "+refs/heads/main:refs/remotes/origin/main"
    ])
    const main = (await command("git", ["rev-parse", "refs/remotes/origin/main"])).trim()
    if ((await command("git", ["rev-parse", "HEAD"])).trim() !== main)
      throw new Error("Heron main changed after checkout/toolchain installation; retry")
    await command("git", ["checkout", "--detach", main])
    const pkg = JSON.parse(await readFile(join(workspace, "package.json"), "utf8")) as {
      scripts?: Record<string, string>
    }
    if (!pkg.scripts?.["plugins:update"] || !pkg.scripts?.["plugins:prepare"])
      throw new Error(
        "Heron main lacks the plug-in artifact consumer. Merge the migration, then retry; the published release is unaffected."
      )
    releaseMetadata(await readFile(join(workspace, LOCK_FILE), "utf8"))

    const pendingCommit = await remoteCommit(command, branchRef)
    const pull = await openUpgrade(command)
    if (pull && pull.head.sha !== pendingCommit)
      throw new Error("Upgrade branch and PR disagree; retry after the branch settles")
    const pendingPath = join(temporary, "pending-lock.json")
    const pendingArgs: string[] = []
    if (pendingCommit) {
      await command("git", ["fetch", "--no-tags", "origin", branchRef])
      if ((await command("git", ["rev-parse", "FETCH_HEAD"])).trim() !== pendingCommit)
        throw new Error("Upgrade branch changed while reading its lock; retry")
      const ancestor = (await command("git", ["merge-base", main, pendingCommit])).trim()
      const changed = (
        await command("git", ["diff", "--name-only", ancestor, pendingCommit])
      ).trim()
      if (changed && changed !== LOCK_FILE)
        throw new Error("Upgrade branch contains unrelated edits; refusing to overwrite them")
      await writeFile(pendingPath, await command("git", ["show", `${pendingCommit}:${LOCK_FILE}`]))
      pendingArgs.push("--pending-lock", pendingPath)
    }
    await command("mise", [
      "exec",
      "--",
      "pnpm",
      "plugins:update",
      ...pendingArgs,
      ...(options.version ? ["--", options.version] : ["--latest"])
    ])
    // Always prepare the runtime archive, including retries where the
    // main or pending lock already names this version.
    await command("mise", ["exec", "--", "pnpm", "plugins:prepare", "--platform", "linux-x64"])
    const changed = (await command("git", ["diff", "--name-only", "HEAD"])).trim()
    if (changed && changed !== LOCK_FILE)
      throw new Error("Consumer validation modified unrelated tracked files")
    if (
      (await remoteCommit(command, "refs/heads/main")) !== main ||
      (await remoteCommit(command, branchRef)) !== pendingCommit
    )
      throw new Error("Heron main or upgrade branch changed during validation; retry")
    if (!changed) {
      // A manual main update can supersede the rolling PR; do not leave an open
      // downgrade proposal behind. Leave the branch for audit/retry history.
      if (pull) {
        const closePath = join(temporary, "close.json")
        await writeFile(closePath, JSON.stringify({ state: "closed" }))
        await command("gh", [
          "api",
          "--method",
          "PATCH",
          `repos/${HERON_REPOSITORY}/pulls/${pull.number}`,
          "--input",
          closePath
        ])
      }
      return "current"
    }
    const { version, sourceCommit } = releaseMetadata(
      await readFile(join(workspace, LOCK_FILE), "utf8")
    )
    await command("git", ["add", "--", LOCK_FILE])
    const tree = (await command("git", ["write-tree"])).trim()
    let upgradeCommit = pendingCommit
    const reusable =
      pendingCommit &&
      (await command("git", ["merge-base", main, pendingCommit])).trim() === main &&
      (await command("git", ["rev-parse", `${pendingCommit}^{tree}`])).trim() === tree
    if (!reusable) {
      await command("git", ["switch", "-C", UPGRADE_BRANCH])
      await command("git", [
        "-c",
        "user.name=Heron plug-in updater",
        "-c",
        "user.email=41898282+github-actions[bot]@users.noreply.github.com",
        "commit",
        "-m",
        `Update Heron plug-ins to ${version}`
      ])
      upgradeCommit = (await command("git", ["rev-parse", "HEAD"])).trim()
      await command("git", [
        "push",
        `--force-with-lease=${branchRef}:${pendingCommit}`,
        "origin",
        `HEAD:${branchRef}`
      ])
    }
    if ((await remoteCommit(command, branchRef)) !== upgradeCommit)
      throw new Error("Upgrade branch changed before PR reconciliation; retry")
    // Re-read after pushing. A prior push may have succeeded while PR creation
    // failed, or another actor may have opened the same PR in the meantime.
    const currentPull = await openUpgrade(command)
    if (currentPull && currentPull.head.sha !== upgradeCommit)
      throw new Error("Upgrade PR changed before metadata reconciliation; retry")
    const title = `Update Heron plug-ins to ${version}`
    const body = pullBody(version, sourceCommit)
    if (currentPull) {
      if (currentPull.title !== title || currentPull.body !== body) {
        const metadataPath = join(temporary, "metadata.json")
        await writeFile(metadataPath, JSON.stringify({ title, body }))
        await command("gh", [
          "api",
          "--method",
          "PATCH",
          `repos/${HERON_REPOSITORY}/pulls/${currentPull.number}`,
          "--input",
          metadataPath
        ])
      }
    } else {
      const createPath = join(temporary, "create.json")
      await writeFile(
        createPath,
        JSON.stringify({ title, body, head: UPGRADE_BRANCH, base: "main" })
      )
      try {
        await command("gh", [
          "api",
          "--method",
          "POST",
          `repos/${HERON_REPOSITORY}/pulls`,
          "--input",
          createPath
        ])
      } catch (error) {
        // A response can be lost after GitHub accepted the creation. Only claim
        // success if the exact intended PR is observable; otherwise fail/retry.
        const accepted = await openUpgrade(command)
        if (
          !accepted ||
          accepted.head.sha !== upgradeCommit ||
          accepted.title !== title ||
          accepted.body !== body
        )
          throw error
      }
    }
    return "updated"
  } finally {
    await rm(temporary, { recursive: true, force: true })
  }
}

if (import.meta.main) {
  const { values } = parseArgs({ options: { workspace: { type: "string", default: "heron" } } })
  try {
    if (!process.env.GH_TOKEN) throw new Error("Configure the producer's HERON_PLUGINS_SYNC_TOKEN")
    const result = await maintainHeronUpgrade({
      workspace: values.workspace!,
      version: process.env.PLUGIN_VERSION?.trim() || undefined
    })
    console.log(
      result === "current" ? "Heron's plug-in lock is current" : "Heron upgrade PR is current"
    )
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  }
}
