import assert from "node:assert/strict"
import { mkdtemp, mkdir, readFile, readdir, rm, stat, utimes, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { waitForAuRegistration, withInstalledAuBundles } from "./au-registration.ts"

async function fixture() {
  const directory = await mkdtemp(join(tmpdir(), "heron-au-registration-test-"))
  const source = join(directory, "source")
  const install = join(directory, "Components")
  await mkdir(source)
  await mkdir(install)
  return { directory, source, install }
}
async function bundle(root: string, name: string, text: string) {
  const directory = join(root, name, "Contents/MacOS")
  await mkdir(directory, { recursive: true })
  await writeFile(join(directory, "executable"), text)
}

test("AU validation installs complete bundles and restores the prior installation after failure", async () => {
  const { directory, source, install } = await fixture()
  try {
    await bundle(source, "Gain.component", "new gain")
    await bundle(source, "EQ.component", "new EQ")
    await bundle(install, "Gain.component", "old gain")
    await utimes(install, new Date(0), new Date(0))
    const failure = new Error("instance validation failed")
    await assert.rejects(
      withInstalledAuBundles(source, ["Gain.component", "EQ.component"], install, async () => {
        assert.equal(
          await readFile(join(install, "Gain.component/Contents/MacOS/executable"), "utf8"),
          "new gain"
        )
        assert.equal(
          await readFile(join(install, "EQ.component/Contents/MacOS/executable"), "utf8"),
          "new EQ"
        )
        assert.deepEqual((await readdir(install)).sort(), ["EQ.component", "Gain.component"])
        assert.ok((await stat(install)).mtimeMs > 0)
        throw failure
      }),
      (error) => error === failure
    )
    assert.equal(
      await readFile(join(install, "Gain.component/Contents/MacOS/executable"), "utf8"),
      "old gain"
    )
    assert.deepEqual(await readdir(install), ["Gain.component"])
    assert.ok((await stat(install)).mtimeMs > 0)
    assert.deepEqual((await readdir(directory)).sort(), ["Components", "source"])
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test("AU copy failure leaves installed components untouched before publication", async () => {
  const { directory, source, install } = await fixture()
  try {
    await bundle(source, "Gain.component", "new gain")
    await bundle(install, "Gain.component", "old gain")
    let validated = false
    await assert.rejects(
      withInstalledAuBundles(source, ["Gain.component", "Missing.component"], install, async () => {
        validated = true
      })
    )
    assert.equal(validated, false)
    assert.equal(
      await readFile(join(install, "Gain.component/Contents/MacOS/executable"), "utf8"),
      "old gain"
    )
    assert.deepEqual(await readdir(install), ["Gain.component"])
    assert.deepEqual((await readdir(directory)).sort(), ["Components", "source"])
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test("fresh AU probe processes retry missing registrations and keep every identity argument", async () => {
  let calls = 0
  let waits = 0
  const identities = ["aufx", "HRGN", "HRON", "1540", "aumf", "HREQ", "HRON", "1540"]
  await waitForAuRegistration("/probe", "arm64", identities, {
    execute(command, args) {
      assert.equal(command, "arch")
      assert.deepEqual(args, ["-arm64", "/probe", ...identities])
      return { status: ++calls === 1 ? 3 : 0, stdout: "", stderr: "" }
    },
    sleep: async () => {
      waits++
    }
  })
  assert.equal(calls, 2)
  assert.equal(waits, 1)
})

test("AU duplicate, version and load failures are fatal without retry", async () => {
  for (const status of [8, 5, 6]) {
    let calls = 0
    let waits = 0
    await assert.rejects(
      waitForAuRegistration("/probe", "x86_64", [], {
        execute: () => {
          calls++
          return { status, stdout: "", stderr: "" }
        },
        sleep: async () => {
          waits++
        }
      }),
      /registration\/load probe failed/
    )
    assert.equal(calls, 1)
    assert.equal(waits, 0)
  }
})

test("AU registration discovery stops at its deadline and never retries a killed probe", async () => {
  let now = 0
  let calls = 0
  await assert.rejects(
    waitForAuRegistration("/probe", "arm64", [], {
      execute: () => {
        calls++
        return { status: 3, stdout: "", stderr: "" }
      },
      now: () => now,
      timeoutMs: 1000,
      sleep: async () => {
        now += 500
      }
    }),
    /bounded discovery/
  )
  assert.equal(calls, 2)
  await assert.rejects(
    waitForAuRegistration("/probe", "arm64", [], {
      execute: () => ({ status: null, signal: "SIGABRT", stdout: "", stderr: "" }),
      sleep: async () => {
        assert.fail("signal failure must not retry")
      }
    }),
    /SIGABRT/
  )
})
