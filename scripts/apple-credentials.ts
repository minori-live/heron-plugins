import { randomBytes } from "node:crypto"
import { spawnSync } from "node:child_process"
import { appendFile, mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { run } from "./process.ts"
import { normalizeAppleApiPrivateKey } from "./apple-private-key.ts"

const directory = join(process.env.RUNNER_TEMP ?? "", "heron-plugins-apple-signing")
const keychain = join(directory, "release.keychain-db")
if (import.meta.main) {
  if (process.platform !== "darwin" || !process.env.RUNNER_TEMP)
    throw new Error("Apple CI credentials require a macOS runner with RUNNER_TEMP")
  if (process.argv[2] === "clean") {
    const previous = await readFile(join(directory, "keychains.json"), "utf8").then(
      (text) => JSON.parse(text) as string[],
      () => null
    )
    try {
      if (previous) {
        run("security", ["list-keychains", "-d", "user", "-s", ...previous])
        run("security", ["delete-keychain", keychain])
      }
    } finally {
      await rm(directory, { recursive: true, force: true })
    }
  } else {
    for (const name of ["MAC_CSC_LINK", "APPLE_API_KEY_P8", "APPLE_API_KEY_ID", "APPLE_API_ISSUER"])
      if (!process.env[name]) throw new Error(`Signed release requires ${name}`)
    if (process.env.MAC_CSC_KEY_PASSWORD === undefined)
      throw new Error("Signed release requires MAC_CSC_KEY_PASSWORD")
    const normalizedKey = normalizeAppleApiPrivateKey(process.env.APPLE_API_KEY_P8!)
    console.log(`Apple API private key input: ${normalizedKey.inputFormat}`)
    await mkdir(directory, { recursive: true, mode: 0o700 })
    const previous = [
      ...run("security", ["list-keychains", "-d", "user"], { capture: true }).matchAll(
        /"([^"\n]+)"/g
      )
    ].map((match) => match[1]!)
    await writeFile(join(directory, "keychains.json"), JSON.stringify(previous), { mode: 0o600 })
    const certificate = join(directory, "certificate.p12")
    const apiKey = join(directory, "AuthKey.p8")
    await writeFile(certificate, Buffer.from(process.env.MAC_CSC_LINK!, "base64"), { mode: 0o600 })
    await writeFile(apiKey, normalizedKey.pem, { mode: 0o600 })
    const password = randomBytes(32).toString("hex")
    run("security", ["create-keychain", "-p", password, keychain])
    run("security", ["set-keychain-settings", "-lut", "21600", keychain])
    run("security", ["unlock-keychain", "-p", password, keychain])
    run("security", [
      "import",
      certificate,
      "-k",
      keychain,
      "-P",
      process.env.MAC_CSC_KEY_PASSWORD!,
      "-T",
      "/usr/bin/codesign"
    ])
    run("security", [
      "set-key-partition-list",
      "-S",
      "apple-tool:,apple:",
      "-s",
      "-k",
      password,
      keychain
    ])
    run("security", ["list-keychains", "-d", "user", "-s", keychain, ...previous])
    const identity = run("security", ["find-identity", "-v", "-p", "codesigning", keychain], {
      capture: true
    }).match(/"(Developer ID Application:[^"\n]+)"/)?.[1]
    if (!identity)
      throw new Error("PKCS#12 does not contain a valid Developer ID Application identity")
    const authentication = spawnSync(
      "xcrun",
      [
        "notarytool",
        "store-credentials",
        "heron-plugins-release",
        "--key",
        apiKey,
        "--key-id",
        process.env.APPLE_API_KEY_ID!,
        "--issuer",
        process.env.APPLE_API_ISSUER!,
        "--keychain",
        keychain,
        "--validate"
      ],
      {
        stdio: ["ignore", "pipe", "pipe"],
        encoding: "utf8",
        timeout: 60_000,
        maxBuffer: 1024 * 1024
      }
    )
    if (authentication.error || authentication.status !== 0)
      throw new Error(
        "Apple notarization authentication failed or timed out; verify the API key, key ID, issuer and network access"
      )
    console.log("Apple notarization credentials validated")
    if (!process.env.GITHUB_ENV) throw new Error("Missing GITHUB_ENV")
    await appendFile(
      process.env.GITHUB_ENV,
      `TRUCE_SIGNING_IDENTITY=${identity}\nAPPLE_API_KEY=${apiKey}\n`
    )
  }
}
