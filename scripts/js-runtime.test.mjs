// @vitest-environment node
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { expect, it } from "vitest";

// Execute the exact JS sent by Rust to the external runtime. No network or
// installed Deno is needed to verify version rejection and the minimum boundary.
const source = readFileSync(new URL("../src-tauri/src/sources.rs", import.meta.url), "utf8");
function probe(name) {
  const script = new RegExp(`const ${name}: &str =\\s*"([^"\\r\\n]+)";`).exec(source)?.[1];
  if (!script) throw new Error(`Missing production version probe: ${name}`);
  return script;
}

it.each([["20.19.0", 1], ["21.0.0", 1], ["22.0.0", 0], ["24.19.0", 0], ["invalid", 1]])(
  "Node %s has probe exit %i", (version, expected) => {
    let status;
    runInNewContext(probe("NODE_VERSION_CHECK"), { process: { versions: { node: version }, exit: (code) => { status = code; } } });
    expect(status).toBe(expected);
  },
);

it.each([["1.46.0", 1], ["2.2.9", 1], ["2.3.0", 0], ["2.4.0", 0], ["3.0.0", 0], ["invalid", 1]])(
  "Deno %s has probe exit %i", (version, expected) => {
    let status;
    runInNewContext(probe("DENO_VERSION_CHECK"), { Deno: { version: { deno: version }, exit: (code) => { status = code; } } });
    expect(status).toBe(expected);
  },
);
