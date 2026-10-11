export const platforms = ["macos-universal", "windows-x64", "linux-x64", "linux-arm64"] as const
export type Platform = (typeof platforms)[number]
export type Format = "vst3" | "au2" | "aax"

export const plugins = [
  {
    builtinId: "live.minori.heron.gain",
    name: "Heron Gain",
    kind: "effect",
    auType: "aufx",
    crate: "heron-gain",
    slug: "gain",
    nativeId: "46774F504DF84B4AC1F308AB88DD3677",
    fourcc: "HRGN",
    aaxIds: "48524F4E:4852474E:4953464F,4953464D"
  },
  {
    builtinId: "live.minori.heron.eq",
    name: "Heron EQ",
    kind: "effect",
    auType: "aumf",
    crate: "heron-eq",
    slug: "eq",
    nativeId: "8A8341D5CA36B6C9A9572788F40EBB9F",
    fourcc: "HREQ",
    aaxIds: "48524F4E:48524551:49534450,49534452"
  },
  {
    builtinId: "live.minori.heron.sine",
    name: "Heron Sine",
    kind: "instrument",
    auType: "aumu",
    crate: "heron-sine",
    slug: "sine",
    nativeId: "C1351DFA4DDD4B4AC1F30896F6D9DF76",
    fourcc: "HRSN",
    aaxIds: "48524F4E:4852534E:4953524D"
  },
  {
    builtinId: "live.minori.heron.metronome",
    name: "Heron Metronome",
    kind: "instrument",
    auType: "aumu",
    crate: "heron-metronome",
    slug: "metronome",
    nativeId: "8CD16A11027ACC7FDF0C1419E86D1024",
    fourcc: "HRMT",
    aaxIds: "48524F4E:48524D54:49534C57"
  }
] as const
export type Plugin = (typeof plugins)[number]

export function hostPlatform(): Platform {
  if (process.platform === "darwin") return "macos-universal"
  if (process.platform === "win32" && process.arch === "x64") return "windows-x64"
  if (process.platform === "linux" && process.arch === "x64") return "linux-x64"
  if (process.platform === "linux" && process.arch === "arm64") return "linux-arm64"
  throw new Error(`Unsupported build host ${process.platform}/${process.arch}`)
}

export function bundleName(plugin: Plugin, format: Format): string {
  return `${plugin.name}.${format === "au2" ? "component" : format === "aax" ? "aaxplugin" : "vst3"}`
}

export function manifestPlugins(format: Format) {
  return plugins.map((plugin) => ({
    builtinId: plugin.builtinId,
    name: plugin.name,
    kind: plugin.kind,
    format,
    nativeId:
      format === "vst3"
        ? plugin.nativeId
        : format === "aax"
          ? plugin.aaxIds
          : `${plugin.auType}:${plugin.fourcc}:HRON`,
    bundleName: bundleName(plugin, format)
  }))
}

export function assetName(version: string, platform: Platform, format: Format) {
  return `heron-plugins-${version}-${platform}-${format}.zip`
}
