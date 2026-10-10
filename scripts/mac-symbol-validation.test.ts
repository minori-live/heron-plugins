import { test } from "node:test"
import assert from "node:assert/strict"
import { plugins } from "./catalog.ts"
import { assertRustSourceLine, rustProcessAddress } from "./mac-symbol-validation.ts"

const eq = plugins.find((plugin) => plugin.slug === "eq")!

test("macOS source validation probes the owned process address in the final binary", () => {
  const symbols = [
    "00000000000607e8 t __RNv...8heron_eq...EqDspState7process",
    "0000000000070000 t __RNv...10other_crate...EqDspState7process",
    "0000000000080000 t __RNv...8heron_eq...EqDspState9configure"
  ].join("\n")
  assert.equal(rustProcessAddress(symbols, eq), "0x00000000000607e8")
  assert.throws(() => rustProcessAddress("", eq))
  assert.throws(() => rustProcessAddress(`${symbols}\n${symbols}`, eq))
})

test("UUID-matched partial AU symbols cannot pass without owned Rust source lines", () => {
  const complete = [
    'DW_AT_name ("plugins/heron-eq/src/lib.rs/@/heron_eq")',
    "Line info: file 'processor.rs', line 277, column 13, start line 277"
  ].join("\n")
  assert.equal(assertRustSourceLine(complete, eq), "processor.rs:277")
  assert.throws(() => assertRustSourceLine(".debug_info contents:\n", eq))
  assert.throws(() => assertRustSourceLine(complete.replace("line 277,", "line 0,"), eq))
  assert.throws(() => assertRustSourceLine(complete.replace("heron-eq", "other-plugin"), eq))
  assert.throws(() => assertRustSourceLine(complete.replace("processor.rs", "lib.rs"), eq))
})
