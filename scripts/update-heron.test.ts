import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { test } from "node:test"
import {
  HERON_REPOSITORY,
  UPGRADE_BRANCH,
  maintainHeronUpgrade,
  type UpdateCommand
} from "./update-heron.ts"

const lockFile = "heron-plugins.lock.json"
const upgradeRef = `refs/heads/${UPGRADE_BRANCH}`

function git(cwd: string, ...args: string[]): string {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" })
  if (result.status !== 0) throw new Error(`git ${args.join(" ")}: ${result.stderr}`)
  return result.stdout.trim()
}

function lock(version: string, commit = "a".repeat(40)): string {
  return `${JSON.stringify({ release: { version, sourceCommit: commit } }, null, 2)}\n`
}

interface Pull {
  number: number
  title: string
  body: string | null
  head: { sha: string; ref: string; repo: { full_name: string } }
  base: { ref: string; repo: { full_name: string } }
}

async function fixture() {
  const directory = await mkdtemp(join(tmpdir(), "heron-upgrade-test-"))
  const remote = join(directory, "remote.git")
  const mainCheckout = join(directory, "main")
  await mkdir(mainCheckout)
  git(directory, "init", "--bare", remote)
  git(mainCheckout, "init", "-b", "main")
  git(mainCheckout, "config", "user.name", "Fixture owner")
  git(mainCheckout, "config", "user.email", "fixture@example.invalid")
  await writeFile(
    join(mainCheckout, "package.json"),
    JSON.stringify({
      scripts: { "plugins:update": "owned updater", "plugins:prepare": "owned consumer" }
    })
  )
  await writeFile(join(mainCheckout, lockFile), lock("0.6.4"))
  await writeFile(join(mainCheckout, "owned-code.txt"), "main code\n")
  git(mainCheckout, "add", ".")
  git(mainCheckout, "commit", "-m", "Bootstrap consumer")
  git(mainCheckout, "remote", "add", "origin", remote)
  git(mainCheckout, "push", "origin", "main")
  let sequence = 0
  let candidate = lock("0.6.5", "b".repeat(40))
  let rejectUpdate = false
  let rejectPrepare = false
  let createFailure: "before" | "after" | undefined
  let beforePush: (() => Promise<void>) | undefined
  let afterPrepare: (() => Promise<void>) | undefined
  let pulls: Pull[] = []
  const prepares: string[][] = []
  const pendingLocks: string[] = []
  let pushes = 0
  let creates = 0
  let edits = 0
  const remoteHead = () => git(directory, "--git-dir", remote, "rev-parse", upgradeRef)
  const getArg = (args: string[], flag: string) => args[args.indexOf(flag) + 1]!
  const createPull = (title: string, body: string): Pull => ({
    number: 17,
    title,
    body,
    head: { sha: remoteHead(), ref: UPGRADE_BRANCH, repo: { full_name: HERON_REPOSITORY } },
    base: { ref: "main", repo: { full_name: HERON_REPOSITORY } }
  })
  const checkout = async () => {
    const workspace = join(directory, `checkout-${++sequence}`)
    git(directory, "clone", "--branch", "main", remote, workspace)
    const command: UpdateCommand = async (tool, args) => {
      if (tool === "git") {
        if (args[0] === "push") {
          pushes += 1
          if (beforePush) {
            const action = beforePush
            beforePush = undefined
            await action()
          }
        }
        return `${git(workspace, ...args)}\n`
      }
      if (tool === "mise") {
        assert.deepEqual(args.slice(0, 3), ["exec", "--", "pnpm"])
        if (args[3] === "plugins:update") {
          if (args.includes("--pending-lock"))
            pendingLocks.push(await readFile(getArg(args, "--pending-lock"), "utf8"))
          if (rejectUpdate) throw new Error("Consumer rejected the immutable release")
          await writeFile(join(workspace, lockFile), candidate)
        } else {
          assert.equal(args[3], "plugins:prepare")
          prepares.push(args)
          if (rejectPrepare) throw new Error("Consumer rejected the runtime")
          if (afterPrepare) {
            const action = afterPrepare
            afterPrepare = undefined
            await action()
          }
        }
        return ""
      }
      assert.equal(tool, "gh")
      if (args[0] === "auth") return ""
      if (args[0] === "api") {
        if (args[2] === "POST") {
          assert.equal(args[3], `repos/${HERON_REPOSITORY}/pulls`)
          const change = JSON.parse(await readFile(getArg(args, "--input"), "utf8")) as {
            head: string
            base: string
            title: string
            body: string
          }
          assert.equal(change.head, UPGRADE_BRANCH)
          assert.equal(change.base, "main")
          creates += 1
          if (createFailure === "before") {
            createFailure = undefined
            throw new Error("PR creation failed before acceptance")
          }
          assert.equal(pulls.length, 0)
          pulls.push(createPull(change.title, change.body))
          if (createFailure === "after") {
            createFailure = undefined
            throw new Error("PR creation response was lost")
          }
          return ""
        }
        if (args[2] === "PATCH") {
          assert.equal(args[3], `repos/${HERON_REPOSITORY}/pulls/17`)
          const change = JSON.parse(await readFile(getArg(args, "--input"), "utf8")) as {
            state?: string
            title?: string
            body?: string
          }
          if (change.state === "closed") pulls = []
          else {
            edits += 1
            assert.equal(pulls.length, 1)
            pulls[0]!.title = change.title!
            pulls[0]!.body = change.body!
          }
          return ""
        }
        assert.equal(args[3], `repos/${HERON_REPOSITORY}/pulls`)
        assert.ok(args.includes(`head=minori-live:${UPGRADE_BRANCH}`))
        return JSON.stringify(
          pulls.map((pull) => ({ ...pull, head: { ...pull.head, sha: remoteHead() } }))
        )
      }
      throw new Error(`Unexpected gh command: ${args.join(" ")}`)
    }
    return {
      workspace,
      execute: () => maintainHeronUpgrade({ workspace, command }),
      explicit: (version: string) => maintainHeronUpgrade({ workspace, command, version })
    }
  }
  return {
    directory,
    remote,
    mainCheckout,
    checkout,
    remoteHead,
    mainHead: () => git(mainCheckout, "rev-parse", "HEAD"),
    lockAt: (ref: string) =>
      `${git(directory, "--git-dir", remote, "show", `${ref}:${lockFile}`)}\n`,
    parentAt: (ref: string) => git(directory, "--git-dir", remote, "rev-parse", `${ref}^`),
    async advanceMain() {
      await writeFile(join(mainCheckout, "owned-code.txt"), "new main code\n")
      git(mainCheckout, "add", ".")
      git(mainCheckout, "commit", "-m", "Advance main")
      git(mainCheckout, "push", "origin", "main")
    },
    async humanEdit() {
      const workspace = join(directory, `human-${++sequence}`)
      git(directory, "clone", "--branch", UPGRADE_BRANCH, remote, workspace)
      await writeFile(join(workspace, "owned-code.txt"), "human branch edit\n")
      git(workspace, "add", ".")
      git(
        workspace,
        "-c",
        "user.name=Human",
        "-c",
        "user.email=human@example.invalid",
        "commit",
        "-m",
        "Human edit"
      )
      git(workspace, "push", "origin", `HEAD:${upgradeRef}`)
      return git(workspace, "rev-parse", "HEAD")
    },
    set candidate(value: string) {
      candidate = value
    },
    set rejectUpdate(value: boolean) {
      rejectUpdate = value
    },
    set rejectPrepare(value: boolean) {
      rejectPrepare = value
    },
    set createFailure(value: "before" | "after" | undefined) {
      createFailure = value
    },
    set beforePush(value: (() => Promise<void>) | undefined) {
      beforePush = value
    },
    set afterPrepare(value: (() => Promise<void>) | undefined) {
      afterPrepare = value
    },
    get pulls() {
      return pulls
    },
    set pulls(value: Pull[]) {
      pulls = value
    },
    get pushes() {
      return pushes
    },
    get creates() {
      return creates
    },
    get edits() {
      return edits
    },
    prepares,
    pendingLocks,
    async cleanup() {
      await rm(directory, { recursive: true, force: true })
    }
  }
}

