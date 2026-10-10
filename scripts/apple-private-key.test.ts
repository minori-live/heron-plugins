import { test } from "node:test"
import assert from "node:assert/strict"
import { createPublicKey, generateKeyPairSync } from "node:crypto"
import { normalizeAppleApiPrivateKey } from "./apple-private-key.ts"

const { privateKey, publicKey } = generateKeyPairSync("ec", { namedCurve: "prime256v1" })
const pem = privateKey.export({ format: "pem", type: "pkcs8" }).toString()
const expectedPublicKey = publicKey.export({ format: "der", type: "spki" })

test("Apple API key normalization preserves EC identity for PEM and encoded PEM", () => {
  const crlf = pem.replaceAll("\n", "\r\n")
  const base64 = Buffer.from(crlf).toString("base64")
  for (const [input, format] of [
    [pem, "PEM"],
    [` \n${crlf}\t`, "PEM"],
    [Buffer.from(pem).toString("base64"), "Base64"],
    [base64.match(/.{1,64}/g)!.join("\r\n"), "Base64"]
  ] as const) {
    const result = normalizeAppleApiPrivateKey(input)
    assert.equal(result.inputFormat, format)
    assert.equal(result.pem.includes("\r"), false)
    assert.equal(result.pem.endsWith("\n"), true)
    assert.deepEqual(
      createPublicKey(result.pem).export({ format: "der", type: "spki" }),
      expectedPublicKey
    )
  }
})

test("Apple API key decoding rejects malformed or noncanonical outer Base64", () => {
  const encoded = Buffer.from(pem).toString("base64")
  for (const input of [
    `${encoded}!`,
    encoded.slice(0, -1),
    `${encoded}AA==`,
    `${encoded}\u00a0${encoded}`,
    "/x==",
    Buffer.from([0xff, 0xfe]).toString("base64")
  ])
    assert.throws(() => normalizeAppleApiPrivateKey(input))
})

test("Apple API keys must be unencrypted PKCS#8 EC private keys", () => {
  const rsa = generateKeyPairSync("rsa", { modulusLength: 2048 }).privateKey
  const ed25519 = generateKeyPairSync("ed25519").privateKey
  const inputs = [
    privateKey.export({ format: "pem", type: "sec1" }).toString(),
    privateKey.export({ format: "der", type: "pkcs8" }).toString("base64"),
    publicKey.export({ format: "pem", type: "spki" }).toString(),
    rsa.export({ format: "pem", type: "pkcs8" }).toString(),
    ed25519.export({ format: "pem", type: "pkcs8" }).toString(),
    privateKey
      .export({ format: "pem", type: "pkcs8", cipher: "aes-256-cbc", passphrase: "ephemeral" })
      .toString(),
    `${pem}${pem}`
  ]
  for (const input of inputs)
    for (const representation of [input, Buffer.from(input).toString("base64")])
      assert.throws(() => normalizeAppleApiPrivateKey(representation))
})

test("Apple API PEM contains exactly one complete DER key without trailing data", () => {
  const der = privateKey.export({ format: "der", type: "pkcs8" })
  for (const bytes of [
    Buffer.concat([der, Buffer.from([0])]),
    Buffer.concat([der, Buffer.from("trailing-data")]),
    Buffer.concat([der, der]),
    der.subarray(0, -1)
  ]) {
    const input = `-----BEGIN PRIVATE KEY-----\n${bytes.toString("base64")}\n-----END PRIVATE KEY-----\n`
    assert.throws(() => normalizeAppleApiPrivateKey(input))
    assert.throws(() => normalizeAppleApiPrivateKey(Buffer.from(input).toString("base64")))
  }
})

test("Invalid key input exposes one actionable error without parser causes or key contents", () => {
  const invalidBody = Buffer.from("private-key-material-must-not-appear").toString("base64")
  const invalidPem = `-----BEGIN PRIVATE KEY-----\n${invalidBody}\n-----END PRIVATE KEY-----\n`
  const messages: string[] = []
  for (const input of [
    "",
    "invalid secret input",
    invalidPem,
    Buffer.from(invalidPem).toString("base64")
  ])
    assert.throws(
      () => normalizeAppleApiPrivateKey(input),
      (error: unknown) => {
        assert(error instanceof Error)
        assert.equal(error.cause, undefined)
        assert.match(error.message, /APPLE_API_KEY_P8/)
        assert.equal(error.message.includes(invalidBody), false)
        assert.equal(error.message.includes("private-key-material-must-not-appear"), false)
        messages.push(error.message)
        return true
      }
    )
  assert.equal(new Set(messages).size, 1)
})
