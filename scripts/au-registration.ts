import { spawnSync } from "node:child_process"
import { cp, lstat, mkdir, mkdtemp, rename, rm, utimes } from "node:fs/promises"
import { dirname, join } from "node:path"
import { setTimeout } from "node:timers/promises"

export async function refreshAuDirectory(directory: string): Promise<void> {
  const now = new Date()
  await utimes(directory, now, now)
}

// Registrar watches the Components directory timestamp. Copying into that
// directory exposes an incomplete bundle before its executable has arrived.
export async function withInstalledAuBundles(
  source: string,
  names: readonly string[],
  install: string,
  validate: () => Promise<void>
): Promise<void> {
  await mkdir(install, { recursive: true })
  const temporary = await mkdtemp(join(dirname(install), ".heron-au-validation-"))
  const prepared = join(temporary, "prepared")
  const previous = join(temporary, "previous")
  const records = names.map((name) => ({
    name,
    installed: join(install, name),
    prepared: join(prepared, name),
    backup: join(previous, name),
    hadPrevious: false,
    published: false
  }))
  let failed = false
  let failure: unknown
  try {
    await mkdir(prepared)
    await mkdir(previous)
    for (const record of records) {
      await cp(join(source, record.name), record.prepared, {
        recursive: true,
        verbatimSymlinks: true
      })
    }
    for (const record of records) {
      const exists = await lstat(record.installed).then(
        () => true,
        (error: NodeJS.ErrnoException) => {
          if (error.code === "ENOENT") return false
          throw error
        }
      )
      if (exists) {
        await rename(record.installed, record.backup)
        record.hadPrevious = true
      }
      await rename(record.prepared, record.installed)
      record.published = true
    }
    await refreshAuDirectory(install)
    await validate()
  } catch (error) {
    failed = true
    failure = error
  }
  const errors: unknown[] = []
  for (const record of records.reverse()) {
    try {
      if (record.published) await rm(record.installed, { recursive: true })
      if (record.hadPrevious) await rename(record.backup, record.installed)
    } catch (error) {
      errors.push(error)
    }
  }
  try {
    if (records.some((record) => record.published || record.hadPrevious))
      await refreshAuDirectory(install)
  } catch (error) {
    errors.push(error)
  }
  if (errors.length > 0)
    throw new AggregateError(
      failed ? [failure, ...errors] : errors,
      `AU restoration failed; backups retained at ${temporary}`
    )
  await rm(temporary, { recursive: true })
  if (failed) throw failure
}

export interface AuProbeResult {
  status: number | null
  stdout: string
  stderr: string
  signal?: NodeJS.Signals | null
  error?: Error
}
interface RegistrationOptions {
  execute?: (command: string, args: string[]) => AuProbeResult
  sleep?: () => Promise<void>
  now?: () => number
  timeoutMs?: number
}

export async function waitForAuRegistration(
  probe: string,
  architecture: string,
  identities: string[],
  options: RegistrationOptions = {}
): Promise<void> {
  const execute =
    options.execute ??
    ((command: string, args: string[]) =>
      spawnSync(command, args, { encoding: "utf8", timeout: 10_000, maxBuffer: 1024 ** 2 }))
  const now = options.now ?? (() => performance.now())
  const deadline = now() + (options.timeoutMs ?? 30_000)
  let attempts = 0
  while (true) {
    if (attempts > 0 && now() >= deadline)
      throw new Error(`AU registration still missing for ${architecture} after bounded discovery`)
    // Each lookup gets a fresh process; a process may cache a missing registration.
    const result = execute("arch", [`-${architecture}`, probe, ...identities])
    if (result.error) throw result.error
    if (result.status === 0) {
      if (result.stdout) process.stdout.write(result.stdout)
      return
    }
    if (result.stderr) process.stderr.write(result.stderr)
    if (result.status !== 3)
      throw new Error(
        `AU registration/load probe failed for ${architecture} (${result.status ?? result.signal})`
      )
    if (now() >= deadline)
      throw new Error(`AU registration still missing for ${architecture} after bounded discovery`)
    console.log(`Waiting for AU registration: ${architecture}, attempt ${++attempts}`)
    await (options.sleep ?? (() => setTimeout(500)))()
  }
}
