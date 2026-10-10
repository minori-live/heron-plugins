import { createPrivateKey } from "node:crypto"

export interface AppleApiPrivateKey {
  pem: string
  inputFormat: "PEM" | "Base64"
}

const invalidInput =
  "APPLE_API_KEY_P8 must contain a valid, unencrypted PKCS#8 EC .p8 private key as complete PEM or Base64 of that PEM"

function decodeBase64(value: string): Buffer {
  const compact = value.replace(/[ \t\n]/g, "")
  if (!compact || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(compact))
    throw new Error(invalidInput)
  const decoded = Buffer.from(compact, "base64")
  if (decoded.toString("base64") !== compact) throw new Error(invalidInput)
  return decoded
}

function requireSingleDerObject(der: Buffer) {
  if (der.length < 2 || der[0] !== 0x30) throw new Error(invalidInput)
  const firstLength = der[1]!
  if (firstLength < 0x80) {
    if (der.length !== firstLength + 2) throw new Error(invalidInput)
    return
  }
  const lengthBytes = firstLength & 0x7f
  if (lengthBytes === 0 || lengthBytes > 4 || der.length < lengthBytes + 2 || der[2] === 0)
    throw new Error(invalidInput)
  let length = 0
  for (let index = 2; index < lengthBytes + 2; index++) length = length * 256 + der[index]!
  if (length < 0x80 || length !== der.length - lengthBytes - 2) throw new Error(invalidInput)
}

export function normalizeAppleApiPrivateKey(input: string): AppleApiPrivateKey {
  try {
    const normalized = input.replace(/\r\n?/g, "\n").trim()
    const inputFormat = normalized.startsWith("-----") ? "PEM" : "Base64"
    const pem =
      inputFormat === "PEM"
        ? normalized
        : new TextDecoder("utf-8", { fatal: true }).decode(decodeBase64(normalized))
    const body =
      /^-----BEGIN PRIVATE KEY-----\n([A-Za-z0-9+/= \t\n]+)\n-----END PRIVATE KEY-----$/.exec(
        pem.replace(/\r\n?/g, "\n").trim()
      )?.[1]
    if (!body) throw new Error(invalidInput)
    const der = decodeBase64(body)
    // OpenSSL accepts trailing DER. Require one complete outer object before key parsing.
    requireSingleDerObject(der)
    const key = createPrivateKey({ key: der, format: "der", type: "pkcs8" })
    if (key.asymmetricKeyType !== "ec") throw new Error(invalidInput)
    return { pem: key.export({ format: "pem", type: "pkcs8" }).toString(), inputFormat }
  } catch {
    // Parser diagnostics can contain key material. Expose neither input nor the cause.
    throw new Error(invalidInput)
  }
}
