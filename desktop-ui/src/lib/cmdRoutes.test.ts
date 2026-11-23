import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// `app.cmd` sends only through a route; a name no route takes is an error at
// run time. A route takes a command by naming it, so every `cmd("…")` name
// must appear in a module that defines a `route(command` method.
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
    const sent = new Map<string, string>();
    for (const [file, text] of files) {
      for (const m of text.matchAll(/\bcmd\(\s*["']([a-z0-9_]+)["']/g)) {
        if (!sent.has(m[1])) sent.set(m[1], file);
      }
    }
    expect(sent.size).toBeGreaterThan(20);
    const unrouted = [...sent]
      .filter(([name]) => !routeModules.some((text) => new RegExp(`["']${name}["']`).test(text)))
      .map(([name, file]) => `${name} (${file})`);
    expect(unrouted).toEqual([]);
  });
});