test("producer creates one lock-only upgrade and repairs metadata without changing its commit", async () => {
  const f = await fixture()
  try {
    assert.equal(await (await f.checkout()).execute(), "updated")
    const commit = f.remoteHead()
    assert.equal(f.parentAt(commit), f.mainHead())
    assert.equal(f.lockAt(commit), lock("0.6.5", "b".repeat(40)))
    assert.equal(
      git(f.directory, "--git-dir", f.remote, "diff", "--name-only", f.mainHead(), commit),
      lockFile
    )
    assert.equal(f.pulls.length, 1)
    assert.match(f.pulls[0]!.body!, /Linux x64 runtime archive/)
    f.pulls[0]!.title = "Stale title"
    f.pulls[0]!.body = "Stale body"
    assert.equal(await (await f.checkout()).execute(), "updated")
    assert.equal(f.remoteHead(), commit)
    assert.equal(f.pushes, 1)
    assert.equal(f.creates, 1)
    assert.equal(f.edits, 1)
    assert.equal(f.pulls[0]!.title, "Update Heron plug-ins to 0.6.5")
    assert.equal(f.pendingLocks[0], f.lockAt(commit))
    assert.deepEqual(f.prepares, [
      ["exec", "--", "pnpm", "plugins:prepare", "--platform", "linux-x64"],
      ["exec", "--", "pnpm", "plugins:prepare", "--platform", "linux-x64"]
    ])
  } finally {
    await f.cleanup()
  }
})

