import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// `app.cmd` sends only through a route; a name no route takes is an error at
// run time. A route takes a command by naming it in its table: a `case` of its
// switch, an `if (command === …)` or an entry of a `new Set([...])`, so every `cmd("…")` name must be
// in the table of a module that defines a `route(command` method.
const srcDir = join(dirname(fileURLToPath(import.meta.url)), "..");

function sources(): [string, string][] {
  return readdirSync(srcDir, { recursive: true, encoding: "utf8" })
    .filter((f) => /\.(ts|svelte)$/.test(f) && !/\.test\.ts$/.test(f))
    .map((f) => [f, readFileSync(join(srcDir, f), "utf8")]);
}

describe("app.cmd routes", () => {
  it("routes every command name the UI sends through cmd", () => {
    const files = sources();
    const routeModules = files.filter(([, text]) => /\broute\(command\b/.test(text)).map(([, text]) => text);
    expect(routeModules.length).toBeGreaterThan(5);
    const routed = new Set<string>();
    for (const text of routeModules) {
      for (const m of text.matchAll(/\bcase\s+["']([a-z0-9_]+)["']\s*:/g)) routed.add(m[1]);
      for (const m of text.matchAll(/\bif \(command === ["']([a-z0-9_]+)["']\)/g)) routed.add(m[1]);
      for (const set of text.matchAll(/new Set\(\[([^\]]*)\]\)/g)) {
        for (const m of set[1].matchAll(/["']([a-z0-9_]+)["']/g)) routed.add(m[1]);
      }
    }
    const sent = new Map<string, string>();
    for (const [file, text] of files) {
      for (const m of text.matchAll(/\bcmd\(\s*["']([a-z0-9_]+)["']/g)) {
        if (!sent.has(m[1])) sent.set(m[1], file);
      }
    }
    expect(sent.size).toBeGreaterThan(20);
    const unrouted = [...sent]
      .filter(([name]) => !routed.has(name))
      .map(([name, file]) => `${name} (${file})`);
    expect(unrouted).toEqual([]);
  });
});