test("retry recovers a successful push followed by failed PR creation without another commit", async () => {
  const f = await fixture()
  try {
    f.createFailure = "before"
    await assert.rejects((await f.checkout()).execute(), /PR creation failed/)
    const commit = f.remoteHead()
    assert.equal(f.pulls.length, 0)
    assert.equal(await (await f.checkout()).execute(), "updated")
    assert.equal(f.remoteHead(), commit)
    assert.equal(f.pushes, 1)
    assert.equal(f.pulls.length, 1)
  } finally {
    await f.cleanup()
  }
})

test("lost PR creation response reconciles the already accepted PR", async () => {
  const f = await fixture()
  try {
    f.createFailure = "after"
    assert.equal(await (await f.checkout()).execute(), "updated")
    const commit = f.remoteHead()
    assert.equal(f.pulls.length, 1)
    assert.equal(await (await f.checkout()).execute(), "updated")
    assert.equal(f.remoteHead(), commit)
    assert.equal(f.creates, 1)
    assert.equal(f.pushes, 1)
    assert.equal(f.pulls.length, 1)
  } finally {
    await f.cleanup()
  }
})

test("manual versions cannot inject command options or select noncanonical releases", async () => {
  for (const version of ["--latest", "01.0.0", "1.0.0-rc.1", "9007199254740992.0.0"]) {
    await assert.rejects(
      maintainHeronUpgrade({
        workspace: "/unused-workspace",
        version,
        command: async () => {
          throw new Error("Unexpected git/API mutation")
        }
      }),
      /canonical stable suite version/
    )
  }
})

test("a newer main rebuilds the pending lock-only branch on the current main", async () => {
  const f = await fixture()
  try {
    await (await f.checkout()).execute()
    const previous = f.remoteHead()
    await f.advanceMain()
    assert.equal(await (await f.checkout()).execute(), "updated")
    assert.notEqual(f.remoteHead(), previous)
    assert.equal(f.parentAt(f.remoteHead()), f.mainHead())
    assert.equal(f.lockAt(f.remoteHead()), f.lockAt(previous))
    assert.equal(
      git(f.directory, "--git-dir", f.remote, "show", `${f.remoteHead()}:owned-code.txt`),
      "new main code"
    )
    assert.equal(f.pulls.length, 1)
  } finally {
    await f.cleanup()
  }
})

test("consumer rejection protects the pending branch and PR", async () => {
  const f = await fixture()
  try {
    await (await f.checkout()).execute()
    const previous = f.remoteHead()
    const pull = structuredClone(f.pulls[0])
    f.rejectUpdate = true
    await assert.rejects((await f.checkout()).explicit("0.6.4"), /immutable release/)
    assert.equal(f.remoteHead(), previous)
    assert.deepEqual(f.pulls[0], pull)
    assert.equal(f.pushes, 1)
    assert.equal(f.pendingLocks[0], f.lockAt(previous))
  } finally {
    await f.cleanup()
  }
})

test("invalid final runtime fails before creating a remote branch or PR", async () => {
  const f = await fixture()
  try {
    f.rejectPrepare = true
    await assert.rejects((await f.checkout()).execute(), /rejected the runtime/)
    assert.equal(f.pushes, 0)
    assert.equal(f.pulls.length, 0)
    assert.equal(
      git(f.directory, "--git-dir", f.remote, "show-ref", "--heads"),
      `${f.mainHead()} refs/heads/main`
    )
  } finally {
    await f.cleanup()
  }
})

test("a concurrent branch edit fails the exact lease and preserves the human commit", async () => {
  const f = await fixture()
  try {
    await (await f.checkout()).execute()
    f.candidate = lock("0.6.6", "c".repeat(40))
    let human = ""
    f.beforePush = async () => {
      human = await f.humanEdit()
    }
    await assert.rejects((await f.checkout()).execute(), /stale info|rejected/)
    assert.equal(f.remoteHead(), human)
    assert.equal(f.pulls[0]!.title, "Update Heron plug-ins to 0.6.5")
    await assert.rejects((await f.checkout()).execute(), /unrelated edits/)
    assert.equal(f.remoteHead(), human)
  } finally {
    await f.cleanup()
  }
})

test("main movement during artifact validation fails without publishing a stale branch", async () => {
  const f = await fixture()
  try {
    f.afterPrepare = () => f.advanceMain()
    await assert.rejects((await f.checkout()).execute(), /changed during validation/)
    assert.equal(f.pushes, 0)
    assert.equal(f.pulls.length, 0)
    assert.equal(await (await f.checkout()).execute(), "updated")
    assert.equal(f.parentAt(f.remoteHead()), f.mainHead())
  } finally {
    await f.cleanup()
  }
})

test("already current main creates no empty upgrade PR and supersedes a pending proposal", async () => {
  const f = await fixture()
  try {
    f.candidate = lock("0.6.4")
    assert.equal(await (await f.checkout()).execute(), "current")
    assert.equal(f.pushes, 0)
    assert.equal(f.pulls.length, 0)
    f.candidate = lock("0.6.5", "b".repeat(40))
    await (await f.checkout()).execute()
    const pending = f.remoteHead()
    await writeFile(join(f.mainCheckout, lockFile), f.lockAt(pending))
    git(f.mainCheckout, "add", ".")
    git(f.mainCheckout, "commit", "-m", "Manual suite upgrade")
    git(f.mainCheckout, "push", "origin", "main")
    assert.equal(await (await f.checkout()).execute(), "current")
    assert.equal(f.pulls.length, 0)
    assert.equal(f.remoteHead(), pending)
  } finally {
    await f.cleanup()
  }
})

test("uninitialized Heron main fails with the bootstrap remedy and leaves release handling untouched", async () => {
  const f = await fixture()
  try {
    await writeFile(join(f.mainCheckout, lockFile), '{"release":null}\n')
    git(f.mainCheckout, "add", ".")
    git(f.mainCheckout, "commit", "-m", "Uninitialized migration")
    git(f.mainCheckout, "push", "origin", "main")
    await assert.rejects((await f.checkout()).execute(), /not bootstrapped.*retry.*unaffected/)
    assert.equal(f.pushes, 0)
    assert.equal(f.pulls.length, 0)
  } finally {
    await f.cleanup()
  }
})

test("ambiguous or forked PR metadata cannot redirect the producer writes", async () => {
  const f = await fixture()
  try {
    await (await f.checkout()).execute()
    const pending = f.remoteHead()
    f.pulls.push(structuredClone(f.pulls[0]!))
    await assert.rejects((await f.checkout()).execute(), /at most one/)
    f.pulls.pop()
    f.pulls[0]!.head.repo.full_name = "unexpected/fork"
    await assert.rejects((await f.checkout()).execute(), /Unexpected repository/)
    assert.equal(f.remoteHead(), pending)
    assert.equal(f.pushes, 1)
  } finally {
    await f.cleanup()
  }
})
